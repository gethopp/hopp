use crate::livekit_utils;
use socket_lib::CaptureContent;
use socket_lib::{
    Content, ContentType, EventSocket, Extent, Message, ScreenShareMessage, SocketSender,
};
use std::env;
use std::io;
use std::time::Duration;

/// Creates and connects to the cursor socket.
pub fn connect_socket() -> io::Result<(SocketSender, EventSocket)> {
    let tmp_folder = std::env::temp_dir();
    // Consider making the socket name configurable or discoverable if needed
    let socket_path = format!("{}/core-socket", tmp_folder.display());
    println!("Connecting to socket: {socket_path}");
    socket_lib::connect(&socket_path)
}

/// Sends a request to get available screen content and returns the response.
pub fn get_available_content(
    sender: &SocketSender,
    event_socket: &EventSocket,
) -> io::Result<Message> {
    let message = Message::GetAvailableContent;
    sender.send(message)?;
    event_socket
        .responses
        .recv_timeout(Duration::from_secs(5))
        .map_err(|e| io::Error::other(format!("Failed to receive response: {e:?}")))
}

/// Sends a request to start screen sharing.
pub fn request_screenshare(
    sender: &SocketSender,
    event_socket: &EventSocket,
    content_id: u32,
    width: f64,
    height: f64,
) -> io::Result<()> {
    let token = livekit_utils::generate_token("Test Screenshare");

    let message = Message::StartScreenShare(ScreenShareMessage {
        content: Content {
            content_type: ContentType::Display, // Assuming Display type
            id: content_id,
        },
        token,
        resolution: Extent { width, height },
        accessibility_permission: true,
        use_av1: false,
    });
    sender.send(message).unwrap();

    match event_socket.responses.recv_timeout(Duration::from_secs(5)) {
        Ok(_message) => Ok(()),
        Err(e) => Err(io::Error::other(format!(
            "Failed to receive message: {e:?}"
        ))),
    }
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

    let available_content = match get_available_content(&sender, &event_socket)? {
        Message::AvailableContent(available_content) => available_content,
        _ => return Err(io::Error::other("Failed to get available content")),
    };

    // Start screen share
    let width = 1920.0;
    let height = 1080.0;
    request_screenshare(
        &sender,
        &event_socket,
        available_content.content[0].content.id,
        width,
        height,
    )?;
    println!("Screen share started.");

    std::thread::sleep(std::time::Duration::from_secs(20)); // Wait for a moment

    // Stop screen share
    stop_screenshare(&sender)?;
    println!("Screen share stopped.");

    Ok(())
}

pub fn start_screenshare_session() -> io::Result<(SocketSender, EventSocket, Vec<CaptureContent>)> {
    println!("Connecting to screenshare socket...");
    let (sender, event_socket) = connect_socket()?;
    println!("Connected to socket.");

    let livekit_server_url =
        env::var("LIVEKIT_URL").expect("LIVEKIT_URL environment variable not set");
    sender.send(Message::LivekitServerUrl(livekit_server_url))?;

    let available_content = match get_available_content(&sender, &event_socket)? {
        Message::AvailableContent(available_content) => available_content,
        _ => return Err(io::Error::other("Failed to get available content")),
    };

    let width = 1920.0;
    let height = 1080.0;

    println!("Requesting screenshare start...");
    request_screenshare(
        &sender,
        &event_socket,
        available_content.content[0].content.id,
        width,
        height,
    )?;
    println!("Screenshare requested. Waiting a moment for it to initialize...");
    std::thread::sleep(std::time::Duration::from_secs(2));
    Ok((sender, event_socket, available_content.content))
}

pub fn stop_screenshare_session(sender: &SocketSender) -> io::Result<()> {
    println!("Stopping screenshare...");
    stop_screenshare(sender)?;
    println!("Screenshare stopped.");
    Ok(())
}

/// Tests that get_available_content returns consistent results across multiple calls.
pub fn test_available_content_consistency() -> io::Result<()> {
    println!("Testing available content consistency...");
    let (sender, event_socket) = connect_socket()?;
    println!("Connected to socket.");

    let livekit_server_url =
        env::var("LIVEKIT_URL").expect("LIVEKIT_URL environment variable not set");
    sender.send(Message::LivekitServerUrl(livekit_server_url))?;

    let mut content_lengths = Vec::new();

    // Request available content 10 times
    for i in 1..=10 {
        let available_content = match get_available_content(&sender, &event_socket)? {
            Message::AvailableContent(available_content) => available_content,
            _ => return Err(io::Error::other("Failed to get available content")),
        };

        let content_len = available_content.content.len();
        content_lengths.push(content_len);
        println!("Request {}: got {} content items", i, content_len);
    }

    // Verify all lengths are the same
    let first_len = content_lengths[0];
    let all_same = content_lengths.iter().all(|&len| len == first_len);

    if all_same {
        println!(
            "✓ Success: All 10 requests returned {} content items",
            first_len
        );
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "Content length inconsistency detected. Lengths: {:?}",
            content_lengths
        )))
    }
}

pub fn test_every_monitor() -> io::Result<()> {
    let (sender, event_socket) = connect_socket()?;
    println!("Connected to socket.");

    let livekit_server_url =
        env::var("LIVEKIT_URL").expect("LIVEKIT_URL environment variable not set");
    sender.send(Message::LivekitServerUrl(livekit_server_url))?;

    let available_content = match get_available_content(&sender, &event_socket)? {
        Message::AvailableContent(available_content) => available_content,
        _ => return Err(io::Error::other("Failed to get available content")),
    };

    let monitors: Vec<_> = available_content
        .content
        .into_iter()
        .filter(|c| matches!(c.content.content_type, ContentType::Display))
        .collect();

    println!("Found {} monitors to test.", monitors.len());

    for (i, monitor) in monitors.iter().enumerate() {
        println!(
            "Testing monitor {}/{} (ID: {})",
            i + 1,
            monitors.len(),
            monitor.content.id
        );

        // Start screen share
        let width = 1920.0;
        let height = 1080.0;
        request_screenshare(&sender, &event_socket, monitor.content.id, width, height)?;
        println!("Screen share started for monitor {}.", monitor.content.id);

        std::thread::sleep(std::time::Duration::from_secs(10));

        // Stop screen share
        stop_screenshare(&sender)?;
        println!("Screen share stopped for monitor {}.", monitor.content.id);

        // Small delay between monitors
        std::thread::sleep(std::time::Duration::from_secs(1));
    }

    println!("✓ Success: All monitors tested.");
    Ok(())
}
