use std::sync::atomic::{AtomicBool, AtomicU64};

use dashmap::DashMap;

pub struct State {
    url_traffic: DashMap<String, (u64, u64)>,
    http_traffic: AtomicU64,
    ws_traffic: AtomicU64,
    shutdown: AtomicBool,
}

impl State {
    pub fn new() -> Self {
        State {
            url_traffic: DashMap::new(),
            http_traffic: AtomicU64::new(0),
            ws_traffic: AtomicU64::new(0),
            shutdown: AtomicBool::new(false)
        }
    }

    pub fn update_sent(&self, url: &str, count: u64) {
        let mut entry = self.url_traffic.entry(url.to_string()).or_insert((0, 0));
        entry.0 += count;
        self.http_traffic.fetch_add(count, std::sync::atomic::Ordering::AcqRel);
    }

    pub fn update_received(&self, url: &str, count: u64) {
        let mut entry = self.url_traffic.entry(url.to_string()).or_insert((0, 0));
        entry.1 += count;
        self.http_traffic.fetch_add(count, std::sync::atomic::Ordering::AcqRel);
    }

    pub fn get_info(&self) -> &DashMap<String, (u64, u64)> {
        &self.url_traffic
    }

    pub fn shutdown(&self) {
        self.shutdown.store(true, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn is_shutdown(&self) -> bool {
        self.shutdown.load(std::sync::atomic::Ordering::Relaxed)
    }

    pub fn total_traffic(&self) -> u64 {
        self.http_traffic.load(std::sync::atomic::Ordering::Relaxed)
    }

    pub fn update_ws_traffic(&self, count: u64) {
        self.ws_traffic.fetch_add(count, std::sync::atomic::Ordering::AcqRel);
    }

    pub fn websocket_traffic(&self) -> u64 {
        self.ws_traffic.load(std::sync::atomic::Ordering::Relaxed)
    }
}