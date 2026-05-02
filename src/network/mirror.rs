use std::sync::Arc;
use parking_lot::RwLock;

#[derive(Debug, Clone)]
pub struct Mirror {
    pub url: String,
    pub priority: usize,
    pub enabled: bool,
}

pub struct MirrorManager {
    mirrors: Arc<RwLock<Vec<Mirror>>>,
    fail_count: Arc<dashmap::DashMap<String, u32>>,
}

impl MirrorManager {
    pub fn new(urls: Vec<String>) -> Self {
        let mirrors: Vec<Mirror> = urls
            .into_iter()
            .enumerate()
            .map(|(i, url)| Mirror {
                url,
                priority: i,
                enabled: true,
            })
            .collect();

        Self {
            mirrors: Arc::new(RwLock::new(mirrors)),
            fail_count: Arc::new(dashmap::DashMap::new()),
        }
    }

    pub fn get_mirrors(&self) -> Vec<String> {
        let mirrors = self.mirrors.read();
        let mut sorted: Vec<_> = mirrors
            .iter()
            .filter(|m| m.enabled)
            .collect();
        sorted.sort_by_key(|m| {
            let fails = self.fail_count.get(&m.url).map(|v| *v).unwrap_or(0);
            (fails, m.priority)
        });
        sorted.into_iter().map(|m| m.url.clone()).collect()
    }

    pub fn record_success(&self, mirror_url: &str) {
        self.fail_count.remove(mirror_url);
    }

    pub fn record_failure(&self, mirror_url: &str) {
        *self.fail_count.entry(mirror_url.to_string()).or_insert(0) += 1;
    }

    pub fn disable_mirror(&self, mirror_url: &str) {
        let mut mirrors = self.mirrors.write();
        if let Some(m) = mirrors.iter_mut().find(|m| m.url == mirror_url) {
            m.enabled = false;
        }
    }

    pub fn enable_mirror(&self, mirror_url: &str) {
        let mut mirrors = self.mirrors.write();
        if let Some(m) = mirrors.iter_mut().find(|m| m.url == mirror_url) {
            m.enabled = true;
            self.fail_count.remove(mirror_url);
        }
    }

    pub fn is_any_enabled(&self) -> bool {
        self.mirrors.read().iter().any(|m| m.enabled)
    }
}
