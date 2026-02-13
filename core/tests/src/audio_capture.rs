use crate::screenshare_client::{call_start, connect_socket};
use socket_lib::{AudioCaptureMessage, EventSocket, Message, SocketSender};
use std::env;
use std::io;
use std::time::Duration;

/// Sends LivekitServerUrl + CallStart, the required setup before audio capture.
fn setup_audio(sender: &SocketSender, event_socket: &EventSocket) -> io::Result<()> {
    let livekit_server_url =
        env::var("LIVEKIT_URL").expect("LIVEKIT_URL environment variable not set");
    sender.send(Message::LivekitServerUrl(livekit_server_url))?;
    call_start(sender, event_socket)
}

pub fn test_list_devices() -> io::Result<()> {
    let (sender, event_socket) = connect_socket()?;
    setup_audio(&sender, &event_socket)?;

    sender.send(Message::ListAudioDevices)?;
    let response = event_socket
        .responses
        .recv_timeout(Duration::from_secs(5))
        .map_err(|e| io::Error::other(format!("Failed to receive AudioDeviceList: {e:?}")))?;

    match response {
        Message::AudioDeviceList(devices) => {
            println!("Found {} audio devices:", devices.len());
            for device in &devices {
                println!("  [{}] {}", device.id, device.name);
            }
            assert!(!devices.is_empty(), "Expected at least one audio device");
        }
        other => {
            return Err(io::Error::other(format!("Unexpected response: {other:?}")));
        }
    }

    Ok(())
}

pub fn test_capture_all_devices(duration_secs: u64) -> io::Result<()> {
    let (sender, event_socket) = connect_socket()?;
    setup_audio(&sender, &event_socket)?;

    // List devices first
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

    println!(
        "Testing {} devices for {}s each",
        devices.len(),
        duration_secs
    );

    for device in &devices {
        println!("Capturing from: {} (id {})", device.name, device.id);

        sender.send(Message::StartAudioCapture(AudioCaptureMessage {
            device_id: device.id.clone(),
        }))?;

        let result = event_socket
            .responses
            .recv_timeout(Duration::from_secs(10))
            .map_err(|e| {
                io::Error::other(format!("Failed to receive StartAudioCaptureResult: {e:?}"))
            })?;

        match result {
            Message::StartAudioCaptureResult(Ok(())) => {
                println!("  Capture started successfully");
            }
            Message::StartAudioCaptureResult(Err(e)) => {
                println!("  Capture failed: {e}, skipping");
                continue;
            }
            other => {
                return Err(io::Error::other(format!("Unexpected response: {other:?}")));
            }
        }

        std::thread::sleep(Duration::from_secs(duration_secs));

        sender.send(Message::StopAudioCapture)?;
        // Give it a moment to clean up
        std::thread::sleep(Duration::from_secs(1));
        println!("  Capture stopped");
    }

    Ok(())
}

pub fn test_mute_unmute() -> io::Result<()> {
    let (sender, event_socket) = connect_socket()?;
    setup_audio(&sender, &event_socket)?;

    // List devices and pick first one
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
        .first()
        .ok_or_else(|| io::Error::other("No audio devices found"))?;

    println!("Using device: {} (id: {})", device.name, device.id);

    // Start capture
    sender.send(Message::StartAudioCapture(AudioCaptureMessage {
        device_id: device.id.clone(),
    }))?;

    let result = event_socket
        .responses
        .recv_timeout(Duration::from_secs(10))
        .map_err(|e| {
            io::Error::other(format!("Failed to receive StartAudioCaptureResult: {e:?}"))
        })?;

    match result {
        Message::StartAudioCaptureResult(Ok(())) => println!("Capture started"),
        Message::StartAudioCaptureResult(Err(e)) => {
            return Err(io::Error::other(format!("Capture failed: {e}")));
        }
        other => {
            return Err(io::Error::other(format!("Unexpected response: {other:?}")));
        }
    }

    println!("Capturing for 2s...");
    std::thread::sleep(Duration::from_secs(2));

    println!("Muting...");
    sender.send(Message::MuteAudio)?;
    std::thread::sleep(Duration::from_secs(2));

    println!("Unmuting...");
    sender.send(Message::UnmuteAudio)?;
    std::thread::sleep(Duration::from_secs(2));

    println!("Stopping capture...");
    sender.send(Message::StopAudioCapture)?;
    std::thread::sleep(Duration::from_secs(1));

    println!("Mute/unmute test complete");
    Ok(())
}

pub fn test_capture_30s(mic_id: Option<&str>) -> io::Result<()> {
    let (sender, event_socket) = connect_socket()?;
    setup_audio(&sender, &event_socket)?;

    let device_id = if let Some(id) = mic_id {
        println!("Using explicitly provided device ID: {}", id);
        id.to_string()
    } else {
        // List devices and pick first one
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
            .first()
            .ok_or_else(|| io::Error::other("No audio devices found"))?;

        println!("Using device: {} (id: {})", device.name, device.id);
        device.id.clone()
    };

    sender.send(Message::StartAudioCapture(AudioCaptureMessage {
        device_id,
    }))?;

    let result = event_socket
        .responses
        .recv_timeout(Duration::from_secs(10))
        .map_err(|e| {
            io::Error::other(format!("Failed to receive StartAudioCaptureResult: {e:?}"))
        })?;

    match result {
        Message::StartAudioCaptureResult(Ok(())) => println!("Capture started"),
        Message::StartAudioCaptureResult(Err(e)) => {
            return Err(io::Error::other(format!("Capture failed: {e}")));
        }
        other => {
            return Err(io::Error::other(format!("Unexpected response: {other:?}")));
        }
    }

    println!("Capturing for 30s...");
    std::thread::sleep(Duration::from_secs(30));

    println!("Stopping capture...");
    sender.send(Message::StopAudioCapture)?;
    std::thread::sleep(Duration::from_secs(1));

    println!("30s capture test complete");
    Ok(())
}
