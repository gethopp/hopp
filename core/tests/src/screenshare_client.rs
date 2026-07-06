use crate::livekit_utils;
use socket_lib::{
    CallStartMessage, Content, ContentType, EventSocket, Extent, Message, ScreenShareMessage,
    SocketSender,
};
use std::env;
use std::io;
use std::time::Duration;

/// Creates and connects to the cursor socket.
pub fn connect_socket() -> io::Result<(SocketSender, EventSocket)> {
    let socket_path = crate::SOCKET_PATH
        .get()
        .expect("SOCKET_PATH not initialized");
    println!("Connecting to socket: {socket_path}");
    socket_lib::connect(socket_path)
}

/// Returns the screen content id to capture, read from the `HOPP_TEST_SCREEN_ID`
/// environment variable. Falls back to `0` if unset.
fn screen_id() -> u32 {
    env::var("HOPP_TEST_SCREEN_ID")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0)
}

/// Sends a CallStart message with a token and waits for the result.
pub fn call_start(sender: &SocketSender, event_socket: &EventSocket) -> io::Result<()> {
    call_start_with_name(sender, event_socket, "Test Screenshare")
}

pub fn call_start_with_name(
    sender: &SocketSender,
    event_socket: &EventSocket,
    name: &str,
) -> io::Result<()> {
    let audio_token = livekit_utils::generate_token(&format!("{name} Audio"));
    let video_token = livekit_utils::generate_token(&format!("{name} Video"));
    sender.send(Message::CallStart(CallStartMessage {
        audio_token,
        video_token,
        audio_device_name: String::new(),
        start_mic_on_call: None,
        start_camera_on_call: None,
    }))?;
    match event_socket.responses.recv_timeout(Duration::from_secs(10)) {
        Ok(Message::CallStartResult(Ok(()))) => Ok(()),
        Ok(Message::CallStartResult(Err(e))) => {
            Err(io::Error::other(format!("CallStart failed: {e}")))
        }
        Ok(msg) => Err(io::Error::other(format!(
            "Unexpected response to CallStart: {msg:?}"
        ))),
        Err(e) => Err(io::Error::other(format!(
            "Failed to receive CallStartResult: {e:?}"
        ))),
    }
}

/// Sends a CallEnd message.
pub fn call_end(sender: &SocketSender) -> io::Result<()> {
    sender.send(Message::CallEnd)
}

/// Sends a request to start screen sharing.
pub fn request_screenshare(
    sender: &SocketSender,
    event_socket: &EventSocket,
    content_id: u32,
    width: f64,
    height: f64,
) -> io::Result<()> {
    let message = Message::StartScreenShare(ScreenShareMessage {
        content: Content {
            content_type: ContentType::Display,
            id: content_id,
        },
        resolution: Extent { width, height },
    });
    sender.send(message).unwrap();

    match event_socket.events.recv_timeout(Duration::from_secs(5)) {
        Ok(Message::StartScreenShareResult(Ok(()))) => Ok(()),
        Ok(Message::StartScreenShareResult(Err(e))) => {
            Err(io::Error::other(format!("StartScreenShare failed: {e}")))
        }
        Ok(msg) => Err(io::Error::other(format!(
            "Unexpected response to StartScreenShare: {msg:?}"
        ))),
        Err(e) => Err(io::Error::other(format!(
            "Failed to receive StartScreenShareResult: {e:?}"
        ))),
    }
}

/// Sends a request to open the camera window.
pub fn open_camera(sender: &SocketSender) -> io::Result<()> {
    sender.send(Message::OpenCamera)
}

/// Sends a request to open the screensharing window.
pub fn open_screensharing(sender: &SocketSender) -> io::Result<()> {
    sender.send(Message::OpenScreensharing)
}

/// Sends a request to stop screen sharing.
pub fn stop_screenshare(sender: &SocketSender) -> io::Result<()> {
    let message = Message::StopScreenshare;
    sender.send(message)
}

