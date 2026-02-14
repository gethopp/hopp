use crate::livekit_utils;
use crate::screenshare_client::{self, call_start, connect_socket};
use livekit::prelude::*;
use socket_lib::{CameraStartMessage, EventSocket, Message, SocketSender};
use std::env;
use std::io;
use std::time::Duration;

fn setup_camera(sender: &SocketSender, event_socket: &EventSocket) -> io::Result<()> {
    let livekit_server_url =
        env::var("LIVEKIT_URL").expect("LIVEKIT_URL environment variable not set");
    sender.send(Message::LivekitServerUrl(livekit_server_url))?;
    call_start(sender, event_socket)
}

pub fn test_list_cameras() -> io::Result<()> {
    let (sender, event_socket) = connect_socket()?;
    setup_camera(&sender, &event_socket)?;

    sender.send(Message::ListCameras)?;
    let response = event_socket
        .responses
        .recv_timeout(Duration::from_secs(5))
        .map_err(|e| io::Error::other(format!("Failed to receive CameraList: {e:?}")))?;

    match response {
        Message::CameraList(devices) => {
            println!("Found {} cameras:", devices.len());
            for device in &devices {
                println!("  {}", device.name);
            }
            assert!(!devices.is_empty(), "Expected at least one camera");
        }
        other => {
            return Err(io::Error::other(format!("Unexpected response: {other:?}")));
        }
    }

    Ok(())
}

pub fn test_camera_30s(camera_name: Option<&str>) -> io::Result<()> {
    let (sender, event_socket) = connect_socket()?;
    setup_camera(&sender, &event_socket)?;

    let device_name = if let Some(name) = camera_name {
        println!("Using explicitly provided camera: {}", name);
        name.to_string()
    } else {
        sender.send(Message::ListCameras)?;
        let response = event_socket
            .responses
            .recv_timeout(Duration::from_secs(5))
            .map_err(|e| io::Error::other(format!("Failed to receive CameraList: {e:?}")))?;

        let devices = match response {
            Message::CameraList(devices) => devices,
            other => {
                return Err(io::Error::other(format!("Unexpected response: {other:?}")));
            }
        };

        let device = devices
            .first()
            .ok_or_else(|| io::Error::other("No cameras found"))?;

        println!("Using camera: {}", device.name);
        device.name.clone()
    };

    sender.send(Message::StartCamera(CameraStartMessage {
        device_name: device_name.clone(),
    }))?;

    let result = event_socket
        .responses
        .recv_timeout(Duration::from_secs(10))
        .map_err(|e| io::Error::other(format!("Failed to receive StartCameraResult: {e:?}")))?;

    match result {
        Message::StartCameraResult(Ok(())) => {
            println!("Camera started successfully");
        }
        Message::StartCameraResult(Err(e)) => {
            return Err(io::Error::other(format!("Camera start failed: {e}")));
        }
        other => {
            return Err(io::Error::other(format!("Unexpected response: {other:?}")));
        }
    }

    println!("Capturing for 30s...");
    std::thread::sleep(Duration::from_secs(30));

    println!("Stopping camera...");
    sender.send(Message::StopCamera)?;
    std::thread::sleep(Duration::from_secs(1));

    println!("Camera 30s test complete");
    Ok(())
}

pub async fn test_camera_track_subscribe() -> io::Result<()> {
    let token = livekit_utils::generate_token("Test Camera Track");
    let url = std::env::var("LIVEKIT_URL").expect("LIVEKIT_URL environment variable not set");

    let (room, mut rx) = Room::connect(&url, &token, RoomOptions::default())
        .await
        .map_err(io::Error::other)?;

    println!("Connected to room: {}", room.name());
    println!("Waiting for camera tracks to be subscribed...");

    // Listen for track events
    while let Some(event) = rx.recv().await {
        match event {
            RoomEvent::TrackSubscribed {
                track,
                participant,
                publication,
            } => {
                if track.kind() == TrackKind::Video {
                    println!(
                        "Camera track subscribed from participant '{}' (sid: {}): track name '{}', publication name '{}'",
                        participant.identity(),
                        participant.sid(),
                        track.name(),
                        publication.name()
                    );
                }
            }
            RoomEvent::TrackUnsubscribed {
                track, participant, ..
            } => {
                if track.kind() == TrackKind::Video {
                    println!(
                        "Camera track unsubscribed from participant '{}': track name '{}'",
                        participant.identity(),
                        track.name()
                    );
                }
            }
            RoomEvent::TrackUnpublished {
                publication,
                participant,
            } => {
                if publication.kind() == TrackKind::Video {
                    println!(
                        "Camera track unpublished from participant '{}': publication name '{}'",
                        participant.identity(),
                        publication.name()
                    );
                }
            }
            RoomEvent::ParticipantConnected(participant) => {
                println!(
                    "Participant connected: '{}' (sid: {})",
                    participant.identity(),
                    participant.sid()
                );
            }
            RoomEvent::ParticipantDisconnected(participant) => {
                println!(
                    "Participant disconnected: '{}' (sid: {})",
                    participant.identity(),
                    participant.sid()
                );
            }
            _ => {}
        }
    }

    Ok(())
}

/// Opens the camera window and keeps it alive for manual interaction.
///
/// Usage: `cargo run -- camera open`
///
/// Requires a running core process (`task dev` in core/).
/// The camera window will stay open for 30 seconds.
pub fn test_open_camera() -> io::Result<()> {
    println!("\n=== TEST: Open Camera Window ===");

    let (sender, _event_socket) = screenshare_client::connect_socket()?;
    println!("Connected to socket.");

    screenshare_client::open_camera(&sender)?;
    println!("OpenCamera sent. Camera window should appear.");
    println!("You have 15_000 seconds to interact with the window...");

    std::thread::sleep(Duration::from_secs(15_000));

    println!("Test completed.");
    Ok(())
}
