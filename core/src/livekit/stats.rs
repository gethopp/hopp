use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};

use livekit::track::{LocalTrack, TrackSource};
use livekit::webrtc::stats::RtcStats;

use crate::room_service::RoomServiceInner;

const WARMUP: Duration = Duration::from_secs(10);
const QUALITY_WINDOW: Duration = Duration::from_secs(5);
const STALL_SAMPLES: u8 = 3;
const LIMIT_SAMPLES: u8 = 5;
const MIN_DROP_FRAMES: u64 = 30;
const MAX_DROP_RATIO: f64 = 0.20;
const MIN_FREEZE_DURATION: f64 = 2.0;

#[derive(Debug, Clone, Default)]
pub struct RoomStats {
    pub screenshare_fps: f64,
    pub screenshare_width: u32,
    pub screenshare_height: u32,
    pub screenshare_codec_id: String,
    pub screenshare_jitter_buffer_delay: f64,
    pub screenshare_input_bps: f64,
    pub total_input_bps: f64,
    pub total_output_bps: f64,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct VideoHealthSummary {
    started_at: Option<Instant>,
    inbound_observed: bool,
    inbound_frames_received: u64,
    inbound_frames_decoded: u64,
    inbound_frames_dropped: u64,
    inbound_freeze_duration: f64,
    max_decoder_stall_seconds: u8,
    outbound_source_frames: u64,
    outbound_observed: bool,
    outbound_frames_encoded: u64,
    outbound_quality_limited_seconds: f64,
    max_encoder_stall_seconds: u8,
    quality_limitation_reasons: BTreeSet<String>,
    codecs: BTreeSet<String>,
    implementations: BTreeSet<String>,
    alerts: BTreeSet<String>,
}

impl VideoHealthSummary {
    pub(crate) fn log(&self) {
        let duration = self
            .started_at
            .map(|started_at| started_at.elapsed().as_secs())
            .unwrap_or_default();
        log::info!(
            "VideoHealthSummary Call: duration_s={duration} alerts={:?} codecs={:?} implementations={:?}",
            self.alerts,
            self.codecs,
            self.implementations,
        );
        if self.inbound_observed {
            log::info!(
                "VideoHealthSummary Inbound: received={} decoded={} dropped={} freeze_duration_s={:.1} max_stall_s={}",
                self.inbound_frames_received,
                self.inbound_frames_decoded,
                self.inbound_frames_dropped,
                self.inbound_freeze_duration,
                self.max_decoder_stall_seconds,
            );
        }
        if self.outbound_observed {
            log::info!(
                "VideoHealthSummary Outbound: source={} encoded={} quality_limited_s={:.1} reasons={:?} max_stall_s={}",
                self.outbound_source_frames,
                self.outbound_frames_encoded,
                self.outbound_quality_limited_seconds,
                self.quality_limitation_reasons,
                self.max_encoder_stall_seconds
            );
        }
    }
}

#[derive(Default)]
struct CumulativeCounters {
    screenshare_inbound_bytes: u64,
    total_inbound_bytes: u64,
    total_outbound_bytes: u64,
    screenshare_jitter_buffer_delay: f64,
    screenshare_jitter_buffer_emitted_count: u64,
}

#[derive(Clone)]
struct InboundSample {
    id: String,
    codec: String,
    decoder: String,
    frames_received: u64,
    frames_decoded: u64,
    frames_dropped: u64,
    freeze_duration: f64,
}

#[derive(Clone)]
struct OutboundSample {
    id: String,
    codec: String,
    encoder: String,
    source_frames: u64,
    frames_encoded: u64,
    quality_reason: String,
    quality_limited_seconds: f64,
}

struct InboundState {
    started_at: Instant,
    previous: InboundSample,
    stall_samples: u8,
    window: VecDeque<(Instant, u64, u64, u64, f64)>,
}

struct OutboundState {
    started_at: Instant,
    previous: OutboundSample,
    stall_samples: u8,
    limitation_samples: u8,
    limitation_reason: String,
}

struct HealthEvent {
    issue: &'static str,
    tags: BTreeMap<String, String>,
    metrics: BTreeMap<String, String>,
}

struct HealthMonitor {
    inbound: Option<InboundState>,
    outbound: Option<OutboundState>,
    reported: HashSet<&'static str>,
    summary: VideoHealthSummary,
}

impl HealthMonitor {
    fn new() -> Self {
        Self {
            inbound: None,
            outbound: None,
            reported: HashSet::new(),
            summary: VideoHealthSummary {
                started_at: Some(Instant::now()),
                ..Default::default()
            },
        }
    }

