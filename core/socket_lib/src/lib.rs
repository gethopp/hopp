use serde::{Deserialize, Serialize};
use std::fmt;
use std::fs;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::Duration;

// Platform-specific imports
#[cfg(unix)]
use std::os::unix::net::{UnixListener, UnixStream};

#[cfg(windows)]
use std::net::{TcpListener, TcpStream};

pub type RequestId = u64;
pub type CallId = String;
pub mod call;

#[repr(C)]
#[derive(Debug, Copy, Clone, bytemuck::Pod, bytemuck::Zeroable, Serialize, Deserialize)]
pub struct Extent {
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy)]
pub enum ScreenShareResolution {
    P1080,
    P1440,
    P4K,
}

#[derive(Debug, Default, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
pub enum ScreenSharePickerMode {
    #[default]
    Screen,
    Window,
}

impl ScreenShareResolution {
    pub fn extent(self) -> Extent {
        match self {
            ScreenShareResolution::P1080 => Extent {
                width: 1920.0,
                height: 1080.0,
            },
            ScreenShareResolution::P1440 => Extent {
                width: 2560.0,
                height: 1440.0,
            },
            ScreenShareResolution::P4K => Extent {
                width: 4096.0,
                height: 2160.0,
            },
        }
    }
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
    Window,
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
            ContentType::Window => write!(f, "Window {}", self.id),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ScreenShareMessage {
    pub content: Content,
    pub resolution: Extent,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CallStartMessage {
    pub call_id: CallId,
    pub audio_token: String,
    pub video_token: String,
    pub audio_device_name: String,
    pub start_mic_on_call: Option<bool>,
    pub start_camera_on_call: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SentryMetadata {
    pub user_id: String,
    pub app_version: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DrawingEnabled {
    pub permanent: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AudioDevice {
    pub name: String,
    pub default: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AudioCaptureMessage {
    pub device_name: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CameraDevice {
    pub name: String,
    // Camera index now is not stable, the `stable_id` is not yet released.
    // For now we will use the camera name as the id and hope for a new release and
    // no duplicate names 🤞:
    // https://github.com/l1npengtul/nokhwa/blob/ab2bedfbf13dfed4fabe77dcf725012fcaa4d305/nokhwa-core/src/types.rs#L478-L480
    pub id: String,
    pub default: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CameraStartMessage {
    pub call_id: CallId,
    pub device_name: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CoreParticipantState {
    pub identity: String,
    pub name: String,
    pub connected: bool,
    pub muted: bool,
    pub has_camera: bool,
    pub is_screensharing: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum CoreRoleChange {
    Sharer,
    Controller,
    None,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CoreRoleEvent {
    pub role: CoreRoleChange,
}

/// Represents the user's preferred interaction mode for screen sharing sessions.
/// This is stored persistently and restored when the user joins a new session.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type")]
pub enum StoredMode {
    /// Remote control mode - mouse and keyboard events are forwarded to the sharer
    RemoteControl,
    /// Click animation mode - clicks are visualized on the shared screen
    ClickAnimation,
    /// Drawing mode - freehand drawing on the shared screen
    Draw {
        /// If true, drawings persist until manually cleared; if false, they auto-expire
        permanent: bool,
    },
}

#[derive(Debug, Serialize, Deserialize)]
pub enum Message {
    Request {
        request_id: RequestId,
        message: Box<Message>,
    },
    Response {
        request_id: RequestId,
        message: Box<Message>,
    },
    GetAvailableContent,
    CallStart(CallStartMessage),
    CallStartResult(Result<(), String>),
    CallEnd(Option<CallId>),
    StartScreenShare(ScreenShareMessage),
    StartScreenShareResult(Result<(), String>),
    StopScreenshare,
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
    StopCamera(CallId),
    CameraFailed(String),
    OpenCamera,
    OpenScreensharing,
    ToggleMic,
    OpenScreenShareWindow,
    CloseScreenShareWindow,
    OpenStatsWindow,
    BringWindowsToFront,
    BringWindowsToFrontResult(bool),
    SetPreferredCamera(Option<String>),
    // Core → Tauri event forwarding
    ParticipantsSnapshot(Vec<CoreParticipantState>),
    RoleChange(CoreRoleEvent),
    CallEnded(CallId), // When call ends from a participant's side in Camera or Screen sharing window.
    RoomConnectionFailed {
        call_id: CallId,
        reason: String,
    },
    /// App Veil could not be initialized; protected apps may be visible while sharing.
    AppVeilFailed(String),
    ControllerDrawPersistChanged(bool),
    SharerDrawPersistChanged(bool),
    LastModeChanged(StoredMode),
    ActiveMicChanged(String),
    ActiveCameraChanged(String),
    DrawingDisabled,
    ExitRequested,
    SetNoiseCancellation(bool),
    SetScreenShareResolution(ScreenShareResolution),
    SetScreenSharePickerMode(ScreenSharePickerMode),
    SetAppVeilBundleIds(Vec<String>),
    SetTelemetryEnabled(bool),
    /// Microphone RMS level in [0.0, 1.0], emitted ~1 Hz from core capturer.
    MicrophoneAudioLevel(f32),
}

impl Message {
    pub fn is_response(&self) -> bool {
        matches!(
            self,
            Message::Response { .. }
                | Message::CallStartResult(_)
                | Message::AudioDeviceList(_)
                | Message::StartAudioCaptureResult(_)
                | Message::CameraList(_)
                | Message::StartCameraResult(_)
                | Message::BringWindowsToFrontResult(_)
        )
    }
}

// Platform-specific stream type alias
#[cfg(unix)]
type Stream = UnixStream;
#[cfg(windows)]
type Stream = TcpStream;

enum Outbound {
    Write(Vec<u8>),
    Flush(mpsc::Sender<()>),
}

/// Cloneable send half. One writer thread writes messages in enqueue order.
#[derive(Clone)]
pub struct SocketSender {
    outgoing: mpsc::Sender<Outbound>,
    next_request_id: Arc<AtomicU64>,
    shutdown_stream: Arc<Stream>,
}

impl SocketSender {
    pub fn send_request(&self, message: Message) -> Result<RequestId, std::io::Error> {
        let request_id = self.next_request_id.fetch_add(1, Ordering::Relaxed);
        self.send(Message::Request {
            request_id,
            message: Box::new(message),
        })?;
        Ok(request_id)
    }

    pub fn reply(
        &self,
        request_id: Option<RequestId>,
        message: Message,
    ) -> Result<(), std::io::Error> {
        self.send(match request_id {
            Some(request_id) => Message::Response {
                request_id,
                message: Box::new(message),
            },
            None => message,
        })
    }

    /// Success means enqueued; a write failure closes the connection and rejects later sends.
    pub fn send(&self, message: Message) -> Result<(), std::io::Error> {
        let serialized_message = serde_json::to_string(&message)?;
        let serialized_message = serialized_message.as_bytes();
        let size = serialized_message.len();
        let mut message_bytes = size.to_le_bytes().to_vec();
        message_bytes.extend_from_slice(serialized_message);
        self.enqueue(Outbound::Write(message_bytes))
    }

    /// Waits for preceding writes, not for the peer to process them.
    pub fn flush(&self, timeout: Duration) -> Result<(), std::io::Error> {
        let (done, written) = mpsc::channel();
        self.enqueue(Outbound::Flush(done))?;
        written.recv_timeout(timeout).map_err(|error| {
            let kind = match error {
                mpsc::RecvTimeoutError::Timeout => std::io::ErrorKind::TimedOut,
                mpsc::RecvTimeoutError::Disconnected => std::io::ErrorKind::BrokenPipe,
            };
            std::io::Error::new(kind, error)
        })
    }

    /// Interrupts blocked reads/writes without acquiring a socket or application lock.
    pub fn shutdown(&self) {
        let _ = self.shutdown_stream.shutdown(std::net::Shutdown::Both);
    }

    fn enqueue(&self, message: Outbound) -> Result<(), std::io::Error> {
        self.outgoing
            .send(message)
            .map_err(|_| std::io::Error::new(std::io::ErrorKind::BrokenPipe, "IPC writer closed"))
    }
}

/// Dual-channel receiver that owns the background reader thread.
/// Dropping this struct shuts down the underlying stream, which causes the
/// reader thread to exit and closes both channels on the remote side.
pub struct EventSocket {
    pub events: mpsc::Receiver<Message>,
    pub responses: mpsc::Receiver<Message>,
    stream: Stream,
}

impl EventSocket {
    fn new(mut read_stream: Stream, shutdown_stream: Stream) -> Self {
        let (event_tx, event_rx) = mpsc::channel();
        let (response_tx, response_rx) = mpsc::channel();

        log::info!("EventSocket: spawning background reader thread");
        std::thread::spawn(move || {
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

impl EventSocket {
    pub fn take_events(&mut self) -> mpsc::Receiver<Message> {
        let (_tx, dummy_rx) = mpsc::channel();
        std::mem::replace(&mut self.events, dummy_rx)
    }

    pub fn take_responses(&mut self) -> mpsc::Receiver<Message> {
        let (_tx, dummy_rx) = mpsc::channel();
        std::mem::replace(&mut self.responses, dummy_rx)
    }
}

impl Drop for EventSocket {
    fn drop(&mut self) {
        log::info!("EventSocket: shutting down");
        use std::net::Shutdown;
        let _ = self.stream.shutdown(Shutdown::Both);
        log::info!("EventSocket: shutdown complete");
    }
}

fn build_pair(stream: Stream) -> std::io::Result<(SocketSender, EventSocket)> {
    let mut write_stream = stream.try_clone()?;
    let read_stream = stream.try_clone()?;
    let shutdown_stream = stream;

    let sender_shutdown = Arc::new(shutdown_stream.try_clone()?);
    let writer_shutdown = sender_shutdown.clone();
    // shortcut: unbounded control-message queue; add backpressure if high-volume data uses it.
    let (outgoing, incoming) = mpsc::channel();
    std::thread::Builder::new()
        .name("ipc-writer".into())
        .spawn(move || {
            for message in incoming {
                match message {
                    Outbound::Write(bytes) => {
                        if let Err(error) = write_stream.write_all(&bytes) {
                            log::error!("IPC writer failed: {error}");
                            let _ = writer_shutdown.shutdown(std::net::Shutdown::Both);
                            break;
                        }
                    }
                    Outbound::Flush(done) => {
                        let _ = done.send(());
                    }
                }
            }
        })?;

    let sender = SocketSender {
        outgoing,
        next_request_id: Arc::new(AtomicU64::new(1)),
        shutdown_stream: sender_shutdown,
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
        let times = if cfg!(debug_assertions) { 100 } else { 50 };
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
    #[test]
    fn blocked_writer_keeps_sends_nonblocking_and_shutdown_releases_flush() {
        let (mut client, _peer) = UnixStream::pair().unwrap();
        client.set_nonblocking(true).unwrap();
        loop {
            match client.write(&[0; 4096]) {
                Ok(size) => assert!(size > 0),
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(error) => panic!("failed to fill socket: {error}"),
            }
        }
        client.set_nonblocking(false).unwrap();
        let (sender, _socket) = build_pair(client).unwrap();
        let caller_sender = sender.clone();
        let (done, completed) = mpsc::channel();
        let caller = std::thread::spawn(move || {
            caller_sender.send(Message::Ping).unwrap();
            let error = caller_sender.flush(Duration::from_millis(50)).unwrap_err();
            caller_sender
                .send(Message::CallEnd(Some("A".into())))
                .unwrap();
            done.send(error.kind()).unwrap();
        });
        let result = completed.recv_timeout(Duration::from_secs(2));
        // Release blocked I/O before reporting a failure, without locking the writer.
        sender.shutdown();
        caller.join().unwrap();
        assert_eq!(result.unwrap(), std::io::ErrorKind::TimedOut);
        assert_eq!(
            sender.flush(Duration::from_secs(1)).unwrap_err().kind(),
            std::io::ErrorKind::BrokenPipe
        );
        assert!(matches!(
            _socket.responses.recv_timeout(Duration::from_secs(1)),
            Err(mpsc::RecvTimeoutError::Disconnected)
        ));
    }

    #[cfg(unix)]
    #[test]
    fn write_failure_closes_response_waiters_and_rejects_later_sends() {
        let (client, _peer) = UnixStream::pair().unwrap();
        client.shutdown(std::net::Shutdown::Write).unwrap();
        let (sender, socket) = build_pair(client).unwrap();
        sender.send(Message::Ping).unwrap();
        assert_eq!(
            sender.flush(Duration::from_secs(1)).unwrap_err().kind(),
            std::io::ErrorKind::BrokenPipe
        );
        assert!(sender.send(Message::Ping).is_err());
        assert!(matches!(
            socket.responses.recv_timeout(Duration::from_secs(1)),
            Err(mpsc::RecvTimeoutError::Disconnected)
        ));
    }

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
    fn requests_from_clones_have_unique_ids_and_replies_preserve_them() {
        let ((server_sender, server_events), (client_sender, client_events)) = test_pair();
        let clone = client_sender.clone();
        let audio_id = client_sender
            .send_request(Message::ListAudioDevices)
            .unwrap();
        let camera_id = clone.send_request(Message::ListCameras).unwrap();
        assert_ne!(audio_id, camera_id);

        for expected_id in [audio_id, camera_id] {
            let request = server_events
                .events
                .recv_timeout(Duration::from_secs(5))
                .unwrap();
            match request {
                Message::Request {
                    request_id,
                    message,
                } => {
                    assert_eq!(request_id, expected_id);
                    assert!(matches!(
                        *message,
                        Message::ListAudioDevices | Message::ListCameras
                    ));
                }
                other => panic!("expected request, got {other:?}"),
            }
        }

        // Replies can arrive in a different order from their requests.
        for id in [camera_id, audio_id] {
            server_sender
                .reply(Some(id), Message::AudioDeviceList(vec![]))
                .unwrap();
        }
        for expected_id in [camera_id, audio_id] {
            let response = client_events
                .responses
                .recv_timeout(Duration::from_secs(5))
                .unwrap();
            assert!(matches!(
                response,
                Message::Response { request_id, message }
                    if request_id == expected_id && matches!(*message, Message::AudioDeviceList(_))
            ));
        }

        server_sender
            .reply(None, Message::StartCameraResult(Ok(())))
            .unwrap();
        assert!(matches!(
            client_events
                .responses
                .recv_timeout(Duration::from_secs(5))
                .unwrap(),
            Message::StartCameraResult(Ok(()))
        ));
    }

    #[test]
    fn test_send_recv_screen_share_result_event() {
        let ((server_sender, _server_events), (_client_sender, client_events)) = test_pair();

        // Send StartScreenShareResult as an event (not response)
        server_sender
            .send(Message::StartScreenShareResult(Ok(())))
            .unwrap();

        let msg = client_events
            .events
            .recv_timeout(Duration::from_secs(5))
            .unwrap();
        assert!(matches!(msg, Message::StartScreenShareResult(_)));
    }

    #[test]
    fn test_window_content_round_trip() {
        let serialized = serde_json::to_string(&Content {
            content_type: ContentType::Window,
            id: 42,
        })
        .unwrap();
        let content: Content = serde_json::from_str(&serialized).unwrap();

        assert!(matches!(content.content_type, ContentType::Window));
        assert_eq!(content.id, 42);
    }

    #[test]
    fn test_app_veil_bundle_ids_transport_round_trip() {
        let ((_server_sender, server_events), (client_sender, _client_events)) = test_pair();
        let bundle_ids = vec![
            "com.apple.notificationcenterui".to_string(),
            "com.example.private".to_string(),
        ];

        client_sender
            .send(Message::SetAppVeilBundleIds(bundle_ids.clone()))
            .unwrap();

        let message = server_events
            .events
            .recv_timeout(Duration::from_secs(5))
            .unwrap();
        match message {
            Message::SetAppVeilBundleIds(received) => assert_eq!(received, bundle_ids),
            _ => panic!("expected app veil bundle IDs"),
        }
    }

    #[test]
    fn preferred_camera_is_delivered_before_camera_start() {
        let ((_server_sender, server_events), (client_sender, _client_events)) = test_pair();
        for preferred in [Some("Saved camera".to_string()), None] {
            client_sender
                .send(Message::SetPreferredCamera(preferred.clone()))
                .unwrap();
            client_sender
                .send(Message::StartCamera(CameraStartMessage {
                    call_id: "camera-test".into(),
                    device_name: None,
                }))
                .unwrap();
            let setting = server_events
                .events
                .recv_timeout(Duration::from_secs(5))
                .unwrap();
            assert!(matches!(setting, Message::SetPreferredCamera(name) if name == preferred));
            let start = server_events
                .events
                .recv_timeout(Duration::from_secs(5))
                .unwrap();
            assert!(
                matches!(start, Message::StartCamera(message) if message.call_id == "camera-test")
            );
            client_sender
                .send(Message::StopCamera("camera-test".into()))
                .unwrap();
            assert!(
                matches!(server_events.events.recv_timeout(Duration::from_secs(5)).unwrap(), Message::StopCamera(call_id) if call_id == "camera-test")
            );
        }
        assert!(server_events.responses.try_recv().is_err());
    }

    #[test]
    fn lifecycle_events_preserve_call_identity() {
        let ((server, server_socket), (client, client_socket)) = test_pair();
        client.send(Message::CallEnd(Some("A".into()))).unwrap();
        assert!(
            matches!(server_socket.events.recv_timeout(Duration::from_secs(5)).unwrap(), Message::CallEnd(Some(id)) if id == "A")
        );
        server.send(Message::CallEnded("A".into())).unwrap();
        server
            .send(Message::RoomConnectionFailed {
                call_id: "B".into(),
                reason: "failed".into(),
            })
            .unwrap();
        assert!(
            matches!(client_socket.events.recv_timeout(Duration::from_secs(5)).unwrap(), Message::CallEnded(id) if id == "A")
        );
        assert!(
            matches!(client_socket.events.recv_timeout(Duration::from_secs(5)).unwrap(), Message::RoomConnectionFailed { call_id, reason } if call_id == "B" && reason == "failed")
        );
    }

    #[test]
    fn test_routing_mixed() {
        let ((_server_sender, server_events), (client_sender, _client_events)) = test_pair();

        // Send mix of events and responses from client to server
        client_sender.send(Message::GetAvailableContent).unwrap();
        client_sender.send(Message::Ping).unwrap();
        client_sender
            .send(Message::StartScreenShareResult(Ok(())))
            .unwrap();
        client_sender
            .send(Message::CallStart(CallStartMessage {
                call_id: "socket-test-call".into(),
                audio_token: "test-audio-token".to_string(),
                video_token: "test-video-token".to_string(),
                audio_device_name: String::new(),
                start_mic_on_call: None,
                start_camera_on_call: None,
            }))
            .unwrap();
        client_sender.send(Message::CallEnd(None)).unwrap();
        client_sender
            .send(Message::CallStartResult(Ok(())))
            .unwrap();

        // Events: GetAvailableContent, Ping, StartScreenShareResult, CallStart, CallEnd
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
        let msg3 = server_events
            .events
            .recv_timeout(Duration::from_secs(5))
            .unwrap();
        assert!(matches!(msg3, Message::StartScreenShareResult(_)));
        let msg5 = server_events
            .events
            .recv_timeout(Duration::from_secs(5))
            .unwrap();
        assert!(matches!(msg5, Message::CallStart(_)));
        let msg6 = server_events
            .events
            .recv_timeout(Duration::from_secs(5))
            .unwrap();
        assert!(matches!(msg6, Message::CallEnd(None)));

        // Responses: CallStartResult
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
        client_sender.flush(Duration::from_secs(1)).unwrap();
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
        let ((_server_sender, server_events), (client_sender, _client_events)) = test_pair();

        // Client sends request
        client_sender.send(Message::Ping).unwrap();

        // Server receives event
        let msg = server_events
            .events
            .recv_timeout(Duration::from_secs(5))
            .unwrap();
        assert!(matches!(msg, Message::Ping));
    }
}
