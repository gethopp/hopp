use futures_util::{stream::SplitStream, SinkExt, StreamExt};
use livekit::{
    prelude::LocalParticipant,
    webrtc::{
        prelude::{RtcVideoTrack, VideoBuffer},
        video_stream::native::NativeVideoStream,
    },
    DataPacket,
};
use tokio::{
    net::{TcpListener, TcpStream},
    sync::broadcast::error::TryRecvError,
};
use tokio_tungstenite::{accept_async, tungstenite::Message, WebSocketStream};

use crate::room_service::room_service::{ClientPoint, ParticipantInControl, RemoteControlEnabled};

const MAX_TRIES: u32 = 100;

#[derive(Debug, Clone)]
enum SendFramesMessage {
    Stop,
}

#[derive(Debug)]
pub struct VideoClient {
    sender: tokio::sync::broadcast::Sender<SendFramesMessage>,
    track: String,
    port: u16,
    controller_event_sender: std::sync::mpsc::Sender<ControllerEvent>,
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

#[derive(Debug)]
pub enum ControllerEvent {
    RemoteControlEnabled(RemoteControlEnabled),
    ShowCustomCursor(bool),
    ParticipantLocation(ClientPoint, String),
}

impl VideoClient {
    pub async fn new(
        track: RtcVideoTrack,
        name: String,
        local_participant: LocalParticipant,
    ) -> Result<Self, VideoClientError> {
        let (sender, receiver) = tokio::sync::broadcast::channel(2);
        let (listener, port) = create_listener().await?;
        let receiver_clone = sender.subscribe();
        let (controller_event_sender, controller_event_receiver) = std::sync::mpsc::channel();
        tokio::spawn(send_frames(
            receiver,
            receiver_clone,
            track,
            listener,
            controller_event_receiver,
            local_participant,
        ));
        log::info!("VideoClient created: {name}, port: {port}");
        Ok(Self {
            sender,
            track: name,
            port,
            controller_event_sender,
        })
    }