    fn update(
        &mut self,
        inbound: Option<InboundSample>,
        outbound: Option<OutboundSample>,
        now: Instant,
    ) {
        let mut events = Vec::new();
        match inbound {
            Some(sample) => self.update_inbound(sample, now, &mut events),
            None => self.inbound = None,
        }
        match outbound {
            Some(sample) => self.update_outbound(sample, now, &mut events),
            None => self.outbound = None,
        }

        for event in events {
            if self.reported.insert(event.issue) {
                self.summary.alerts.insert(event.issue.to_string());
                sentry_utils::video_quality_event(event.issue, event.tags, event.metrics);
            }
        }
    }

    fn update_inbound(
        &mut self,
        sample: InboundSample,
        now: Instant,
        events: &mut Vec<HealthEvent>,
    ) {
        let needs_reset = self
            .inbound
            .as_ref()
            .map(|state| {
                state.previous.id != sample.id
                    || sample.frames_received < state.previous.frames_received
                    || sample.frames_decoded < state.previous.frames_decoded
                    || sample.frames_dropped < state.previous.frames_dropped
            })
            .unwrap_or(true);

        if needs_reset {
            self.summary.inbound_observed = true;
            self.summary.inbound_frames_received += sample.frames_received;
            self.summary.inbound_frames_decoded += sample.frames_decoded;
            self.summary.inbound_frames_dropped += sample.frames_dropped;
            self.summary.inbound_freeze_duration += sample.freeze_duration;
            self.record_inbound_labels(&sample);
            self.inbound = Some(InboundState {
                started_at: now,
                previous: sample.clone(),
                stall_samples: 0,
                window: VecDeque::from([(
                    now,
                    sample.frames_received,
                    sample.frames_decoded,
                    sample.frames_dropped,
                    sample.freeze_duration,
                )]),
            });
            return;
        }

        let state = self.inbound.as_mut().unwrap();
        let received = sample.frames_received - state.previous.frames_received;
        let decoded = sample.frames_decoded - state.previous.frames_decoded;
        let dropped = sample.frames_dropped - state.previous.frames_dropped;
        let freeze_duration = (sample.freeze_duration - state.previous.freeze_duration).max(0.0);

        self.summary.inbound_frames_received += received;
        self.summary.inbound_frames_decoded += decoded;
        self.summary.inbound_frames_dropped += dropped;
        self.summary.inbound_freeze_duration += freeze_duration;

        let warmed_up = now.duration_since(state.started_at) >= WARMUP;
        if !warmed_up {
            state.stall_samples = 0;
            state.window.clear();
            state.window.push_back((
                now,
                sample.frames_received,
                sample.frames_decoded,
                sample.frames_dropped,
                sample.freeze_duration,
            ));
            state.previous = sample;
            return;
        }

        state.stall_samples = if received > 0 && decoded == 0 {
            state.stall_samples.saturating_add(1)
        } else {
            0
        };
        self.summary.max_decoder_stall_seconds = self
            .summary
            .max_decoder_stall_seconds
            .max(state.stall_samples);

        state.window.push_back((
            now,
            sample.frames_received,
            sample.frames_decoded,
            sample.frames_dropped,
            sample.freeze_duration,
        ));
        while state.window.len() >= 2
            && state.window[1].0 <= now.checked_sub(QUALITY_WINDOW).unwrap_or(now)
        {
            state.window.pop_front();
        }

        if state.stall_samples >= STALL_SAMPLES {
            events.push(inbound_event(
                "decoder_stalled",
                &sample,
                received,
                decoded,
                dropped,
                freeze_duration,
            ));
        }
        if let (Some(first), Some(last)) = (state.window.front(), state.window.back()) {
            if last.0.duration_since(first.0) >= QUALITY_WINDOW {
                let window_received = last.1.saturating_sub(first.1);
                let window_decoded = last.2.saturating_sub(first.2);
                let window_dropped = last.3.saturating_sub(first.3);
                let window_total = window_decoded + window_dropped;
                let window_freeze = (last.4 - first.4).max(0.0);
                let high_drops = window_total >= MIN_DROP_FRAMES
                    && window_dropped as f64 / window_total as f64 >= MAX_DROP_RATIO;
                if window_freeze >= MIN_FREEZE_DURATION || high_drops {
                    events.push(inbound_event(
                        "decoder_quality_degraded",
                        &sample,
                        window_received,
                        window_decoded,
                        window_dropped,
                        window_freeze,
                    ));
                }
            }
        }

        state.previous = sample;
    }

