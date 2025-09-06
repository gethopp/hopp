use futures_util::{stream::SplitStream, SinkExt, StreamExt};
use livekit::{
    prelude::LocalParticipant,
    webrtc::{
        prelude::{RtcVideoTrack, VideoBuffer},
        video_stream::native::NativeVideoStream,
    },
};
use tokio::{
    net::{TcpListener, TcpStream},
    sync::broadcast::error::TryRecvError,
};
use tokio_tungstenite::{accept_async, tungstenite::Message, WebSocketStream};

const MAX_TRIES: u32 = 100;
const DEFAULT_CHUNKS_TOTAL: u32 = 8; // Default to 2 chunks as requested

#[derive(Debug, Clone)]
enum SendFramesMessage {
    Stop,
}

#[derive(Debug)]
pub struct VideoClient {
    sender: tokio::sync::broadcast::Sender<SendFramesMessage>,
    track: String,
    port: u16,
    local_participant: LocalParticipant,
}

#[derive(Debug, thiserror::Error)]
pub enum VideoClientError {
    #[error("Failed to create listener")]
    FailedToCreateListener,
    #[error("Failed to accept connection")]
    FailedToAcceptConnection,
    #[error("Waiting for connection timeout")]
    WaitingForConnectionTimeout,
    #[error("Failed to set socket to nodelay")]
    FailedToSetNodelay,
    #[error("Failed to accept websocket")]
    FailedToAcceptWebSocket,
}

impl VideoClient {
    pub async fn new(
        track: RtcVideoTrack,
        name: String,
        local_participant: LocalParticipant,
    ) -> Result<Self, VideoClientError> {
        Self::new_with_chunks(track, name, local_participant, DEFAULT_CHUNKS_TOTAL).await
    }

    pub async fn new_with_chunks(
        track: RtcVideoTrack,
        name: String,
        local_participant: LocalParticipant,
        chunks_total: u32,
    ) -> Result<Self, VideoClientError> {
        let (sender, receiver) = tokio::sync::broadcast::channel(2);
        let (listener, port) = create_listener().await?;
        let receiver_clone = sender.subscribe();
        tokio::spawn(send_frames(
            receiver,
            receiver_clone,
            track,
            listener,
            chunks_total,
        ));
        log::info!("VideoClient created: {name}, port: {port}, chunks_total: {chunks_total}");
        Ok(Self {
            sender,
            track: name,
            port,
            local_participant,
        })
    }

