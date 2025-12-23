use std::sync::Arc;

use livekit::options::{TrackPublishOptions, VideoCodec, VideoEncoding};
use livekit::track::{LocalTrack, LocalVideoTrack, TrackSource};
use livekit::webrtc::prelude::{RtcVideoSource, VideoResolution};
use livekit::webrtc::stats::RtcStats;
use livekit::webrtc::video_source::native::NativeVideoSource;
use livekit::{DataPacket, Room, RoomEvent, RoomOptions};

use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tokio::sync::Mutex;
use winit::event_loop::EventLoopProxy;

use crate::{ParticipantData, UserEvent};

// Constants for magic values
const TOPIC_SHARER_LOCATION: &str = "participant_location";
const TOPIC_REMOTE_CONTROL_ENABLED: &str = "remote_control_enabled";
const TOPIC_PARTICIPANT_IN_CONTROL: &str = "participant_in_control";
const TOPIC_TICK_RESPONSE: &str = "tick_response";
const VIDEO_TRACK_NAME: &str = "screen_share";
const MAX_FRAMERATE: f64 = 40.0;

// Bitrate constants (in bits per second)
const BITRATE_1920: u64 = 2_000_000; // 2 Mbps
const BITRATE_2048: u64 = 3_500_000; // 3.5 Mbps
const BITRATE_2560: u64 = 5_000_000; // 5 Mbps
const BITRATE_DEFAULT: u64 = 8_000_000; // 8 Mbps

const AV1_BITRATE_1920: u64 = 1_500_000; // 1.5 Mbps
const AV1_BITRATE_2048: u64 = 2_500_000; // 2.5 Mbps
const AV1_BITRATE_2560: u64 = 3_750_000; // 3.75 Mbps
const AV1_BITRATE_DEFAULT: u64 = 5_000_000; // 5 Mbps

// Resolution thresholds
const WIDTH_THRESHOLD_1920: u32 = 1920;
const WIDTH_THRESHOLD_2048: u32 = 2048;
const WIDTH_THRESHOLD_2560: u32 = 2560;

#[derive(Debug)]
enum RoomServiceCommand {
    CreateRoom {
        token: String,
        event_loop_proxy: EventLoopProxy<UserEvent>,
    },
    PublishTrack {
        width: u32,
        height: u32,
        use_av1: bool,
    },
    PublishSharerLocation(f64, f64, bool),
    PublishControllerCursorEnabled(bool),
    DestroyRoom,
    TickResponse(u128),
    IterateParticipants,
    PublishParticipantInControl(String),
}

#[derive(Debug)]
enum RoomServiceCommandResult {
    Success,
    Failure,
}

#[derive(Debug, thiserror::Error)]
pub enum RoomServiceError {
    #[error("Failed to create room: {0}")]
    CreateRoom(String),
    #[error("Failed to publish track: {0}")]
    PublishTrack(String),
}

/*
 * This struct is used for handling room events and functions
 * from a thread in the async runtime.
 */
#[derive(Debug)]
struct RoomServiceInner {
    // TODO: See if we can use a sync::Mutex instead of tokio::sync::Mutex
    room: Mutex<Option<Room>>,
    buffer_source: Arc<std::sync::Mutex<Option<NativeVideoSource>>>,
}

/// RoomService is a wrapper around the LiveKit room, on creation it
/// spawns a thread for handling async code.
/// It exposes a few functions for sending commands to the room service.
///
/// The room service is responsible for:
/// - Creating a room
/// - Destroying a room
/// - Publishing sharer location
/// - Publishing controller cursor enabled
/// - Publishing tick response
#[derive(Debug)]
pub struct RoomService {
    /* The runtime is used to spawn a thread for handling room events. */
    _async_runtime: tokio::runtime::Runtime,
    service_command_tx: mpsc::UnboundedSender<RoomServiceCommand>,
    /* This is used to receive the result of the command, now only for create room. */
    service_command_res_rx: std::sync::mpsc::Receiver<RoomServiceCommandResult>,
    inner: Arc<RoomServiceInner>,
}

