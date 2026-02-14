//! Congestion estimation and adaptive bitrate control.
//!
//! `CongestionEstimator` consumes `ReceiverReport` data from the media control
//! channel and produces `QualityRecommendation`s that pipelines use to adjust
//! encoder bitrates.

use std::time::{Duration, Instant};

/// Quality recommendation produced by the congestion estimator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QualityRecommendation {
    /// Target bitrate in bits per second.
    pub target_bitrate_bps: u32,
}

/// Configuration for the congestion estimator.
#[derive(Debug, Clone)]
pub struct CongestionConfig {
    /// Minimum allowed bitrate (bps).
    pub min_bitrate_bps: u32,
    /// Maximum allowed bitrate (bps).
    pub max_bitrate_bps: u32,
    /// Initial bitrate (bps).
    pub initial_bitrate_bps: u32,
    /// Loss rate threshold above which to reduce bitrate (fraction, e.g. 0.05 = 5%).
    pub loss_reduce_threshold: f64,
    /// Loss rate threshold below which to increase bitrate (fraction, e.g. 0.01 = 1%).
    pub loss_increase_threshold: f64,
    /// Duration of sustained low loss required before increasing bitrate.
    pub increase_delay: Duration,
    /// Factor to multiply bitrate by when reducing (e.g. 0.8 = reduce by 20%).
    pub reduce_factor: f64,
    /// Factor to multiply bitrate by when increasing (e.g. 1.1 = increase by 10%).
    pub increase_factor: f64,
}

impl Default for CongestionConfig {
    fn default() -> Self {
        Self {
            min_bitrate_bps: 32_000,
            max_bitrate_bps: 2_000_000,
            initial_bitrate_bps: 500_000,
            loss_reduce_threshold: 0.05,
            loss_increase_threshold: 0.01,
            increase_delay: Duration::from_secs(10),
            reduce_factor: 0.8,
            increase_factor: 1.1,
        }
    }
}

/// Congestion estimator that tracks network conditions and recommends bitrate adjustments.
///
/// Feed it `ReceiverReport` data via [`on_report`](Self::on_report) and it will
/// return a [`QualityRecommendation`] when the bitrate should change.
pub struct CongestionEstimator {
    config: CongestionConfig,
    current_bitrate_bps: u32,
    /// Current packet loss rate (0.0 to 1.0).
    loss_rate: f64,
    /// Current RTT estimate in milliseconds.
    rtt_ms: u16,
    /// Current jitter in milliseconds.
    jitter_ms: u32,
    /// When loss first went below the increase threshold.
    low_loss_since: Option<Instant>,
    /// Total reports received.
    reports_received: u64,
}

impl CongestionEstimator {
    /// Create a new estimator with the given configuration.
    pub fn new(config: CongestionConfig) -> Self {
        let initial = config.initial_bitrate_bps;
        tracing::info!(
            initial_bitrate = initial,
            min = config.min_bitrate_bps,
            max = config.max_bitrate_bps,
            "congestion estimator created"
        );
        Self {
            config,
            current_bitrate_bps: initial,
            loss_rate: 0.0,
            rtt_ms: 0,
            jitter_ms: 0,
            low_loss_since: None,
            reports_received: 0,
        }
    }

    /// Process a receiver report and return a quality recommendation if the bitrate should change.
    ///
    /// The `now` parameter allows tests to control time.
    pub fn on_report(
        &mut self,
        packets_received: u32,
        packets_lost: u32,
        jitter: u32,
        rtt_estimate_ms: u16,
        now: Instant,
    ) -> Option<QualityRecommendation> {
        self.reports_received += 1;
        self.rtt_ms = rtt_estimate_ms;
        self.jitter_ms = jitter;

        // Calculate loss rate
        let total = packets_received.saturating_add(packets_lost);
        self.loss_rate = if total > 0 {
            packets_lost as f64 / total as f64
        } else {
            0.0
        };

        tracing::debug!(
            target: "bisc_media::congestion",
            loss_rate = self.loss_rate,
            rtt_ms = self.rtt_ms,
            jitter_ms = self.jitter_ms,
            current_bitrate = self.current_bitrate_bps,
            "processed receiver report"
        );

        let old_bitrate = self.current_bitrate_bps;

        if self.loss_rate > self.config.loss_reduce_threshold {
            // High loss: reduce bitrate immediately
            self.low_loss_since = None;
            let new = (self.current_bitrate_bps as f64 * self.config.reduce_factor) as u32;
            self.current_bitrate_bps =
                new.clamp(self.config.min_bitrate_bps, self.config.max_bitrate_bps);

            tracing::debug!(
                target: "bisc_media::congestion",
                old_bitrate,
                new_bitrate = self.current_bitrate_bps,
                loss_rate = self.loss_rate,
                "reducing bitrate due to high loss"
            );
        } else if self.loss_rate < self.config.loss_increase_threshold {
            // Low loss: track sustained period before increasing
            match self.low_loss_since {
                None => {
                    self.low_loss_since = Some(now);
                }
                Some(since) => {
                    if now.duration_since(since) >= self.config.increase_delay {
                        let new =
                            (self.current_bitrate_bps as f64 * self.config.increase_factor) as u32;
                        self.current_bitrate_bps =
                            new.clamp(self.config.min_bitrate_bps, self.config.max_bitrate_bps);
                        // Reset timer so we don't increase every tick
                        self.low_loss_since = Some(now);

                        tracing::debug!(
                            target: "bisc_media::congestion",
                            old_bitrate,
                            new_bitrate = self.current_bitrate_bps,
                            "increasing bitrate due to sustained low loss"
                        );
                    }
                }
            }
        } else {
            // Loss between thresholds: hold steady, reset increase timer
            self.low_loss_since = None;
        }

        if self.current_bitrate_bps != old_bitrate {
            Some(QualityRecommendation {
                target_bitrate_bps: self.current_bitrate_bps,
            })
        } else {
            None
        }
    }

