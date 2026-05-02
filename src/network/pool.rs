use crate::config::DownloadConfig;
use reqwest::Client;
use std::time::Duration;

pub fn build_http_client(config: &DownloadConfig) -> reqwest::Result<Client> {
    let mut client_builder = Client::builder()
        .http2_adaptive_window(config.http2_adaptive_window)
        .tcp_nodelay(config.tcp_nodelay)
        .connect_timeout(config.connect_timeout)
        .timeout(config.request_timeout)
        .pool_max_idle_per_host(config.pool_max_idle_per_host)
        .pool_idle_timeout(config.pool_idle_timeout)
        .http2_keep_alive_timeout(config.keep_alive_timeout)
        .http2_keep_alive_interval(Duration::from_secs(30))
        .http2_keep_alive_while_idle(true)
        .brotli(true)
        .gzip(true)
        .deflate(true)
        .user_agent(format!(
            "VixRepoDownloadManager/{}",
            env!("CARGO_PKG_VERSION")
        ))
        .redirect(reqwest::redirect::Policy::limited(10));

    if let Some(token) = &config.github_token {
        client_builder = client_builder.default_headers({
            let mut headers = reqwest::header::HeaderMap::new();
            let auth_value = format!("Bearer {token}");
            if let Ok(header_val) = reqwest::header::HeaderValue::from_str(&auth_value) {
                headers.insert(reqwest::header::AUTHORIZATION, header_val);
            }
            headers
        });
    }

    client_builder.build()
}