impl RoomService {
    /// Creates a new RoomService instance.
    ///
    /// This function initializes a multi-threaded async runtime and spawns a background
    /// task to handle room service commands. The service manages LiveKit room connections
    /// and provides methods for publishing data to the room.
    ///
    /// # Arguments
    ///
    /// * `livekit_server_url` - The URL of the LiveKit server to connect to
    /// * `event_loop_proxy` - The event loop proxy to send events to
    ///
    /// # Returns
    ///
    /// * `Ok(RoomService)` - A new room service instance
    /// * `Err(std::io::Error)` - If the async runtime could not be created
    pub fn new(
        livekit_server_url: String,
        event_loop_proxy: EventLoopProxy<UserEvent>,
    ) -> Result<Self, std::io::Error> {
        let async_runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()?;

        let inner = Arc::new(RoomServiceInner {
            room: Mutex::new(None),
            buffer_source: Arc::new(std::sync::Mutex::new(None)),
        });
        let (service_command_tx, service_command_rx) = mpsc::unbounded_channel();
        let (service_command_res_tx, service_command_res_rx) = std::sync::mpsc::channel();
        async_runtime.spawn(room_service_commands(
            service_command_rx,
            service_command_res_tx,
            inner.clone(),
            livekit_server_url,
            event_loop_proxy,
        ));

        Ok(Self {
            _async_runtime: async_runtime,
            service_command_tx,
            service_command_res_rx,
            inner,
        })
    }

    /// Creates a room, this will block until the room is created.
    ///
    /// This function will block until the room is created in the
    /// async runtime thread.
    ///
    /// # Arguments
    ///
    /// * `token` - The token to use to connect to the room
    /// * `event_loop_proxy` - The event loop proxy to send events to
    ///
    /// # Returns
    ///
    /// * `Ok(())` - The room was created successfully
    /// * `Err(())` - The room was not created successfully
    pub fn create_room(
        &self,
        token: String,
        event_loop_proxy: EventLoopProxy<UserEvent>,
    ) -> Result<(), RoomServiceError> {
        log::info!("create_room");
        let res = self
            .service_command_tx
            .send(RoomServiceCommand::CreateRoom {
                token,
                event_loop_proxy,
            });
        if let Err(e) = res {
            return Err(RoomServiceError::CreateRoom(format!(
                "Failed to send command: {e:?}"
            )));
        }
        let res = self.service_command_res_rx.recv();
        match res {
            Ok(RoomServiceCommandResult::Success) => Ok(()),
            Ok(RoomServiceCommandResult::Failure) => Err(RoomServiceError::CreateRoom(
                "Failed to create room".to_string(),
            )),
            Err(e) => Err(RoomServiceError::CreateRoom(format!(
                "Failed to receive result: {e:?}"
            ))),
        }
    }

    /// Publishes a video track, this will block until the room is created.
    ///
    /// # Arguments
    ///
    /// * `width` - The width of the video track
    /// * `height` - The height of the video track
    /// * `use_av1` - If av1 codec is being used
    ///
    /// # Returns
    ///
    /// * `Ok(())` - The track was published successfully
    /// * `Err(())` - The track was not published successfully
    pub fn publish_track(
        &self,
        width: u32,
        height: u32,
        use_av1: bool,
    ) -> Result<(), RoomServiceError> {
        log::info!("publish_track: {width:?}, {height:?}");
        let res = self
            .service_command_tx
            .send(RoomServiceCommand::PublishTrack {
                width,
                height,
                use_av1,
            });
        if let Err(e) = res {
            return Err(RoomServiceError::PublishTrack(format!(
                "Failed to send command: {e:?}"
            )));
        }
        let res = self.service_command_res_rx.recv();
        match res {
            Ok(RoomServiceCommandResult::Success) => Ok(()),
            Ok(RoomServiceCommandResult::Failure) => Err(RoomServiceError::PublishTrack(
                "Failed to publish track".to_string(),
            )),
            Err(e) => Err(RoomServiceError::PublishTrack(format!(
                "Failed to receive result: {e:?}"
            ))),
        }
    }

    /// Destroys the current room connection.
    pub fn destroy_room(&self) {
        log::info!("destroy_room");
        let res = self
            .service_command_tx
            .send(RoomServiceCommand::DestroyRoom);
        if let Err(e) = res {
            log::error!("destroy_room: Failed to send command: {e:?}");
        }
    }

    /// Retrieves the native video source buffer for screen sharing.
    ///
    /// This function returns a clone of the `NativeVideoSource` that was created
    /// when the room was established. The buffer source is used to send video
    /// frames to the LiveKit room for screen sharing.
    ///
    /// This is only called after the room has been created otherwise it will panic.
    ///
    /// # Returns
    ///
    /// * `NativeVideoSource` - The video source buffer for sending frames
    pub fn get_buffer_source(&self) -> NativeVideoSource {
        log::info!("get_buffer_source");
        let buffer_source = {
            let inner = self.inner.buffer_source.lock().unwrap();
            inner.clone()
        };
        buffer_source.expect("get_buffer_source: Buffer source not found (this shouldn't happen)")
    }

