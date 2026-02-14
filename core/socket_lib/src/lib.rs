use serde::{Deserialize, Serialize};
use std::fmt;
use std::fs;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

// Platform-specific imports
#[cfg(unix)]
use std::os::unix::net::{UnixListener, UnixStream};

#[cfg(windows)]
use std::net::{TcpListener, TcpStream};

#[repr(C)]
#[derive(Debug, Copy, Clone, bytemuck::Pod, bytemuck::Zeroable, Serialize, Deserialize)]
pub struct Extent {
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct WindowFrameMessage {
    pub origin_x: f64,
    pub origin_y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CursorPositionMessage {
    pub x: f32,
    pub y: f32,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MouseClickMessage {
    pub x: f32,
    pub y: f32,
    pub button: u32,
    pub clicks: f32,
    pub shift_key: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ScrollMessage {
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct KeystrokeMessage {
    pub key: String,
    pub meta: bool,
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub down: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy)]
pub enum ContentType {
    Display,
    Window { display_id: u32 },
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy)]
pub struct Content {
    pub content_type: ContentType,
    pub id: u32,
}

impl fmt::Display for Content {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.content_type {
            ContentType::Display => write!(f, "Display {}", self.id),
            ContentType::Window { display_id } => {
                write!(f, "Window {} on Display {}", self.id, display_id)
            }
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CaptureContent {
    pub content: Content,
    pub base64: String,
    pub title: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AvailableContentMessage {
    pub content: Vec<CaptureContent>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ScreenShareMessage {
    pub content: Content,
    pub resolution: Extent,
    pub accessibility_permission: bool,
    pub use_av1: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CallStartMessage {
    pub token: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SentryMetadata {
    pub user_email: String,
    pub app_version: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DrawingEnabled {
    pub permanent: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AudioDevice {
    pub name: String,
    pub id: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AudioCaptureMessage {
    pub device_id: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CameraDevice {
    pub name: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CameraStartMessage {
    pub device_name: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum Message {
    GetAvailableContent,
    AvailableContent(AvailableContentMessage),
    CallStart(CallStartMessage),
    CallStartResult(Result<(), String>),
    CallEnd,
    StartScreenShare(ScreenShareMessage),
    StartScreenShareResult(Result<(), String>),
    StopScreenshare,
    Reset,
    Ping,
    ControllerCursorEnabled(bool),
    LivekitServerUrl(String),
    SentryMetadata(SentryMetadata),
    DrawingEnabled(DrawingEnabled),
    ListAudioDevices,
    AudioDeviceList(Vec<AudioDevice>),
    StartAudioCapture(AudioCaptureMessage),
    StartAudioCaptureResult(Result<(), String>),
    StopAudioCapture,
    MuteAudio,
    UnmuteAudio,
    ListCameras,
    CameraList(Vec<CameraDevice>),
    StartCamera(CameraStartMessage),
    StartCameraResult(Result<(), String>),
    StopCamera,
    CameraFailed(String),
    OpenCamera,
    OpenScreensharing,
}

impl Message {
    pub fn is_response(&self) -> bool {
        matches!(
            self,
            Message::AvailableContent(_)
                | Message::StartScreenShareResult(_)
                | Message::CallStartResult(_)
                | Message::AudioDeviceList(_)
                | Message::StartAudioCaptureResult(_)
                | Message::CameraList(_)
                | Message::StartCameraResult(_)
        )
    }
}

// Platform-specific stream type alias
#[cfg(unix)]
type Stream = UnixStream;
#[cfg(windows)]
type Stream = TcpStream;

/// Send half of the socket. Clone-safe via Arc<Mutex<Stream>>.
#[derive(Clone)]
pub struct SocketSender {
    stream: Arc<Mutex<Stream>>,
}

impl SocketSender {
    pub fn send(&self, message: Message) -> Result<(), std::io::Error> {
        let serialized_message = serde_json::to_string(&message)?;
        let serialized_message = serialized_message.as_bytes();
        let size = serialized_message.len();
        let mut message_bytes = size.to_le_bytes().to_vec();
        message_bytes.extend_from_slice(serialized_message);
        let mut stream = self.stream.lock().unwrap();
        stream.write_all(&message_bytes)?;
        Ok(())
    }
}

/// Dual-channel receiver that owns the background reader thread.
/// Dropping this struct shuts down the underlying stream, which causes the
/// reader thread to exit and closes both channels on the remote side.
pub struct EventSocket {
    pub events: mpsc::Receiver<Message>,
    pub responses: mpsc::Receiver<Message>,
    reader_thread: Option<std::thread::JoinHandle<()>>,
    stream: Stream,
}

impl EventSocket {
    fn new(mut read_stream: Stream, shutdown_stream: Stream) -> Self {
        let (event_tx, event_rx) = mpsc::channel();
        let (response_tx, response_rx) = mpsc::channel();

        log::info!("EventSocket: spawning background reader thread");
        let reader_thread = std::thread::spawn(move || {
            let _ = read_stream.set_read_timeout(Some(Duration::from_secs(1)));
            loop {
                match Self::read_message(&mut read_stream) {
                    Ok(message) => {
                        let tx = if message.is_response() {
                            &response_tx
                        } else {
                            &event_tx
                        };
                        if tx.send(message).is_err() {
                            log::info!("Background reader: channel closed, stopping");
                            break;
                        }
                    }
                    Err(e) => {
                        if e.kind() == std::io::ErrorKind::WouldBlock
                            || e.kind() == std::io::ErrorKind::TimedOut
                        {
                            continue;
                        }
                        log::error!("Background reader: IO error: {e:?}");
                        break;
                    }
                }
            }
            log::info!("Background reader: thread exiting");
        });

        Self {
            events: event_rx,
            responses: response_rx,
            reader_thread: Some(reader_thread),
            stream: shutdown_stream,
        }
    }

    fn read_message(stream: &mut Stream) -> Result<Message, std::io::Error> {
        let mut size_buffer = [0u8; std::mem::size_of::<usize>()];
        stream.read_exact(&mut size_buffer)?;
        let message_size = usize::from_le_bytes(size_buffer);

        let mut message_buffer = vec![0u8; message_size];
        stream.read_exact(&mut message_buffer)?;
        let buffer_str =
            String::from_utf8(message_buffer).expect("Failed to convert buffer to string");
        let deserialized_message: Message = serde_json::from_str(&buffer_str)?;
        Ok(deserialized_message)
    }
}

impl Drop for EventSocket {
    fn drop(&mut self) {
        log::info!("EventSocket: shutting down");
        use std::net::Shutdown;
        let _ = self.stream.shutdown(Shutdown::Both);
        if let Some(handle) = self.reader_thread.take() {
            let _ = handle.join();
        }
        log::info!("EventSocket: shutdown complete");
    }
}

fn build_pair(stream: Stream) -> std::io::Result<(SocketSender, EventSocket)> {
    let write_stream = stream.try_clone()?;
    let read_stream = stream.try_clone()?;
    let shutdown_stream = stream;

    let sender = SocketSender {
        stream: Arc::new(Mutex::new(write_stream)),
    };

    let event_socket = EventSocket::new(read_stream, shutdown_stream);

    Ok((sender, event_socket))
}

/// Connect to an existing socket (client side).
pub fn connect(socket_path: &str) -> Result<(SocketSender, EventSocket), std::io::Error> {
    log::info!("Connecting to socket at {socket_path}");
    #[cfg(unix)]
    let stream = {
        let stream = UnixStream::connect(socket_path)?;
        stream.set_read_timeout(None)?;
        stream
    };

    #[cfg(windows)]
    let stream = {
        let port = socket_path_to_port(socket_path);
        let addr = format!("127.0.0.1:{port}");
        let stream = TcpStream::connect(addr)?;
        stream.set_read_timeout(None)?;
        stream
    };

    build_pair(stream)
}

/// Create a socket and wait for a client to connect (server side).
pub fn listen(socket_path: &str) -> Result<(SocketSender, EventSocket), std::io::Error> {
    log::info!("Creating socket at {socket_path}");
    #[cfg(unix)]
    let stream = {
        if Path::new(socket_path).exists() {
            fs::remove_file(socket_path)?;
        }

        let listener = UnixListener::bind(socket_path)?;
        listener.set_nonblocking(true)?;
        let mut stream = None;
        let times = if cfg!(debug_assertions) { 100 } else { 10 };
        for i in 0..times {
            log::info!("Waiting for client {i}/{times}");
            match listener.accept() {
                Ok((s, _)) => {
                    stream = Some(s);
                    break;
                }
                Err(_) => {
                    std::thread::sleep(std::time::Duration::from_secs(1));
                }
            }
        }
        let stream = stream.ok_or_else(|| {
            std::io::Error::other("Client did not connect after multiple attempts")
        })?;
        stream.set_nonblocking(false)?;
        log::info!("Client connected");
        stream.set_read_timeout(None)?;
        stream
    };

    #[cfg(windows)]
    let stream = {
        if Path::new(socket_path).exists() {
            fs::remove_file(socket_path)?;
        }

        // Get initial port to try
        let mut port = socket_path_to_port(socket_path);
        let mut listener = None;

        // Try to bind, incrementing port if necessary
        for try_num in 0..500 {
            let addr = format!("127.0.0.1:{port}");
            match TcpListener::bind(addr) {
                Ok(l) => {
                    listener = Some(l);
                    break;
                }
                Err(_) => {
                    log::info!("Port {port} is in use, trying next port...");
                    port += 1;
                    if try_num != 0 && try_num % 100 == 0 {
                        port += 100;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
            }
        }

        let listener = listener.ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::AddrInUse,
                "Could not find available port after multiple attempts",
            )
        })?;

        // Store just the port number in the file
        fs::write(socket_path, port.to_string())?;

        log::info!("Listening on port {port}, waiting for client");
        listener.set_nonblocking(true)?;
        let mut stream = None;
        for i in 0..10 {
            log::info!("Waiting for client {i}/10");
            match listener.accept() {
                Ok((s, _)) => {
                    stream = Some(s);
                    break;
                }
                Err(_) => {
                    std::thread::sleep(std::time::Duration::from_secs(1));
                }
            }
        }
        let stream = stream.ok_or_else(|| {
            std::io::Error::other("Client did not connect after multiple attempts")
        })?;
        stream.set_nonblocking(false)?;
        log::info!("Client connected");
        stream.set_read_timeout(None)?;
        stream
    };

    build_pair(stream)
}

#[cfg(windows)]
fn socket_path_to_port(socket_path: &str) -> u16 {
    // First try to read the port from the file
    if let Ok(content) = fs::read_to_string(socket_path) {
        if let Ok(port) = content.trim().parse::<u16>() {
            log::info!("Found port {port} in file {socket_path}");
            port
        } else {
            log::warn!("Could not parse port from file {socket_path}: '{content}'");
            calculate_port_from_hash(socket_path)
        }
    } else {
        log::debug!("Could not read port from file {socket_path}, calculating from hash");
        calculate_port_from_hash(socket_path)
    }
}

#[cfg(windows)]
fn calculate_port_from_hash(socket_path: &str) -> u16 {
    let mut hash: u32 = 5381;
    for byte in socket_path.bytes() {
        hash = ((hash << 5).wrapping_add(hash)).wrapping_add(byte as u32);
    }
    // Use ports in range 49152-65535 (dynamic/private range)
    (hash % 15900 + 49152) as u16
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[cfg(unix)]
    fn test_pair() -> ((SocketSender, EventSocket), (SocketSender, EventSocket)) {
        let dir = tempfile::tempdir().unwrap();
        let socket_path = dir.path().join("test.sock");
        let socket_path_str = socket_path.to_str().unwrap().to_string();
        let socket_path_str2 = socket_path_str.clone();

        let server_handle = std::thread::spawn(move || listen(&socket_path_str).unwrap());

        // Small delay to let server start listening
        std::thread::sleep(Duration::from_millis(100));

        let client = connect(&socket_path_str2).unwrap();
        let server = server_handle.join().unwrap();
        // Keep tempdir alive by leaking it (tests are short-lived)
        std::mem::forget(dir);
        (server, client)
    }

    #[test]
    fn test_send_recv_event() {
        let ((_server_sender, server_events), (client_sender, _client_events)) = test_pair();

        // Send an event-type message from client to server
        client_sender.send(Message::GetAvailableContent).unwrap();

        let msg = server_events
            .events
            .recv_timeout(Duration::from_secs(5))
            .unwrap();
        assert!(matches!(msg, Message::GetAvailableContent));
    }

    #[test]
    fn test_send_recv_response() {
        let ((server_sender, _server_events), (_client_sender, client_events)) = test_pair();

        // Send a response-type message from server to client
        server_sender
            .send(Message::AvailableContent(AvailableContentMessage {
                content: vec![],
            }))
            .unwrap();

        let msg = client_events
            .responses
            .recv_timeout(Duration::from_secs(5))
            .unwrap();
        assert!(matches!(msg, Message::AvailableContent(_)));
    }

    #[test]
    fn test_routing_mixed() {
        let ((_server_sender, server_events), (client_sender, _client_events)) = test_pair();

        // Send mix of events and responses from client to server
        client_sender.send(Message::GetAvailableContent).unwrap();
        client_sender
            .send(Message::AvailableContent(AvailableContentMessage {
                content: vec![],
            }))
            .unwrap();
        client_sender.send(Message::Ping).unwrap();
        client_sender
            .send(Message::StartScreenShareResult(Ok(())))
            .unwrap();
        client_sender
            .send(Message::CallStart(CallStartMessage {
                token: "test-token".to_string(),
            }))
            .unwrap();
        client_sender.send(Message::CallEnd).unwrap();
        client_sender
            .send(Message::CallStartResult(Ok(())))
            .unwrap();

        // Events: GetAvailableContent, Ping, CallStart, CallEnd
        let msg1 = server_events
            .events
            .recv_timeout(Duration::from_secs(5))
            .unwrap();
        assert!(matches!(msg1, Message::GetAvailableContent));
        let msg2 = server_events
            .events
            .recv_timeout(Duration::from_secs(5))
            .unwrap();
        assert!(matches!(msg2, Message::Ping));
        let msg5 = server_events
            .events
            .recv_timeout(Duration::from_secs(5))
            .unwrap();
        assert!(matches!(msg5, Message::CallStart(_)));
        let msg6 = server_events
            .events
            .recv_timeout(Duration::from_secs(5))
            .unwrap();
        assert!(matches!(msg6, Message::CallEnd));

        // Responses: AvailableContent, StartScreenShareResult, CallStartResult
        let msg3 = server_events
            .responses
            .recv_timeout(Duration::from_secs(5))
            .unwrap();
        assert!(matches!(msg3, Message::AvailableContent(_)));
        let msg4 = server_events
            .responses
            .recv_timeout(Duration::from_secs(5))
            .unwrap();
        assert!(matches!(msg4, Message::StartScreenShareResult(_)));
        let msg7 = server_events
            .responses
            .recv_timeout(Duration::from_secs(5))
            .unwrap();
        assert!(matches!(msg7, Message::CallStartResult(_)));
    }

    #[test]
    fn test_multiple_messages_ordering() {
        let ((_server_sender, server_events), (client_sender, _client_events)) = test_pair();

        let n = 10;
        for _ in 0..n {
            client_sender.send(Message::Ping).unwrap();
        }

        for _ in 0..n {
            let msg = server_events
                .events
                .recv_timeout(Duration::from_secs(5))
                .unwrap();
            assert!(matches!(msg, Message::Ping));
        }
    }

    #[test]
    fn test_sender_clone() {
        let ((_server_sender, server_events), (client_sender, _client_events)) = test_pair();

        let clone = client_sender.clone();
        client_sender.send(Message::Ping).unwrap();
        clone.send(Message::GetAvailableContent).unwrap();

        let msg1 = server_events
            .events
            .recv_timeout(Duration::from_secs(5))
            .unwrap();
        let msg2 = server_events
            .events
            .recv_timeout(Duration::from_secs(5))
            .unwrap();

        // Both should arrive (order guaranteed since same underlying stream with mutex)
        assert!(matches!(msg1, Message::Ping));
        assert!(matches!(msg2, Message::GetAvailableContent));
    }

    #[test]
    fn test_response_timeout() {
        let ((_server_sender, server_events), (_client_sender, _client_events)) = test_pair();

        let result = server_events
            .responses
            .recv_timeout(Duration::from_millis(100));
        assert!(result.is_err());
    }

    #[test]
    fn test_disconnect_closes_channels() {
        let ((_server_sender, server_events), (client_sender, _client_events)) = test_pair();

        // Drop client sender to trigger disconnect
        drop(client_sender);
        drop(_client_events);

        // Server's event channel should eventually close
        // Keep receiving until we get a disconnect error
        loop {
            match server_events.events.recv_timeout(Duration::from_secs(5)) {
                Ok(_) => continue,
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    panic!("Timed out waiting for channel to close")
                }
            }
        }
    }

    #[test]
    fn test_request_response_pattern() {
        let ((server_sender, server_events), (client_sender, client_events)) = test_pair();

        // Client sends request
        client_sender.send(Message::GetAvailableContent).unwrap();

        // Server receives event
        let msg = server_events
            .events
            .recv_timeout(Duration::from_secs(5))
            .unwrap();
        assert!(matches!(msg, Message::GetAvailableContent));

        // Server sends response
        server_sender
            .send(Message::AvailableContent(AvailableContentMessage {
                content: vec![],
            }))
            .unwrap();

        // Client receives response on responses channel
        let response = client_events
            .responses
            .recv_timeout(Duration::from_secs(5))
            .unwrap();
        assert!(matches!(response, Message::AvailableContent(_)));
    }
}
