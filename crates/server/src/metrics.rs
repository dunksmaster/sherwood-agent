//! A tiny Prometheus text exposition — no `prometheus` crate, just counters.
//!
//! Enough to see the server is alive and how it is answering: total requests,
//! a breakdown by status class, and process uptime. Mode and kill-switch
//! gauges are added by the handler, which can read the control state.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

/// Process-wide request counters. Cheap to update from a middleware.
pub struct Metrics {
    started: Instant,
    requests_total: AtomicU64,
    /// Indexed by `status / 100` (1xx..5xx → 0..4); index 5 catches anything else.
    by_class: [AtomicU64; 6],
}

impl Default for Metrics {
    fn default() -> Self {
        Self {
            started: Instant::now(),
            requests_total: AtomicU64::new(0),
            by_class: Default::default(),
        }
    }
}

impl Metrics {
    pub fn record(&self, status: u16) {
        self.requests_total.fetch_add(1, Ordering::Relaxed);
        let idx = ((status / 100) as usize).wrapping_sub(1);
        self.by_class[idx.min(5)].fetch_add(1, Ordering::Relaxed);
    }

    pub fn uptime_secs(&self) -> u64 {
        self.started.elapsed().as_secs()
    }

    /// Render the base metrics. `extra` lines (mode / kill-switch gauges) are
    /// appended by the caller.
    pub fn render(&self, extra: &str) -> String {
        let total = self.requests_total.load(Ordering::Relaxed);
        let mut out = String::with_capacity(512);
        out.push_str("# HELP sherwood_requests_total HTTP requests handled.\n");
        out.push_str("# TYPE sherwood_requests_total counter\n");
        out.push_str(&format!("sherwood_requests_total {total}\n"));
        out.push_str("# HELP sherwood_responses_total HTTP responses by status class.\n");
        out.push_str("# TYPE sherwood_responses_total counter\n");
        for (i, c) in self.by_class.iter().enumerate() {
            let class = match i {
                0 => "1xx",
                1 => "2xx",
                2 => "3xx",
                3 => "4xx",
                4 => "5xx",
                _ => "other",
            };
            out.push_str(&format!(
                "sherwood_responses_total{{class=\"{class}\"}} {}\n",
                c.load(Ordering::Relaxed)
            ));
        }
        out.push_str("# HELP sherwood_uptime_seconds Seconds since server start.\n");
        out.push_str("# TYPE sherwood_uptime_seconds gauge\n");
        out.push_str(&format!(
            "sherwood_uptime_seconds {}\n",
            self.started.elapsed().as_secs()
        ));
        out.push_str(extra);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_and_renders() {
        let m = Metrics::default();
        m.record(200);
        m.record(204);
        m.record(404);
        m.record(500);
        let text = m.render("sherwood_kill_switch 0\n");
        assert!(text.contains("sherwood_requests_total 4"));
        assert!(text.contains("sherwood_responses_total{class=\"2xx\"} 2"));
        assert!(text.contains("sherwood_responses_total{class=\"4xx\"} 1"));
        assert!(text.contains("sherwood_responses_total{class=\"5xx\"} 1"));
        assert!(text.contains("sherwood_kill_switch 0"));
    }
}
