use lru::LruCache;
use parking_lot::Mutex;
use std::num::NonZeroUsize;
use std::sync::Arc;

#[derive(Clone)]
pub struct TlsSessionCache {
    cache: Arc<Mutex<LruCache<String, Vec<u8>>>>,
}

impl TlsSessionCache {
    pub fn new(capacity: usize) -> Self {
        let cap = NonZeroUsize::new(capacity).unwrap_or(NonZeroUsize::new(32).unwrap());
        Self {
            cache: Arc::new(Mutex::new(LruCache::new(cap))),
        }
    }

    pub fn store(&self, host: &str, session_data: Vec<u8>) {
        let mut cache = self.cache.lock();
        cache.put(host.to_string(), session_data);
    }

    pub fn load(&self, host: &str) -> Option<Vec<u8>> {
        let mut cache = self.cache.lock();
        cache.get(host).cloned()
    }

    pub fn invalidate(&self, host: &str) {
        let mut cache = self.cache.lock();
        cache.pop(host);
    }

    pub fn invalidate_all(&self) {
        let mut cache = self.cache.lock();
        cache.clear();
    }
}

impl Default for TlsSessionCache {
    fn default() -> Self {
        Self::new(32)
    }
}