    /// Get the current recommended bitrate.
    pub fn current_bitrate_bps(&self) -> u32 {
        self.current_bitrate_bps
    }

    /// Get the last measured loss rate (0.0 to 1.0).
    pub fn loss_rate(&self) -> f64 {
        self.loss_rate
    }

    /// Get the last measured RTT in milliseconds.
    pub fn rtt_ms(&self) -> u16 {
        self.rtt_ms
    }

    /// Get the last measured jitter in milliseconds.
    pub fn jitter_ms(&self) -> u32 {
        self.jitter_ms
    }

    /// Total number of reports processed.
    pub fn reports_received(&self) -> u64 {
        self.reports_received
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn high_loss_reduces_bitrate() {
        let config = CongestionConfig {
            initial_bitrate_bps: 500_000,
            ..CongestionConfig::default()
        };
        let mut estimator = CongestionEstimator::new(config);
        let now = Instant::now();

        // 10% loss (above 5% threshold)
        let rec = estimator.on_report(90, 10, 20, 50, now);
        assert!(rec.is_some());
        // 500_000 * 0.8 = 400_000
        assert_eq!(rec.unwrap().target_bitrate_bps, 400_000);
        assert_eq!(estimator.current_bitrate_bps(), 400_000);
    }

    #[test]
    fn repeated_high_loss_keeps_reducing() {
        let config = CongestionConfig {
            initial_bitrate_bps: 500_000,
            ..CongestionConfig::default()
        };
        let mut estimator = CongestionEstimator::new(config);
        let now = Instant::now();

        estimator.on_report(50, 50, 100, 200, now);
        assert_eq!(estimator.current_bitrate_bps(), 400_000);

        estimator.on_report(50, 50, 100, 200, now + Duration::from_secs(1));
        assert_eq!(estimator.current_bitrate_bps(), 320_000);

        estimator.on_report(50, 50, 100, 200, now + Duration::from_secs(2));
        assert_eq!(estimator.current_bitrate_bps(), 256_000);
    }

    #[test]
    fn sustained_low_loss_increases_bitrate() {
        let config = CongestionConfig {
            initial_bitrate_bps: 500_000,
            increase_delay: Duration::from_secs(10),
            ..CongestionConfig::default()
        };
        let mut estimator = CongestionEstimator::new(config);
        let now = Instant::now();

        // First report: starts timer
        let rec = estimator.on_report(100, 0, 10, 30, now);
        assert!(rec.is_none());

        // After 5 seconds: still too early
        let rec = estimator.on_report(100, 0, 10, 30, now + Duration::from_secs(5));
        assert!(rec.is_none());

        // After 10 seconds: should increase
        let rec = estimator.on_report(100, 0, 10, 30, now + Duration::from_secs(10));
        assert!(rec.is_some());
        // 500_000 * 1.1 = 550_000
        assert_eq!(rec.unwrap().target_bitrate_bps, 550_000);
    }

    #[test]
    fn bitrate_clamped_to_min() {
        let config = CongestionConfig {
            initial_bitrate_bps: 40_000,
            min_bitrate_bps: 32_000,
            ..CongestionConfig::default()
        };
        let mut estimator = CongestionEstimator::new(config);
        let now = Instant::now();

        // High loss: 40000 * 0.8 = 32000
        estimator.on_report(50, 50, 100, 200, now);
        assert_eq!(estimator.current_bitrate_bps(), 32_000);

        // Already at min, further reduction clamped → no change
        let rec = estimator.on_report(50, 50, 100, 200, now + Duration::from_secs(1));
        assert!(rec.is_none());
        assert_eq!(estimator.current_bitrate_bps(), 32_000);
    }

    #[test]
    fn bitrate_clamped_to_max() {
        let config = CongestionConfig {
            initial_bitrate_bps: 1_900_000,
            max_bitrate_bps: 2_000_000,
            increase_delay: Duration::from_secs(1),
            ..CongestionConfig::default()
        };
        let mut estimator = CongestionEstimator::new(config);
        let now = Instant::now();

        // Low loss to start timer
        estimator.on_report(100, 0, 5, 20, now);

        // After delay: 1_900_000 * 1.1 = 2_090_000 → clamped to 2_000_000
        let rec = estimator.on_report(100, 0, 5, 20, now + Duration::from_secs(1));
        assert!(rec.is_some());
        assert_eq!(estimator.current_bitrate_bps(), 2_000_000);

        // Already at max → no change
        let rec = estimator.on_report(100, 0, 5, 20, now + Duration::from_secs(2));
        assert!(rec.is_none());
        assert_eq!(estimator.current_bitrate_bps(), 2_000_000);
    }

    #[test]
    fn rtt_and_jitter_tracked() {
        let mut estimator = CongestionEstimator::new(CongestionConfig::default());
        let now = Instant::now();

        estimator.on_report(100, 2, 35, 75, now);
        assert_eq!(estimator.rtt_ms(), 75);
        assert_eq!(estimator.jitter_ms(), 35);
        // loss = 2 / 102 ≈ 0.0196
        assert!((estimator.loss_rate() - 2.0 / 102.0).abs() < 0.001);
        assert_eq!(estimator.reports_received(), 1);

        estimator.on_report(200, 5, 40, 80, now + Duration::from_secs(1));
        assert_eq!(estimator.rtt_ms(), 80);
        assert_eq!(estimator.jitter_ms(), 40);
        assert_eq!(estimator.reports_received(), 2);
    }

    #[test]
    fn medium_loss_resets_increase_timer() {
        let config = CongestionConfig {
            initial_bitrate_bps: 500_000,
            increase_delay: Duration::from_secs(5),
            ..CongestionConfig::default()
        };
        let mut estimator = CongestionEstimator::new(config);
        let now = Instant::now();

        // Start with low loss at t=0
        estimator.on_report(100, 0, 10, 30, now);

        // At t=3: medium loss (3%) resets timer
        estimator.on_report(97, 3, 10, 30, now + Duration::from_secs(3));

        // At t=6: low loss again, restarts timer
        estimator.on_report(100, 0, 10, 30, now + Duration::from_secs(6));

        // At t=10: only 4 seconds since restart, not enough
        let rec = estimator.on_report(100, 0, 10, 30, now + Duration::from_secs(10));
        assert!(rec.is_none());

        // At t=11: 5 seconds since restart
        let rec = estimator.on_report(100, 0, 10, 30, now + Duration::from_secs(11));
        assert!(rec.is_some());
        assert_eq!(rec.unwrap().target_bitrate_bps, 550_000);
    }

    #[test]
    fn encoder_bitrate_changes_with_recommendation() {
        // Integration test: verify that OpusEncoder actually changes bitrate
        // when a recommendation is applied.
        use crate::opus_codec::OpusEncoder;

        let config = CongestionConfig {
            initial_bitrate_bps: 64_000,
            min_bitrate_bps: 16_000,
            max_bitrate_bps: 128_000,
            ..CongestionConfig::default()
        };
        let mut estimator = CongestionEstimator::new(config);
        let mut encoder = OpusEncoder::new(1, 64_000).unwrap();
        let now = Instant::now();

        // Verify initial bitrate
        let initial = encoder.bitrate().unwrap();
        assert_eq!(initial, 64_000);

        // High loss → reduce: 64_000 * 0.8 = 51_200
        if let Some(rec) = estimator.on_report(80, 20, 50, 100, now) {
            encoder.set_bitrate(rec.target_bitrate_bps).unwrap();
        }

        let current = encoder.bitrate().unwrap();
        assert_eq!(current, 51_200);
    }

    #[test]
    fn zero_packets_no_crash() {
        let mut estimator = CongestionEstimator::new(CongestionConfig::default());
        let now = Instant::now();

        // Edge case: zero packets
        let rec = estimator.on_report(0, 0, 0, 0, now);
        assert!(rec.is_none());
        assert!((estimator.loss_rate() - 0.0).abs() < f64::EPSILON);
    }
}
