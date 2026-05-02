use crate::error::{DownloadError, Result};
use crate::network::NetworkAccelerator;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitHubRepo {
    pub owner: String,
    pub name: String,
    pub default_branch: String,
    pub description: Option<String>,
    pub clone_url: String,
    pub archive_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoListResponse {
    pub repos: Vec<GitHubRepo>,
    pub total_count: usize,
}

pub struct GitHubClient {
    accelerator: Arc<NetworkAccelerator>,
    org: String,
}

impl GitHubClient {
    pub fn new(accelerator: Arc<NetworkAccelerator>, org: &str) -> Self {
        Self {
            accelerator,
            org: org.to_string(),
        }
    }

    pub async fn list_org_repos(&self) -> Result<Vec<GitHubRepo>> {
        let url = format!("https://api.github.com/orgs/{}/repos?per_page=100&type=public", self.org);
        let mut all_repos = Vec::new();
        let mut page = 1;

        loop {
            let page_url = format!("{url}&page={page}");
            let response = self
                .accelerator
                .client()
                .get(&page_url)
                .header(reqwest::header::ACCEPT, "application/vnd.github.v3+json")
                .header("User-Agent", "VixRepoDownloadManager")
                .send()
                .await?;

            if !response.status().is_success() {
                return Err(DownloadError::Other(anyhow::anyhow!(
                    "GitHub API returned: {}",
                    response.status()
                )));
            }

            let repos: Vec<serde_json::Value> = response.json().await?;
            if repos.is_empty() {
                break;
            }

            for repo in &repos {
                let gh_repo = GitHubRepo {
                    owner: repo["owner"]["login"].as_str().unwrap_or("").to_string(),
                    name: repo["name"].as_str().unwrap_or("").to_string(),
                    default_branch: repo["default_branch"].as_str().unwrap_or("main").to_string(),
                    description: repo["description"].as_str().map(|s| s.to_string()),
                    clone_url: repo["clone_url"].as_str().unwrap_or("").to_string(),
                    archive_url: format!(
                        "https://api.github.com/repos/{}/{}/zipball/{}",
                        self.org,
                        repo["name"].as_str().unwrap_or(""),
                        repo["default_branch"].as_str().unwrap_or("main")
                    ),
                };
                all_repos.push(gh_repo);
            }

            page += 1;
        }

        Ok(all_repos)
    }

    pub async fn download_repo_archive(
        &self,
        repo: &GitHubRepo,
        dest_dir: &Path,
    ) -> Result<PathBuf> {
        let archive_url = format!(
            "https://github.com/{}/{}/archive/refs/heads/{}.zip",
            repo.owner, repo.name, repo.default_branch
        );

        let zip_name = format!("{}.zip", repo.name);
        let dest_path = dest_dir.join(&zip_name);

        tokio::fs::create_dir_all(dest_dir).await
            .map_err(|e| DownloadError::Other(anyhow::anyhow!("Failed to create dest dir: {e}")))?;

        self.accelerator
            .download_with_mirror_fallback(&archive_url, &dest_path, None)
            .await
    }
}
