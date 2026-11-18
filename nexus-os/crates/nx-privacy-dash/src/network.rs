//! A network-per-app view model (bytes, destinations, blocked vs allowed) fed with samples.
//!
//! Aggregates network traffic metrics without running an active capture engine.

use crate::consent_store::AppIdentifier;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Verdict on network packet / connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum TrafficDisposition {
    Allowed,
    BlockedTracker,
    BlockedTelemetry,
    BlockedUserRule,
}

impl TrafficDisposition {
    pub fn is_blocked(&self) -> bool {
        !matches!(self, Self::Allowed)
    }

    pub fn is_telemetry_or_tracker(&self) -> bool {
        matches!(self, Self::BlockedTracker | Self::BlockedTelemetry)
    }
}

/// Category of destination endpoint.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Default,
)]
pub enum NetworkCategory {
    Telemetry,
    Analytics,
    Ads,
    Content,
    System,
    #[default]
    Unknown,
}

/// A discrete network activity sample.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetworkSample {
    pub app: AppIdentifier,
    pub timestamp_ms: u64,
    pub remote_addr: String,
    pub domain: Option<String>,
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub disposition: TrafficDisposition,
    pub category: NetworkCategory,
}

impl NetworkSample {
    pub fn total_bytes(&self) -> u64 {
        self.bytes_sent.saturating_add(self.bytes_received)
    }
}

/// Destination endpoint statistics for an app.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DestinationStats {
    pub destination: String,
    pub domain: Option<String>,
    pub total_bytes: u64,
    pub connection_count: u64,
    pub category: NetworkCategory,
    pub blocked_count: u64,
}

/// Aggregated network metrics for a single app over a time window.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppNetworkSummary {
    pub app: AppIdentifier,
    pub total_bytes_sent: u64,
    pub total_bytes_received: u64,
    pub total_bytes: u64,
    pub allowed_bytes: u64,
    pub blocked_bytes: u64,
    pub allowed_count: u64,
    pub blocked_count: u64,
    pub top_destinations: Vec<DestinationStats>,
    pub telemetry_blocked_pct: f32,
}

/// Time window filter for traffic aggregation.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Default,
)]
pub enum TimeWindow {
    #[default]
    Last5Minutes,
    LastHour,
    Last24Hours,
    AllTime,
}

impl TimeWindow {
    pub fn duration_ms(&self) -> Option<u64> {
        match self {
            Self::Last5Minutes => Some(5 * 60 * 1000),
            Self::LastHour => Some(60 * 60 * 1000),
            Self::Last24Hours => Some(24 * 60 * 60 * 1000),
            Self::AllTime => None,
        }
    }
}

/// A bucket in a time series (e.g. for sparklines).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetworkTimeBucket {
    pub start_ms: u64,
    pub end_ms: u64,
    pub allowed_bytes: u64,
    pub blocked_bytes: u64,
}

/// In-memory view model aggregating network samples by application.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NetworkPerAppViewModel {
    samples: Vec<NetworkSample>,
}

impl NetworkPerAppViewModel {
    pub fn new() -> Self {
        Self {
            samples: Vec::new(),
        }
    }

    /// Feed a collection of network samples into the view model.
    pub fn ingest_samples(&mut self, samples: impl IntoIterator<Item = NetworkSample>) {
        self.samples.extend(samples);
    }

    /// Ingest a single sample.
    pub fn ingest_sample(&mut self, sample: NetworkSample) {
        self.samples.push(sample);
    }

    /// Clear all recorded samples.
    pub fn clear(&mut self) {
        self.samples.clear();
    }

    /// Total sample count.
    pub fn len(&self) -> usize {
        self.samples.len()
    }

    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    /// Filter samples based on time window relative to `now_ms`.
    fn filtered_samples(
        &self,
        window: TimeWindow,
        now_ms: u64,
    ) -> impl Iterator<Item = &NetworkSample> {
        let cutoff = window.duration_ms().map(|dur| now_ms.saturating_sub(dur));
        self.samples.iter().filter(move |s| match cutoff {
            Some(t) => s.timestamp_ms >= t && s.timestamp_ms <= now_ms,
            None => true,
        })
    }

