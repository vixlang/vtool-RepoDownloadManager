use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::sync::Arc;
use tracing::info;

use vix_repodownloadmanager::config::DownloadConfig;
use vix_repodownloadmanager::github::GitHubClient;
use vix_repodownloadmanager::network::NetworkAccelerator;

#[derive(Parser)]
#[command(name = "vix-dl")]
#[command(about = "Vix 统一下载管理器 - 网络超级加速引擎", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    #[command(about = "下载 vixlang 组织下的所有仓库")]
    Sync {
        #[arg(short, long, default_value = "vixlang")]
        org: String,

        #[arg(short, long, default_value = "./repos")]
        output: PathBuf,

        #[arg(short, long, default_value = "16")]
        concurrency: usize,

        #[arg(short, long)]
        github_token: Option<String>,
    },

    #[command(about = "下载单个文件（带加速）")]
    Download {
        #[arg(short, long)]
        url: String,

        #[arg(short, long, default_value = ".")]
        output: PathBuf,

        #[arg(short, long, default_value = "16")]
        concurrency: usize,
    },

    #[command(about = "预热网络连接")]
    Warmup,

    #[command(about = "显示网络加速状态")]
    Status,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Warmup => {
            let config = DownloadConfig::default();
            let accel = NetworkAccelerator::new(config)
                .expect("Failed to create network accelerator");

            info!("正在预热网络连接...");
            accel.pre_warm().await;
            info!("网络预热完成！");
        }

        Commands::Status => {
            let config = DownloadConfig::default();
            let accel = NetworkAccelerator::new(config)
                .expect("Failed to create network accelerator");

            accel.pre_warm().await;
            let stats = accel.dns_cache.stats();
            info!("=== 网络加速器状态 ===");
            info!("DNS 缓存条目: {}", stats.cached_entries);
            info!("DNS 过期条目: {}", stats.expired_entries);
            info!("镜像源数量: {}", accel.mirror_manager.get_mirrors().len());
            info!("=====================");
        }

        Commands::Sync {
            org,
            output,
            concurrency,
            github_token,
        } => {
            let mut config = DownloadConfig::default();
            config.max_concurrency = concurrency;
            config.github_token = github_token;

            let accel = Arc::new(NetworkAccelerator::new(config.clone())
                .expect("Failed to create network accelerator"));

            info!("正在预热网络...");
            accel.pre_warm().await;

            let github = GitHubClient::new(Arc::clone(&accel), &org);

            info!("正在获取 {} 组织的仓库列表...", org);
            let repos = github.list_org_repos().await
                .expect("Failed to fetch repo list");

            info!("发现 {} 个仓库，开始下载...", repos.len());

            let semaphore = Arc::new(tokio::sync::Semaphore::new(concurrency));
            let mut handles = Vec::new();

            for repo in repos {
                let github = GitHubClient::new(Arc::clone(&accel), &org);
                let output = output.clone();
                let permit = Arc::clone(&semaphore);

                let handle = tokio::spawn(async move {
                    let _permit = permit.acquire().await;
                    info!("下载 {}/{} ...", repo.owner, repo.name);
                    match github.download_repo_archive(&repo, &output).await {
                        Ok(path) => info!("✓ {}/{} -> {}", repo.owner, repo.name, path.display()),
                        Err(e) => tracing::error!("✗ {}/{} : {}", repo.owner, repo.name, e),
                    }
                });

                handles.push(handle);
            }

            for handle in handles {
                let _ = handle.await;
            }

            info!("所有仓库下载完成！");
        }

        Commands::Download {
            url,
            output,
            concurrency,
        } => {
            let mut config = DownloadConfig::default();
            config.max_concurrency = concurrency;

            let accel = Arc::new(NetworkAccelerator::new(config)
                .expect("Failed to create network accelerator"));

            accel.pre_warm().await;

            let filename = url.split('/').last().unwrap_or("download");
            let dest = output.join(filename);

            info!("开始下载: {url}");
            info!("目标: {}", dest.display());

            match accel.download_with_mirror_fallback(&url, &dest, None).await {
                Ok(path) => info!("下载完成: {}", path.display()),
                Err(e) => tracing::error!("下载失败: {e}"),
            }
        }
    }
}
