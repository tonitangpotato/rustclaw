//! Tool call frequency and duration tracking.
//!
//! Provides a thread-safe tracker that records tool invocations
//! and exposes snapshot queries for the dashboard.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::RwLock;

use serde::Serialize;

/// Per-tool statistics (interior-mutable via atomics).
#[derive(Debug, Default)]
struct ToolStat {
    call_count: AtomicU64,
    total_duration_ms: AtomicU64,
}

/// Thread-safe aggregate tracker for tool call statistics.
#[derive(Debug, Default)]
pub struct ToolStatsTracker {
    stats: RwLock<HashMap<String, ToolStat>>,
}

/// Point-in-time snapshot of a single tool's stats (serialisable).
#[derive(Serialize, Clone, Debug)]
pub struct ToolStatSnapshot {
    pub name: String,
    pub call_count: u64,
    pub avg_duration_ms: f64,
    pub total_duration_ms: u64,
}

impl ToolStatsTracker {
    pub fn new() -> Self {
        Self {
            stats: RwLock::new(HashMap::new()),
        }
    }

    /// Record a tool call with its duration in milliseconds.
    ///
    /// Uses a write lock for simplicity — tool calls are not extremely
    /// high frequency so contention is negligible.
    pub fn record(&self, tool_name: &str, duration_ms: u64) {
        let mut map = self.stats.write().expect("ToolStatsTracker lock poisoned");
        let stat = map
            .entry(tool_name.to_string())
            .or_insert_with(ToolStat::default);
        stat.call_count.fetch_add(1, Ordering::Relaxed);
        stat.total_duration_ms.fetch_add(duration_ms, Ordering::Relaxed);
    }

    /// Return the top N tools by call count (descending).
    pub fn top_n(&self, n: usize) -> Vec<ToolStatSnapshot> {
        let mut all = self.all_stats();
        all.sort_by(|a, b| b.call_count.cmp(&a.call_count));
        all.truncate(n);
        all
    }

    /// Return snapshots for every tracked tool.
    pub fn all_stats(&self) -> Vec<ToolStatSnapshot> {
        let map = self.stats.read().expect("ToolStatsTracker lock poisoned");
        map.iter()
            .map(|(name, stat)| {
                let call_count = stat.call_count.load(Ordering::Relaxed);
                let total_duration_ms = stat.total_duration_ms.load(Ordering::Relaxed);
                let avg_duration_ms = if call_count > 0 {
                    total_duration_ms as f64 / call_count as f64
                } else {
                    0.0
                };
                ToolStatSnapshot {
                    name: name.clone(),
                    call_count,
                    avg_duration_ms,
                    total_duration_ms,
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn test_record_and_retrieve() {
        let tracker = ToolStatsTracker::new();
        tracker.record("read_file", 100);
        tracker.record("read_file", 200);
        tracker.record("read_file", 300);

        let stats = tracker.all_stats();
        assert_eq!(stats.len(), 1);
        let s = &stats[0];
        assert_eq!(s.name, "read_file");
        assert_eq!(s.call_count, 3);
        assert_eq!(s.total_duration_ms, 600);
        assert!((s.avg_duration_ms - 200.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_top_n() {
        let tracker = ToolStatsTracker::new();

        // "exec" — 5 calls
        for _ in 0..5 {
            tracker.record("exec", 10);
        }
        // "read_file" — 3 calls
        for _ in 0..3 {
            tracker.record("read_file", 20);
        }
        // "write_file" — 1 call
        tracker.record("write_file", 50);

        // top_n(2) should return the top 2 by call_count descending
        let top2 = tracker.top_n(2);
        assert_eq!(top2.len(), 2);
        assert_eq!(top2[0].name, "exec");
        assert_eq!(top2[0].call_count, 5);
        assert_eq!(top2[1].name, "read_file");
        assert_eq!(top2[1].call_count, 3);

        // top_n(10) with only 3 tools should return all 3
        let top10 = tracker.top_n(10);
        assert_eq!(top10.len(), 3);
    }

    #[test]
    fn test_empty() {
        let tracker = ToolStatsTracker::new();
        let result = tracker.top_n(10);
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn test_concurrent_recording() {
        let tracker = Arc::new(ToolStatsTracker::new());
        let num_tasks = 10;
        let calls_per_task = 100;

        let mut handles = Vec::new();
        for _ in 0..num_tasks {
            let t = Arc::clone(&tracker);
            handles.push(tokio::spawn(async move {
                for _ in 0..calls_per_task {
                    t.record("concurrent_tool", 5);
                }
            }));
        }

        for h in handles {
            h.await.unwrap();
        }

        let stats = tracker.top_n(10);
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].name, "concurrent_tool");
        assert_eq!(stats[0].call_count, (num_tasks * calls_per_task) as u64);
        assert_eq!(
            stats[0].total_duration_ms,
            (num_tasks * calls_per_task * 5) as u64
        );
        assert!((stats[0].avg_duration_ms - 5.0).abs() < f64::EPSILON);
    }
}
