use serde::{Deserialize, Serialize};
use std::fmt;
use std::fs;
use std::io::{Read, Write};
use std::path::Path;
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
    pub token: String,
    pub resolution: Extent,
    pub accessibility_permission: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum Message {
    GetAvailableContent,
    AvailableContent(AvailableContentMessage),
    StartScreenShare(ScreenShareMessage),
    StartScreenShareResult(Result<(), String>),
    StopScreenshare,
    Reset,
    Ping,
    ControllerCursorEnabled(bool),
    LivekitServerUrl(String),
}

#[derive(Debug)]
pub struct CursorSocket {
    #[cfg(unix)]
    stream: UnixStream,
    #[cfg(unix)]
    _listener: Option<UnixListener>,

    #[cfg(windows)]
    stream: TcpStream,
    #[cfg(windows)]
    _listener: Option<TcpListener>,
}

impl CursorSocket {
    pub fn new(socket_path: &str) -> Result<Self, std::io::Error> {
        #[cfg(unix)]
        {
            let stream = UnixStream::connect(socket_path)?;
            stream.set_read_timeout(None)?;
            Ok(Self {
                stream,
                _listener: None,
            })
        }

        #[cfg(windows)]
        {
            let port = socket_path_to_port(socket_path);
            let addr = format!("127.0.0.1:{port}");
            let stream = TcpStream::connect(addr)?;
            stream.set_read_timeout(None)?;
            Ok(Self {
                stream,
                _listener: None,
            })
        }
    }

    pub fn new_create(socket_path: &str) -> Result<Self, std::io::Error> {
        log::info!("Creating socket at {socket_path}");
        #[cfg(unix)]
        {
            if Path::new(socket_path).exists() {
                fs::remove_file(socket_path)?;
            }

            let listener = UnixListener::bind(socket_path)?;
            log::info!("Wait for client");
            let (stream, _) = listener.accept()?;
            stream.set_read_timeout(None)?;

            Ok(Self {
                stream,
                _listener: Some(listener),
            })
        }

        #[cfg(windows)]
        {
            if Path::new(socket_path).exists() {
                fs::remove_file(socket_path)?;
            }

            // Get initial port to try
            let mut port = socket_path_to_port(socket_path);
            let mut listener = None;

            // Try to bind, incrementing port if necessary
            for _ in 0..100 {
                let addr = format!("127.0.0.1:{port}");
                match TcpListener::bind(addr) {
                    Ok(l) => {
                        listener = Some(l);
                        break;
                    }
                    Err(_) => {
                        log::info!("Port {port} is in use, trying next port...");
                        port += 1;
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
            let (stream, _) = listener.accept()?;
            stream.set_read_timeout(None)?;

            Ok(Self {
                stream,
                _listener: Some(listener),
            })
        }
    }

    pub fn send_message(&mut self, message: Message) -> Result<(), std::io::Error> {
        let serialized_message = serde_json::to_string(&message)?;
        let serialized_message = serialized_message.as_bytes();
        let size = serialized_message.len();
        let mut message_bytes = size.to_le_bytes().to_vec();
        message_bytes.extend_from_slice(serialized_message);
        self.stream.write_all(&message_bytes)?;
        Ok(())
    }

    pub fn receive_message(&mut self) -> Result<Message, std::io::Error> {
        let mut size_buffer = [0u8; std::mem::size_of::<usize>()];
        self.stream.read_exact(&mut size_buffer)?;
        let message_size = usize::from_le_bytes(size_buffer);

        let mut message_buffer = vec![0u8; message_size];
        self.stream.read_exact(&mut message_buffer)?;
        let buffer_str =
            String::from_utf8(message_buffer).expect("Failed to convert buffer to string");
        let deserialized_message: Message = serde_json::from_str(&buffer_str)?;
        Ok(deserialized_message)
    }

    pub fn receive_message_with_timeout(
        &mut self,
        timeout: Duration,
    ) -> Result<Message, std::io::Error> {
        let original_timeout = self.stream.read_timeout()?;
        self.stream.set_read_timeout(Some(timeout))?;

        let result = (|| {
            let mut size_buffer = [0u8; std::mem::size_of::<usize>()];
            self.stream.read_exact(&mut size_buffer)?;
            let message_size = usize::from_le_bytes(size_buffer);

            let mut message_buffer = vec![0u8; message_size];
            self.stream.read_exact(&mut message_buffer)?;
            let buffer_str =
                String::from_utf8(message_buffer).expect("Failed to convert buffer to string");
            let deserialized_message: Message = serde_json::from_str(&buffer_str)?;
            Ok(deserialized_message)
        })();

        self.stream.set_read_timeout(original_timeout)?;

        result
    }

    pub fn duplicate(&self) -> Result<Self, std::io::Error> {
        let new_stream = self.stream.try_clone()?;
        Ok(Self {
            stream: new_stream,
            _listener: None,
        })
    }
}

#[cfg(windows)]
fn socket_path_to_port(socket_path: &str) -> u16 {
    // First try to read the port from the file
    if let Ok(content) = fs::read_to_string(socket_path) {
        if let Ok(port) = content.trim().parse::<u16>() {
            log::debug!("Found port {port} in file {socket_path}");
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
    (hash % 16384 + 49152) as u16
}