    /// Produce sorted summaries for all apps in the selected time window.
    pub fn summaries(&self, window: TimeWindow, now_ms: u64) -> Vec<AppNetworkSummary> {
        let mut app_map: BTreeMap<String, (AppIdentifier, Vec<&NetworkSample>)> = BTreeMap::new();

        for sample in self.filtered_samples(window, now_ms) {
            let key = sample.app.identifier().to_string();
            app_map
                .entry(key)
                .or_insert_with(|| (sample.app.clone(), Vec::new()))
                .1
                .push(sample);
        }

        let mut results = Vec::new();

        for (_, (app, app_samples)) in app_map {
            let mut sent = 0u64;
            let mut recv = 0u64;
            let mut allowed_bytes = 0u64;
            let mut blocked_bytes = 0u64;
            let mut allowed_count = 0u64;
            let mut blocked_count = 0u64;

            let mut dest_map: BTreeMap<String, (Option<String>, u64, u64, NetworkCategory, u64)> =
                BTreeMap::new();

            for s in app_samples {
                let sample_bytes = s.total_bytes();
                sent = sent.saturating_add(s.bytes_sent);
                recv = recv.saturating_add(s.bytes_received);

                if s.disposition.is_blocked() {
                    blocked_bytes = blocked_bytes.saturating_add(sample_bytes);
                    blocked_count += 1;
                } else {
                    allowed_bytes = allowed_bytes.saturating_add(sample_bytes);
                    allowed_count += 1;
                }

                let dest_key = s.domain.clone().unwrap_or_else(|| s.remote_addr.clone());

                let entry =
                    dest_map
                        .entry(dest_key)
                        .or_insert((s.domain.clone(), 0, 0, s.category, 0));
                entry.1 = entry.1.saturating_add(sample_bytes);
                entry.2 += 1;
                if s.disposition.is_blocked() {
                    entry.4 += 1;
                }
            }

            let total_bytes = sent.saturating_add(recv);
            let telemetry_blocked_pct = if total_bytes > 0 {
                (blocked_bytes as f32 / total_bytes as f32) * 100.0
            } else {
                0.0
            };

            let mut top_destinations: Vec<DestinationStats> = dest_map
                .into_iter()
                .map(
                    |(dest, (domain, bytes, conns, cat, blocked))| DestinationStats {
                        destination: dest,
                        domain,
                        total_bytes: bytes,
                        connection_count: conns,
                        category: cat,
                        blocked_count: blocked,
                    },
                )
                .collect();

            top_destinations.sort_by_key(|a| std::cmp::Reverse(a.total_bytes));
            top_destinations.truncate(5);

            results.push(AppNetworkSummary {
                app,
                total_bytes_sent: sent,
                total_bytes_received: recv,
                total_bytes,
                allowed_bytes,
                blocked_bytes,
                allowed_count,
                blocked_count,
                top_destinations,
                telemetry_blocked_pct,
            });
        }

        // Default sort by total bytes descending
        results.sort_by_key(|a| std::cmp::Reverse(a.total_bytes));
        results
    }

    /// Global telemetry and tracker block rate percentage across all apps in window.
    pub fn global_telemetry_blocked_pct(&self, window: TimeWindow, now_ms: u64) -> f32 {
        let mut total = 0u64;
        let mut blocked = 0u64;

        for s in self.filtered_samples(window, now_ms) {
            let bytes = s.total_bytes();
            total = total.saturating_add(bytes);
            if s.disposition.is_telemetry_or_tracker() {
                blocked = blocked.saturating_add(bytes);
            }
        }

        if total > 0 {
            (blocked as f32 / total as f32) * 100.0
        } else {
            0.0
        }
    }