    fn update_outbound(
        &mut self,
        sample: OutboundSample,
        now: Instant,
        events: &mut Vec<HealthEvent>,
    ) {
        let needs_reset = self
            .outbound
            .as_ref()
            .map(|state| {
                state.previous.id != sample.id
                    || sample.source_frames < state.previous.source_frames
                    || sample.frames_encoded < state.previous.frames_encoded
            })
            .unwrap_or(true);

        if needs_reset {
            self.summary.outbound_observed = true;
            self.summary.outbound_source_frames += sample.source_frames;
            self.summary.outbound_frames_encoded += sample.frames_encoded;
            self.summary.outbound_quality_limited_seconds += sample.quality_limited_seconds;
            self.record_outbound_labels(&sample);
            self.outbound = Some(OutboundState {
                started_at: now,
                previous: sample,
                stall_samples: 0,
                limitation_samples: 0,
                limitation_reason: String::new(),
            });
            return;
        }

        self.record_outbound_labels(&sample);
        let state = self.outbound.as_mut().unwrap();
        let source = sample.source_frames - state.previous.source_frames;
        let encoded = sample.frames_encoded - state.previous.frames_encoded;
        self.summary.outbound_source_frames += source;
        self.summary.outbound_frames_encoded += encoded;
        self.summary.outbound_quality_limited_seconds +=
            (sample.quality_limited_seconds - state.previous.quality_limited_seconds).max(0.0);

        let warmed_up = now.duration_since(state.started_at) >= WARMUP;
        if !warmed_up {
            state.stall_samples = 0;
            state.limitation_samples = 0;
            state.limitation_reason.clear();
            state.previous = sample;
            return;
        }

        state.stall_samples = if source > 0 && encoded == 0 {
            state.stall_samples.saturating_add(1)
        } else {
            0
        };
        self.summary.max_encoder_stall_seconds = self
            .summary
            .max_encoder_stall_seconds
            .max(state.stall_samples);

        if sample.quality_reason != "none" {
            if state.limitation_reason == sample.quality_reason {
                state.limitation_samples = state.limitation_samples.saturating_add(1);
            } else {
                state.limitation_reason.clone_from(&sample.quality_reason);
                state.limitation_samples = 1;
            }
        } else {
            state.limitation_reason.clear();
            state.limitation_samples = 0;
        }

        if state.stall_samples >= STALL_SAMPLES {
            events.push(outbound_event("encoder_stalled", &sample, source, encoded));
        }
        if state.limitation_samples >= LIMIT_SAMPLES {
            events.push(outbound_event(
                "encoder_quality_limited",
                &sample,
                source,
                encoded,
            ));
        }

        state.previous = sample;
    }

    fn record_inbound_labels(&mut self, sample: &InboundSample) {
        if !sample.codec.is_empty() {
            self.summary.codecs.insert(sample.codec.clone());
        }
        if !sample.decoder.is_empty() {
            self.summary.implementations.insert(sample.decoder.clone());
        }
    }

