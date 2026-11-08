//! Automated scenarios against a fresh real core, with real LiveKit transport.

use crate::events::{
    ClientEvent, ClientPoint, DrawPathPoint, DrawPoint, DrawSettings, DrawingMode, KeystrokeData,
    MouseClickData,
};
use crate::ipc::CoreClient;
use crate::media::{
    self, check, identity, token, AudioPublisher, Fixture, Peer, VideoPublisher, CORE_USER,
    MEDIA_TIMEOUT,
};
use livekit::prelude::TrackSource;
use socket_lib::{
    AudioCaptureMessage, AudioDevice, CallId, CallStartMessage, CameraDevice, CameraStartMessage,
    Content, ContentType, DrawingEnabled, Extent, Message, ScreenShareMessage,
    ScreenSharePickerMode, ScreenShareResolution, TestAction, TestCallControl, TestStatus,
};
use std::io;
use std::process::Command;
use std::time::{Duration, Instant};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);

pub const SCENARIOS: &[&str] = &[
    "call-lifecycle",
    "call-races",
    "screenshare-call",
    "screenshare-room",
    "viewer-lifecycle",
    "camera",
    "camera-viewer",
    "microphone",
    "audio-signal",
    "combined",
    "controls",
    "network-drop",
];

pub async fn run(name: &str, cycles: u32, require_devices: bool) -> io::Result<()> {
    let mut core = CoreClient::connect()?;
    let url = std::env::var("LIVEKIT_URL").map_err(io::Error::other)?;
    core.send(Message::LivekitServerUrl(url))?;
    let microphones = microphones(&mut core)?;
    require(
        !microphones.is_empty(),
        require_devices,
        "no microphone input available; calls require microphone capture",
    )?;
    match name {
        "call-lifecycle" => call_lifecycle(&mut core, cycles).await,
        "call-races" => call_races(&mut core, cycles).await,
        "screenshare-call" => screenshare(&mut core, cycles, false).await,
        "screenshare-room" => screenshare(&mut core, cycles, true).await,
        "viewer-lifecycle" => viewer_lifecycle(&mut core, cycles).await,
        "camera" => camera(&mut core, cycles, require_devices).await,
        "camera-viewer" => camera_viewer(&mut core, cycles).await,
        "audio-signal" => audio_signal(&mut core, require_devices).await,
        "microphone" => microphone(&mut core, cycles, &microphones).await,
        "combined" => combined(&mut core, cycles, require_devices).await,
        "controls" => controls(&mut core, cycles).await,
        "network-drop" => network_drop(&mut core).await,
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("unknown smoke scenario {name}"),
        )),
    }
}

fn require(condition: bool, strict: bool, reason: &str) -> io::Result<()> {
    if condition {
        return Ok(());
    }
    Err(io::Error::new(
        if strict {
            io::ErrorKind::Other
        } else {
            io::ErrorKind::Unsupported
        },
        reason.to_string(),
    ))
}

fn step(name: impl AsRef<str>) {
    println!("STEP {}", name.as_ref());
}

fn microphones(core: &mut CoreClient) -> io::Result<Vec<AudioDevice>> {
    match core.request(Message::ListAudioDevices, REQUEST_TIMEOUT)? {
        Message::AudioDeviceList(mut devices) => {
            if let Ok(selected) = std::env::var("HOPP_TEST_MICROPHONE") {
                check(
                    devices.iter().any(|d| d.name == selected),
                    format!("selected test microphone {selected:?} is unavailable"),
                )?;
                devices.retain(|d| d.name == selected);
            }
            Ok(devices)
        }
        _ => Err(io::Error::other("expected AudioDeviceList")),
    }
}

fn cameras(core: &mut CoreClient) -> io::Result<Vec<CameraDevice>> {
    match core.request(Message::ListCameras, REQUEST_TIMEOUT)? {
        Message::CameraList(mut devices) => {
            if let Ok(selected) = std::env::var("HOPP_TEST_CAMERA") {
                check(
                    devices.iter().any(|d| d.name == selected),
                    format!("selected test camera {selected:?} is unavailable"),
                )?;
                devices.retain(|d| d.name == selected);
            }
            Ok(devices)
        }
        _ => Err(io::Error::other("expected CameraList")),
    }
}

fn status(core: &mut CoreClient) -> io::Result<TestStatus> {
    match core.request(Message::TestStatus, REQUEST_TIMEOUT)? {
        Message::TestStatusResult(status) => Ok(status),
        _ => Err(io::Error::other(
            "expected TestStatusResult; build core with test-support",
        )),
    }
}

