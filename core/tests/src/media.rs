//! Real LiveKit peers and decoded-media assertions shared by automated scenarios.

use crate::livekit_utils::generate_room_token;
use futures::StreamExt;
use livekit::options::{AudioEncoding, TrackPublishOptions, VideoCodec, VideoEncoding};
use livekit::prelude::*;
use livekit::webrtc::audio_frame::AudioFrame;
use livekit::webrtc::audio_source::native::NativeAudioSource;
use livekit::webrtc::audio_stream::native::NativeAudioStream;
use livekit::webrtc::prelude::{AudioSourceOptions, RtcAudioSource, VideoResolution};
use livekit::webrtc::video_frame::{I420Buffer, VideoFrame, VideoRotation};
use livekit::webrtc::video_source::native::NativeVideoSource;
use livekit::webrtc::video_source::RtcVideoSource;
use livekit::webrtc::video_stream::native::NativeVideoStream;
use serde::Deserialize;
use std::io::{self, BufRead};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::task::JoinHandle;

pub const MEDIA_TIMEOUT: Duration = Duration::from_secs(20);
pub const CORE_USER: &str = "smoke-core";

pub fn room_name() -> String {
    std::env::var("HOPP_TEST_ROOM").expect("runner must set HOPP_TEST_ROOM")
}

pub fn identity(user: &str, track: &str) -> String {
    format!("room:{}:{user}:{track}", room_name())
}

pub fn token(user: &str, track: &str) -> String {
    generate_room_token(&identity(user, track), &room_name())
}

pub fn check(condition: bool, message: impl Into<String>) -> io::Result<()> {
    if condition {
        Ok(())
    } else {
        Err(io::Error::other(message.into()))
    }
}

pub fn timeout(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::TimedOut, message.into())
}

pub struct Peer {
    pub room: Room,
    events: UnboundedReceiver<RoomEvent>,
    pub identity: String,
}

impl Peer {
    pub async fn join(user: &str, track: &str) -> io::Result<Self> {
        let url = std::env::var("LIVEKIT_URL").map_err(io::Error::other)?;
        let identity = identity(user, track);
        let (room, events) = tokio::time::timeout(
            MEDIA_TIMEOUT,
            Room::connect(&url, &token(user, track), RoomOptions::default()),
        )
        .await
        .map_err(|_| timeout(format!("{identity} joining LiveKit")))?
        .map_err(io::Error::other)?;
        Ok(Self {
            room,
            events,
            identity,
        })
    }