    /// Publishes the sharer's cursor location to the room.
    ///
    /// This function sends the current cursor position of the person sharing their screen
    /// to all participants in the LiveKit room. The data is sent reliably using the
    /// "sharer_location" topic.
    ///
    /// # Arguments
    ///
    /// * `x` - The x-coordinate of the cursor position
    /// * `y` - The y-coordinate of the cursor position
    /// * `pointer` - Whether the pointer is visible (currently unused in the implementation)
    pub fn publish_sharer_location(&self, x: f64, y: f64, pointer: bool) {
        log::debug!("publish_sharer_location: {x:?}, {y:?}, {pointer:?}");
        let res = self
            .service_command_tx
            .send(RoomServiceCommand::PublishSharerLocation(x, y, pointer));
        if let Err(e) = res {
            log::error!("publish_sharer_location: Failed to send command: {e:?}");
        }
    }

    /// Publishes the remote control enabled status to the room.
    /// # Arguments
    ///
    /// * `enabled` - Whether remote control is enabled (true) or disabled (false)
    pub fn publish_controller_cursor_enabled(&self, enabled: bool) {
        log::info!("publish_controller_cursor_enabled: {enabled:?}");
        let res = self
            .service_command_tx
            .send(RoomServiceCommand::PublishControllerCursorEnabled(enabled));

        if let Err(e) = res {
            log::error!("publish_controller_cursor_enabled: Failed to send command: {e:?}");
        }
    }

    /// This was used for latency measurement, needs to
    /// be integrated properly for production usage.
    pub fn tick_response(&self, time: u128) {
        log::info!("tick_response: {time:?}");
        let res = self
            .service_command_tx
            .send(RoomServiceCommand::TickResponse(time));
        if let Err(e) = res {
            log::error!("publish_sharer_location: Failed to send command: {e:?}");
        }
    }

    /// Iterates over the participants in the room and sends an event to the event loop
    /// for each participant that is not an audio participant.
    pub fn iterate_participants(&self) {
        log::info!("iterate_participants");
        let res = self
            .service_command_tx
            .send(RoomServiceCommand::IterateParticipants);
        if let Err(e) = res {
            log::error!("iterate_participants: Failed to send command: {e:?}");
        }
    }

    /// Publishes controller controls to the room.
    pub fn publish_participant_in_control(&self, participant: String) {
        log::info!("publish_participant_in_control: {participant:?}");
        let res = self
            .service_command_tx
            .send(RoomServiceCommand::PublishParticipantInControl(participant));
        if let Err(e) = res {
            log::error!("publish_participant_in_control: Failed to send command: {e:?}");
        }
    }
}

