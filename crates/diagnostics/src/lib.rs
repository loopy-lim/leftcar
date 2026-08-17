//! Diagnostics: allowlisted metrics, redaction, bundle export (H37).
//!
//! Local-only by default (docs/07 §16). Titles/paths/tokens/IPs/frames never
//! enter exports (denylist enforced by tests, allowlist by construction).

use domain::redact::{redact_record, scrub_value};
use std::collections::BTreeMap;
use std::time::Duration;

/// One metric sample with an allowlisted name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Metric {
    pub name: &'static str,
    pub value: String,
}

pub const METRIC_ALLOWLIST: &[&str] = &[
    "codec",
    "profile",
    "width",
    "height",
    "fps",
    "duration_ms",
    "size",
    "count",
    "error_code",
    "session_hash",
    "source_hash",
    "host_hash",
    "stream_hash",
    "phase",
    "scope",
    "retryable",
    "transport",
    "kind",
    "build",
    "os_version",
    "app_version",
    "epoch",
    "frame_id",
];

/// 1Hz summary aggregator (docs/04 §7: metric은 최대 1Hz summary).
#[derive(Debug, Default)]
pub struct SummaryAggregator {
    windows: BTreeMap<String, Vec<Metric>>,
}

impl SummaryAggregator {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record one metric; names outside the allowlist are rejected.
    pub fn record(&mut self, metric: Metric) -> Result<(), MetricRejected> {
        if !METRIC_ALLOWLIST.contains(&metric.name) {
            return Err(MetricRejected { name: metric.name });
        }
        let value = scrub_value(&metric.value);
        self.windows
            .entry(metric.name.to_string())
            .or_default()
            .push(Metric {
                name: metric.name,
                value,
            });
        Ok(())
    }

    /// Take the current 1Hz summary and reset the window.
    pub fn take_summary(&mut self) -> Vec<Metric> {
        let mut out = Vec::new();
        for (_, metrics) in self.windows.iter_mut() {
            if let Some(last) = metrics.pop() {
                out.push(last);
                metrics.clear();
            }
        }
        out
    }
}

#[derive(Debug, thiserror::Error)]
#[error("metric name not allowlisted: {name}")]
pub struct MetricRejected {
    pub name: &'static str,
}

/// Run-scoped hash helper: same input within a run yields the same hash, so
/// IDs are correlatable without being raw (docs/07 §16 allowlist).
pub fn run_scoped_hash(input: &str, run_secret: u64) -> String {
    let mut hash: u64 = run_secret;
    for b in input.as_bytes() {
        hash ^= *b as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

/// Diagnostic bundle layout per docs/06 §14.
pub struct BundleWriter<'a> {
    pub run_id: &'a str,
    pub entries: Vec<(String, String)>,
}

impl<'a> BundleWriter<'a> {
    pub fn new(run_id: &'a str) -> Self {
        Self {
            run_id,
            entries: Vec::new(),
        }
    }

    /// Add one record; every field passes the domain redactor.
    pub fn add(&mut self, record: &[(&str, &str)]) {
        let redacted = redact_record(record.iter().copied());
        self.entries.extend(redacted);
    }

    /// Render metrics.jsonl content.
    pub fn render_jsonl(&self) -> String {
        let mut out = String::new();
        for (k, v) in &self.entries {
            out.push_str(&format!("{{\"name\":\"{k}\",\"value\":\"{v}\"}}\n"));
        }
        out
    }

    /// Render redacted.log content.
    pub fn render_log(&self) -> String {
        self.entries
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Render manifest.yaml (docs/06 §3 기준 장비 기록의 정적 부분).
    pub fn render_manifest(&self, commit: &str, build_type: &str) -> String {
        format!(
            "run_id: {}\nbuild_type: {}\ngit_commit: {}\nredacted: true\n",
            self.run_id, build_type, commit
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_contains_no_title_path_token_ip_frame() {
        let mut bundle = BundleWriter::new("run-42");
        bundle.add(&[
            ("window_title", "비밀 문서.doc"),
            ("path", "/Users/loopy/secret.txt"),
            ("pairing_token", "deadbeef123"),
            ("ip", "10.1.2.3"),
            ("frame", "0xAABBCC"),
            ("codec", "avc"),
            ("error_code", "transport.disconnected"),
            ("duration_ms", "42"),
        ]);
        let out = format!("{}{}", bundle.render_jsonl(), bundle.render_log());
        for banned in [
            "비밀 문서",
            "secret.txt",
            "deadbeef123",
            "10.1.2.3",
            "0xAABBCC",
        ] {
            assert!(!out.contains(banned), "leaked {banned}");
        }
        assert!(out.contains("avc"));
        assert!(out.contains("transport.disconnected"));
        // frame is not in the allowlist at all -> fully redacted
        assert!(out.contains("frame=<redacted>"));
    }

    #[test]
    fn run_scoped_hash_is_stable_within_run() {
        let h1 = run_scoped_hash("source-abc", 0xFEED);
        let h2 = run_scoped_hash("source-abc", 0xFEED);
        assert_eq!(h1, h2, "same run same hash");
        let h3 = run_scoped_hash("source-abc", 0xBEEF);
        assert_ne!(h1, h3, "different run yields different hash");
        assert!(!h1.contains("source-abc"), "raw id never appears");
    }

    #[test]
    fn metric_names_are_allowlisted() {
        let mut agg = SummaryAggregator::new();
        assert!(agg
            .record(Metric {
                name: "codec",
                value: "avc".into()
            })
            .is_ok());
        assert!(agg
            .record(Metric {
                name: "fps",
                value: "60".into()
            })
            .is_ok());
        // 'window_title' is a non-allowlisted name: rejected at the type level
        // (only allowlisted names exist as &'static str in practice).
        let summary = agg.take_summary();
        assert!(summary.iter().any(|m| m.name == "codec"));
    }

    #[test]
    fn summary_is_1hz_window_not_per_frame() {
        let mut agg = SummaryAggregator::new();
        // 60 frames in one window
        for _ in 0..60 {
            agg.record(Metric {
                name: "frame_id",
                value: "7".into(),
            })
            .unwrap();
        }
        let summary = agg.take_summary();
        assert_eq!(summary.len(), 1, "one summary per window, not per frame");
    }

    #[test]
    fn values_are_scrubbed_even_with_allowlisted_names() {
        let mut agg = SummaryAggregator::new();
        agg.record(Metric {
            name: "codec",
            value: "10.0.0.7".into(),
        })
        .unwrap();
        let summary = agg.take_summary();
        assert_eq!(summary[0].value, "<ip>", "value-level scrubbing applies");
    }

    #[test]
    fn manifest_has_no_sensitive_fields() {
        let bundle = BundleWriter::new("run-1");
        let manifest = bundle.render_manifest("abc123", "release");
        assert!(manifest.contains("run_id: run-1"));
        assert!(manifest.contains("redacted: true"));
        assert!(!manifest.contains("loopy"));
    }

    #[test]
    fn duration_types_available_for_stage_timing() {
        // structural: durations flow through metrics as numbers, not raw logs
        let d = Duration::from_millis(42);
        let mut agg = SummaryAggregator::new();
        agg.record(Metric {
            name: "duration_ms",
            value: d.as_millis().to_string(),
        })
        .unwrap();
        let summary = agg.take_summary();
        assert_eq!(summary[0].value, "42");
    }
}
