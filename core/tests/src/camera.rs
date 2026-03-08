use crate::livekit_utils;
use crate::screenshare_client::{self, call_start_with_name, connect_socket};
use livekit::prelude::*;
use socket_lib::{AudioCaptureMessage, CameraStartMessage, EventSocket, Message, SocketSender};
use std::env;
use std::io;
use std::time::Duration;

fn setup_camera(sender: &SocketSender, event_socket: &EventSocket, name: &str) -> io::Result<()> {
    let livekit_server_url =
        env::var("LIVEKIT_URL").expect("LIVEKIT_URL environment variable not set");
    sender.send(Message::LivekitServerUrl(livekit_server_url))?;
    call_start_with_name(sender, event_socket, name)
}

pub fn test_list_cameras() -> io::Result<()> {
    let (sender, event_socket) = connect_socket()?;
    setup_camera(&sender, &event_socket, "Test Camera")?;

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
    setup_camera(&sender, &event_socket, "Test Camera")?;

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
        device_name: Some(device_name.clone()),
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

/// Joins a call with camera and mic, stays until Ctrl-C.
pub fn test_call(
    camera_name: Option<&str>,
    mic_id: Option<&str>,
    name: &str,
    screenshare: bool,
) -> io::Result<()> {
    println!("\n=== TEST: Call with Camera + Mic ===");

    let (sender, event_socket) = connect_socket()?;
    setup_camera(&sender, &event_socket, name)?;

    // Start camera — validate the name against available devices first
    let mut camera_started = false;

    let device_name = if let Some(name) = camera_name {
        // Check if the requested camera actually exists
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

        if devices.iter().any(|d| d.name == name) {
            println!("Using explicitly provided camera: {}", name);
            Some(name.to_string())
        } else {
            println!(
                "Camera '{}' not found. Available cameras: [{}]. Skipping camera.",
                name,
                devices
                    .iter()
                    .map(|d| d.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            None
        }
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

        println!("Found {:?} cameras:", devices);
        match devices.first() {
            Some(device) => {
                println!("Using camera: {}", device.name);
                Some(device.name.clone())
            }
            None => {
                println!("No cameras found. Skipping camera.");
                None
            }
        }
    };

    // if let Some(device_name) = device_name {
    //     sender.send(Message::StartCamera(CameraStartMessage {
    //         device_name: Some(device_name),
    //     }))?;

    //     match event_socket
    //         .responses
    //         .recv_timeout(Duration::from_secs(10))
    //         .map_err(|e| io::Error::other(format!("Failed to receive StartCameraResult: {e:?}")))?
    //     {
    //         Message::StartCameraResult(Ok(())) => {
    //             println!("Camera started successfully");
    //             camera_started = true;
    //         }
    //         Message::StartCameraResult(Err(e)) => {
    //             println!("Camera start failed: {e}. Continuing without camera.");
    //         }
    //         other => {
    //             return Err(io::Error::other(format!("Unexpected response: {other:?}")));
    //         }
    //     }
    // }

    // Start mic
    let device_name = if let Some(name) = mic_id {
        println!("Using explicitly provided mic: {}", name);
        name.to_string()
    } else {
        sender.send(Message::ListAudioDevices)?;
        let response = event_socket
            .responses
            .recv_timeout(Duration::from_secs(5))
            .map_err(|e| io::Error::other(format!("Failed to receive AudioDeviceList: {e:?}")))?;

        let devices = match response {
            Message::AudioDeviceList(devices) => devices,
            other => {
                return Err(io::Error::other(format!("Unexpected response: {other:?}")));
            }
        };

        let device = devices
            .last()
            .ok_or_else(|| io::Error::other("No audio devices found"))?;

        println!("Using mic: {}", device.name);
        device.name.clone()
    };

    sender.send(Message::StartAudioCapture(AudioCaptureMessage {
        device_name,
    }))?;

    match event_socket
        .responses
        .recv_timeout(Duration::from_secs(10))
        .map_err(|e| {
            io::Error::other(format!("Failed to receive StartAudioCaptureResult: {e:?}"))
        })? {
        Message::StartAudioCaptureResult(Ok(())) => println!("Mic started successfully"),
        Message::StartAudioCaptureResult(Err(e)) => {
            return Err(io::Error::other(format!("Mic start failed: {e}")));
        }
        other => {
            return Err(io::Error::other(format!("Unexpected response: {other:?}")));
        }
    }

    // Start screen sharing if requested
    if screenshare {
        println!("Starting screen share...");
        let available_content = screenshare_client::get_available_content(&sender, &event_socket)?;
        match available_content {
            Message::AvailableContent(content_msg) => {
                if let Some(capture_content) = content_msg.content.first() {
                    println!("Using display: {}", capture_content.content.id);

                    // Use default resolution for screenshare
                    let width = 1920.0;
                    let height = 1080.0;

                    screenshare_client::request_screenshare(
                        &sender,
                        &event_socket,
                        capture_content.content.id,
                        //1,
                        width,
                        height,
                    )?;
                    println!("Screen share started successfully");
                } else {
                    return Err(io::Error::other("No displays found"));
                }
            }
            other => {
                return Err(io::Error::other(format!("Unexpected response: {other:?}")));
            }
        }
    }

    println!("In call with camera and mic. Press Ctrl-C to stop.");

    let (shutdown_tx, shutdown_rx) = std::sync::mpsc::channel();
    ctrlc::set_handler(move || {
        let _ = shutdown_tx.send(());
    })
    .map_err(|e| io::Error::other(format!("Failed to set Ctrl-C handler: {e}")))?;

    shutdown_rx.recv().ok();
    println!("\nCtrl-C received, stopping...");

    if screenshare {
        sender.send(Message::StopScreenshare)?;
    }
    if camera_started {
        sender.send(Message::StopCamera)?;
    }
    sender.send(Message::StopAudioCapture)?;
    std::thread::sleep(Duration::from_secs(1));

    println!("Call test complete");
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