/// Handles room service commands in an async loop.
///
/// This function processes commands sent through the `service_rx` channel and executes
/// corresponding actions on the LiveKit room. It runs continuously until the channel
/// is closed or an unrecoverable error occurs.
///
/// # Arguments
///
/// * `service_rx` - Unbounded receiver for room service commands
/// * `tx` - Synchronous sender for command results (Success/Failure)
/// * `inner` - Shared reference to the room service inner state
///
/// # Commands Handled
///
/// * `CreateRoom` - Creates a new LiveKit room connection and sets up event handing.
///   If a room already exists, it will be closed first.
///
/// * `PublishTrack` - Publishes a video track. The video track is configured with
///   VP9 codec and adaptive bitrate based on width.
///
/// * `DestroyRoom` - Closes the current room connection and cleans up associated
///   resources including the buffer source.
///
/// * `PublishSharerLocation` - Publishes sharer cursor position data to the room
///   with topic "sharer_location".
///
/// * `PublishControllerCursorEnabled` - Publishes remote control enable/disable
///   status to the room with topic "remote_control_enabled".
///
/// * `TickResponse` - Publishes timing data to the room with topic "tick_response".
///
/// * `IterateParticipants` - Iterates over the participants in the room and sends an event
///   to the event loop for each participant that is not an audio participant.
///
/// # Error Handling
///
/// The function logs errors for individual command failures but continues processing
/// subsequent commands. Command results are sent back through the `tx` channel.
/// Room state validation is performed before executing commands that require an
/// active room connection.
async fn room_service_commands(
    mut service_rx: mpsc::UnboundedReceiver<RoomServiceCommand>,
    tx: std::sync::mpsc::Sender<RoomServiceCommandResult>,
    inner: Arc<RoomServiceInner>,
    livekit_server_url: String,
    event_loop_proxy: EventLoopProxy<UserEvent>,
) {
    let mut stats_task: Option<tokio::task::JoinHandle<()>> = None;

    while let Some(command) = service_rx.recv().await {
        log::debug!("room_service_commands: Received command {command:?}");
        match command {
            // TODO: Break this into create room and publish track commands
            RoomServiceCommand::CreateRoom {
                token,
                event_loop_proxy,
            } => {
                {
                    let mut inner_room = inner.room.lock().await;
                    if inner_room.is_some() {
                        log::warn!("room_service_commands: Room already exists, killing it.");
                        let room = inner_room.take().unwrap();
                        let res = room.close().await;
                        if let Err(e) = res {
                            log::error!("room_service_commands: Failed to close room: {e:?}");
                        }
                    }
                }

                let url = livekit_server_url.clone();

                let connect_result = Room::connect(&url, &token, RoomOptions::default()).await;
                let (room, rx) = match connect_result {
                    Ok((room, rx)) => (room, rx),
                    Err(_) => {
                        log::error!("room_service_commands: Failed to connect to room");
                        let res = tx.send(RoomServiceCommandResult::Failure);
                        if let Err(e) = res {
                            log::error!("room_service_commands: Failed to send result: {e:?}");
                        }
                        continue;
                    }
                };

                if cfg!(debug_assertions) {
                    if let Some(task) = stats_task.take() {
                        task.abort();
                    }
                    stats_task = Some(tokio::spawn(stats_loop(inner.clone())));
                }

                let user_sid = room.local_participant().sid().as_str().to_string();
                // TODO: Check if this will need cleanup
                /* Spawn thread for handling livekit data events. */
                tokio::spawn(handle_room_events(rx, event_loop_proxy, user_sid));

                let mut inner_room = inner.room.lock().await;
                *inner_room = Some(room);
                let res = tx.send(RoomServiceCommandResult::Success);
                if let Err(e) = res {
                    log::error!("room_service_commands: Failed to send result: {e:?}");
                }
            }
            RoomServiceCommand::PublishTrack {
                width,
                height,
                use_av1,
            } => {
                let inner_room = inner.room.lock().await;
                if inner_room.is_none() {
                    log::error!("room_service_commands: Room doesn't exist.");
                    let res = tx.send(RoomServiceCommandResult::Failure);
                    if let Err(e) = res {
                        log::error!("room_service_commands: Failed to send result: {e:?}");
                    }
                    continue;
                }
                let room = inner_room.as_ref().unwrap();

                let buffer_source = NativeVideoSource::new(VideoResolution { width, height });
                let track = LocalVideoTrack::create_video_track(
                    VIDEO_TRACK_NAME,
                    RtcVideoSource::Native(buffer_source.clone()),
                );

                /* Have different max_bitrate based on width. */
                let (av1_bitrate, vp9_bitrate) = match width {
                    WIDTH_THRESHOLD_1920 => (AV1_BITRATE_1920, BITRATE_1920),
                    WIDTH_THRESHOLD_2048 => (AV1_BITRATE_2048, BITRATE_2048),
                    WIDTH_THRESHOLD_2560 => (AV1_BITRATE_2560, BITRATE_2560),
                    _ => (AV1_BITRATE_DEFAULT, BITRATE_DEFAULT),
                };

                let max_bitrate = if use_av1 { av1_bitrate } else { vp9_bitrate };
                let video_codec = if use_av1 {
                    VideoCodec::AV1
                } else {
                    VideoCodec::VP9
                };

                let res = room
                    .local_participant()
                    .publish_track(
                        LocalTrack::Video(track),
                        TrackPublishOptions {
                            source: TrackSource::Screenshare,
                            video_codec,
                            video_encoding: Some(VideoEncoding {
                                max_bitrate,
                                max_framerate: MAX_FRAMERATE,
                            }),
                            simulcast: false,
                            ..Default::default()
                        },
                    )
                    .await;

                if let Err(e) = res {
                    log::error!("room_service_command: Failed to publish track: {e:?}");
                    let res = tx.send(RoomServiceCommandResult::Failure);
                    if let Err(e) = res {
                        log::error!("room_service_commands: Failed to send result: {e:?}");
                    }
                    continue;
                }

                let mut inner_buffer_source = inner.buffer_source.lock().unwrap();
                *inner_buffer_source = Some(buffer_source);

                let res = tx.send(RoomServiceCommandResult::Success);
                if let Err(e) = res {
                    log::error!("room_service_commands: Failed to send result: {e:?}");
                }
            }
            RoomServiceCommand::DestroyRoom => {
                if let Some(task) = stats_task.take() {
                    task.abort();
                }

                let room = {
                    let mut inner_room = inner.room.lock().await;
                    if inner_room.is_none() {
                        log::warn!("room_service_commands: Room doesn't exist");
                        continue;
                    }
                    inner_room.take()
                };
                if let Some(room) = room {
                    let res = room.close().await;
                    if let Err(e) = res {
                        log::error!("room_service_commands: Failed to close room: {e:?}");
                    }
                }

                let _buffer_source = {
                    let mut inner_buffer_source = inner.buffer_source.lock().unwrap();
                    inner_buffer_source.take()
                };
            }
            RoomServiceCommand::PublishSharerLocation(x, y, _pointer) => {
                let inner_room = inner.room.lock().await;
                if inner_room.is_none() {
                    log::warn!("room_service_commands: Room doesn't exist");
                    continue;
                }
                let room = inner_room.as_ref().unwrap();
                let local_participant = room.local_participant();
                let res = local_participant
                    .publish_data(DataPacket {
                        payload: serde_json::to_vec(&ClientEvent::MouseMove(ClientPoint { x, y }))
                            .unwrap(),
                        reliable: true,
                        topic: Some(TOPIC_SHARER_LOCATION.to_string()),
                        ..Default::default()
                    })
                    .await;
                if let Err(e) = res {
                    log::error!("room_service_commands: Failed to publish sharer location: {e:?}");
                }
                log::debug!(
                    "Published sharer location with x: {x:?}, y: {y:?} to topic: {TOPIC_SHARER_LOCATION:?}"
                );
            }
            RoomServiceCommand::PublishControllerCursorEnabled(enabled) => {
                let inner_room = inner.room.lock().await;
                if inner_room.is_none() {
                    log::warn!("room_service_commands: Room doesn't exist");
                    continue;
                }
                let room = inner_room.as_ref().unwrap();
                let local_participant = room.local_participant();
                let res = local_participant
                    .publish_data(DataPacket {
                        payload: serde_json::to_vec(&ClientEvent::RemoteControlEnabled(
                            RemoteControlEnabled { enabled },
                        ))
                        .unwrap(),
                        reliable: true,
                        topic: Some(TOPIC_REMOTE_CONTROL_ENABLED.to_string()),
                        ..Default::default()
                    })
                    .await;
                if let Err(e) = res {
                    log::error!(
                        "room_service_commands: Failed to publish remote control change: {e:?}"
                    );
                }
            }
            RoomServiceCommand::TickResponse(time) => {
                let inner_room = inner.room.lock().await;
                if inner_room.is_none() {
                    log::warn!("room_service_commands: Room doesn't exist");
                    continue;
                }
                let room = inner_room.as_ref().unwrap();
                let local_participant = room.local_participant();
                let res = local_participant
                    .publish_data(DataPacket {
                        payload: serde_json::to_vec(&ClientEvent::TickResponse(TickData { time }))
                            .unwrap(),
                        reliable: true,
                        topic: Some(TOPIC_TICK_RESPONSE.to_string()),
                        ..Default::default()
                    })
                    .await;
                if let Err(e) = res {
                    log::error!("room_service_commands: Failed to publish tick response: {e:?}");
                }
            }
            RoomServiceCommand::IterateParticipants => {
                let room = inner.room.lock().await;
                if room.is_none() {
                    log::warn!("room_service_commands: Room doesn't exist");
                    continue;
                }
                let room = room.as_ref().unwrap();
                for participant in room.remote_participants() {
                    log::info!(
                        "room_service_commands: Participant: {}",
                        participant.1.sid()
                    );

                    let name = participant.1.name();
                    if participant.0.as_str().contains("audio")
                        || participant.0.as_str().contains("camera")
                        || name.is_empty()
                    {
                        continue;
                    }

                    if let Err(e) = event_loop_proxy.send_event(UserEvent::ParticipantConnected(
                        ParticipantData {
                            name,
                            sid: participant.1.sid().as_str().to_string(),
                        },
                    )) {
                        log::error!(
                            "handle_room_events: Failed to send participant disconnected event: {e:?}"
                        );
                    }
                }
            }
            RoomServiceCommand::PublishParticipantInControl(participant) => {
                let room = inner.room.lock().await;
                if room.is_none() {
                    log::warn!("room_service_commands: Room doesn't exist");
                    continue;
                }
                let room = room.as_ref().unwrap();
                let local_participant = room.local_participant();
                let res = local_participant
                    .publish_data(DataPacket {
                        payload: participant.to_string().as_bytes().to_vec(),
                        reliable: true,
                        topic: Some(TOPIC_PARTICIPANT_IN_CONTROL.to_string()),
                        ..Default::default()
                    })
                    .await;
                if let Err(e) = res {
                    log::error!(
                        "room_service_commands: Failed to publish participant in control: {e:?}"
                    );
                }
            }
        }
    }
}