    fn record_outbound_labels(&mut self, sample: &OutboundSample) {
        if !sample.codec.is_empty() {
            self.summary.codecs.insert(sample.codec.clone());
        }
        if !sample.encoder.is_empty() {
            self.summary.implementations.insert(sample.encoder.clone());
        }
        if sample.quality_reason != "none" {
            self.summary
                .quality_limitation_reasons
                .insert(sample.quality_reason.clone());
        }
    }
}

fn inbound_event(
    issue: &'static str,
    sample: &InboundSample,
    received: u64,
    decoded: u64,
    dropped: u64,
    freeze_duration: f64,
) -> HealthEvent {
    let tags = BTreeMap::from([
        ("component".to_string(), "livekit".to_string()),
        ("media".to_string(), "screen_share".to_string()),
        ("direction".to_string(), "inbound".to_string()),
        ("codec".to_string(), sample.codec.clone()),
        ("decoder".to_string(), sample.decoder.clone()),
    ]);
    let metrics = BTreeMap::from([
        ("frames_received_delta".to_string(), received.to_string()),
        ("frames_decoded_delta".to_string(), decoded.to_string()),
        ("frames_dropped_delta".to_string(), dropped.to_string()),
        (
            "freeze_duration_delta_s".to_string(),
            format!("{freeze_duration:.3}"),
        ),
    ]);
    HealthEvent {
        issue,
        tags,
        metrics,
    }
}

fn outbound_event(
    issue: &'static str,
    sample: &OutboundSample,
    source: u64,
    encoded: u64,
) -> HealthEvent {
    let tags = BTreeMap::from([
        ("component".to_string(), "livekit".to_string()),
        ("media".to_string(), "screen_share".to_string()),
        ("direction".to_string(), "outbound".to_string()),
        ("codec".to_string(), sample.codec.clone()),
        ("encoder".to_string(), sample.encoder.clone()),
        (
            "limitation_reason".to_string(),
            sample.quality_reason.clone(),
        ),
    ]);
    let metrics = BTreeMap::from([
        ("source_frames_delta".to_string(), source.to_string()),
        ("frames_encoded_delta".to_string(), encoded.to_string()),
    ]);
    HealthEvent {
        issue,
        tags,
        metrics,
    }
}

fn codec_label(mime: &str) -> String {
    let base = mime.split(';').next().unwrap_or(mime).trim();
    let last = base.rsplit('/').next().unwrap_or(base).trim();
    last.to_ascii_uppercase()
}

fn codec_map(stats: &[RtcStats]) -> HashMap<String, String> {
    stats
        .iter()
        .filter_map(|stat| match stat {
            RtcStats::Codec(codec) => {
                Some((codec.rtc.id.clone(), codec_label(&codec.codec.mime_type)))
            }
            _ => None,
        })
        .collect()
}

pub(crate) async fn stats_loop(inner: Arc<RoomServiceInner>) {
    let mut interval = tokio::time::interval(Duration::from_secs(1));
    let mut previous = CumulativeCounters::default();
    let mut monitor = HealthMonitor::new();
    if let Ok(mut summary) = inner.video_health_summary.lock() {
        *summary = monitor.summary.clone();
    }
    interval.tick().await;

    loop {
        interval.tick().await;
        let room_guard = inner.room.lock().await;
        let Some(room) = room_guard.as_ref() else {
            continue;
        };
        let video_room_guard = inner.video_room.lock().await;
        let (counters, mut snapshot, inbound, outbound) =
            collect_stats(room, video_room_guard.as_ref()).await;
        drop(room_guard);
        drop(video_room_guard);

        if previous.screenshare_inbound_bytes > 0 {
            snapshot.screenshare_input_bps = (counters
                .screenshare_inbound_bytes
                .saturating_sub(previous.screenshare_inbound_bytes)
                * 8) as f64;
        }
        if previous.total_inbound_bytes > 0 {
            snapshot.total_input_bps = (counters
                .total_inbound_bytes
                .saturating_sub(previous.total_inbound_bytes)
                * 8) as f64;
        }
        if previous.total_outbound_bytes > 0 {
            snapshot.total_output_bps = (counters
                .total_outbound_bytes
                .saturating_sub(previous.total_outbound_bytes)
                * 8) as f64;
        }
        if previous.screenshare_jitter_buffer_emitted_count > 0 {
            let delay =
                counters.screenshare_jitter_buffer_delay - previous.screenshare_jitter_buffer_delay;
            let count = counters
                .screenshare_jitter_buffer_emitted_count
                .saturating_sub(previous.screenshare_jitter_buffer_emitted_count);
            if count > 0 {
                snapshot.screenshare_jitter_buffer_delay = delay / count as f64 * 1000.0;
            }
        }
        previous = counters;

        monitor.update(inbound, outbound, Instant::now());
        if let Ok(mut summary) = inner.video_health_summary.lock() {
            *summary = monitor.summary.clone();
        }

        log::debug!(
            "RoomStats: ss={}x{}@{:.1}fps codec_id={} jitter_buf={:.1}ms ss_in={:.2}Mbps | in={:.2}Mbps out={:.2}Mbps",
            snapshot.screenshare_width,
            snapshot.screenshare_height,
            snapshot.screenshare_fps,
            snapshot.screenshare_codec_id,
            snapshot.screenshare_jitter_buffer_delay,
            snapshot.screenshare_input_bps / 1_000_000.0,
            snapshot.total_input_bps / 1_000_000.0,
            snapshot.total_output_bps / 1_000_000.0,
        );
        if let Ok(mut stats) = inner.stats.write() {
            *stats = snapshot;
        }
    }
}

async fn collect_stats(
    room: &livekit::Room,
    video_room: Option<&livekit::Room>,
) -> (
    CumulativeCounters,
    RoomStats,
    Option<InboundSample>,
    Option<OutboundSample>,
) {
    let mut counters = CumulativeCounters::default();
    let mut snapshot = RoomStats::default();
    let mut inbound_sample = None;
    let mut outbound_sample = None;
    let video_participant_identity = video_room.map(|video_room| {
        video_room
            .local_participant()
            .identity()
            .as_str()
            .to_string()
    });

    for (_, publication) in room.local_participant().track_publications() {
        let Some(LocalTrack::Video(track)) = publication.track() else {
            continue;
        };
        if let Ok(stats) = track.get_stats().await {
            for stat in &stats {
                if let RtcStats::OutboundRtp(outbound) = stat {
                    counters.total_outbound_bytes += outbound.sent.bytes_sent;
                }
            }
        }
    }

    if let Some(video_room) = video_room {
        for (_, publication) in video_room.local_participant().track_publications() {
            let monitor_publication =
                publication.source() == TrackSource::Screenshare && !publication.is_muted();
            let Some(LocalTrack::Video(track)) = publication.track() else {
                continue;
            };
            let Ok(stats) = track.get_stats().await else {
                continue;
            };
            let codecs = codec_map(&stats);
            let sources: HashMap<String, u64> = stats
                .iter()
                .filter_map(|stat| match stat {
                    RtcStats::MediaSource(source) if source.source.kind == "video" => {
                        Some((source.rtc.id.clone(), source.video.frames as u64))
                    }
                    _ => None,
                })
                .collect();
            for stat in &stats {
                let RtcStats::OutboundRtp(outbound) = stat else {
                    continue;
                };
                counters.total_outbound_bytes += outbound.sent.bytes_sent;
                if !monitor_publication
                    || outbound.stream.kind != "video"
                    || !outbound.outbound.active
                {
                    continue;
                }
                let durations = &outbound.outbound.quality_limitation_durations;
                outbound_sample = Some(OutboundSample {
                    id: outbound.rtc.id.clone(),
                    codec: codecs
                        .get(&outbound.stream.codec_id)
                        .cloned()
                        .unwrap_or_else(|| outbound.stream.codec_id.clone()),
                    encoder: outbound.outbound.encoder_implementation.clone(),
                    source_frames: sources
                        .get(&outbound.outbound.media_source_id)
                        .copied()
                        .unwrap_or_default(),
                    frames_encoded: outbound.outbound.frames_encoded as u64,
                    quality_reason: format!("{:?}", outbound.outbound.quality_limitation_reason)
                        .to_ascii_lowercase(),
                    quality_limited_seconds: durations.get("cpu").copied().unwrap_or_default()
                        + durations.get("bandwidth").copied().unwrap_or_default()
                        + durations.get("other").copied().unwrap_or_default(),
                });
                break;
            }
        }
    }

    for (_, participant) in room.remote_participants() {
        if video_participant_identity.as_deref() == Some(participant.identity().as_str()) {
            continue;
        }
        for (_, publication) in participant.track_publications() {
            let monitor_publication =
                publication.source() == TrackSource::Screenshare && !publication.is_muted();
            let Some(livekit::track::RemoteTrack::Video(track)) = publication.track() else {
                continue;
            };
            let Ok(stats) = track.get_stats().await else {
                continue;
            };
            let codecs = codec_map(&stats);
            for stat in &stats {
                let RtcStats::InboundRtp(inbound) = stat else {
                    continue;
                };
                if inbound.stream.kind != "video" {
                    continue;
                }
                counters.total_inbound_bytes += inbound.inbound.bytes_received;
                if !monitor_publication {
                    continue;
                }
                counters.screenshare_inbound_bytes += inbound.inbound.bytes_received;
                counters.screenshare_jitter_buffer_delay += inbound.inbound.jitter_buffer_delay;
                counters.screenshare_jitter_buffer_emitted_count +=
                    inbound.inbound.jitter_buffer_emitted_count;
                let codec = codecs
                    .get(&inbound.stream.codec_id)
                    .cloned()
                    .unwrap_or_else(|| inbound.stream.codec_id.clone());
                snapshot.screenshare_fps = inbound.inbound.frames_per_second;
                snapshot.screenshare_width = inbound.inbound.frame_width;
                snapshot.screenshare_height = inbound.inbound.frame_height;
                snapshot.screenshare_codec_id.clone_from(&codec);
                inbound_sample = Some(InboundSample {
                    id: inbound.rtc.id.clone(),
                    codec,
                    decoder: inbound.inbound.decoder_implementation.clone(),
                    frames_received: inbound.inbound.frames_received,
                    frames_decoded: inbound.inbound.frames_decoded as u64,
                    frames_dropped: inbound.inbound.frames_dropped as u64,
                    freeze_duration: inbound.inbound.total_freeze_duration,
                });
                break;
            }
        }
    }

    (counters, snapshot, inbound_sample, outbound_sample)
}