async fn wait_status(
    core: &mut CoreClient,
    what: &str,
    mut predicate: impl FnMut(&TestStatus) -> bool,
) -> io::Result<TestStatus> {
    let deadline = Instant::now() + MEDIA_TIMEOUT;
    loop {
        let snapshot = status(core)?;
        if predicate(&snapshot) {
            return Ok(snapshot);
        }
        if Instant::now() >= deadline {
            return Err(media::timeout(format!("{what}; last status {snapshot:?}")));
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

fn action(core: &mut CoreClient, action: TestAction) -> io::Result<()> {
    match core.request(Message::TestAction(action), REQUEST_TIMEOUT)? {
        Message::TestActionResult(result) => result.map_err(io::Error::other),
        _ => Err(io::Error::other("expected TestActionResult")),
    }
}

async fn start_call(core: &mut CoreClient, mic: bool, camera: bool) -> io::Result<CallId> {
    core.drain_events();
    let call_id = format!("smoke-{}", rand::random::<u64>());
    step(format!("start call {call_id}, mic={mic}, camera={camera}"));
    let result = core.request(
        Message::CallStart(CallStartMessage {
            call_id: call_id.clone(),
            audio_token: token(CORE_USER, "audio"),
            video_token: token(CORE_USER, "video"),
            audio_device_name: std::env::var("HOPP_TEST_MICROPHONE").unwrap_or_default(),
            start_mic_on_call: Some(mic),
            start_camera_on_call: Some(camera),
        }),
        Duration::from_secs(15),
    )?;
    match result {
        Message::CallStartResult(Ok(())) => Ok(call_id),
        Message::CallStartResult(Err(error)) => {
            Err(io::Error::other(format!("call start failed: {error}")))
        }
        _ => Err(io::Error::other("unexpected call-start response")),
    }
}

async fn join_call(core: &mut CoreClient, observer: &Peer, mic: bool) -> io::Result<CallId> {
    let id = start_call(core, mic, false).await?;
    observer
        .wait_present(&identity(CORE_USER, "audio"), true)
        .await?;
    observer
        .wait_present(&identity(CORE_USER, "video"), true)
        .await?;
    core.wait_event(MEDIA_TIMEOUT, |event| matches!(event,
        Message::ParticipantsSnapshot(participants) if participants.iter().any(|p| p.identity == "local" && p.connected)))?;
    wait_status(core, "active call", |s| {
        s.call_id.as_deref() == Some(id.as_str())
    })
    .await?;
    core.probe()?;
    Ok(id)
}

async fn end_call(core: &mut CoreClient, id: &str, observer: &Peer) -> io::Result<()> {
    step(format!("end call {id}"));
    core.send(Message::CallEnd(Some(id.to_string())))?;
    core.wait_event(
        MEDIA_TIMEOUT,
        |event| matches!(event, Message::CallEnded(ended) if ended == id),
    )?;
    observer
        .wait_present(&identity(CORE_USER, "audio"), false)
        .await?;
    observer
        .wait_present(&identity(CORE_USER, "video"), false)
        .await?;
    wait_status(core, "call media and windows cleaned up", |s| {
        s.call_id.is_none()
            && !s.camera_active
            && !s.local_sharing
            && !s.mic_capturing
            && !s.camera_visible
            && !s.viewer_visible
            && !s.picker_open
            && !s.stats_visible
    })
    .await?;
    core.probe()?;
    Ok(())
}

async fn start_share(core: &mut CoreClient, fixture: &Fixture) -> io::Result<()> {
    start_share_kind(core, fixture, false).await
}

async fn start_share_kind(
    core: &mut CoreClient,
    fixture: &Fixture,
    display: bool,
) -> io::Result<()> {
    step(if display {
        "start real display capture"
    } else {
        "start real window capture"
    });
    core.send(Message::StartScreenShare(ScreenShareMessage {
        content: Content {
            content_type: if display {
                ContentType::Display
            } else {
                ContentType::Window
            },
            id: if display {
                fixture.info.display_id
            } else {
                fixture.info.id
            },
        },
        resolution: Extent {
            width: 1280.0,
            height: 720.0,
        },
    }))?;
    match core.wait_event(MEDIA_TIMEOUT, |event| {
        matches!(event, Message::StartScreenShareResult(_))
    })? {
        Message::StartScreenShareResult(Ok(())) => {}
        Message::StartScreenShareResult(Err(error)) => {
            return Err(io::Error::other(format!("screen capture failed: {error}")))
        }
        _ => unreachable!(),
    }
    wait_status(core, "local screen share active", |s| s.local_sharing).await?;
    core.probe()?;
    Ok(())
}

async fn stop_share(core: &mut CoreClient, observer: &Peer) -> io::Result<()> {
    step("stop screen share");
    core.send(Message::StopScreenshare)?;
    wait_status(core, "local screen share stopped", |s| !s.local_sharing).await?;
    observer
        .wait_muted(
            &identity(CORE_USER, "video"),
            TrackSource::Screenshare,
            true,
        )
        .await?;
    core.probe()?;
    Ok(())
}

async fn expect_screen(observer: &Peer) -> io::Result<()> {
    media::expect_video(
        observer
            .video(&identity(CORE_USER, "video"), TrackSource::Screenshare)
            .await?,
        true,
    )
    .await?;
    Ok(())
}

async fn expect_camera(observer: &Peer) -> io::Result<()> {
    media::expect_video(
        observer
            .video(&identity(CORE_USER, "audio"), TrackSource::Camera)
            .await?,
        false,
    )
    .await?;
    Ok(())
}

async fn start_camera(core: &mut CoreClient, call_id: &str, name: &str) -> io::Result<()> {
    step(format!("camera on: {name}"));
    match core.request(
        Message::StartCamera(CameraStartMessage {
            call_id: call_id.to_string(),
            device_name: Some(name.to_string()),
        }),
        Duration::from_secs(15),
    )? {
        Message::StartCameraResult(Ok(())) => {}
        Message::StartCameraResult(Err(error)) => {
            return Err(io::Error::other(format!("camera start failed: {error}")))
        }
        _ => return Err(io::Error::other("expected StartCameraResult")),
    }
    wait_status(core, "camera capture active", |s| s.camera_active).await?;
    core.probe()?;
    Ok(())
}

async fn stop_camera(core: &mut CoreClient, call_id: &str, observer: &Peer) -> io::Result<()> {
    core.send(Message::StopCamera(call_id.to_string()))?;
    wait_status(core, "camera capture stopped", |s| !s.camera_active).await?;
    observer
        .wait_muted(&identity(CORE_USER, "audio"), TrackSource::Camera, true)
        .await?;
    core.probe()?;
    Ok(())
}

async fn call_lifecycle(core: &mut CoreClient, cycles: u32) -> io::Result<()> {
    let observer = Peer::join("observer", "audio").await?;
    for round in 1..=cycles {
        step(format!("call lifecycle round {round}/{cycles}"));
        let id = join_call(core, &observer, true).await?;
        media::expect_audio(observer.audio(&identity(CORE_USER, "audio")).await?).await?;
        end_call(core, &id, &observer).await?;
        // A duplicate old hang-up must still be acknowledged without breaking the idle core.
        core.send(Message::CallEnd(Some(id.clone())))?;
        core.wait_event(
            REQUEST_TIMEOUT,
            |m| matches!(m, Message::CallEnded(ended) if ended == &id),
        )?;
        core.probe()?;
    }
    let muted = join_call(core, &observer, false).await?;
    wait_status(core, "start-muted preference", |s| s.mic_muted).await?;
    observer
        .wait_muted(&identity(CORE_USER, "audio"), TrackSource::Microphone, true)
        .await?;
    core.send(Message::UnmuteAudio)?;
    media::expect_audio(observer.audio(&identity(CORE_USER, "audio")).await?).await?;
    end_call(core, &muted, &observer).await?;
    observer.leave().await
}

async fn call_races(core: &mut CoreClient, cycles: u32) -> io::Result<()> {
    let observer = Peer::join("observer", "audio").await?;
    for round in 1..=cycles {
        step(format!("early hang-up round {round}/{cycles}"));
        let old = start_call(core, true, false).await?;
        core.send(Message::CallEnd(Some(old.clone())))?;
        core.wait_event(
            MEDIA_TIMEOUT,
            |m| matches!(m, Message::CallEnded(id) if id == &old),
        )?;
        let id = join_call(core, &observer, true).await?;
        core.send(Message::CallEnd(Some(old.clone())))?;
        core.wait_event(
            REQUEST_TIMEOUT,
            |m| matches!(m, Message::CallEnded(ended) if ended == &old),
        )?;
        core.send(Message::StopCamera(old.clone()))?;
        match core.request(
            Message::StartCamera(CameraStartMessage {
                call_id: old,
                device_name: None,
            }),
            REQUEST_TIMEOUT,
        )? {
            Message::StartCameraResult(Err(_)) => {}
            _ => return Err(io::Error::other("stale camera start was not rejected")),
        }
        wait_status(core, "current call survives stale actions", |s| {
            s.call_id.as_deref() == Some(&id)
        })
        .await?;
        media::expect_audio(observer.audio(&identity(CORE_USER, "audio")).await?).await?;
        end_call(core, &id, &observer).await?;
    }
    observer.leave().await
}

async fn click_control(peer: &Peer) -> io::Result<()> {
    peer.publish_data(
        serde_json::to_vec(&ClientEvent::DrawingMode(DrawingMode::Disabled))
            .map_err(io::Error::other)?,
        Some("draw"),
    )
    .await?;
    peer.publish_data(
        serde_json::to_vec(&ClientEvent::MouseMove(ClientPoint {
            x: 0.5,
            y: 0.5,
            pointer: false,
        }))
        .map_err(io::Error::other)?,
        None,
    )
    .await?;
    for down in [true, false] {
        peer.publish_data(
            serde_json::to_vec(&ClientEvent::MouseClick(MouseClickData {
                x: 0.5,
                y: 0.5,
                button: 0,
                clicks: 1,
                down,
                shift: false,
                meta: false,
                ctrl: false,
                alt: false,
            }))
            .map_err(io::Error::other)?,
            None,
        )
        .await?;
    }
    Ok(())
}

async fn send_key(peer: &Peer, key: &str) -> io::Result<()> {
    for down in [true, false] {
        peer.publish_data(
            serde_json::to_vec(&ClientEvent::Keystroke(KeystrokeData {
                key: vec![key.to_string()],
                meta: false,
                ctrl: false,
                shift: false,
                alt: false,
                down,
            }))
            .map_err(io::Error::other)?,
            None,
        )
        .await?;
    }
    Ok(())
}

async fn screenshare(core: &mut CoreClient, cycles: u32, multi_user: bool) -> io::Result<()> {
    let mut fixture = Fixture::start()?;
    let mut observer = Peer::join("viewer-a", "audio").await?;
    let mut second = if multi_user {
        Some(Peer::join("viewer-b", "audio").await?)
    } else {
        None
    };
    for round in 1..=cycles {
        if multi_user && second.is_none() {
            second = Some(Peer::join("viewer-b", "audio").await?);
        }
        step(format!(
            "{} screen share round {round}/{cycles}",
            if multi_user { "room" } else { "pair call" }
        ));
        let id = join_call(core, &observer, true).await?;
        if let Some(peer) = second.as_ref() {
            peer.wait_present(&identity(CORE_USER, "audio"), true)
                .await?;
        }
        core.send(Message::ControllerCursorEnabled(true))?;
        start_share(core, &fixture).await?;
        expect_screen(&observer).await?;
        if let Some(peer) = second.as_ref() {
            expect_screen(peer).await?;
        }
        observer.drain_events();
        if let Some(peer) = second.as_mut() {
            peer.drain_events();
        }
        step("viewer A takes control by clicking owned fixture");
        fixture.drain_events();
        click_control(&observer).await?;
        fixture.expect_event("remote click", |event| event["event"] == "click")?;
        let controller_a = observer.identity.clone();
        observer.expect_control(&controller_a).await?;
        wait_status(core, "viewer A owns control", |s| {
            s.participant_in_control.as_deref() == Some(controller_a.as_str())
        })
        .await?;
        core.send(Message::ControllerCursorEnabled(false))?;
        wait_status(core, "disabled control returns ownership to sharer", |s| {
            s.participant_in_control.as_deref() == Some("local")
        })
        .await?;
        fixture.drain_events();
        click_control(&observer).await?;
        send_key(&observer, "x").await?;
        fixture.expect_none(
            "input while remote control is disabled",
            Duration::from_millis(500),
            |event| matches!(event["event"].as_str(), Some("click" | "key" | "backspace")),
        )?;
        check(
            status(core)?.participant_in_control.as_deref() == Some("local"),
            "disabled controller took ownership",
        )?;
        core.send(Message::ControllerCursorEnabled(true))?;
        core.probe()?;
        observer.drain_events();
        click_control(&observer).await?;
        observer.expect_control(&controller_a).await?;
        if let Some(peer) = second.as_mut() {
            peer.expect_control(&controller_a).await?;
            observer.drain_events();
            peer.drain_events();
            step("viewer B takes control; both participants observe handoff");
            click_control(peer).await?;
            let controller_b = peer.identity.clone();
            peer.expect_control(&controller_b).await?;
            observer.expect_control(&controller_b).await?;
            wait_status(core, "viewer B owns control", |s| {
                s.participant_in_control.as_deref() == Some(controller_b.as_str())
            })
            .await?;
            expect_screen(peer).await?;
            observer.drain_events();
            peer.drain_events();
            step("handoff B back to A");
            click_control(&observer).await?;
            observer.expect_control(&controller_a).await?;
            peer.expect_control(&controller_a).await?;
            wait_status(core, "viewer A regains control", |s| {
                s.participant_in_control.as_deref() == Some(controller_a.as_str())
            })
            .await?;
            // Make departing B the controller, then let A take over after B leaves.
            click_control(peer).await?;
            let identity_b = peer.identity.clone();
            observer.expect_control(&identity_b).await?;
        }
        if let Some(peer) = second.take() {
            let departed = peer.identity.clone();
            peer.leave().await?;
            observer.wait_present(&departed, false).await?;
            observer.drain_events();
            click_control(&observer).await?;
            observer.expect_control(&controller_a).await?;
            // Local pointer activity may reclaim control during the departure transition.
            let deadline = Instant::now() + MEDIA_TIMEOUT;
            loop {
                if status(core)?.participant_in_control.as_deref() == Some(controller_a.as_str()) {
                    break;
                }
                if Instant::now() >= deadline {
                    return Err(media::timeout(
                        "remaining viewer cannot take control; check local pointer activity",
                    ));
                }
                click_control(&observer).await?;
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        }
        core.probe()?;
        expect_screen(&observer).await?;
        stop_share(core, &observer).await?;
        core.send(Message::StopScreenshare)?; // duplicate stop remains harmless
        start_share_kind(core, &fixture, true).await?;
        wait_status(core, "restarted share resets stale ownership", |s| {
            s.participant_in_control.as_deref() == Some("local")
        })
        .await?;
        expect_screen(&observer).await?;
        core.send(Message::SetScreenShareResolution(
            ScreenShareResolution::P1080,
        ))?;
        expect_screen(&observer).await?;
        // End while publishing: no explicit stop should be required for cleanup.
        end_call(core, &id, &observer).await?;
    }
    if let Some(peer) = second {
        peer.leave().await?;
    }
    observer.leave().await
}

async fn viewer_lifecycle(core: &mut CoreClient, cycles: u32) -> io::Result<()> {
    let observer = Peer::join("remote-sharer", "audio").await?;
    let sharer = Peer::join("remote-sharer", "video").await?;
    let id = join_call(core, &observer, false).await?;
    let mut before = status(core)?;
    let video = VideoPublisher::start(&sharer, TrackSource::Screenshare).await?;
    for round in 1..=cycles {
        step(format!("viewer lifecycle round {round}/{cycles}"));
        if round > 1 {
            before = status(core)?;
            video.publication.unmute();
        }
        wait_status(core, "remote screen decoded and presented", |s| {
            s.viewer_visible
                && s.screen_frames_received > before.screen_frames_received
                && s.screen_frames_presented > before.screen_frames_presented
        })
        .await?;
        action(core, TestAction::CloseViewerWindow)?;
        wait_status(core, "viewer native close hides window", |s| {
            !s.viewer_visible
        })
        .await?;
        core.probe()?;
        core.send(Message::OpenScreenShareWindow)?;
        let hidden = status(core)?;
        wait_status(core, "viewer reopens and presents fresh frames", |s| {
            s.viewer_visible && s.screen_frames_presented > hidden.screen_frames_presented
        })
        .await?;
        action(core, TestAction::ViewerControl(TestCallControl::ToggleMic))?;
        wait_status(core, "viewer microphone control unmutes", |s| !s.mic_muted).await?;
        action(core, TestAction::ViewerControl(TestCallControl::ToggleMic))?;
        wait_status(core, "viewer microphone control mutes", |s| s.mic_muted).await?;
        core.send(Message::CloseScreenShareWindow)?;
        wait_status(core, "IPC viewer close hides", |s| !s.viewer_visible).await?;
        let closed = status(core)?;
        core.send(Message::OpenScreenShareWindow)?;
        wait_status(core, "IPC viewer reopen resumes real frames", |s| {
            s.viewer_visible
                && s.screen_frames_received > closed.screen_frames_received
                && s.screen_frames_presented > closed.screen_frames_presented + 2
        })
        .await?;
        for _ in 0..3 {
            video.publication.mute();
            tokio::time::sleep(Duration::from_millis(100)).await;
            video.publication.unmute();
            core.probe()?;
        }
        let storm = status(core)?;
        wait_status(core, "screen continues after mute/unmute storm", |s| {
            s.screen_frames_presented > storm.screen_frames_presented
        })
        .await?;
        // Ownership must stay current even while the viewer is hidden.
        action(core, TestAction::CloseViewerWindow)?;
        wait_status(core, "viewer hidden before control handoff", |s| {
            !s.viewer_visible
        })
        .await?;
        observer
            .publish_data(
                identity(CORE_USER, "audio").into_bytes(),
                Some("participant_in_control"),
            )
            .await?;
        wait_status(core, "hidden viewer receives control", |s| {
            !s.viewer_visible && s.local_in_control
        })
        .await?;
        core.send(Message::OpenScreenShareWindow)?;
        wait_status(core, "reopened viewer keeps current control", |s| {
            s.viewer_visible && s.local_in_control
        })
        .await?;
        observer
            .publish_data(
                observer.identity.as_bytes().to_vec(),
                Some("participant_in_control"),
            )
            .await?;
        wait_status(core, "local viewer relinquishes control", |s| {
            !s.local_in_control
        })
        .await?;
        // Hopp stops sharing by muting its persistent screen track.
        video.publication.mute();
        wait_status(core, "viewer closes after remote share stops", |s| {
            !s.viewer_visible
        })
        .await?;
        core.probe()?;
    }
    video.stop(&sharer).await?;
    // A different participant replaces the departed sharer.
    sharer.leave().await?;
    let replacement_audio = Peer::join("replacement", "audio").await?;
    let replacement = Peer::join("replacement", "video").await?;
    let before = status(core)?;
    let video = VideoPublisher::start(&replacement, TrackSource::Screenshare).await?;
    wait_status(core, "replacement sharer renders", |s| {
        s.viewer_visible && s.screen_frames_presented > before.screen_frames_presented
    })
    .await?;
    end_call(core, &id, &observer).await?;
    video.stop(&replacement).await?;
    replacement.leave().await?;
    replacement_audio.leave().await?;
    observer.leave().await
}

async fn camera(core: &mut CoreClient, cycles: u32, strict: bool) -> io::Result<()> {
    let devices = cameras(core)?;
    require(!devices.is_empty(), strict, "no camera device available")?;
    let observer = Peer::join("camera-observer", "audio").await?;
    let id = join_call(core, &observer, false).await?;
    let remote = VideoPublisher::start(&observer, TrackSource::Camera).await?;
    for round in 0..cycles {
        step(format!("camera lifecycle round {}/{cycles}", round + 1));
        let before = status(core)?;
        let device = &devices[round as usize % devices.len()];
        start_camera(core, &id, &device.name).await?;
        expect_camera(&observer).await?;
        core.send(Message::OpenCamera)?;
        wait_status(core, "camera window renders remote camera", |s| {
            s.camera_visible
                && s.camera_frames_received > before.camera_frames_received
                && s.camera_frames_presented > before.camera_frames_presented
        })
        .await?;
        action(core, TestAction::CloseCameraWindow)?;
        wait_status(core, "camera native close hides but keeps capture", |s| {
            !s.camera_visible && s.camera_active
        })
        .await?;
        expect_camera(&observer).await?;
        core.send(Message::OpenCamera)?;
        let hidden = status(core)?;
        wait_status(core, "camera reopen renders fresh frames", |s| {
            s.camera_visible && s.camera_frames_presented > hidden.camera_frames_presented
        })
        .await?;
        stop_camera(core, &id, &observer).await?;
        core.send(Message::StopCamera(id.clone()))?;
        core.probe()?;
        // Native on/off follows production call controls, separately from IPC on/off.
        action(
            core,
            TestAction::CameraControl(TestCallControl::ToggleCamera),
        )?;
        wait_status(core, "native camera control turns capture on", |s| {
            s.camera_active
        })
        .await?;
        expect_camera(&observer).await?;
        action(
            core,
            TestAction::CameraControl(TestCallControl::ToggleCamera),
        )?;
        wait_status(core, "native camera control turns capture off", |s| {
            !s.camera_active
        })
        .await?;
        // Queue opposite operations without waiting between them.
        core.send(Message::StartCamera(CameraStartMessage {
            call_id: id.clone(),
            device_name: Some(device.name.clone()),
        }))?;
        core.send(Message::StopCamera(id.clone()))?;
        wait_status(core, "rapid camera start/stop settles off", |s| {
            !s.camera_active
        })
        .await?;
        core.probe()?;
    }
    remote.stop(&observer).await?;
    end_call(core, &id, &observer).await?;
    core.send(Message::SetPreferredCamera(Some(devices[0].name.clone())))?;
    let auto = start_call(core, false, true).await?;
    observer
        .wait_present(&identity(CORE_USER, "audio"), true)
        .await?;
    observer
        .wait_present(&identity(CORE_USER, "video"), true)
        .await?;
    wait_status(
        core,
        "camera-on / microphone-muted startup preferences",
        |s| s.camera_active && s.mic_muted,
    )
    .await?;
    expect_camera(&observer).await?;
    end_call(core, &auto, &observer).await?;

    step("camera off while connecting must cancel deferred camera startup");
    let url = std::env::var("LIVEKIT_URL").map_err(io::Error::other)?;
    require(
        url.starts_with("ws://"),
        strict,
        "connecting-camera regression requires local ws:// signaling",
    )?;
    let (host, port) = crate::runner::livekit_address(&url)?;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let upstream = url.strip_prefix("ws://").unwrap();
    let authority = upstream.split(['/', '?', '#']).next().unwrap();
    let suffix = &upstream[authority.len()..];
    core.send(Message::LivekitServerUrl(format!(
        "ws://{}{suffix}",
        listener.local_addr()?
    )))?;

    // No signaling is forwarded until OFF has passed through core's main loop.
    let connecting = start_call(core, false, true).await?;
    core.send(Message::StopCamera(connecting.clone()))?;
    core.probe()?;
    check(
        !status(core)?.camera_active,
        "camera was not stopped before connection",
    )?;

    let mut proxy = tokio::task::JoinSet::<io::Result<()>>::new();
    proxy.spawn(async move {
        let mut streams = tokio::task::JoinSet::<io::Result<()>>::new();
        loop {
            tokio::select! {
                incoming = listener.accept() => {
                    let (mut client, _) = incoming?;
                    let host = host.clone();
                    streams.spawn(async move {
                        let mut server = tokio::net::TcpStream::connect((host.as_str(), port)).await?;
                        tokio::io::copy_bidirectional(&mut client, &mut server).await?;
                        Ok(())
                    });
                }
                completed = streams.join_next(), if !streams.is_empty() => {
                    completed.unwrap().map_err(io::Error::other)??;
                }
            }
        }
    });
    observer
        .wait_present(&identity(CORE_USER, "audio"), true)
        .await?;
    observer
        .wait_present(&identity(CORE_USER, "video"), true)
        .await?;
    let connected = core.wait_event(MEDIA_TIMEOUT, |event| matches!(event,
        Message::ParticipantsSnapshot(participants) if participants.iter().any(|p| p.identity == "local" && p.connected)))?;
    // Two round trips also catch an auto-start queued behind the first probe.
    core.probe()?;
    let after_connection = status(core)?;
    end_call(core, &connecting, &observer).await?;
    core.send(Message::LivekitServerUrl(url))?;
    check(
        !after_connection.camera_active,
        "camera turned back on after OFF was processed while the call was connecting",
    )?;
    check(
        matches!(connected, Message::ParticipantsSnapshot(participants)
            if participants.iter().any(|p| p.identity == "local" && !p.has_camera)),
        "the connected participant snapshot reported camera ON after it was turned OFF",
    )?;
    observer.leave().await
}

async fn camera_viewer(core: &mut CoreClient, cycles: u32) -> io::Result<()> {
    let remote = Peer::join("camera-publisher", "audio").await?;
    let id = join_call(core, &remote, false).await?;
    for round in 1..=cycles {
        step(format!("remote camera viewing round {round}/{cycles}"));
        let before = status(core)?;
        let video = VideoPublisher::start(&remote, TrackSource::Camera).await?;
        core.send(Message::OpenCamera)?;
        wait_status(core, "remote camera decodes and presents", |s| {
            s.camera_visible
                && s.camera_frames_received > before.camera_frames_received
                && s.camera_frames_presented > before.camera_frames_presented
        })
        .await?;
        action(core, TestAction::CloseCameraWindow)?;
        wait_status(core, "remote camera window hides", |s| !s.camera_visible).await?;
        core.probe()?;
        let hidden = status(core)?;
        core.send(Message::OpenCamera)?;
        wait_status(core, "remote camera reopens with fresh presentation", |s| {
            s.camera_visible && s.camera_frames_presented > hidden.camera_frames_presented
        })
        .await?;
        video.stop(&remote).await?;
        core.probe()?;
    }
    // A participant disconnect must release the source; a new participant must render.
    remote.leave().await?;
    let replacement = Peer::join("replacement-camera", "audio").await?;
    let before = status(core)?;
    let camera = VideoPublisher::start(&replacement, TrackSource::Camera).await?;
    core.send(Message::OpenCamera)?;
    wait_status(core, "replacement participant camera renders", |s| {
        s.camera_visible && s.camera_frames_presented > before.camera_frames_presented
    })
    .await?;
    end_call(core, &id, &replacement).await?;
    camera.stop(&replacement).await?;
    replacement.leave().await
}

async fn audio_signal(core: &mut CoreClient, strict: bool) -> io::Result<()> {
    let selected = std::env::var("HOPP_TEST_LOOPBACK_MICROPHONE").unwrap_or_default();
    require(
        !selected.is_empty(),
        strict,
        "audio-signal needs HOPP_TEST_LOOPBACK_MICROPHONE and system output routed into that input",
    )?;
    check(
        microphones(core)?.iter().any(|d| d.name == selected),
        format!("configured loopback microphone {selected:?} is unavailable"),
    )?;
    // Each scenario is isolated in its own process; use this selected input for call setup.
    std::env::set_var("HOPP_TEST_MICROPHONE", &selected);
    let observer = Peer::join("signal-observer", "audio").await?;
    core.send(Message::SetNoiseCancellation(false))?;
    let id = join_call(core, &observer, true).await?;
    let _stimulus = media::SignalStimulus::start()?;
    step("known PCM signal through configured loopback input and real microphone pipeline");
    let evidence = media::audio_window(
        observer.audio(&identity(CORE_USER, "audio")).await?,
        Duration::from_secs(3),
    )
    .await?;
    check(evidence.has_fixture_signal(), format!("decoded input does not contain fixture tones: {evidence:?}; check system output/loopback routing"))?;
    core.send(Message::MuteAudio)?;
    observer
        .wait_muted(&identity(CORE_USER, "audio"), TrackSource::Microphone, true)
        .await?;
    tokio::time::sleep(Duration::from_millis(300)).await;
    let muted = media::audio_window(
        observer_audio_track(&observer).await?,
        Duration::from_secs(1),
    )
    .await?;
    check(
        muted.peak <= 32,
        format!("known audio fixture leaks while muted: {muted:?}"),
    )?;
    core.send(Message::UnmuteAudio)?;
    let recovered = media::audio_window(
        observer.audio(&identity(CORE_USER, "audio")).await?,
        Duration::from_secs(3),
    )
    .await?;
    check(
        recovered.has_fixture_signal(),
        format!("known microphone signal did not recover: {recovered:?}"),
    )?;
    end_call(core, &id, &observer).await?;
    observer.leave().await
}

async fn microphone(core: &mut CoreClient, cycles: u32, devices: &[AudioDevice]) -> io::Result<()> {
    let observer = Peer::join("audio-observer", "audio").await?;
    let id = join_call(core, &observer, true).await?;
    let baseline = status(core)?;
    let remote_audio = AudioPublisher::start(&observer).await?;
    wait_status(core, "known remote audio reaches receive/mix/output", |s| {
        s.audio_frames_received > baseline.audio_frames_received
            && s.audio_frames_mixed > baseline.audio_frames_mixed
            && s.audio_received_nonzero_samples > baseline.audio_received_nonzero_samples
            && s.audio_mixed_nonzero_samples > baseline.audio_mixed_nonzero_samples
            && s.audio_output_nonzero_samples > baseline.audio_output_nonzero_samples
    })
    .await?;
    for round in 0..cycles {
        step(format!("microphone lifecycle round {}/{cycles}", round + 1));
        media::expect_audio(observer.audio(&identity(CORE_USER, "audio")).await?).await?;
        core.send(Message::MuteAudio)?;
        wait_status(core, "mic muted", |s| s.mic_muted).await?;
        observer
            .wait_muted(&identity(CORE_USER, "audio"), TrackSource::Microphone, true)
            .await?;
        tokio::time::sleep(Duration::from_millis(300)).await;
        let muted = media::audio_window(
            observer_audio_track(&observer).await?,
            Duration::from_millis(600),
        )
        .await?;
        check(
            muted.peak <= 32,
            format!("muted microphone still delivers signal: {muted:?}"),
        )?;
        core.send(Message::UnmuteAudio)?;
        wait_status(core, "mic unmuted", |s| !s.mic_muted).await?;
        media::expect_audio(observer.audio(&identity(CORE_USER, "audio")).await?).await?;
        for muted in [true, false] {
            core.send(Message::ToggleMic)?;
            wait_status(core, "toggle microphone", |s| s.mic_muted == muted).await?;
        }
        let device = &devices[round as usize % devices.len()];
        match core.request(
            Message::StartAudioCapture(AudioCaptureMessage {
                device_name: device.name.clone(),
            }),
            Duration::from_secs(15),
        )? {
            Message::StartAudioCaptureResult(Ok(())) => {}
            Message::StartAudioCaptureResult(Err(error)) => {
                return Err(io::Error::other(format!(
                    "mic device switch failed: {error}"
                )))
            }
            _ => return Err(io::Error::other("expected audio capture result")),
        }
        wait_status(core, "capture continues after device switch", |s| {
            s.mic_capturing
        })
        .await?;
        core.send(Message::UnmuteAudio)?;
        media::expect_audio(observer.audio(&identity(CORE_USER, "audio")).await?).await?;
        for enabled in [false, true] {
            core.send(Message::SetNoiseCancellation(enabled))?;
            core.probe()?;
            media::expect_audio(observer.audio(&identity(CORE_USER, "audio")).await?).await?;
        }
    }
    // Another source can leave without stopping the surviving source.
    let second = Peer::join("second-audio", "audio").await?;
    let second_audio = AudioPublisher::start(&second).await?;
    let before = status(core)?;
    wait_status(core, "second remote audio decoded", |s| {
        s.audio_frames_received > before.audio_frames_received + 10
    })
    .await?;
    remote_audio.stop(&observer).await?;
    tokio::time::sleep(Duration::from_secs(1)).await;
    for _ in 0..3 {
        let before = status(core)?;
        tokio::time::sleep(Duration::from_millis(300)).await;
        let after = status(core)?;
        check(
            after.audio_frames_received > before.audio_frames_received
                && after.audio_output_nonzero_samples > before.audio_output_nonzero_samples,
            "remaining source stopped delivering fresh decoded/output audio",
        )?;
    }
    second_audio.stop(&second).await?;
    second.leave().await?;
    core.send(Message::StopAudioCapture)?;
    wait_status(core, "capture stopped for teardown", |s| !s.mic_capturing).await?;
    match core.request(
        Message::StartAudioCapture(AudioCaptureMessage {
            device_name: devices[0].name.clone(),
        }),
        REQUEST_TIMEOUT,
    )? {
        Message::StartAudioCaptureResult(Err(_)) => {}
        _ => {
            return Err(io::Error::other(
                "device selection unexpectedly restarted torn-down capture",
            ))
        }
    }
    core.probe()?;
    end_call(core, &id, &observer).await?;
    let next = join_call(core, &observer, true).await?;
    media::expect_audio(observer.audio(&identity(CORE_USER, "audio")).await?).await?;
    end_call(core, &next, &observer).await?;
    observer.leave().await
}

async fn observer_audio_track(observer: &Peer) -> io::Result<livekit::prelude::RemoteAudioTrack> {
    // Muted tracks intentionally remain subscribed; don't wait for unmuted state here.
    for participant in observer.room.remote_participants().values() {
        if participant.identity().as_str() == identity(CORE_USER, "audio") {
            for publication in participant.track_publications().values() {
                if publication.source() == TrackSource::Microphone {
                    if let Some(livekit::prelude::RemoteTrack::Audio(track)) = publication.track() {
                        return Ok(track);
                    }
                }
            }
        }
    }
    Err(io::Error::other(
        "muted microphone track disappeared before silence check",
    ))
}

async fn combined(core: &mut CoreClient, cycles: u32, strict: bool) -> io::Result<()> {
    let cameras = cameras(core)?;
    require(
        !cameras.is_empty(),
        strict,
        "combined media requires a real camera",
    )?;
    let fixture = Fixture::start()?;
    let observer = Peer::join("combined-observer", "audio").await?;
    for round in 1..=cycles {
        step(format!("combined media round {round}/{cycles}"));
        let id = join_call(core, &observer, true).await?;
        start_camera(core, &id, &cameras[0].name).await?;
        start_share(core, &fixture).await?;
        let inbound_camera = VideoPublisher::start(&observer, TrackSource::Camera).await?;
        let inbound_audio = AudioPublisher::start(&observer).await?;
        expect_screen(&observer).await?;
        expect_camera(&observer).await?;
        media::expect_audio(observer.audio(&identity(CORE_USER, "audio")).await?).await?;
        core.send(Message::MuteAudio)?;
        wait_status(core, "combined mic muted", |s| s.mic_muted).await?;
        expect_screen(&observer).await?;
        expect_camera(&observer).await?;
        core.send(Message::UnmuteAudio)?;
        stop_camera(core, &id, &observer).await?;
        expect_screen(&observer).await?;
        media::expect_audio(observer.audio(&identity(CORE_USER, "audio")).await?).await?;
        start_camera(core, &id, &cameras[0].name).await?;
        stop_share(core, &observer).await?;
        expect_camera(&observer).await?;
        media::expect_audio(observer.audio(&identity(CORE_USER, "audio")).await?).await?;
        start_share(core, &fixture).await?;
        expect_screen(&observer).await?;
        end_call(core, &id, &observer).await?;
        inbound_camera.stop(&observer).await?;
        inbound_audio.stop(&observer).await?;
    }
    observer.leave().await
}

async fn controls(core: &mut CoreClient, cycles: u32) -> io::Result<()> {
    let observer = Peer::join("controls-observer", "audio").await?;
    let mut fixture = Fixture::start()?;
    let id = join_call(core, &observer, true).await?;
    for round in 1..=cycles {
        step(format!("native controls round {round}/{cycles}"));
        core.send(Message::OpenCamera)?;
        wait_status(core, "camera window opens without local camera", |s| {
            s.camera_visible
        })
        .await?;
        action(core, TestAction::CameraControl(TestCallControl::ToggleMic))?;
        wait_status(core, "native mute button", |s| s.mic_muted).await?;
        action(core, TestAction::CameraControl(TestCallControl::ToggleMic))?;
        wait_status(core, "native unmute button", |s| !s.mic_muted).await?;
        for mode in [ScreenSharePickerMode::Screen, ScreenSharePickerMode::Window] {
            core.send(Message::SetScreenSharePickerMode(mode))?;
            action(core, TestAction::CameraControl(TestCallControl::OpenPicker))?;
            wait_status(core, "picker opens", |s| s.picker_open).await?;
            action(core, TestAction::CancelPicker)?;
            wait_status(core, "picker cancels", |s| !s.picker_open).await?;
            core.probe()?;
        }
        core.send(Message::OpenStatsWindow)?;
        wait_status(core, "stats window opens", |s| s.stats_visible).await?;
        action(core, TestAction::CloseStatsWindow)?;
        wait_status(core, "stats window closes", |s| !s.stats_visible).await?;
        core.request(Message::BringWindowsToFront, REQUEST_TIMEOUT)?;
        start_share(core, &fixture).await?;
        for permanent in [false, true] {
            core.send(Message::DrawingEnabled(DrawingEnabled { permanent }))?;
            wait_status(core, "drawing opens", |s| s.drawing_visible).await?;
            core.probe()?;
            core.send(Message::DrawingEnabled(DrawingEnabled { permanent }))?;
            core.wait_event(REQUEST_TIMEOUT, |m| matches!(m, Message::DrawingDisabled))?;
            wait_status(core, "drawing closes", |s| !s.drawing_visible).await?;
        }
        // Exercise reliable annotation packets without replaying keyboard into unrelated apps.
        observer
            .publish_data(
                serde_json::to_vec(&ClientEvent::DrawingMode(DrawingMode::Draw(DrawSettings {
                    permanent: true,
                })))
                .map_err(io::Error::other)?,
                Some("draw"),
            )
            .await?;
        for event in [
            ClientEvent::DrawStart(DrawPathPoint {
                point: DrawPoint { x: 0.2, y: 0.2 },
                path_id: u64::from(round),
            }),
            ClientEvent::DrawAddPoint(DrawPoint { x: 0.4, y: 0.4 }),
            ClientEvent::DrawEnd(DrawPoint { x: 0.5, y: 0.5 }),
            ClientEvent::DrawClearAllPaths,
            ClientEvent::DrawingMode(DrawingMode::ClickAnimation),
            ClientEvent::ClickAnimation(DrawPoint { x: 0.5, y: 0.5 }),
            ClientEvent::DrawingMode(DrawingMode::Disabled),
        ] {
            observer
                .publish_data(
                    serde_json::to_vec(&event).map_err(io::Error::other)?,
                    Some("draw"),
                )
                .await?;
        }
        core.probe()?;
        println!("annotation packets checked for liveness; rendered annotation correctness remains manual");
        action(core, TestAction::CloseCameraWindow)?;
        wait_status(core, "hide call controls before owned-target input", |s| {
            !s.camera_visible
        })
        .await?;
        fixture.drain_events();
        click_control(&observer).await?;
        fixture.expect_event("remote click reaches owned target", |event| {
            event["event"] == "click"
        })?;
        send_key(&observer, "x").await?;
        fixture.expect_event("remote key reaches owned target", |event| {
            event["event"] == "key" && event["text"] == "x"
        })?;
        println!("clipboard copy/paste remains manual: preserving all original formats requires a native snapshot");
        expect_screen(&observer).await?;
        core.send(Message::OpenCamera)?;
        wait_status(core, "show controls for native screen toggle", |s| {
            s.camera_visible
        })
        .await?;
        action(
            core,
            TestAction::CameraControl(TestCallControl::ToggleScreenShare),
        )?;
        wait_status(core, "native screen button stops sharing", |s| {
            !s.local_sharing
        })
        .await?;
        observer
            .wait_muted(
                &identity(CORE_USER, "video"),
                TrackSource::Screenshare,
                true,
            )
            .await?;
        action(
            core,
            TestAction::CameraControl(TestCallControl::ToggleScreenShare),
        )?;
        wait_status(core, "native screen button opens picker", |s| s.picker_open).await?;
        action(core, TestAction::CancelPicker)?;
        wait_status(core, "native picker cancellation", |s| !s.picker_open).await?;
        core.send(Message::ControllerCursorEnabled(false))?;
        core.probe()?;
        core.send(Message::ControllerCursorEnabled(true))?;
        core.probe()?;
    }
    // Hang up while picker is open, then verify the next call remains functional.
    core.send(Message::GetAvailableContent)?;
    wait_status(core, "picker opens before hangup", |s| s.picker_open).await?;
    action(core, TestAction::CameraControl(TestCallControl::EndCall))?;
    core.wait_event(
        MEDIA_TIMEOUT,
        |m| matches!(m, Message::CallEnded(ended) if ended == &id),
    )?;
    observer
        .wait_present(&identity(CORE_USER, "audio"), false)
        .await?;
    wait_status(core, "hangup cancels picker", |s| {
        !s.picker_open && s.call_id.is_none()
    })
    .await?;
    let next = join_call(core, &observer, true).await?;
    media::expect_audio(observer.audio(&identity(CORE_USER, "audio")).await?).await?;
    end_call(core, &next, &observer).await?;
    observer.leave().await
}

struct NetworkDrop {
    script: std::path::PathBuf,
    session: String,
}

impl NetworkDrop {
    fn start() -> io::Result<Self> {
        let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("netem.sh");
        let host =
            std::env::var("HOPP_TEST_NETEM_HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
        let port = std::env::var("HOPP_TEST_NETEM_PORT").unwrap_or_else(|_| "7880".to_string());
        let session = format!(
            "{}-drop",
            std::env::var("HOPP_TEST_NETEM_SESSION")
                .unwrap_or_else(|_| format!("hopp-smoke-{}", std::process::id()))
        );
        let result = Command::new("sudo")
            .args(["-n"])
            .arg(&script)
            .args([
                "drop",
                "--remote",
                &host,
                "--session",
                &session,
                "--port",
                &port,
            ])
            .status()?;
        check(
            result.success(),
            "network-drop requires sudo credentials; run sudo -v before this scenario",
        )?;
        Ok(Self { script, session })
    }

    fn restore(&self) -> io::Result<()> {
        let result = Command::new("sudo")
            .arg("-n")
            .arg(&self.script)
            .args(["off", "--session", &self.session])
            .status()?;
        check(result.success(), "failed to restore network-drop lease")
    }
}

impl Drop for NetworkDrop {
    fn drop(&mut self) {
        let _ = self.restore();
    }
}

async fn network_drop(core: &mut CoreClient) -> io::Result<()> {
    let observer = Peer::join("outage-observer", "audio").await?;
    let fixture = Fixture::start()?;
    let id = join_call(core, &observer, true).await?;
    start_share(core, &fixture).await?;
    let audio = AudioPublisher::start(&observer).await?;
    expect_screen(&observer).await?;
    media::expect_audio(observer.audio(&identity(CORE_USER, "audio")).await?).await?;
    step("drop all test LiveKit traffic for ten seconds; controls must still respond");
    let lease = NetworkDrop::start()?;
    for _ in 0..3 {
        core.probe()?;
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    // New stream after buffering settles proves the shaper really interrupted delivery.
    let frozen = media::video_frames_during(
        observer
            .video(&identity(CORE_USER, "video"), TrackSource::Screenshare)
            .await?,
        Duration::from_secs(2),
    )
    .await?;
    check(frozen == 0, format!("network-drop did not stop decoded video ({frozen} fresh frames); check shaping interface"))?;
    for _ in 0..5 {
        core.probe()?;
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    lease.restore()?;
    drop(lease);
    step("require fresh decoded media after outage");
    expect_screen(&observer).await?;
    media::expect_audio(observer.audio(&identity(CORE_USER, "audio")).await?).await?;
    let before = status(core)?;
    wait_status(core, "incoming audio recovers after outage", |s| {
        s.audio_received_nonzero_samples > before.audio_received_nonzero_samples
            && s.audio_output_nonzero_samples > before.audio_output_nonzero_samples
    })
    .await?;
    end_call(core, &id, &observer).await?;
    audio.stop(&observer).await?;
    let next = join_call(core, &observer, true).await?;
    media::expect_audio(observer.audio(&identity(CORE_USER, "audio")).await?).await?;
    end_call(core, &next, &observer).await?;
    observer.leave().await
}