/// Represents a 2D point with floating-point coordinates.
///
/// This structure is used to represent cursor positions, mouse coordinates,
/// and other 2D locations within the room service.
#[derive(Debug, Serialize, Deserialize, Clone, Copy)]
pub struct ClientPoint {
    /// The x-coordinate of the point
    pub x: f64,
    /// The y-coordinate of the point
    pub y: f64,
}

/// Contains data for mouse click events.
///
/// This structure captures all the information needed to represent a mouse click,
/// including position, button information, modifier keys, and click state.
#[derive(Debug, Serialize, Deserialize)]
pub struct MouseClickData {
    /// The x-coordinate where the click occurred
    pub x: f64,
    /// The y-coordinate where the click occurred
    pub y: f64,
    /// The mouse button that was clicked (0=left, 1=right, 2=middle)
    pub button: u32,
    /// The number of clicks (1=single, 2=double, etc.)
    pub clicks: u32,
    /// Whether the button is being pressed down (true) or released (false)
    pub down: bool,
    /// Whether the Shift key was held during the click
    pub shift: bool,
    /// Whether the Meta/Cmd key was held during the click
    pub meta: bool,
    /// Whether the Ctrl key was held during the click
    pub ctrl: bool,
    /// Whether the Alt key was held during the click
    pub alt: bool,
}

