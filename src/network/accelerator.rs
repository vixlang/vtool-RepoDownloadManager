use crate::config::DownloadConfig;
use crate::error::{DownloadError, Result};
use crate::network::dns::DnsCache;
use crate::network::mirror::MirrorManager;
use crate::network::pool::build_http_client;
use crate::network::retry::RetryPolicy;
use reqwest::Client;
use sha2::{Sha256, Digest};
use std::io::{SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncSeekExt, AsyncWriteExt};

#[derive(Debug)]
pub struct DownloadProgress {
    pub total_size: u64,
    pub downloaded: Arc<AtomicU64>,
    pub speed: Arc<AtomicU64>,
}

impl Clone for DownloadProgress {
    fn clone(&self) -> Self {
        Self {
            total_size: self.total_size,
            downloaded: Arc::clone(&self.downloaded),
            speed: Arc::clone(&self.speed),
        }
    }
}

impl DownloadProgress {
    pub fn downloaded_bytes(&self) -> u64 {
        self.downloaded.load(Ordering::Relaxed)
    }
}

pub struct NetworkAccelerator {
    client: Client,
    pub dns_cache: Arc<DnsCache>,
    pub mirror_manager: Arc<MirrorManager>,
    config: DownloadConfig,
    abort_flag: Arc<AtomicBool>,
}

impl NetworkAccelerator {
    pub fn new(config: DownloadConfig) -> Result<Self> {
        let client = build_http_client(&config)
            .map_err(|e| DownloadError::Other(anyhow::anyhow!("Failed to build HTTP client: {e}")))?;

        let dns_cache = Arc::new(DnsCache::new(&config));
        let mirror_manager = Arc::new(MirrorManager::new(config.mirrors.clone()));

        Ok(Self {
            client,
            dns_cache,
            mirror_manager,
            config,
            abort_flag: Arc::new(AtomicBool::new(false)),
        })
    }

    pub fn client(&self) -> &Client {
        &self.client
    }

    pub fn config(&self) -> &DownloadConfig {
        &self.config
    }

    pub fn abort(&self) {
        self.abort_flag.store(true, Ordering::SeqCst);
    }

    pub async fn pre_warm(&self) {
        let hosts = vec![
            "github.com",
            "api.github.com",
            "codeload.github.com",
            "objects.githubusercontent.com",
            "raw.githubusercontent.com",
        ];
        self.dns_cache.pre_resolve_hosts(&hosts, 443).await;
    }

    pub async fn fetch_file_size(&self, url: &str) -> Result<u64> {
        let response = self
            .client
            .head(url)
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(DownloadError::Other(anyhow::anyhow!(
                "HEAD request failed: {}",
                response.status()
            )));
        }