    pub async fn wait_present(&self, identity: &str, present: bool) -> io::Result<()> {
        let deadline = Instant::now() + MEDIA_TIMEOUT;
        loop {
            let found = self
                .room
                .remote_participants()
                .values()
                .any(|p| p.identity().as_str() == identity);
            if found == present {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(timeout(format!("participant {identity} present={present}")));
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    pub async fn video(&self, identity: &str, source: TrackSource) -> io::Result<RemoteVideoTrack> {
        let deadline = Instant::now() + MEDIA_TIMEOUT;
        loop {
            for participant in self.room.remote_participants().values() {
                if participant.identity().as_str() != identity {
                    continue;
                }
                for publication in participant.track_publications().values() {
                    if publication.source() == source && !publication.is_muted() {
                        if let Some(RemoteTrack::Video(track)) = publication.track() {
                            return Ok(track);
                        }
                    }
                }
            }
            if Instant::now() >= deadline {
                return Err(timeout(format!(
                    "decoded video track from {identity} source {source:?}"
                )));
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    pub async fn audio(&self, identity: &str) -> io::Result<RemoteAudioTrack> {
        let deadline = Instant::now() + MEDIA_TIMEOUT;
        loop {
            for participant in self.room.remote_participants().values() {
                if participant.identity().as_str() != identity {
                    continue;
                }
                for publication in participant.track_publications().values() {
                    if publication.source() == TrackSource::Microphone && !publication.is_muted() {
                        if let Some(RemoteTrack::Audio(track)) = publication.track() {
                            return Ok(track);
                        }
                    }
                }
            }
            if Instant::now() >= deadline {
                return Err(timeout(format!("microphone track from {identity}")));
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    pub async fn wait_muted(
        &self,
        identity: &str,
        source: TrackSource,
        muted: bool,
    ) -> io::Result<()> {
        let deadline = Instant::now() + MEDIA_TIMEOUT;
        loop {
            let publications: Vec<_> = self
                .room
                .remote_participants()
                .values()
                .filter(|p| p.identity().as_str() == identity)
                .flat_map(|p| p.track_publications().into_values())
                .filter(|p| p.source() == source)
                .collect();
            if if muted {
                publications.iter().all(|p| p.is_muted())
            } else {
                publications.iter().any(|p| !p.is_muted())
            } {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(timeout(format!("{identity} {source:?} muted={muted}")));
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    pub async fn publish_data(&self, payload: Vec<u8>, topic: Option<&str>) -> io::Result<()> {
        self.room
            .local_participant()
            .publish_data(DataPacket {
                payload,
                reliable: true,
                topic: topic.map(str::to_string),
                ..Default::default()
            })
            .await
            .map_err(io::Error::other)
    }

    pub fn drain_events(&mut self) {
        while self.events.try_recv().is_ok() {}
    }

    pub async fn expect_control(&mut self, identity: &str) -> io::Result<()> {
        let waiting = async {
            while let Some(event) = self.events.recv().await {
                if let RoomEvent::DataReceived { payload, topic, .. } = event {
                    if topic.as_deref() == Some("participant_in_control")
                        && payload.as_slice() == identity.as_bytes()
                    {
                        return Ok(());
                    }
                }
            }
            Err(io::Error::other(
                "room events closed waiting for control handoff",
            ))
        };
        tokio::time::timeout(MEDIA_TIMEOUT, waiting)
            .await
            .map_err(|_| timeout(format!("control handoff to {identity}")))?
    }

    pub async fn leave(self) -> io::Result<()> {
        self.room.close().await.map_err(io::Error::other)
    }
}

pub struct VideoPublisher {
    pub track: LocalVideoTrack,
    pub publication: LocalTrackPublication,
    task: JoinHandle<()>,
}

impl VideoPublisher {
    pub async fn start(peer: &Peer, source_kind: TrackSource) -> io::Result<Self> {
        let source = NativeVideoSource::new(
            VideoResolution {
                width: 640,
                height: 360,
            },
            source_kind == TrackSource::Screenshare,
        );
        let track = LocalVideoTrack::create_video_track(
            if source_kind == TrackSource::Screenshare {
                "screen_share"
            } else {
                "camera"
            },
            RtcVideoSource::Native(source.clone()),
        );
        let publication = peer
            .room
            .local_participant()
            .publish_track(
                LocalTrack::Video(track.clone()),
                TrackPublishOptions {
                    source: source_kind,
                    video_codec: VideoCodec::H264,
                    video_encoding: Some(VideoEncoding {
                        max_bitrate: 1_000_000,
                        max_framerate: 15.0,
                    }),
                    simulcast: false,
                    ..Default::default()
                },
            )
            .await
            .map_err(io::Error::other)?;
        let task = tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_millis(66));
            let mut n = 0_u8;
            loop {
                interval.tick().await;
                let mut buffer = I420Buffer::new(640, 360);
                let (y, u, v) = buffer.data_mut();
                y.fill(64 + (n % 128));
                u.fill(110);
                v.fill(150);
                source.capture_frame(&VideoFrame {
                    rotation: VideoRotation::VideoRotation0,
                    timestamp_us: 0,
                    frame_metadata: None,
                    buffer,
                });
                n = n.wrapping_add(7);
            }
        });
        Ok(Self {
            track,
            publication,
            task,
        })
    }

    pub async fn stop(self, peer: &Peer) -> io::Result<()> {
        self.task.abort();
        peer.room
            .local_participant()
            .unpublish_track(&self.track.sid())
            .await
            .map(|_| ())
            .map_err(io::Error::other)
    }
}

impl Drop for VideoPublisher {
    fn drop(&mut self) {
        self.task.abort();
    }
}

pub struct AudioPublisher {
    pub track: LocalAudioTrack,
    task: JoinHandle<()>,
}

impl AudioPublisher {
    pub async fn start(peer: &Peer) -> io::Result<Self> {
        let source = NativeAudioSource::new(
            AudioSourceOptions {
                echo_cancellation: false,
                noise_suppression: false,
                auto_gain_control: false,
            },
            16_000,
            1,
            100,
        );
        let track = LocalAudioTrack::create_audio_track(
            "microphone",
            RtcAudioSource::Native(source.clone()),
        );
        peer.room
            .local_participant()
            .publish_track(
                LocalTrack::Audio(track.clone()),
                TrackPublishOptions {
                    source: TrackSource::Microphone,
                    audio_encoding: Some(AudioEncoding {
                        max_bitrate: 24_000,
                    }),
                    dtx: false,
                    ..Default::default()
                },
            )
            .await
            .map_err(io::Error::other)?;
        let task = tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_millis(10));
            let mut sample_index = 0_u64;
            loop {
                interval.tick().await;
                // Quiet, deterministic multitone keeps real receive/mix paths active without loud playback.
                let samples: Vec<i16> = (0..160)
                    .map(|_| {
                        let value = fixture_sample(sample_index as usize) / 3;
                        sample_index += 1;
                        value
                    })
                    .collect();
                let frame = AudioFrame {
                    data: samples.into(),
                    sample_rate: 16_000,
                    num_channels: 1,
                    samples_per_channel: 160,
                };
                if source.capture_frame(&frame).await.is_err() {
                    break;
                }
            }
        });
        Ok(Self { track, task })
    }

    pub async fn stop(self, peer: &Peer) -> io::Result<()> {
        self.task.abort();
        peer.room
            .local_participant()
            .unpublish_track(&self.track.sid())
            .await
            .map(|_| ())
            .map_err(io::Error::other)
    }
}

impl Drop for AudioPublisher {
    fn drop(&mut self) {
        self.task.abort();
    }
}

#[derive(Debug, Default)]
pub struct VideoEvidence {
    pub frames: u32,
    pub width: u32,
    pub height: u32,
    pub red: bool,
    pub blue: bool,
}

pub async fn expect_video(
    track: RemoteVideoTrack,
    fixture_pattern: bool,
) -> io::Result<VideoEvidence> {
    let mut stream = NativeVideoStream::new(track.rtc_track());
    let deadline = Instant::now() + MEDIA_TIMEOUT;
    let mut evidence = VideoEvidence::default();
    while Instant::now() < deadline {
        let frame = match tokio::time::timeout(
            deadline.saturating_duration_since(Instant::now()),
            stream.next(),
        )
        .await
        {
            Ok(Some(frame)) => frame,
            Ok(None) => return Err(io::Error::other("decoded video stream closed")),
            Err(_) => {
                if let Ok(stats) = track.get_stats().await {
                    for stat in stats {
                        if let livekit::webrtc::stats::RtcStats::InboundRtp(stat) = stat {
                            if stat.stream.kind == "video" {
                                println!("video timeout: packets={} bytes={} received={} decoded={} dropped={} dimensions={}x{} decoder={}",
                                    stat.received.packets_received, stat.inbound.bytes_received,
                                    stat.inbound.frames_received, stat.inbound.frames_decoded,
                                    stat.inbound.frames_dropped, stat.inbound.frame_width,
                                    stat.inbound.frame_height, stat.inbound.decoder_implementation);
                            }
                        }
                    }
                }
                return Err(timeout(format!(
                    "receiving decoded video frames; observed {evidence:?}"
                )));
            }
        };
        if frame.buffer.width() <= 16 || frame.buffer.height() <= 16 {
            continue; // SDK keepalives do not establish actual capture.
        }
        evidence.frames += 1;
        evidence.width = frame.buffer.width();
        evidence.height = frame.buffer.height();
        if fixture_pattern {
            let i420 = frame.buffer.to_i420();
            let (_, u, v) = i420.data();
            let (.., stride_v) = i420.strides();
            let (_, stride_u, _) = i420.strides();
            let cx = (evidence.width / 4) as usize;
            let cy = (evidence.height / 4) as usize;
            let u = u[cy * stride_u as usize + cx] as i16;
            let v = v[cy * stride_v as usize + cx] as i16;
            evidence.red |= v - u > 50;
            evidence.blue |= u - v > 50;
        }
        if evidence.frames >= 5 && (!fixture_pattern || (evidence.red && evidence.blue)) {
            println!(
                "decoded {} video frames, {}x{}, fixture red={} blue={}",
                evidence.frames, evidence.width, evidence.height, evidence.red, evidence.blue
            );
            stream.close();
            return Ok(evidence);
        }
    }
    stream.close();
    Err(timeout(format!(
        "insufficient decoded video evidence: {evidence:?}"
    )))
}

pub async fn video_frames_during(track: RemoteVideoTrack, duration: Duration) -> io::Result<u32> {
    let mut stream = NativeVideoStream::new(track.rtc_track());
    let deadline = Instant::now() + duration;
    let mut frames = 0;
    while let Ok(Some(frame)) = tokio::time::timeout(
        deadline.saturating_duration_since(Instant::now()),
        stream.next(),
    )
    .await
    {
        if frame.buffer.width() > 16 && frame.buffer.height() > 16 {
            frames += 1;
        }
        if Instant::now() >= deadline {
            break;
        }
    }
    stream.close();
    Ok(frames)
}

#[derive(Debug, Default)]
pub struct AudioEvidence {
    pub frames: u32,
    pub samples: usize,
    pub nonzero: usize,
    pub peak: i32,
    energy: f64,
    tone_re: [f64; 2],
    tone_im: [f64; 2],
}

impl AudioEvidence {
    fn observe(&mut self, frame: &AudioFrame<'_>) -> io::Result<()> {
        check(
            frame.sample_rate == 16_000
                && frame.num_channels == 1
                && frame.samples_per_channel > 0
                && frame.samples_per_channel as usize == frame.data.len(),
            "invalid decoded audio format or sample count",
        )?;
        self.frames += 1;
        for &sample in frame.data.iter() {
            let sample = f64::from(sample);
            for (i, hz) in [440.0, 713.0].into_iter().enumerate() {
                let phase = self.samples as f64 * hz * std::f64::consts::TAU / 16_000.0;
                self.tone_re[i] += sample * phase.cos();
                self.tone_im[i] += sample * phase.sin();
            }
            self.energy += sample * sample;
            self.peak = self.peak.max(sample.abs() as i32);
            self.nonzero += usize::from(sample != 0.0);
            self.samples += 1;
        }
        Ok(())
    }

    pub fn has_fixture_signal(&self) -> bool {
        self.frames >= 5
            && self.peak > 32
            && self.energy > 0.0
            && (0..2).all(|i| {
                let power = self.tone_re[i].powi(2) + self.tone_im[i].powi(2);
                2.0 * power / (self.samples as f64 * self.energy) > 0.08
            })
    }
}

pub async fn audio_window(
    track: RemoteAudioTrack,
    duration: Duration,
) -> io::Result<AudioEvidence> {
    let mut stream = NativeAudioStream::new(track.rtc_track(), 16_000, 1);
    let deadline = Instant::now() + duration;
    let mut evidence = AudioEvidence::default();
    while let Ok(Some(frame)) = tokio::time::timeout(
        deadline.saturating_duration_since(Instant::now()),
        stream.next(),
    )
    .await
    {
        evidence.observe(&frame)?;
        if Instant::now() >= deadline {
            break;
        }
    }
    stream.close();
    Ok(evidence)
}

pub async fn expect_audio(track: RemoteAudioTrack) -> io::Result<AudioEvidence> {
    let result = audio_window(track, Duration::from_secs(3)).await?;
    check(
        result.frames >= 5 && result.samples >= 800,
        format!("no sustained decoded microphone audio: {result:?}"),
    )?;
    println!("decoded microphone PCM: {result:?} (silent input is valid)");
    Ok(result)
}

#[derive(Debug, Deserialize)]
pub struct FixtureInfo {
    pub id: u32,
    pub display_id: u32,
}

pub struct Fixture {
    pub info: FixtureInfo,
    child: Child,
    events: std::sync::mpsc::Receiver<serde_json::Value>,
}

impl Fixture {
    pub fn start() -> io::Result<Self> {
        let binary = std::env::var_os("HOPP_TEST_FIXTURE_BIN")
            .ok_or_else(|| io::Error::other("HOPP_TEST_FIXTURE_BIN missing"))?;
        let mut child = Command::new(binary)
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("fixture stdout unavailable"))?;
        let (tx, rx) = std::sync::mpsc::channel();
        let (event_tx, events) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut reader = io::BufReader::new(stdout);
            let mut line = String::new();
            let result = reader.read_line(&mut line).and_then(|n| {
                check(n > 0, "fixture exited without metadata")?;
                serde_json::from_str::<FixtureInfo>(&line).map_err(io::Error::other)
            });
            let _ = tx.send(result);
            for line in reader.lines() {
                let Ok(line) = line else {
                    break;
                };
                if let Ok(event) = serde_json::from_str(&line) {
                    if event_tx.send(event).is_err() {
                        break;
                    }
                }
            }
        });
        let result = rx
            .recv_timeout(MEDIA_TIMEOUT)
            .map_err(|_| timeout("fixture window startup"))
            .and_then(|r| r);
        match result {
            Ok(info) => Ok(Self {
                info,
                child,
                events,
            }),
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                Err(error)
            }
        }
    }

    pub fn drain_events(&mut self) {
        while self.events.try_recv().is_ok() {}
    }

    pub fn expect_none(
        &mut self,
        what: &str,
        duration: Duration,
        mut forbidden: impl FnMut(&serde_json::Value) -> bool,
    ) -> io::Result<()> {
        let deadline = Instant::now() + duration;
        loop {
            match self
                .events
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            {
                Ok(event) if forbidden(&event) => {
                    return Err(io::Error::other(format!(
                        "owned fixture received forbidden {what}"
                    )))
                }
                Ok(_) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => return Ok(()),
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    return Err(io::Error::other(
                        "fixture exited during negative input assertion",
                    ))
                }
            }
        }
    }

    pub fn expect_event(
        &mut self,
        what: &str,
        mut predicate: impl FnMut(&serde_json::Value) -> bool,
    ) -> io::Result<serde_json::Value> {
        let deadline = Instant::now() + MEDIA_TIMEOUT;
        loop {
            let event = self
                .events
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .map_err(|_| timeout(format!("owned fixture {what}")))?;
            if predicate(&event) {
                return Ok(event);
            }
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn fixture_sample(index: usize) -> i16 {
    let time = index as f64 / 16_000.0;
    let envelope = 0.6 + 0.4 * (time * 3.0 * std::f64::consts::TAU).sin();
    (600.0
        * envelope
        * ((time * 440.0 * std::f64::consts::TAU).sin()
            + (time * 713.0 * std::f64::consts::TAU).sin())) as i16
}

/// The user configures system output into their selected loopback input.
/// Keeping this external to core avoids feeding AEC its own reference signal.
pub struct SignalStimulus {
    child: Child,
    path: std::path::PathBuf,
}

impl SignalStimulus {
    pub fn start() -> io::Result<Self> {
        #[cfg(not(target_os = "macos"))]
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "audio signal player currently requires macOS afplay",
        ));
        #[cfg(target_os = "macos")]
        {
            use std::io::Write;
            let path = std::env::temp_dir().join(format!("hopp-signal-{}.wav", std::process::id()));
            let samples = 16_000 * 30;
            let bytes = samples as u32 * 2;
            let mut file = io::BufWriter::new(
                std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&path)?,
            );
            file.write_all(b"RIFF")?;
            file.write_all(&(36 + bytes).to_le_bytes())?;
            file.write_all(b"WAVEfmt ")?;
            file.write_all(&16_u32.to_le_bytes())?;
            file.write_all(&1_u16.to_le_bytes())?;
            file.write_all(&1_u16.to_le_bytes())?;
            file.write_all(&16_000_u32.to_le_bytes())?;
            file.write_all(&32_000_u32.to_le_bytes())?;
            file.write_all(&2_u16.to_le_bytes())?;
            file.write_all(&16_u16.to_le_bytes())?;
            file.write_all(b"data")?;
            file.write_all(&bytes.to_le_bytes())?;
            for index in 0..samples {
                file.write_all(&fixture_sample(index).to_le_bytes())?;
            }
            file.flush()?;
            drop(file);
            match Command::new("/usr/bin/afplay")
                .arg(&path)
                .stdout(Stdio::null())
                .stderr(Stdio::inherit())
                .spawn()
            {
                Ok(child) => Ok(Self { child, path }),
                Err(error) => {
                    let _ = std::fs::remove_file(&path);
                    Err(error)
                }
            }
        }
    }
}

impl Drop for SignalStimulus {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_file(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audio_evidence_checks_format_and_recognizes_the_actual_fixture() {
        let mut evidence = AudioEvidence::default();
        let extreme = AudioFrame {
            data: vec![0, i16::MIN, i16::MAX].into(),
            sample_rate: 16_000,
            num_channels: 1,
            samples_per_channel: 3,
        };
        evidence.observe(&extreme).unwrap();
        assert_eq!(evidence.nonzero, 2);
        assert_eq!(evidence.peak, 32768);
        assert!(!evidence.has_fixture_signal());
        let malformed = AudioFrame {
            data: Vec::new().into(),
            sample_rate: 16_000,
            num_channels: 1,
            samples_per_channel: 160,
        };
        assert!(evidence.observe(&malformed).is_err());
        let mut signal = AudioEvidence::default();
        for round in 0..100 {
            let samples: Vec<i16> = (0..160).map(|n| fixture_sample(round * 160 + n)).collect();
            signal
                .observe(&AudioFrame {
                    data: samples.into(),
                    sample_rate: 16_000,
                    num_channels: 1,
                    samples_per_channel: 160,
                })
                .unwrap();
        }
        assert!(signal.has_fixture_signal());
    }
}
