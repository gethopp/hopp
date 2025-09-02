use crate::livekit_utils;
use socket_lib::CaptureContent;
use socket_lib::{Content, ContentType, CursorSocket, Extent, Message, ScreenShareMessage};
use std::env;
use std::io;

/// Creates and connects to the cursor socket.
pub fn connect_socket() -> io::Result<CursorSocket> {
    let tmp_folder = std::env::temp_dir();
    // Consider making the socket name configurable or discoverable if needed
    let socket_path = format!("{}/core-socket", tmp_folder.display());
    println!("Connecting to socket: {socket_path}");
    // Use the function from the new module
    CursorSocket::new(&socket_path)
}

/// Sends a request to get available screen content and returns the response.
pub fn get_available_content(socket: &mut CursorSocket) -> io::Result<Message> {
    let message = Message::GetAvailableContent;
    socket.send_message(message)?;
    socket.receive_message()
}

/// Sends a request to start screen sharing.
pub fn request_screenshare(
    socket: &mut CursorSocket,
    content_id: u32,
    width: f64,
    height: f64,
) -> io::Result<()> {
    let token = livekit_utils::generate_token("Test Screenshare");

    let message = Message::CallStarted { token };
    socket.send_message(message)?;
    let res = socket.receive_message()?;
    if let Message::CallStartedResult(res) = res {
        if !res {
            return Err(io::Error::other("Failed to start call"));
        }
    } else {
        return Err(io::Error::other("Failed to receive call started result"));
    }

    let message = Message::StartScreenShare(ScreenShareMessage {
        content: Content {
            content_type: ContentType::Display, // Assuming Display type
            id: content_id,
        },
        resolution: Extent { width, height },
    });
    socket.send_message(message)
}

/// Sends a request to stop screen sharing.
pub fn stop_screenshare(socket: &mut CursorSocket) -> io::Result<()> {
    let message = Message::StopScreenshare;
    socket.send_message(message)
}

pub fn screenshare_test() -> io::Result<()> {
    let mut socket = connect_socket()?;
    println!("Connected to socket.");

    let livekit_server_url =
        env::var("LIVEKIT_URL").expect("LIVEKIT_URL environment variable not set");
    socket.send_message(Message::LivekitServerUrl(livekit_server_url))?;

    let available_content = match get_available_content(&mut socket)? {
        Message::AvailableContent(available_content) => available_content,
        _ => return Err(io::Error::other("Failed to get available content")),
    };

    // Start screen share
    let width = 1920.0;
    let height = 1080.0;
    request_screenshare(
        &mut socket,
        available_content.content[0].content.id,
        width,
        height,
    )?;
    println!("Screen share started.");

    std::thread::sleep(std::time::Duration::from_secs(20)); // Wait for a moment

    // Stop screen share
    stop_screenshare(&mut socket)?;
    println!("Screen share stopped.");

    Ok(())
}

pub fn start_screenshare_session() -> io::Result<(CursorSocket, Vec<CaptureContent>)> {
    println!("Connecting to screenshare socket...");
    let mut socket = connect_socket()?;
    println!("Connected to socket.");

    let livekit_server_url =
        env::var("LIVEKIT_URL").expect("LIVEKIT_URL environment variable not set");
    socket.send_message(Message::LivekitServerUrl(livekit_server_url))?;

    let available_content = match get_available_content(&mut socket)? {
        Message::AvailableContent(available_content) => available_content,
        _ => return Err(io::Error::other("Failed to get available content")),
    };

    let width = 1920.0;
    let height = 1080.0;

    println!("Requesting screenshare start...");
    request_screenshare(
        &mut socket,
        available_content.content[0].content.id,
        width,
        height,
    )?;
    println!("Screenshare requested. Waiting a moment for it to initialize...");
    std::thread::sleep(std::time::Duration::from_secs(2));
    Ok((socket, available_content.content))
}

pub fn stop_screenshare_session(socket: &mut CursorSocket) -> io::Result<()> {
    println!("Stopping screenshare...");
    stop_screenshare(socket)?;
    println!("Screenshare stopped.");
    Ok(())
}