pub fn screenshare_test() -> io::Result<()> {
    let (sender, event_socket) = connect_socket()?;
    println!("Connected to socket.");

    let livekit_server_url =
        env::var("LIVEKIT_URL").expect("LIVEKIT_URL environment variable not set");
    sender.send(Message::LivekitServerUrl(livekit_server_url))?;

    // Start call
    call_start(&sender, &event_socket)?;
    println!("Call started.");

    // Start screen share
    let width = 1920.0;
    let height = 1080.0;
    request_screenshare(&sender, &event_socket, screen_id(), width, height)?;
    println!("Screen share started.");

    std::thread::sleep(std::time::Duration::from_secs(20)); // Wait for a moment

    // Stop screen share
    stop_screenshare(&sender)?;
    println!("Screen share stopped.");

    // End call
    call_end(&sender)?;
    println!("Call ended.");

    Ok(())
}

pub fn start_screenshare_session() -> io::Result<(SocketSender, EventSocket)> {
    println!("Connecting to screenshare socket...");
    let (sender, event_socket) = connect_socket()?;
    println!("Connected to socket.");

    let livekit_server_url =
        env::var("LIVEKIT_URL").expect("LIVEKIT_URL environment variable not set");
    sender.send(Message::LivekitServerUrl(livekit_server_url))?;

    // Start the call first
    call_start(&sender, &event_socket)?;
    println!("Call started.");

    std::thread::sleep(std::time::Duration::from_secs(5));

    let width = 1920.0;
    let height = 1080.0;

    println!("Requesting screenshare start...");
    request_screenshare(&sender, &event_socket, screen_id(), width, height)?;
    println!("Screenshare requested.");
    Ok((sender, event_socket))
}

pub fn stop_screenshare_session(sender: &SocketSender) -> io::Result<()> {
    println!("Stopping screenshare...");
    stop_screenshare(sender)?;
    println!("Screenshare stopped.");

    call_end(sender)?;
    println!("Call ended.");
    Ok(())
}

pub fn test_every_monitor() -> io::Result<()> {
    let (sender, event_socket) = connect_socket()?;
    println!("Connected to socket.");

    let livekit_server_url =
        env::var("LIVEKIT_URL").expect("LIVEKIT_URL environment variable not set");
    sender.send(Message::LivekitServerUrl(livekit_server_url))?;

    let id = screen_id();
    println!("Testing screen ID {id}");

    call_start(&sender, &event_socket)?;
    println!("Call started.");

    let width = 1920.0;
    let height = 1080.0;
    request_screenshare(&sender, &event_socket, id, width, height)?;
    println!("Screen share started for screen {id}.");

    std::thread::sleep(std::time::Duration::from_secs(10));

    stop_screenshare(&sender)?;
    println!("Screen share stopped for screen {id}.");

    call_end(&sender)?;
    println!("Call ended.");

    println!("✓ Success: screen tested.");
    Ok(())
}

/// Test call restart cycle: start call, wait 5s, end call, start another call
pub fn test_call_restart_cycle() -> io::Result<()> {
    println!("Testing call restart cycle...");
    let (sender, event_socket) = connect_socket()?;
    println!("Connected to socket.");

    let livekit_server_url =
        env::var("LIVEKIT_URL").expect("LIVEKIT_URL environment variable not set");
    sender.send(Message::LivekitServerUrl(livekit_server_url))?;

    // First call
    println!("Starting first call...");
    call_start(&sender, &event_socket)?;
    println!("First call started.");

    println!("Waiting 5 seconds...");
    std::thread::sleep(std::time::Duration::from_secs(5));

    println!("Ending first call...");
    call_end(&sender)?;
    println!("First call ended.");

    // Small delay before starting second call
    std::thread::sleep(std::time::Duration::from_secs(1));

    // Second call
    println!("Starting second call...");
    call_start(&sender, &event_socket)?;
    println!("Second call started.");

    println!("Waiting 5 seconds...");
    std::thread::sleep(std::time::Duration::from_secs(5));

    println!("Ending second call...");
    call_end(&sender)?;
    println!("Second call ended.");

    println!("✓ Success: Call restart cycle completed.");
    Ok(())
}