    pub fn track(&self) -> &str {
        &self.track
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn forward_controller_event(&self, event: ControllerEvent) {
        if let Err(e) = self.controller_event_sender.send(event) {
            log::error!("VideoClient::forward_controller_event: Failed to send event: {e:?}");
        }
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
    controller_event_receiver: std::sync::mpsc::Receiver<ControllerEvent>,
    local_participant: LocalParticipant,
) {
    let ws_socket = match setup_websocket(listener).await {
        Ok(ws_socket) => ws_socket,
        Err(e) => {
            log::error!("send_frames: Failed to setup websocket: {e:?}");
            return;
        }
    };
    let (mut ws_sender, ws_receiver) = ws_socket.split();
    tokio::spawn(receive_controller_events(
        receiver_clone,
        ws_receiver,
        local_participant,
    ));

    let mut video_sink = NativeVideoStream::new(track);
    let start_time = std::time::SystemTime::now();
    let mut frame_id: u32 = 0;

    // Pre-allocate vectors to avoid repeated allocations
    let base_capacity = 48;
    let max_sid_len = 256; // Reasonable max session ID length
    let mut header = Vec::with_capacity(base_capacity + max_sid_len);
    let mut chunk = Vec::new();

    while let Ok(Some(frame)) =
        tokio::time::timeout(std::time::Duration::from_millis(5000), video_sink.next()).await
    {
        // Clear vectors for reuse
        header.clear();
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

        // Check if there is a controller event
        let mut participant_location = None;
        let mut remote_control_enabled = None;
        let mut show_custom_cursor = None;
        while let Ok(event) = controller_event_receiver.try_recv() {
            match event {
                ControllerEvent::ParticipantLocation(location, participant) => {
                    participant_location = Some((location, participant));
                }
                ControllerEvent::RemoteControlEnabled(enabled) => {
                    remote_control_enabled = Some(enabled);
                }
                ControllerEvent::ShowCustomCursor(enabled) => {
                    show_custom_cursor = Some(enabled);
                }
            }
        }

        let now = std::time::SystemTime::now();
        let capture_ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        frame_id += 1;

        let buffer = frame.buffer.to_i420();
        let stream_width = buffer.width();
        let stream_height = buffer.height();
        let (y_data, u_data, v_data) = buffer.data();

        /* We send each frame in chunks.
         * For this reason we can have two different types of payloads:
         *   - The first which will describe the frame and the number of chunks
         *   - The second which will contain the actual frame data
         *
         * The type could be either 'H' for header or 'D' for data.
         *
         * Header:
         *   - type:1 ('H')
         *   - width:2
         *   - height:2
         *   - capture_ts:8
         *   - frame_id:4
         *   - chunks_total:1
         *   - remote_control_enabled: 1 // if there is a remote control enabled
         *   - remote_control_enabled_value: 1 // if remote control is enabled
         *   - show_custom_cursor: 1 // show custom cursor
         *   - show_custom_cursor_value: 1 // show custom cursor
         *   - participant_location: 1 // if there is a participant location
         *   - x: 8 // float64
         *   - y: 8 // float64
         *   - sid_length: 4
         *   - sid: sid_length // sid string
         *
         * Data:
         *   - type:1 ('D')
         *   - frame_id:4
         *   - chunk_index:2
         *   - chunk_size:4
         *   - data: variable
         */

        let total_chunks: u8 = 4;

        let mut location_sid = String::new();
        let mut location_x = 0.0;
        let mut location_y = 0.0;
        if let Some((location, participant)) = participant_location {
            location_x = location.x;
            location_y = location.y;
            location_sid = participant;
        }

        header.extend_from_slice(&[b'H']);
        header.extend_from_slice(&(stream_width as u16).to_le_bytes());
        header.extend_from_slice(&(stream_height as u16).to_le_bytes());
        header.extend_from_slice(&capture_ts.to_le_bytes());
        header.extend_from_slice(&frame_id.to_le_bytes());
        header.extend_from_slice(&total_chunks.to_le_bytes());
        if let Some(enabled) = remote_control_enabled {
            header.extend_from_slice(&1u8.to_le_bytes());
            header.extend_from_slice(&(enabled.enabled as u8).to_le_bytes());
        } else {
            header.extend_from_slice(&0u8.to_le_bytes());
            header.extend_from_slice(&0u8.to_le_bytes());
        }
        if let Some(show_custom_cursor) = show_custom_cursor {
            header.extend_from_slice(&1u8.to_le_bytes());
            header.extend_from_slice(&(show_custom_cursor as u8).to_le_bytes());
        } else {
            header.extend_from_slice(&0u8.to_le_bytes());
            header.extend_from_slice(&0u8.to_le_bytes());
        }
        if location_sid != "" {
            header.extend_from_slice(&1u8.to_le_bytes());
            header.extend_from_slice(&(location_x as f64).to_le_bytes());
            header.extend_from_slice(&(location_y as f64).to_le_bytes());
            header.extend_from_slice(&(location_sid.len() as u32).to_le_bytes());
            header.extend_from_slice(&location_sid.as_bytes());
        } else {
            header.extend_from_slice(&0u8.to_le_bytes());
            header.extend_from_slice(&0u8.to_le_bytes());
            header.extend_from_slice(&0u8.to_le_bytes());
            header.extend_from_slice(&0u32.to_le_bytes());
        }

        if let Err(e) = ws_sender.send(Message::Binary(header.clone().into())).await {
            log::error!("Failed to send header: {e:?}");
            break;
        }

        let total_size = y_data.len() + u_data.len() + v_data.len();
        let chunk_size = total_size / (total_chunks as usize);
        let buffers = [y_data, u_data, v_data];
        let buffers_limits = [
            (0 as usize, y_data.len()),
            (y_data.len(), y_data.len() + u_data.len()),
            (y_data.len() + u_data.len(), total_size),
        ];
        let mut global_index = 0;
        for chunk_index in 0..total_chunks {
            chunk.clear();

            chunk.extend_from_slice(&[b'D']);
            chunk.extend_from_slice(&frame_id.to_le_bytes());
            chunk.extend_from_slice(&(chunk_index as u16).to_le_bytes());
            chunk.extend_from_slice(&(0u32).to_le_bytes());

            let mut i = 0;
            let mut bytes_copied = 0;
            for (start, end) in buffers_limits {
                if global_index >= start && global_index < end {
                    let remaining_bytes = chunk_size - bytes_copied;
                    if remaining_bytes == 0 {
                        break;
                    }
                    let buffer_start = global_index - start;
                    let buffer_end = if global_index + remaining_bytes > end {
                        end - start
                    } else {
                        (global_index + remaining_bytes) - start
                    };
                    chunk.extend_from_slice(&buffers[i][buffer_start..buffer_end]);
                    let copied_size = buffer_end - buffer_start;
                    bytes_copied += copied_size;
                    global_index += copied_size;
                }
                i += 1;
            }

            chunk[7..11].copy_from_slice(&(bytes_copied as u32).to_le_bytes());

            if frame_id % 30 == 0 {
                log::info!("send_frames: chunk {chunk_index} elapsed: {}", now.elapsed().unwrap().as_millis());
            }
            if let Err(e) = ws_sender.send(Message::Binary(chunk.clone().into())).await {
                log::error!("Failed to send chunk: {e:?}");
                break;
            }
        }

        if frame_id % 30 == 0 {
            let fps = frame_id as f64 / start_time.elapsed().unwrap().as_secs_f64();
            log::info!("send_frames: fps: {fps}");
            log::info!("send_frames: capture ts elapsed: {}", now.elapsed().unwrap().as_millis());
        }
    }
    log::info!("send_frames: stopped");
}

async fn receive_controller_events(
    mut receiver: tokio::sync::broadcast::Receiver<SendFramesMessage>,
    mut ws_receiver: SplitStream<WebSocketStream<TcpStream>>,
    local_participant: LocalParticipant,
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
                _ => {}
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
                    Ok(Message::Binary(data)) => {
                        if let Err(e) = local_participant
                            .publish_data(DataPacket {
                                payload: data.to_vec(),
                                reliable: true,
                                ..Default::default()
                            })
                            .await
                        {
                            log::error!("receive_controller_events: Failed to publish data: {e:?}");
                        }
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
