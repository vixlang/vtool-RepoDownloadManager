use crate::config::DownloadConfig;
use std::net::{IpAddr, SocketAddr, ToSocketAddrs};
use std::sync::Arc;
use std::time::{Duration, Instant};
use dashmap::DashMap;
use hickory_resolver::config::ResolverConfig;
use hickory_resolver::name_server::TokioConnectionProvider;
use hickory_resolver::Resolver;
use parking_lot::RwLock;

#[derive(Clone)]
struct DnsEntry {
    ips: Vec<IpAddr>,
    resolved_at: Instant,
    ttl: Duration,
}

impl DnsEntry {
    fn is_expired(&self) -> bool {
        self.resolved_at.elapsed() >= self.ttl
    }
}

pub struct DnsCache {
    cache: Arc<DashMap<String, DnsEntry>>,
    resolver: RwLock<Option<Resolver<TokioConnectionProvider>>>,
    ttl: Duration,
}

impl DnsCache {
    pub fn new(config: &DownloadConfig) -> Self {
        Self {
            cache: Arc::new(DashMap::new()),
            resolver: RwLock::new(None),
            ttl: config.dns_cache_ttl,
        }
    }

    pub async fn resolve(&self, host: &str, port: u16) -> Result<Vec<SocketAddr>, crate::error::DownloadError> {
        if let Some(entry) = self.cache.get(host) {
            if !entry.is_expired() {
                return Ok(entry.ips.iter().map(|ip| SocketAddr::new(*ip, port)).collect());
            }
        }

        let resolver = {
            let lock = self.resolver.read();
            lock.clone()
        };

        let ips = if let Some(resolver) = resolver {
            let response = resolver.lookup_ip(host).await
                .map_err(|e| crate::error::DownloadError::Dns(format!("DNS resolve failed for {host}: {e}")))?;
            response.iter().collect()
        } else {
            let addrs: Vec<_> = format!("{host}:{port}")
                .to_socket_addrs()
                .map_err(|e| crate::error::DownloadError::Dns(format!("DNS resolve failed for {host}: {e}")))?
                .map(|a| a.ip())
                .collect();

            if addrs.is_empty() {
                return Err(crate::error::DownloadError::Dns(format!("No IPs found for {host}")));
            }
            addrs
        };

        self.cache.insert(
            host.to_string(),
            DnsEntry {
                ips: ips.clone(),
                resolved_at: Instant::now(),
                ttl: self.ttl,
            },
        );

        Ok(ips.into_iter().map(|ip| SocketAddr::new(ip, port)).collect())
    }

    pub async fn init_resolver(&self) {
        let mut lock = self.resolver.write();
        if lock.is_none() {
            let resolver = Resolver::builder_with_config(
                ResolverConfig::quad9(),
                TokioConnectionProvider::default(),
            ).build();
            *lock = Some(resolver);
        }
    }

    pub async fn pre_resolve_hosts(&self, hosts: &[&str], port: u16) {
        let tasks: Vec<_> = hosts
            .iter()
            .map(|host| {
                let host = host.to_string();
                let cache = self.cache.clone();
                let ttl = self.ttl;
                async move {
                    if cache.contains_key(&host) {
                        return;
                    }
                    if let Ok(ips) = format!("{host}:{port}")
                        .to_socket_addrs()
                        .map(|a| a.map(|s| s.ip()).collect::<Vec<_>>())
                    {
                        cache.insert(
                            host,
                            DnsEntry {
                                ips,
                                resolved_at: Instant::now(),
                                ttl,
                            },
                        );
                    }
                }
            })
            .collect();

        futures::future::join_all(tasks).await;
    }

    pub fn stats(&self) -> DnsStats {
        let total = self.cache.len();
        let expired = self.cache.iter().filter(|e| e.is_expired()).count();
        DnsStats {
            cached_entries: total,
            expired_entries: expired,
        }
    }
}

#[derive(Debug, Clone)]
pub struct DnsStats {
    pub cached_entries: usize,
    pub expired_entries: usize,
}
