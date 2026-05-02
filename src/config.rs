use std::time::Duration;

#[derive(Debug, Clone)]
pub struct DownloadConfig {
    pub chunk_size: u64,
    pub max_concurrency: usize,
    pub max_retries: u32,
    pub dns_cache_ttl: Duration,
    pub connect_timeout: Duration,
    pub request_timeout: Duration,
    pub keep_alive_timeout: Duration,
    pub pool_max_idle_per_host: usize,
    pub pool_idle_timeout: Duration,
    pub http2_adaptive_window: bool,
    pub tcp_nodelay: bool,
    pub tls_session_cache_size: usize,
    pub mirrors: Vec<String>,
    pub github_token: Option<String>,
}

impl Default for DownloadConfig {
    fn default() -> Self {
        Self {
            chunk_size: 8 * 1024 * 1024,
            max_concurrency: 16,
            max_retries: 5,
            dns_cache_ttl: Duration::from_secs(300),
            connect_timeout: Duration::from_secs(10),
            request_timeout: Duration::from_secs(60),
            keep_alive_timeout: Duration::from_secs(90),
            pool_max_idle_per_host: 8,
            pool_idle_timeout: Duration::from_secs(90),
            http2_adaptive_window: true,
            tcp_nodelay: true,
            tls_session_cache_size: 32,
            mirrors: vec![
                "https://github.com".to_string(),
                "https://hub.fastgit.xyz".to_string(),
                "https://ghproxy.com/https://github.com".to_string(),
            ],
            github_token: None,
        }
    }
}
