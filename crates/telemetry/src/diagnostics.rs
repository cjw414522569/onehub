//! Local performance sampling and user-exportable diagnostics (T148).
//!
//! [`DiagnosticsSampler`] records numeric performance samples for the
//! network, parse, render, and memory paths. [`DiagnosticReport::export`]
//! produces a versioned, user-exportable JSON report of aggregates
//! (count/mean/p50/p95/p99/min/max). The API accepts **only numbers**, so
//! the exported diagnostics can locate bottlenecks without ever exposing
//! content: no hostnames, commands, terminal text, or payloads.

/// The diagnostics report schema version.
pub const DIAGNOSTIC_SCHEMA_VERSION: u32 = 1;

/// The metric categories the sampler covers (fixed labels only).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum DiagnosticMetric {
    /// Network round-trip latency (ms).
    NetworkLatencyMs,
    /// Terminal parse throughput (MB/s).
    ParseThroughputMbps,
    /// Render frame time (ms).
    RenderFrameMs,
    /// Steady-state memory (KB).
    MemoryKb,
}

impl DiagnosticMetric {
    /// The stable label used in reports.
    pub fn label(self) -> &'static str {
        match self {
            DiagnosticMetric::NetworkLatencyMs => "network_latency_ms",
            DiagnosticMetric::ParseThroughputMbps => "parse_throughput_mbps",
            DiagnosticMetric::RenderFrameMs => "render_frame_ms",
            DiagnosticMetric::MemoryKb => "memory_kb",
        }
    }
}

/// A per-metric sample set.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SampleSet {
    values: Vec<u64>,
}

impl SampleSet {
    /// Records a sample.
    pub fn record(&mut self, value: u64) {
        self.values.push(value);
    }

    /// The number of samples.
    pub fn count(&self) -> usize {
        self.values.len()
    }

    /// The arithmetic mean (0 when empty).
    pub fn mean(&self) -> f64 {
        if self.values.is_empty() {
            return 0.0;
        }
        self.values.iter().sum::<u64>() as f64 / self.values.len() as f64
    }

    /// A percentile (linear interpolation), or 0 when empty.
    pub fn percentile(&self, pct: f64) -> u64 {
        if self.values.is_empty() {
            return 0;
        }
        let mut sorted = self.values.clone();
        sorted.sort_unstable();
        let index = ((sorted.len() - 1) as f64 * pct / 100.0).round() as usize;
        sorted[index.min(sorted.len() - 1)]
    }

    fn min(&self) -> u64 {
        self.values.iter().copied().min().unwrap_or(0)
    }

    fn max(&self) -> u64 {
        self.values.iter().copied().max().unwrap_or(0)
    }
}

/// The local performance sampler.
#[derive(Debug, Clone, Default)]
pub struct DiagnosticsSampler {
    /// Samples by metric.
    pub samples: std::collections::BTreeMap<DiagnosticMetric, SampleSet>,
}

impl DiagnosticsSampler {
    /// A fresh sampler.
    pub fn new() -> Self {
        Self::default()
    }

    /// Records a network round-trip latency sample (ms).
    pub fn record_network_latency(&mut self, ms: u64) {
        self.samples
            .entry(DiagnosticMetric::NetworkLatencyMs)
            .or_default()
            .record(ms);
    }

    /// Records a parse throughput sample (MB/s).
    pub fn record_parse_throughput(&mut self, mbps: u64) {
        self.samples
            .entry(DiagnosticMetric::ParseThroughputMbps)
            .or_default()
            .record(mbps);
    }

    /// Records a render frame-time sample (ms).
    pub fn record_render_frame(&mut self, ms: u64) {
        self.samples
            .entry(DiagnosticMetric::RenderFrameMs)
            .or_default()
            .record(ms);
    }

    /// Records a memory sample (KB).
    pub fn record_memory(&mut self, kb: u64) {
        self.samples
            .entry(DiagnosticMetric::MemoryKb)
            .or_default()
            .record(kb);
    }
}

/// One aggregate row of the exported report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReportRow {
    /// The metric label (fixed, never user content).
    pub metric: &'static str,
    /// Sample count.
    pub count: usize,
    /// Mean (rounded).
    pub mean: u64,
    /// P50.
    pub p50: u64,
    /// P95.
    pub p95: u64,
    /// P99.
    pub p99: u64,
    /// Min.
    pub min: u64,
    /// Max.
    pub max: u64,
}