    pub fn track(&self) -> &str {
        &self.track
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    /// Configure the number of chunks to send per frame for experimentation
    pub fn set_chunks_total(&mut self, chunks_total: u32) {
        // Note: This will only affect new VideoClient instances
        // The current implementation doesn't support dynamic reconfiguration
        // during runtime due to the spawned task architecture
        log::info!(
            "VideoClient: chunks_total configuration noted for future instances: {}",
            chunks_total
        );
    }
}

impl Drop for VideoClient {
    fn drop(&mut self) {
        log::info!("VideoClient::drop");
        if let Err(e) = self.sender.send(SendFramesMessage::Stop) {
            log::error!("VideoClient::drop: Failed to send stop message: {e:?}");
        }
    }
}

async fn send_frames(
    mut receiver: tokio::sync::broadcast::Receiver<SendFramesMessage>,
    receiver_clone: tokio::sync::broadcast::Receiver<SendFramesMessage>,
    track: RtcVideoTrack,
    listener: TcpListener,
    chunks_total: u32,
) {
    let ws_socket = match setup_websocket(listener).await {
        Ok(ws_socket) => ws_socket,
        Err(e) => {
            log::error!("send_frames: Failed to setup websocket: {e:?}");
            return;
        }
    };
    let (mut ws_sender, ws_receiver) = ws_socket.split();
    tokio::spawn(receive_controller_events(receiver_clone, ws_receiver));

    let mut video_sink = NativeVideoStream::new(track);
    let mut frame_count = 0;
    let start_time = std::time::SystemTime::now();
    let mut frame_id: u64 = 0;
    while let Ok(Some(frame)) =
        tokio::time::timeout(std::time::Duration::from_millis(5000), video_sink.next()).await
    {
        let res = receiver.try_recv();
        if let Ok(msg) = res {
            match msg {
                SendFramesMessage::Stop => {
                    log::info!("send_frames: stopped message received");
                    break;
                }
            }
        } else if let Err(e) = res {
            match e {
                TryRecvError::Closed => {
                    log::info!("send_frames: receiver disconnected");
                    break;
                }
                _ => {
                    //log::error!("send_frames: Failed to receive message: {e:?}");
                }
            }
        }
        let capture_ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;

        let buffer = frame.buffer.to_i420();
        let stream_width = buffer.width();
        let stream_height = buffer.height();
        let (y_data, u_data, v_data) = buffer.data();

        // Create the complete payload (y_data + u_data + v_data)
        let mut payload = Vec::with_capacity(y_data.len() + u_data.len() + v_data.len());
        payload.extend_from_slice(y_data);
        payload.extend_from_slice(u_data);
        payload.extend_from_slice(v_data);

        let total_len = payload.len();
        frame_id += 1;

        // Calculate chunk distribution
        let base_chunk = total_len as u32 / chunks_total;
        let remainder = total_len as u32 % chunks_total;

        // Send frame in chunks
        for chunk_index in 0..chunks_total {
            let extra = if chunk_index < remainder { 1 } else { 0 };
            let start_offset =
                (chunk_index * base_chunk + u32::min(chunk_index, remainder)) as usize;
            let length = (base_chunk + extra) as usize;
            let end_offset = start_offset + length;

            // Chunk header (little-endian):
            // magic:4 ('CHNK'), width:4, height:4, capture_ts:8, send_ts:8, frame_id:8,
            // chunk_index:4, chunks_total:4, total_length:4, chunk_offset:4, chunk_length:4
            let mut chunk = Vec::with_capacity(56 + length);
            // magic 'CHNK'
            chunk.extend_from_slice(&[b'C', b'H', b'N', b'K']);
            // dims
            chunk.extend_from_slice(&(stream_width as u32).to_le_bytes());
            chunk.extend_from_slice(&(stream_height as u32).to_le_bytes());
            // capture_ts
            chunk.extend_from_slice(&capture_ts.to_le_bytes());
            // send_ts placeholder
            chunk.extend_from_slice(&0u64.to_le_bytes());
            // frame_id
            chunk.extend_from_slice(&frame_id.to_le_bytes());
            // chunk_index, chunks_total
            chunk.extend_from_slice(&(chunk_index as u32).to_le_bytes());
            chunk.extend_from_slice(&chunks_total.to_le_bytes());
            // total_length, chunk_offset, chunk_length
            chunk.extend_from_slice(&(total_len as u32).to_le_bytes());
            chunk.extend_from_slice(&(start_offset as u32).to_le_bytes());
            chunk.extend_from_slice(&(length as u32).to_le_bytes());
            // payload slice
            chunk.extend_from_slice(&payload[start_offset..end_offset]);

            // Stamp send_ts immediately before send
            let send_ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64;
            let send_ts_bytes = send_ts.to_le_bytes();
            let send_ts_offset = 4 + 4 + 4 + 8; // magic(4)+w(4)+h(4)+capture(8)
            for i in 0..8 {
                chunk[send_ts_offset + i] = send_ts_bytes[i];
            }

            // Send chunk
            if let Err(e) = ws_sender.send(Message::Binary(chunk.into())).await {
                log::error!(
                    "Failed to send frame chunk {} / {}: {}",
                    chunk_index + 1,
                    chunks_total,
                    e
                );
                break;
            }
        }

        frame_count += 1;
        if frame_count % 30 == 0 {
            let fps = frame_count as f64 / start_time.elapsed().unwrap().as_secs_f64();
            log::info!("send_frames: fps: {fps}, chunks_total: {chunks_total}");
        }
    }
    log::info!("send_frames: stopped");
}

async fn receive_controller_events(
    mut receiver: tokio::sync::broadcast::Receiver<SendFramesMessage>,
    mut ws_receiver: SplitStream<WebSocketStream<TcpStream>>,
) {
    loop {
        let res = receiver.try_recv();
        if let Ok(msg) = res {
            match msg {
                SendFramesMessage::Stop => {
                    log::info!("receive_controller_events: stopped message received");
                    break;
                }
            }
        } else if let Err(e) = res {
            match e {
                TryRecvError::Closed => {
                    log::info!("receive_controller_events: receiver disconnected");
                    break;
                }
                _ => {
                    log::error!("receive_controller_events: Failed to receive message: {e:?}");
                }
            }
        }

        if let Ok(msg) =
            tokio::time::timeout(std::time::Duration::from_secs(5), ws_receiver.next()).await
        {
            log::info!("receive_controller_events: received message: {msg:?}");
            match msg {
                Some(msg) => match msg {
                    Ok(Message::Close(_)) => {
                        log::info!("receive_controller_events: received close message");
                        break;
                    }
                    _ => {
                        log::error!("receive_controller_events: Received unknown message: {msg:?}");
                    }
                },
                None => {
                    log::error!("receive_controller_events: Failed to receive message");
                    break;
                }
            }
        }
    }
    log::info!("receive_controller_events: stopped");
}

async fn create_listener() -> Result<(TcpListener, u16), VideoClientError> {
    let mut port = 50000;
    for _ in 0..100 {
        let addr = format!("127.0.0.1:{port}");
        match TcpListener::bind(addr).await {
            Ok(l) => return Ok((l, port)),
            Err(_) => {
                log::info!("Port {port} is in use, trying next port...");
                port += 1;
            }
        }
    }
    Err(VideoClientError::FailedToCreateListener)
}

async fn setup_websocket(
    listener: TcpListener,
) -> Result<WebSocketStream<TcpStream>, VideoClientError> {
    let socket =
        match tokio::time::timeout(std::time::Duration::from_secs(30), listener.accept()).await {
            Ok(Ok((s, _))) => s,
            Ok(Err(_)) => {
                return Err(VideoClientError::FailedToAcceptConnection);
            }
            Err(_) => {
                return Err(VideoClientError::WaitingForConnectionTimeout);
            }
        };

    if let Err(_) = socket.set_nodelay(true) {
        return Err(VideoClientError::FailedToSetNodelay);
    }

    accept_async(socket)
        .await
        .map_err(|_| VideoClientError::FailedToAcceptWebSocket)
}