    /// Global total blocked telemetry bytes saved.
    pub fn global_telemetry_blocked_bytes(&self, window: TimeWindow, now_ms: u64) -> u64 {
        self.filtered_samples(window, now_ms)
            .filter(|s| s.disposition.is_telemetry_or_tracker())
            .map(|s| s.total_bytes())
            .sum()
    }

    /// Time series buckets for rendering sparkline charts.
    pub fn time_buckets(
        &self,
        window: TimeWindow,
        num_buckets: usize,
        now_ms: u64,
    ) -> Vec<NetworkTimeBucket> {
        let duration = window.duration_ms().unwrap_or(24 * 60 * 60 * 1000);
        let start_time = now_ms.saturating_sub(duration);
        let bucket_size = (duration / (num_buckets as u64)).max(1);

        let mut buckets = Vec::with_capacity(num_buckets);
        for i in 0..num_buckets {
            let b_start = start_time + (i as u64 * bucket_size);
            let b_end = b_start + bucket_size;
            buckets.push(NetworkTimeBucket {
                start_ms: b_start,
                end_ms: b_end,
                allowed_bytes: 0,
                blocked_bytes: 0,
            });
        }

        for s in self.filtered_samples(window, now_ms) {
            if s.timestamp_ms < start_time || s.timestamp_ms > now_ms {
                continue;
            }
            let offset = s.timestamp_ms.saturating_sub(start_time);
            let idx = ((offset / bucket_size) as usize).min(num_buckets.saturating_sub(1));
            let bytes = s.total_bytes();

            if s.disposition.is_blocked() {
                buckets[idx].blocked_bytes = buckets[idx].blocked_bytes.saturating_add(bytes);
            } else {
                buckets[idx].allowed_bytes = buckets[idx].allowed_bytes.saturating_add(bytes);
            }
        }

        buckets
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn network_per_app_aggregation() {
        let mut vm = NetworkPerAppViewModel::new();
        let now_ms = 1_000_000;

        let app1 = AppIdentifier::from_desktop_path("C:\\Chrome\\chrome.exe");
        let app2 = AppIdentifier::from_desktop_path("C:\\Telemetry\\diagtrack.exe");

        vm.ingest_samples(vec![
            NetworkSample {
                app: app1.clone(),
                timestamp_ms: now_ms - 1000,
                remote_addr: "142.250.190.46:443".into(),
                domain: Some("google.com".into()),
                bytes_sent: 500,
                bytes_received: 2000,
                disposition: TrafficDisposition::Allowed,
                category: NetworkCategory::Content,
            },
            NetworkSample {
                app: app1.clone(),
                timestamp_ms: now_ms - 500,
                remote_addr: "142.250.190.46:443".into(),
                domain: Some("google.com".into()),
                bytes_sent: 100,
                bytes_received: 400,
                disposition: TrafficDisposition::Allowed,
                category: NetworkCategory::Content,
            },
            NetworkSample {
                app: app2.clone(),
                timestamp_ms: now_ms - 200,
                remote_addr: "20.190.159.0:443".into(),
                domain: Some("telemetry.microsoft.com".into()),
                bytes_sent: 1000,
                bytes_received: 0,
                disposition: TrafficDisposition::BlockedTelemetry,
                category: NetworkCategory::Telemetry,
            },
        ]);

        let summaries = vm.summaries(TimeWindow::Last5Minutes, now_ms);
        assert_eq!(summaries.len(), 2);

        // App 1 should be first (3000 bytes vs 1000 bytes)
        assert_eq!(summaries[0].app, app1);
        assert_eq!(summaries[0].total_bytes, 3000);
        assert_eq!(summaries[0].allowed_bytes, 3000);
        assert_eq!(summaries[0].blocked_bytes, 0);
        assert_eq!(summaries[0].telemetry_blocked_pct, 0.0);
        assert_eq!(summaries[0].top_destinations.len(), 1);
        assert_eq!(summaries[0].top_destinations[0].destination, "google.com");

        // App 2 should be second (1000 bytes, 100% blocked telemetry)
        assert_eq!(summaries[1].app, app2);
        assert_eq!(summaries[1].total_bytes, 1000);
        assert_eq!(summaries[1].blocked_bytes, 1000);
        assert_eq!(summaries[1].telemetry_blocked_pct, 100.0);

        // Global stats: 1000 blocked out of 4000 total = 25%
        assert_eq!(
            vm.global_telemetry_blocked_bytes(TimeWindow::Last5Minutes, now_ms),
            1000
        );
        assert_eq!(
            vm.global_telemetry_blocked_pct(TimeWindow::Last5Minutes, now_ms),
            25.0
        );
    }

    #[test]
    fn sparkline_time_buckets() {
        let mut vm = NetworkPerAppViewModel::new();
        let now_ms = 300_000; // 5 minutes = 300,000 ms

        let app = AppIdentifier::from_desktop_path("C:\\App\\app.exe");
        vm.ingest_sample(NetworkSample {
            app,
            timestamp_ms: 100_000,
            remote_addr: "1.1.1.1:53".into(),
            domain: None,
            bytes_sent: 50,
            bytes_received: 150,
            disposition: TrafficDisposition::Allowed,
            category: NetworkCategory::System,
        });

        let buckets = vm.time_buckets(TimeWindow::Last5Minutes, 5, now_ms);
        assert_eq!(buckets.len(), 5);
        let total_allowed: u64 = buckets.iter().map(|b| b.allowed_bytes).sum();
        assert_eq!(total_allowed, 200);
    }

    #[test]
    fn time_window_filtering() {
        let mut vm = NetworkPerAppViewModel::new();
        let now_ms = 1_000_000_000;
        let ten_mins_ago = now_ms - (10 * 60 * 1000);
        let two_mins_ago = now_ms - (2 * 60 * 1000);

        let app = AppIdentifier::from_desktop_path("C:\\App\\app.exe");

        // Old sample (10 mins ago)
        vm.ingest_sample(NetworkSample {
            app: app.clone(),
            timestamp_ms: ten_mins_ago,
            remote_addr: "1.2.3.4:443".into(),
            domain: Some("old.example.com".into()),
            bytes_sent: 1000,
            bytes_received: 1000,
            disposition: TrafficDisposition::Allowed,
            category: NetworkCategory::Content,
        });

        // Recent sample (2 mins ago)
        vm.ingest_sample(NetworkSample {
            app: app.clone(),
            timestamp_ms: two_mins_ago,
            remote_addr: "5.6.7.8:443".into(),
            domain: Some("recent.example.com".into()),
            bytes_sent: 500,
            bytes_received: 500,
            disposition: TrafficDisposition::Allowed,
            category: NetworkCategory::Content,
        });

        // In Last 5 Minutes: only recent sample is included
        let sum_5m = vm.summaries(TimeWindow::Last5Minutes, now_ms);
        assert_eq!(sum_5m.len(), 1);
        assert_eq!(sum_5m[0].total_bytes, 1000);
        assert_eq!(
            sum_5m[0].top_destinations[0].destination,
            "recent.example.com"
        );

        // In Last Hour: both samples are included
        let sum_1h = vm.summaries(TimeWindow::LastHour, now_ms);
        assert_eq!(sum_1h.len(), 1);
        assert_eq!(sum_1h[0].total_bytes, 3000);
    }

    #[test]
    fn empty_view_model() {
        let vm = NetworkPerAppViewModel::new();
        assert!(vm.is_empty());
        assert_eq!(vm.len(), 0);
        assert!(vm.summaries(TimeWindow::Last5Minutes, 1000).is_empty());
        assert_eq!(
            vm.global_telemetry_blocked_bytes(TimeWindow::Last5Minutes, 1000),
            0
        );
        assert_eq!(
            vm.global_telemetry_blocked_pct(TimeWindow::Last5Minutes, 1000),
            0.0
        );
    }
}
