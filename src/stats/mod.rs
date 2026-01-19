//! Statistics

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;
use dashmap::DashMap;
use parking_lot::{RwLock, Mutex};
use lru::LruCache;
use std::num::NonZeroUsize;
use std::hash::{Hash, Hasher};
use std::collections::hash_map::DefaultHasher;

/// Thread-safe statistics
#[derive(Default)]
pub struct Stats {
    // Global counters
    connects_all: AtomicU64,
    connects_bad: AtomicU64,
    handshake_timeouts: AtomicU64,
    
    // Per-user stats
    user_stats: DashMap<String, UserStats>,
    
    // Start time
    start_time: RwLock<Option<Instant>>,
}

/// Per-user statistics
#[derive(Default)]
pub struct UserStats {
    pub connects: AtomicU64,
    pub curr_connects: AtomicU64,
    pub octets_from_client: AtomicU64,
    pub octets_to_client: AtomicU64,
    pub msgs_from_client: AtomicU64,
    pub msgs_to_client: AtomicU64,
}

impl Stats {
    pub fn new() -> Self {
        let stats = Self::default();
        *stats.start_time.write() = Some(Instant::now());
        stats
    }
    
    // Global stats
    pub fn increment_connects_all(&self) {
        self.connects_all.fetch_add(1, Ordering::Relaxed);
    }
    
    pub fn increment_connects_bad(&self) {
        self.connects_bad.fetch_add(1, Ordering::Relaxed);
    }
    
    pub fn increment_handshake_timeouts(&self) {
        self.handshake_timeouts.fetch_add(1, Ordering::Relaxed);
    }
    
    pub fn get_connects_all(&self) -> u64 {
        self.connects_all.load(Ordering::Relaxed)
    }
    
    pub fn get_connects_bad(&self) -> u64 {
        self.connects_bad.load(Ordering::Relaxed)
    }
    
    // User stats
    pub fn increment_user_connects(&self, user: &str) {
        self.user_stats
            .entry(user.to_string())
            .or_default()
            .connects
            .fetch_add(1, Ordering::Relaxed);
    }
    
    pub fn increment_user_curr_connects(&self, user: &str) {
        self.user_stats
            .entry(user.to_string())
            .or_default()
            .curr_connects
            .fetch_add(1, Ordering::Relaxed);
    }
    
    pub fn decrement_user_curr_connects(&self, user: &str) {
        if let Some(stats) = self.user_stats.get(user) {
            stats.curr_connects.fetch_sub(1, Ordering::Relaxed);
        }
    }
    
    pub fn get_user_curr_connects(&self, user: &str) -> u64 {
        self.user_stats
            .get(user)
            .map(|s| s.curr_connects.load(Ordering::Relaxed))
            .unwrap_or(0)
    }
    
    pub fn add_user_octets_from(&self, user: &str, bytes: u64) {
        self.user_stats
            .entry(user.to_string())
            .or_default()
            .octets_from_client
            .fetch_add(bytes, Ordering::Relaxed);
    }
    
    pub fn add_user_octets_to(&self, user: &str, bytes: u64) {
        self.user_stats
            .entry(user.to_string())
            .or_default()
            .octets_to_client
            .fetch_add(bytes, Ordering::Relaxed);
    }
    
    pub fn increment_user_msgs_from(&self, user: &str) {
        self.user_stats
            .entry(user.to_string())
            .or_default()
            .msgs_from_client
            .fetch_add(1, Ordering::Relaxed);
    }
    
    pub fn increment_user_msgs_to(&self, user: &str) {
        self.user_stats
            .entry(user.to_string())
            .or_default()
            .msgs_to_client
            .fetch_add(1, Ordering::Relaxed);
    }
    
    pub fn get_user_total_octets(&self, user: &str) -> u64 {
        self.user_stats
            .get(user)
            .map(|s| {
                s.octets_from_client.load(Ordering::Relaxed) +
                s.octets_to_client.load(Ordering::Relaxed)
            })
            .unwrap_or(0)
    }
    
    pub fn uptime_secs(&self) -> f64 {
        self.start_time.read()
            .map(|t| t.elapsed().as_secs_f64())
            .unwrap_or(0.0)
    }
}

/// Sharded Replay attack checker using LRU cache
/// Uses multiple independent LRU caches to reduce lock contention
pub struct ReplayChecker {
    shards: Vec<Mutex<LruCache<Vec<u8>, ()>>>,
    shard_mask: usize,
}

impl ReplayChecker {
    /// Create new replay checker with specified capacity per shard
    /// Total capacity = capacity * num_shards
    pub fn new(total_capacity: usize) -> Self {
        // Use 64 shards for good concurrency
        let num_shards = 64;
        let shard_capacity = (total_capacity / num_shards).max(1);
        let cap = NonZeroUsize::new(shard_capacity).unwrap();
        
        let mut shards = Vec::with_capacity(num_shards);
        for _ in 0..num_shards {
            shards.push(Mutex::new(LruCache::new(cap)));
        }
        
        Self {
            shards,
            shard_mask: num_shards - 1,
        }
    }
    
    fn get_shard(&self, key: &[u8]) -> usize {
        let mut hasher = DefaultHasher::new();
        key.hash(&mut hasher);
        (hasher.finish() as usize) & self.shard_mask
    }
    
    pub fn check_handshake(&self, data: &[u8]) -> bool {
        let shard_idx = self.get_shard(data);
        self.shards[shard_idx].lock().contains(&data.to_vec())
    }
    
    pub fn add_handshake(&self, data: &[u8]) {
        let shard_idx = self.get_shard(data);
        self.shards[shard_idx].lock().put(data.to_vec(), ());
    }
    
    pub fn check_tls_digest(&self, data: &[u8]) -> bool {
        let shard_idx = self.get_shard(data);
        self.shards[shard_idx].lock().contains(&data.to_vec())
    }
    
    pub fn add_tls_digest(&self, data: &[u8]) {
        let shard_idx = self.get_shard(data);
        self.shards[shard_idx].lock().put(data.to_vec(), ());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_stats_shared_counters() {
        let stats = Arc::new(Stats::new());
        
        let stats1 = Arc::clone(&stats);
        let stats2 = Arc::clone(&stats);
        
        stats1.increment_connects_all();
        stats2.increment_connects_all();
        stats1.increment_connects_all();
        
        assert_eq!(stats.get_connects_all(), 3);
    }
    
    #[test]
    fn test_replay_checker_sharding() {
        let checker = ReplayChecker::new(100);
        let data1 = b"test1";
        let data2 = b"test2";
        
        checker.add_handshake(data1);
        assert!(checker.check_handshake(data1));
        assert!(!checker.check_handshake(data2));
        
        checker.add_handshake(data2);
        assert!(checker.check_handshake(data2));
    }
}