/// A user-exportable diagnostic report (numbers only, no content).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticReport {
    /// Schema version.
    pub schema_version: u32,
    /// Aggregate rows.
    pub rows: Vec<ReportRow>,
}

impl DiagnosticReport {
    /// Exports the sampler as a versioned report. Only fixed metric labels
    /// and numeric aggregates are included; the API never accepted content.
    pub fn export(sampler: &DiagnosticsSampler) -> Self {
        let rows = sampler
            .samples
            .iter()
            .map(|(metric, set)| ReportRow {
                metric: metric.label(),
                count: set.count(),
                mean: set.mean().round() as u64,
                p50: set.percentile(50.0),
                p95: set.percentile(95.0),
                p99: set.percentile(99.0),
                min: set.min(),
                max: set.max(),
            })
            .collect();
        Self {
            schema_version: DIAGNOSTIC_SCHEMA_VERSION,
            rows,
        }
    }

    /// Renders the report as versioned JSON.
    pub fn to_json(&self) -> String {
        let rows = self
            .rows
            .iter()
            .map(|row| {
                format!(
                    "{{\"metric\":\"{}\",\"count\":{},\"mean\":{},\"p50\":{},\"p95\":{},\"p99\":{},\"min\":{},\"max\":{}}}",
                    row.metric, row.count, row.mean, row.p50, row.p95, row.p99, row.min, row.max
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "{{\"schema_version\":{},\"rows\":[{}]}}",
            self.schema_version, rows
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{
        DiagnosticMetric, DiagnosticReport, DiagnosticsSampler, DIAGNOSTIC_SCHEMA_VERSION,
    };

    #[test]
    fn sampling_aggregates_are_correct() {
        let mut sampler = DiagnosticsSampler::new();
        for ms in [10u64, 20, 30, 40, 50] {
            sampler.record_network_latency(ms);
        }
        let report = DiagnosticReport::export(&sampler);
        assert_eq!(report.schema_version, DIAGNOSTIC_SCHEMA_VERSION);
        let row = report
            .rows
            .iter()
            .find(|row| row.metric == DiagnosticMetric::NetworkLatencyMs.label())
            .unwrap();
        assert_eq!(row.count, 5);
        assert_eq!(row.mean, 30);
        assert_eq!(row.p50, 30);
        assert_eq!(row.min, 10);
        assert_eq!(row.max, 50);
    }

    #[test]
    fn percentile_edge_cases() {
        let mut sampler = DiagnosticsSampler::new();
        sampler.record_render_frame(16);
        sampler.record_render_frame(17);
        let report = DiagnosticReport::export(&sampler);
        let row = report
            .rows
            .iter()
            .find(|row| row.metric == DiagnosticMetric::RenderFrameMs.label())
            .unwrap();
        assert_eq!(row.p50, 17);
        assert_eq!(row.p95, 17);
        assert_eq!(row.p99, 17);
    }

    #[test]
    fn report_contains_only_numbers_and_fixed_labels() {
        let mut sampler = DiagnosticsSampler::new();
        for kb in [120_000u64, 130_000, 140_000] {
            sampler.record_memory(kb);
        }
        sampler.record_parse_throughput(200);
        sampler.record_render_frame(9);
        sampler.record_network_latency(40);
        let json = DiagnosticReport::export(&sampler).to_json();
        // No content can appear: the only strings are the fixed labels and
        // numeric values.
        for marker in [
            "host",
            "command",
            "user",
            "token",
            "secret",
            "terminal_text",
            "payload",
        ] {
            assert!(!json.contains(marker), "report must not contain {marker}");
        }
        assert!(json.contains("network_latency_ms") || json.contains("parse_throughput_mbps"));
        assert!(json.contains("render_frame_ms"));
        assert!(json.contains("memory_kb"));
    }

    #[test]
    fn empty_sampler_exports_empty_report() {
        let json = DiagnosticReport::export(&DiagnosticsSampler::new()).to_json();
        assert!(json.contains("\"rows\":[]"));
    }
}