/// Contains data for mouse visibility events.
///
/// This structure is used to communicate whether the mouse cursor should be
/// visible or hidden on remote clients.
#[derive(Debug, Serialize, Deserialize)]
pub struct MouseVisibleData {
    /// Whether the mouse cursor should be visible
    pub visible: bool,
}

/// Contains data for mouse wheel scroll events.
///
/// This structure represents the scroll delta values for both horizontal
/// and vertical scrolling directions.
#[derive(Debug, Serialize, Deserialize)]
#[allow(non_snake_case)]
pub struct WheelDelta {
    /// The horizontal scroll delta (positive = right, negative = left)
    pub deltaX: f64,
    /// The vertical scroll delta (positive = down, negative = up)
    pub deltaY: f64,
}

/// Contains data for keyboard input events.
///
/// This structure captures keyboard input including the keys pressed
/// and any modifier keys that were held during the keystroke.
#[derive(Debug, Serialize, Deserialize)]
pub struct KeystrokeData {
    /// The key(s) that were pressed (as string representations)
    pub key: Vec<String>,
    /// Whether the Meta/Cmd key was held during the keystroke
    pub meta: bool,
    /// Whether the Ctrl key was held during the keystroke
    pub ctrl: bool,
    /// Whether the Shift key was held during the keystroke
    pub shift: bool,
    /// Whether the Alt key was held during the keystroke
    pub alt: bool,
    /// Whether the key is being pressed down (true) or released (false)
    pub down: bool,
}

/// Contains timing data for tick events.
///
/// This structure is used for synchronization and latency measurement
/// between room participants.
#[derive(Debug, Serialize, Deserialize)]
pub struct TickData {
    /// The timestamp value (typically in nanoseconds)
    pub time: u128,
}

/// Contains the remote control enabled/disabled state.
///
/// This structure is used to communicate whether remote control
/// functionality is currently enabled in the room.
#[derive(Debug, Serialize, Deserialize)]
pub struct RemoteControlEnabled {
    /// Whether remote control is currently enabled
    pub enabled: bool,
}

/// Contains data for clipboard events.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AddToClipboardData {
    /// The text to be added to the clipboard
    pub is_copy: bool,
}

/// Contains data for clipboard events.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ClipboardPayload {
    pub packet_id: u64,
    pub total_packets: u64,
    pub data: Vec<u8>,
}

/// Contains data for clipboard events.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PasteFromClipboardData {
    pub data: Option<ClipboardPayload>,
}

/// Settings specific to the Draw mode.
#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
pub struct DrawSettings {
    /// Whether drawn lines should be permanent or fade away after a while
    pub permanent: bool,
}

/// Drawing mode options - specifies the type of drawing operation.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(tag = "type", content = "settings")]
pub enum DrawingModeOption {
    /// Standard drawing mode with its settings
    Draw(DrawSettings),
    /// Click animation mode
    ClickAnimation,
}

