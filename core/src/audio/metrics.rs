use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use std::sync::Arc;

pub struct AudioMetrics {
    pub reverse_frame_drops: AtomicU64,
    pub mic_backpressure_drops: AtomicU64,
    pub mixer_source_overflows: AtomicU64,
    pub mixer_gap_clears: AtomicU64,
    pub stall_over_20ms: AtomicU64,
    pub stall_over_50ms: AtomicU64,
    pub stall_over_100ms: AtomicU64,
    pub total_stall_ms: AtomicU64,
    pub callback_gap_gt_15ms: AtomicU64,
    pub callback_gap_gt_30ms: AtomicU64,
    pub push_bursts: AtomicU64,
    pub max_burst_len: AtomicU64,
}

impl AudioMetrics {
    pub fn new() -> Self {
        Self {
            reverse_frame_drops: AtomicU64::new(0),
            mic_backpressure_drops: AtomicU64::new(0),
            mixer_source_overflows: AtomicU64::new(0),
            mixer_gap_clears: AtomicU64::new(0),
            stall_over_20ms: AtomicU64::new(0),
            stall_over_50ms: AtomicU64::new(0),
            stall_over_100ms: AtomicU64::new(0),
            total_stall_ms: AtomicU64::new(0),
            callback_gap_gt_15ms: AtomicU64::new(0),
            callback_gap_gt_30ms: AtomicU64::new(0),
            push_bursts: AtomicU64::new(0),
            max_burst_len: AtomicU64::new(0),
        }
    }

    pub fn log_summary(&self) {
        log::info!(
            "audio_metrics: reverse_drops={} mic_drops={} mixer_overflows={} gap_clears={} stall_>20ms={} stall_>50ms={} stall_>100ms={} total_stall={}ms cb_gap_>15ms={} cb_gap_>30ms={} push_bursts={} max_burst_len={}",
            self.reverse_frame_drops.load(Relaxed),
            self.mic_backpressure_drops.load(Relaxed),
            self.mixer_source_overflows.load(Relaxed),
            self.mixer_gap_clears.load(Relaxed),
            self.stall_over_20ms.load(Relaxed),
            self.stall_over_50ms.load(Relaxed),
            self.stall_over_100ms.load(Relaxed),
            self.total_stall_ms.load(Relaxed),
            self.callback_gap_gt_15ms.load(Relaxed),
            self.callback_gap_gt_30ms.load(Relaxed),
            self.push_bursts.load(Relaxed),
            self.max_burst_len.load(Relaxed),
        );
    }
}

pub type SharedMetrics = Arc<AudioMetrics>;