        response
            .headers()
            .get(reqwest::header::CONTENT_LENGTH)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse().ok())
            .ok_or_else(|| {
                DownloadError::Other(anyhow::anyhow!("Cannot determine file size"))
            })
    }

    pub async fn check_range_support(&self, url: &str) -> Result<bool> {
        let response = self
            .client
            .head(url)
            .header(reqwest::header::RANGE, "bytes=0-0")
            .send()
            .await?;

        let status = response.status().as_u16();
        Ok(status == 206 || status == 200)
    }

    pub async fn download_file(
        &self,
        url: &str,
        dest_path: &Path,
        progress: Option<DownloadProgress>,
    ) -> Result<PathBuf> {
        let file_size = self.fetch_file_size(url).await?;
        let supports_range = self.check_range_support(url).await?;

        if supports_range && file_size > self.config.chunk_size {
            self.download_chunked(url, dest_path, file_size, progress).await
        } else {
            self.download_single(url, dest_path, file_size, progress).await
        }
    }

    async fn download_single(
        &self,
        url: &str,
        dest_path: &Path,
        _file_size: u64,
        progress: Option<DownloadProgress>,
    ) -> Result<PathBuf> {
        let retry_policy = RetryPolicy::new(self.config.max_retries);

        for attempt in 0..=retry_policy.max_retries {
            if self.abort_flag.load(Ordering::Relaxed) {
                return Err(DownloadError::Aborted);
            }

            if attempt > 0 {
                let delay = retry_policy.delay_for_attempt(attempt);
                tokio::time::sleep(delay).await;
            }

            match self.try_single_download(url, dest_path, &progress).await {
                Ok(path) => return Ok(path),
                Err(e) => {
                    if attempt == retry_policy.max_retries {
                        return Err(e);
                    }
                }
            }
        }

        Err(DownloadError::Other(anyhow::anyhow!("Single download failed")))
    }

    async fn try_single_download(
        &self,
        url: &str,
        dest_path: &Path,
        progress: &Option<DownloadProgress>,
    ) -> Result<PathBuf> {
        let response = self.client.get(url).send().await?;

        let status = response.status();
        if !status.is_success() {
            return Err(DownloadError::Other(anyhow::anyhow!("HTTP {status}")));
        }

        let _total_size = response
            .headers()
            .get(reqwest::header::CONTENT_LENGTH)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(0);

        let mut file = std::fs::File::create(dest_path)?;
        let mut stream = response.bytes_stream();

        use futures::StreamExt;
        while let Some(chunk) = stream.next().await {
            if self.abort_flag.load(Ordering::Relaxed) {
                return Err(DownloadError::Aborted);
            }
            let chunk = chunk?;
            file.write_all(&chunk)?;
            if let Some(p) = progress {
                p.downloaded.fetch_add(chunk.len() as u64, Ordering::Relaxed);
            }
        }

        file.flush()?;
        Ok(dest_path.to_path_buf())
    }

    async fn download_chunked(
        &self,
        url: &str,
        dest_path: &Path,
        file_size: u64,
        progress: Option<DownloadProgress>,
    ) -> Result<PathBuf> {
        let num_chunks = ((file_size + self.config.chunk_size - 1) / self.config.chunk_size) as usize;
        let num_chunks = num_chunks.max(1);

        if let Some(parent) = dest_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        {
            let file = std::fs::File::create(dest_path)?;
            file.set_len(file_size)?;
        }

        let semaphore = Arc::new(tokio::sync::Semaphore::new(self.config.max_concurrency));
        let retry_policy = Arc::new(RetryPolicy::new(self.config.max_retries));
        let url = Arc::from(url.to_string());
        let dest = Arc::from(dest_path.to_path_buf());

        let mut handles = Vec::with_capacity(num_chunks);

        for i in 0..num_chunks {
            let start = i as u64 * self.config.chunk_size;
            let end = std::cmp::min(start + self.config.chunk_size - 1, file_size - 1);

            if start > end {
                continue;
            }

            let client = self.client.clone();
            let semaphore = Arc::clone(&semaphore);
            let progress = progress.clone();
            let url = Arc::clone(&url);
            let dest = Arc::clone(&dest);
            let abort_flag = Arc::clone(&self.abort_flag);
            let retry_policy = Arc::clone(&retry_policy);

            let handle = tokio::spawn(async move {
                let _permit = semaphore.acquire().await;

                for attempt in 0..=retry_policy.max_retries {
                    if abort_flag.load(Ordering::Relaxed) {
                        return Err(DownloadError::Aborted);
                    }

                    if attempt > 0 {
                        let delay = retry_policy.delay_for_attempt(attempt);
                        tokio::time::sleep(delay).await;
                    }

                    match Self::download_chunk(&client, &url, start, end, &dest, abort_flag.as_ref()).await {
                        Ok(bytes_downloaded) => {
                            if let Some(p) = progress {
                                p.downloaded.fetch_add(bytes_downloaded, Ordering::Relaxed);
                            }
                            return Ok(());
                        }
                        Err(e) => {
                            if attempt == retry_policy.max_retries {
                                return Err(e);
                            }
                        }
                    }
                }

                Err(DownloadError::Other(anyhow::anyhow!("Chunk download failed")))
            });

            handles.push(handle);
        }

        for handle in handles {
            handle.await.map_err(|e| {
                DownloadError::Other(anyhow::anyhow!("Chunk task panicked: {e}"))
            })??;
        }

        Ok(dest_path.to_path_buf())
    }

    async fn download_chunk(
        client: &Client,
        url: &str,
        start: u64,
        end: u64,
        dest_path: &Path,
        _abort_flag: &AtomicBool,
    ) -> Result<u64> {
        let range_header = format!("bytes={start}-{end}");

        let response = client
            .get(url)
            .header(reqwest::header::RANGE, &range_header)
            .send()
            .await?;

        let status = response.status().as_u16();
        if status != 206 && status != 200 {
            return Err(DownloadError::Other(anyhow::anyhow!(
                "Chunk request returned HTTP {status}"
            )));
        }

        let mut file = tokio::fs::OpenOptions::new()
            .write(true)
            .read(true)
            .open(dest_path)
            .await?;

        file.seek(SeekFrom::Start(start)).await?;

        let bytes = response.bytes().await?;
        let byte_count = bytes.len() as u64;

        file.write_all(&bytes).await?;

        Ok(byte_count)
    }

    pub async fn download_with_mirror_fallback(
        &self,
        url: &str,
        dest_path: &Path,
        progress: Option<DownloadProgress>,
    ) -> Result<PathBuf> {
        let path_segment = Self::extract_path_segment(url);

        let mirrors = self.mirror_manager.get_mirrors();
        if mirrors.is_empty() {
            return Err(DownloadError::AllMirrorsExhausted(url.to_string()));
        }

        let mut last_error = None;

        for mirror_url in &mirrors {
            if self.abort_flag.load(Ordering::Relaxed) {
                return Err(DownloadError::Aborted);
            }

            let full_url = if mirror_url.contains("ghproxy.com") {
                format!("{mirror_url}/{path_segment}")
            } else {
                format!("{mirror_url}/{path_segment}")
            };

            match self.download_file(&full_url, dest_path, progress.clone()).await {
                Ok(path) => {
                    self.mirror_manager.record_success(mirror_url);
                    return Ok(path);
                }
                Err(e) => {
                    self.mirror_manager.record_failure(mirror_url);
                    last_error = Some(e);
                }
            }
        }

        Err(last_error.unwrap_or_else(|| {
            DownloadError::AllMirrorsExhausted(url.to_string())
        }))
    }

    fn extract_path_segment(url: &str) -> String {
        if let Ok(parsed) = url::Url::parse(url) {
            let path = parsed.path();
            return path.trim_start_matches('/').to_string();
        }
        if let Some(idx) = url.find("://") {
            let after_scheme = &url[idx + 3..];
            if let Some(slash_idx) = after_scheme.find('/') {
                return after_scheme[slash_idx + 1..].to_string();
            }
        }
        url.to_string()
    }

    pub fn compute_sha256(data: &[u8]) -> String {
        let mut hasher = Sha256::new();
        hasher.update(data);
        hex::encode(hasher.finalize())
    }
}