/// Contains data for drawing mode enable/disable events.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DrawingModeData {
    /// Whether drawing mode is enabled
    pub enabled: bool,
    /// The drawing mode option with its settings
    pub mode: DrawingModeOption,
}

/// Represents all possible client events that can be sent between room participants.
///
/// This enum defines the different types of events that can be transmitted through
/// the LiveKit room, including input events, cursor movements, and control messages.
/// Events are serialized as JSON with a `type` field and `payload` field containing
/// the event-specific data.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload")]
pub enum ClientEvent {
    /// Mouse cursor movement event from a remote controller
    MouseMove(ClientPoint),
    /// Mouse click event from a remote controller
    MouseClick(MouseClickData),
    /// Mouse visibility change event
    MouseVisible(MouseVisibleData),
    /// Keyboard input event from a remote controller
    Keystroke(KeystrokeData),
    /// Mouse wheel scroll event from a remote controller
    WheelEvent(WheelDelta),
    /// Timing synchronization request
    Tick(TickData),
    /// Response to a timing synchronization request
    TickResponse(TickData),
    /// Remote control enabled/disabled status change
    RemoteControlEnabled(RemoteControlEnabled),
    /// Copy or cut command from a remote controller
    AddToClipboard(AddToClipboardData),
    /// Paste command from a remote controller
    PasteFromClipboard(PasteFromClipboardData),
    /// Drawing mode enable/disable event
    DrawingMode(DrawingModeData),
    /// Drawing started at a point
    DrawStart(ClientPoint),
    /// Add a point to the current drawing
    DrawAddPoint(ClientPoint),
    /// Drawing ended at a point
    DrawEnd(ClientPoint),
    /// Click animation at a point
    ClickAnimation(ClientPoint),
}

