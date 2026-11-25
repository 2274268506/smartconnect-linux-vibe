//! Small RAM-only thumbnail LRU. Explicit refresh invalidates both entries and in-flight writers.
use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};
const BYTES: usize = 8 * 1024 * 1024;
const ENTRIES: usize = 96;
const TTL: Duration = Duration::from_secs(30);
struct Item {
    device: String,
    path: String,
    created: Instant,
    bytes: Vec<u8>,
}
#[derive(Default)]
pub struct ThumbnailCache {
    items: VecDeque<Item>,
    used: usize,
    epoch: u64,
}
impl ThumbnailCache {
    pub fn epoch(&self) -> u64 {
        self.epoch
    }
    pub fn clear(&mut self) {
        self.items.clear();
        self.used = 0;
        self.epoch = self.epoch.wrapping_add(1);
    }
    pub fn get(&mut self, device: &str, path: &str, now: Instant) -> Option<Vec<u8>> {
        self.items
            .retain(|item| now.saturating_duration_since(item.created) < TTL);
        self.used = self.items.iter().map(|item| item.bytes.len()).sum();
        let index = self
            .items
            .iter()
            .position(|item| item.device == device && item.path == path)?;
        let item = self.items.remove(index)?;
        let result = item.bytes.clone();
        self.items.push_back(item);
        Some(result)
    }
    pub fn insert(&mut self, epoch: u64, device: &str, path: &str, bytes: Vec<u8>, now: Instant) {
        if epoch != self.epoch || bytes.len() > BYTES {
            return;
        }
        if let Some(index) = self
            .items
            .iter()
            .position(|item| item.device == device && item.path == path)
        {
            self.used -= self.items.remove(index).unwrap().bytes.len();
        }
        while !self.items.is_empty()
            && (self.items.len() >= ENTRIES || self.used + bytes.len() > BYTES)
        {
            self.used -= self.items.pop_front().unwrap().bytes.len();
        }
        self.used += bytes.len();
        self.items.push_back(Item {
            device: device.into(),
            path: path.into(),
            created: now,
            bytes,
        });
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cache_is_device_scoped_expires_and_rejects_pre_refresh_results() {
        let mut cache = ThumbnailCache::default();
        let now = Instant::now();
        let epoch = cache.epoch();
        cache.insert(epoch, "phone-a", "/sdcard/a.jpg", vec![1, 2], now);
        assert_eq!(cache.get("phone-a", "/sdcard/a.jpg", now), Some(vec![1, 2]));
        assert!(cache.get("phone-b", "/sdcard/a.jpg", now).is_none());
        assert!(cache.get("phone-a", "/sdcard/a.jpg", now + TTL).is_none());
        cache.clear();
        cache.insert(epoch, "phone-a", "/sdcard/a.jpg", vec![3], now);
        assert!(cache.get("phone-a", "/sdcard/a.jpg", now).is_none());
    }
    #[test]
    fn cache_evicts_least_recently_used_and_obeys_byte_budget() {
        let mut cache = ThumbnailCache::default();
        let now = Instant::now();
        for i in 0..ENTRIES {
            cache.insert(0, "phone", &i.to_string(), vec![1], now);
        }
        cache.get("phone", "0", now);
        cache.insert(0, "phone", "extra", vec![2], now);
        assert!(cache.get("phone", "0", now).is_some());
        assert!(cache.get("phone", "1", now).is_none());
        cache.insert(0, "phone", "large", vec![0; BYTES], now);
        assert_eq!(cache.items.len(), 1);
        assert_eq!(cache.used, BYTES);
    }
}
