use std::collections::HashMap;
use std::sync::Arc;

use livekit::track::{LocalTrack, TrackSource};
use livekit::webrtc::stats::RtcStats;

use crate::room_service::RoomServiceInner;

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

#[derive(Default)]
struct CumulativeCounters {
    screenshare_inbound_bytes: u64,
    total_inbound_bytes: u64,
    total_outbound_bytes: u64,
    screenshare_jitter_buffer_delay: f64,
    screenshare_jitter_buffer_emitted_count: u64,
}

fn codec_label(mime: &str) -> String {
    let base = mime.split(';').next().unwrap_or(mime).trim();
    let last = base.rsplit('/').next().unwrap_or(base).trim();
    last.to_ascii_uppercase()
}

pub(crate) async fn stats_loop(inner: Arc<RoomServiceInner>) {
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(1));
    let mut prev = CumulativeCounters::default();

    let mut codec_map: HashMap<String, String> = HashMap::new();
    interval.tick().await;

    loop {
        interval.tick().await;

        let room_guard = inner.room.lock().await;
        let Some(room) = room_guard.as_ref() else {
            continue;
        };
        let video_room_guard = inner.video_room.lock().await;

        let (counters, mut snapshot) =
            collect_stats(room, video_room_guard.as_ref(), &mut codec_map).await;
        drop(room_guard);
        drop(video_room_guard);

        if prev.screenshare_inbound_bytes > 0 {
            snapshot.screenshare_input_bps = (counters
                .screenshare_inbound_bytes
                .saturating_sub(prev.screenshare_inbound_bytes)
                * 8) as f64;
        }
        if prev.total_inbound_bytes > 0 {
            snapshot.total_input_bps = (counters
                .total_inbound_bytes
                .saturating_sub(prev.total_inbound_bytes)
                * 8) as f64;
        }
        if prev.total_outbound_bytes > 0 {
            snapshot.total_output_bps = (counters
                .total_outbound_bytes
                .saturating_sub(prev.total_outbound_bytes)
                * 8) as f64;
        }
        if prev.screenshare_jitter_buffer_emitted_count > 0 {
            let delta_delay =
                counters.screenshare_jitter_buffer_delay - prev.screenshare_jitter_buffer_delay;
            let delta_count = counters
                .screenshare_jitter_buffer_emitted_count
                .saturating_sub(prev.screenshare_jitter_buffer_emitted_count);
            if delta_count > 0 {
                snapshot.screenshare_jitter_buffer_delay = delta_delay / delta_count as f64 * 1000.;
            }
        }

        prev = counters;

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

        if let Ok(mut w) = inner.stats.write() {
            *w = snapshot;
        }
    }
}

async fn collect_stats(
    room: &livekit::Room,
    video_room: Option<&livekit::Room>,
    codec_map: &mut HashMap<String, String>,
) -> (CumulativeCounters, RoomStats) {
    let mut counters = CumulativeCounters::default();
    let mut snapshot = RoomStats::default();

    // Outbound from main room (camera)
    let local = room.local_participant();
    for (_, publication) in local.track_publications() {
        let Some(track) = publication.track() else {
            continue;
        };
        let LocalTrack::Video(v) = track else {
            continue;
        };
        if let Ok(stats_vec) = v.get_stats().await {
            for stat in &stats_vec {
                if let RtcStats::OutboundRtp(s) = stat {
                    counters.total_outbound_bytes += s.sent.bytes_sent;
                }
            }
        }
    }
    // Outbound from video room (screen share)
    if let Some(vr) = video_room {
        let local = vr.local_participant();
        for (_, publication) in local.track_publications() {
            let Some(track) = publication.track() else {
                continue;
            };
            let LocalTrack::Video(v) = track else {
                continue;
            };
            if let Ok(stats_vec) = v.get_stats().await {
                for stat in &stats_vec {
                    if let RtcStats::OutboundRtp(s) = stat {
                        counters.total_outbound_bytes += s.sent.bytes_sent;
                    }
                }
            }
        }
    }

    // Inbound: remote video tracks
    for (_, participant) in room.remote_participants() {
        for (_, publication) in participant.track_publications() {
            let Some(track) = publication.track() else {
                continue;
            };
            let livekit::track::RemoteTrack::Video(v) = track else {
                continue;
            };
            let Ok(stats_vec) = v.get_stats().await else {
                continue;
            };
            let is_screenshare = publication.source() == TrackSource::Screenshare;

            if is_screenshare && codec_map.is_empty() {
                for stat in &stats_vec {
                    if let RtcStats::Codec(c) = stat {
                        codec_map.insert(c.rtc.id.clone(), c.codec.mime_type.clone());
                    }
                }
            }

            for stat in &stats_vec {
                if let RtcStats::InboundRtp(s) = stat {
                    if s.stream.kind != "video" {
                        continue;
                    }
                    counters.total_inbound_bytes += s.inbound.bytes_received;
                    if is_screenshare {
                        counters.screenshare_inbound_bytes += s.inbound.bytes_received;
                        counters.screenshare_jitter_buffer_delay +=
                            s.inbound.jitter_buffer_delay as f64;
                        counters.screenshare_jitter_buffer_emitted_count +=
                            s.inbound.jitter_buffer_emitted_count;
                        snapshot.screenshare_fps = s.inbound.frames_per_second;
                        snapshot.screenshare_width = s.inbound.frame_width;
                        snapshot.screenshare_height = s.inbound.frame_height;
                        snapshot.screenshare_codec_id = codec_map
                            .get(&s.stream.codec_id)
                            .map(|m| codec_label(m))
                            .unwrap_or_else(|| s.stream.codec_id.clone());
                    }
                }
            }
        }
    }

    (counters, snapshot)
}