async fn handle_room_events(
    mut receiver: mpsc::UnboundedReceiver<RoomEvent>,
    event_loop_proxy: EventLoopProxy<UserEvent>,
    user_sid: String,
) {
    while let Some(msg) = receiver.recv().await {
        match msg {
            RoomEvent::DataReceived {
                payload,
                topic: _,
                kind: _,
                participant,
            } => {
                let client_event: ClientEvent = match serde_json::from_slice(&payload) {
                    Ok(event) => event,
                    Err(e) => {
                        log::error!("handle_room_events: Failed to deserialize event: {e:?}");
                        continue;
                    }
                };
                log::debug!("handle_room_events: Data received: {client_event:?}");
                let sid = if let Some(participant) = participant {
                    participant.sid().as_str().to_string()
                } else {
                    log::warn!("handle_room_events: Participant is none");
                    "".to_string()
                };

                /* Skip our own events. */
                if sid == user_sid {
                    log::debug!("handle_room_events: Skipping own event");
                    continue;
                }

                let res = match client_event {
                    ClientEvent::MouseMove(point) => {
                        /* let point = translate_mouse_position(point, menu_perc); */
                        event_loop_proxy.send_event(UserEvent::CursorPosition(
                            point.x as f32,
                            point.y as f32,
                            sid,
                        ))
                    }
                    ClientEvent::MouseClick(click) => {
                        event_loop_proxy.send_event(UserEvent::MouseClick(
                            crate::MouseClickData {
                                x: click.x as f32,
                                y: click.y as f32,
                                button: click.button,
                                clicks: click.clicks as f32,
                                down: click.down,
                                shift: click.shift,
                                meta: click.meta,
                                ctrl: click.ctrl,
                                alt: click.alt,
                            },
                            sid,
                        ))
                    }
                    ClientEvent::MouseVisible(visible_data) => event_loop_proxy.send_event(
                        UserEvent::ControllerCursorVisible(visible_data.visible, sid),
                    ),
                    ClientEvent::Keystroke(key) => {
                        event_loop_proxy.send_event(UserEvent::Keystroke(crate::KeystrokeData {
                            key: key.key[0].clone(),
                            meta: key.meta,
                            ctrl: key.ctrl,
                            shift: key.shift,
                            alt: key.alt,
                            down: key.down,
                        }))
                    }
                    ClientEvent::WheelEvent(wheel_data) => {
                        event_loop_proxy.send_event(UserEvent::Scroll(
                            crate::ScrollDelta {
                                x: wheel_data.deltaX,
                                y: wheel_data.deltaY,
                            },
                            sid,
                        ))
                    }
                    ClientEvent::Tick(tick_data) => {
                        if cfg!(debug_assertions) {
                            event_loop_proxy.send_event(UserEvent::Tick(tick_data.time))
                        } else {
                            Ok(())
                        }
                    }
                    ClientEvent::AddToClipboard(add_to_clipboard_data) => event_loop_proxy
                        .send_event(UserEvent::AddToClipboard(add_to_clipboard_data)),
                    ClientEvent::PasteFromClipboard(paste_from_clipboard_data) => event_loop_proxy
                        .send_event(UserEvent::PasteFromClipboard(paste_from_clipboard_data)),
                    ClientEvent::DrawingMode(drawing_mode_data) => {
                        event_loop_proxy.send_event(UserEvent::DrawingMode(drawing_mode_data, sid))
                    }
                    ClientEvent::DrawStart(point) => {
                        event_loop_proxy.send_event(UserEvent::DrawStart(point, sid))
                    }
                    ClientEvent::DrawAddPoint(point) => {
                        event_loop_proxy.send_event(UserEvent::DrawAddPoint(point, sid))
                    }
                    ClientEvent::DrawEnd(point) => {
                        event_loop_proxy.send_event(UserEvent::DrawEnd(point, sid))
                    }
                    ClientEvent::ClickAnimation(point) => event_loop_proxy
                        .send_event(UserEvent::ClickAnimationFromParticipant(point, sid)),
                    _ => Ok(()),
                };
                if let Err(e) = res {
                    log::error!("handle_room_events: Failed to send message: {e:?}");
                }
            }
            RoomEvent::ParticipantConnected(participant) => {
                log::info!(
                    "handle_room_events: Participant connected: {}",
                    participant.sid()
                );

                let name = participant.name();
                let participant_id = participant.identity().as_str().to_string();
                if participant_id.contains("audio")
                    || participant_id.contains("camera")
                    || name.is_empty()
                {
                    log::debug!("handle_room_events: Skipping participant: {participant:?}");
                    continue;
                }

                if let Err(e) =
                    event_loop_proxy.send_event(UserEvent::ParticipantConnected(ParticipantData {
                        name,
                        sid: participant.sid().as_str().to_string(),
                    }))
                {
                    log::error!(
                        "handle_room_events: Failed to send participant connected event: {e:?}"
                    );
                }
            }
            RoomEvent::ParticipantDisconnected(participant) => {
                log::info!(
                    "handle_room_events: Participant disconnected: {}",
                    participant.sid()
                );

                if let Err(e) = event_loop_proxy.send_event(UserEvent::ParticipantDisconnected(
                    ParticipantData {
                        name: participant.name(),
                        sid: participant.sid().as_str().to_string(),
                    },
                )) {
                    log::error!(
                        "handle_room_events: Failed to send participant disconnected event: {e:?}"
                    );
                }
            }
            RoomEvent::TrackPublished {
                publication: _,
                participant,
            } => {
                log::info!(
                    "handle_room_events: Track published: from {}",
                    participant.sid()
                );
                let participant_id = participant.identity().as_str().to_string();
                if participant_id.contains("video") {
                    log::info!(
                        "handle_room_events: Controller {} takes screen share",
                        participant.sid()
                    );
                    if let Err(e) =
                        event_loop_proxy.send_event(UserEvent::ControllerTakesScreenShare)
                    {
                        log::error!(
                            "handle_room_events: Failed to send controller takes screen share event: {e:?}"
                        );
                    }
                }
            }
            _ => {}
        }
    }
}

async fn stats_loop(inner: Arc<RoomServiceInner>) {
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(5));
    let mut prev_bytes = 0;

    // Initial tick
    interval.tick().await;

    loop {
        interval.tick().await;
        let current_bytes = {
            let room_guard = inner.room.lock().await;
            if let Some(room) = room_guard.as_ref() {
                get_rtc_stats(room).await
            } else {
                0
            }
        };

        if prev_bytes > 0 && current_bytes > prev_bytes {
            let bitrate = ((current_bytes - prev_bytes) * 8) / 5; // bits per second
            log::debug!("WebRTC Bandwidth: {:.2} Mbps", bitrate as f64 / 1_000_000.0);
        }
        prev_bytes = current_bytes;
    }
}

async fn get_rtc_stats(room: &Room) -> u64 {
    let mut total_bytes_sent = 0;
    let local_participant = room.local_participant();

    for (_, publication) in local_participant.track_publications() {
        let track = publication.track();
        if track.is_none() {
            continue;
        }
        let track = track.unwrap();
        if let LocalTrack::Video(track) = track {
            if let Ok(stats) = track.get_stats().await {
                for stat in stats {
                    if let RtcStats::OutboundRtp(stats) = stat {
                        total_bytes_sent += stats.sent.bytes_sent;
                    }
                }
            }
        }
    }
    total_bytes_sent
}
