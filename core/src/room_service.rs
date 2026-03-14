use std::collections::HashMap;
use std::sync::Arc;

use livekit::options::{TrackPublishOptions, VideoCodec, VideoEncoding};
use livekit::participant::ConnectionQuality;
use livekit::track::{LocalTrack, LocalVideoTrack, TrackSource};
use livekit::webrtc::prelude::{RtcVideoSource, VideoResolution};
use livekit::webrtc::video_source::native::NativeVideoSource;
use livekit::{DataPacket, Room, RoomEvent, RoomOptions};

use crate::audio::mixer::SharedProcessor;
use crate::livekit::audio::AudioPublisher;
use crate::livekit::participant::ParticipantInfo;
use crate::livekit::video::{process_video_stream, VideoBufferManager};

use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tokio::sync::Mutex;
use winit::event_loop::EventLoopProxy;

use crate::{audio, ParticipantData, UserEvent};

// Constants for magic values
const TOPIC_SHARER_LOCATION: &str = "participant_location";
const TOPIC_REMOTE_CONTROL_ENABLED: &str = "remote_control_enabled";
const TOPIC_PARTICIPANT_IN_CONTROL: &str = "participant_in_control";
const TOPIC_TICK_RESPONSE: &str = "tick_response";
const VIDEO_TRACK_NAME: &str = "screen_share";
const TOPIC_DRAW: &str = "draw";
const MAX_FRAMERATE: f64 = 40.0;
const CAMERA_TRACK_NAME: &str = "camera";
const CAMERA_MAX_BITRATE: u64 = 1_700_000;
const CAMERA_MAX_FRAMERATE: f64 = 30.0;

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

const COMMAND_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(2000);

#[derive(Debug)]
enum RoomServiceCommand {
    CreateRoom {
        token: String,
        video_token: String,
        event_loop_proxy: EventLoopProxy<UserEvent>,
    },
    PublishTrack {
        width: u32,
        height: u32,
        use_av1: bool,
    },
    PublishCursorPosition(f64, f64, bool),
    PublishControllerCursorEnabled(bool),
    DestroyRoom,
    TickResponse(u128),
    IterateParticipants,
    PublishParticipantInControl(String),
    PublishDrawStart(DrawPathPoint),
    PublishDrawAddPoint(ClientPoint),
    PublishDrawEnd(ClientPoint),
    PublishDrawClearPaths(Vec<u64>),
    PublishDrawClearAllPaths,
    PublishDrawingMode(DrawingMode),
    PublishAudioTrack {
        sample_rate: u32,
        sample_rx: mpsc::UnboundedReceiver<Vec<i16>>,
    },
    UnpublishAudioTrack,
    MuteAudioTrack,
    UnmuteAudioTrack,
    PublishCameraTrack {
        width: u32,
        height: u32,
    },
    UnpublishCameraTrack,
    UnpublishScreenShareTrack,
    PublishMouseClick(MouseClickData),
    PublishMouseVisible(MouseVisibleData),
    PublishKeystroke(KeystrokeData),
    PublishWheelEvent(WheelDelta),
    PublishAddToClipboard(AddToClipboardData),
    PublishPasteFromClipboard(PasteFromClipboardData),
    PublishClickAnimation(ClientPoint),
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
    #[error("Command timed out")]
    Timeout,
}

#[derive(Debug)]
struct RemoteScreenShare {
    buffer: Arc<std::sync::Mutex<Option<Arc<VideoBufferManager>>>>,
    stop_tx: Arc<std::sync::Mutex<Option<mpsc::UnboundedSender<()>>>>,
    publisher_sid: Arc<std::sync::Mutex<Option<String>>>,
}

/*
 * This struct is used for handling room events and functions
 * from a thread in the async runtime.
 */
#[derive(Debug)]
pub(crate) struct RoomServiceInner {
    // TODO: See if we can use a sync::Mutex instead of tokio::sync::Mutex
    pub(crate) room: Mutex<Option<Room>>,
    pub(crate) video_room: Mutex<Option<Room>>,
    buffer_source: std::sync::Mutex<Option<NativeVideoSource>>,
    camera_buffer_source: std::sync::Mutex<Option<NativeVideoSource>>,
    participants: Arc<std::sync::RwLock<HashMap<String, ParticipantInfo>>>,
    mixer: audio::mixer::MixerHandle,
    remote_screen_share: RemoteScreenShare,
    pub(crate) stats: std::sync::RwLock<crate::livekit::stats::RoomStats>,
    connection_quality: Arc<std::sync::Mutex<Option<ConnectionQuality>>>,
    // TODO: be careful on how to do participants update, with locking, when the camera window integration will happen.
}

/// Inserts a remote participant into the map if not already present.
/// Returns `true` if the participant was newly inserted.
fn insert_participant_if_absent(
    participants: &std::sync::RwLock<HashMap<String, ParticipantInfo>>,
    sid: &str,
    remote_participant: &livekit::participant::RemoteParticipant,
) -> bool {
    if remote_participant.identity().as_str().contains("video") {
        return false;
    }
    let mut guard = participants.write().unwrap();
    if guard.contains_key(sid) {
        return false;
    }
    guard.insert(
        sid.to_string(),
        ParticipantInfo::from_remote_participant(remote_participant),
    );
    true
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
        mixer: audio::mixer::MixerHandle,
        audio_processor: SharedProcessor,
    ) -> Result<Self, std::io::Error> {
        let async_runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()?;

        let inner = Arc::new(RoomServiceInner {
            room: Mutex::new(None),
            video_room: Mutex::new(None),
            buffer_source: std::sync::Mutex::new(None),
            camera_buffer_source: std::sync::Mutex::new(None),
            participants: Arc::new(std::sync::RwLock::new(HashMap::new())),
            mixer,
            remote_screen_share: RemoteScreenShare {
                buffer: Arc::new(std::sync::Mutex::new(None)),
                stop_tx: Arc::new(std::sync::Mutex::new(None)),
                publisher_sid: Arc::new(std::sync::Mutex::new(None)),
            },
            stats: std::sync::RwLock::new(crate::livekit::stats::RoomStats::default()),
            connection_quality: Arc::new(std::sync::Mutex::new(None)),
        });
        let (service_command_tx, service_command_rx) = mpsc::unbounded_channel();
        let (service_command_res_tx, service_command_res_rx) = std::sync::mpsc::channel();
        async_runtime.spawn(room_service_commands(
            service_command_rx,
            service_command_res_tx,
            inner.clone(),
            livekit_server_url,
            event_loop_proxy,
            audio_processor,
        ));

        Ok(Self {
            _async_runtime: async_runtime,
            service_command_tx,
            service_command_res_rx,
            inner,
        })
    }

    pub fn stats(&self) -> crate::livekit::stats::RoomStats {
        self.inner.stats.read().unwrap().clone()
    }

    pub fn connection_quality(&self) -> Option<ConnectionQuality> {
        *self.inner.connection_quality.lock().unwrap()
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
        video_token: String,
        event_loop_proxy: EventLoopProxy<UserEvent>,
    ) -> Result<(), RoomServiceError> {
        log::info!("create_room");
        let res = self
            .service_command_tx
            .send(RoomServiceCommand::CreateRoom {
                token,
                video_token,
                event_loop_proxy,
            });
        if let Err(e) = res {
            return Err(RoomServiceError::CreateRoom(format!(
                "Failed to send command: {e:?}"
            )));
        }
        let res = self.service_command_res_rx.recv_timeout(COMMAND_TIMEOUT);
        match res {
            Ok(RoomServiceCommandResult::Success) => Ok(()),
            Ok(RoomServiceCommandResult::Failure) => Err(RoomServiceError::CreateRoom(
                "Failed to create room".to_string(),
            )),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => Err(RoomServiceError::Timeout),
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
    ///
    /// # Returns
    ///
    /// * `Ok(())` - The track was published successfully
    /// * `Err(())` - The track was not published successfully
    pub fn publish_track(&self, width: u32, height: u32) -> Result<(), RoomServiceError> {
        log::info!("publish_track: {width:?}, {height:?}");
        let use_av1 = true;
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
        let res = self.service_command_res_rx.recv_timeout(COMMAND_TIMEOUT);
        match res {
            Ok(RoomServiceCommandResult::Success) => Ok(()),
            Ok(RoomServiceCommandResult::Failure) => Err(RoomServiceError::PublishTrack(
                "Failed to publish track".to_string(),
            )),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => Err(RoomServiceError::Timeout),
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

    /// Publishes the sharer's cursor position to the room.
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
    pub fn publish_cursor_position(&self, x: f64, y: f64, pointer: bool) {
        log::debug!("publish_cursor_position: {x:?}, {y:?}, {pointer:?}");
        let res = self
            .service_command_tx
            .send(RoomServiceCommand::PublishCursorPosition(x, y, pointer));
        if let Err(e) = res {
            log::error!("publish_cursor_position: Failed to send command: {e:?}");
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
            log::error!("tick_response: Failed to send command: {e:?}");
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

    pub fn publish_draw_start(&self, point: DrawPathPoint) {
        log::debug!("publish_draw_start: {:?}", point);
        let res = self
            .service_command_tx
            .send(RoomServiceCommand::PublishDrawStart(point));
        if let Err(e) = res {
            log::error!("publish_draw_start: Error sending command: {e:?}");
        }
    }

    pub fn publish_draw_add_point(&self, point: ClientPoint) {
        log::debug!("publish_draw_add_point: {:?}", point);
        let res = self
            .service_command_tx
            .send(RoomServiceCommand::PublishDrawAddPoint(point));
        if let Err(e) = res {
            log::error!("publish_draw_add_point: Error sending command: {e:?}");
        }
    }

    pub fn publish_draw_end(&self, point: ClientPoint) {
        log::debug!("publish_draw_end: {:?}", point);
        let res = self
            .service_command_tx
            .send(RoomServiceCommand::PublishDrawEnd(point));
        if let Err(e) = res {
            log::error!("publish_draw_end: Error sending command: {e:?}");
        }
    }

    pub fn publish_draw_clear_paths(&self, path_ids: Vec<u64>) {
        log::debug!("publish_draw_clear_paths: {:?}", path_ids);
        let res = self
            .service_command_tx
            .send(RoomServiceCommand::PublishDrawClearPaths(path_ids));
        if let Err(e) = res {
            log::error!("publish_draw_clear_paths: Error sending command: {e:?}");
        }
    }

    pub fn publish_draw_clear_all_paths(&self) {
        log::debug!("publish_draw_clear_all_paths");
        let res = self
            .service_command_tx
            .send(RoomServiceCommand::PublishDrawClearAllPaths);
        if let Err(e) = res {
            log::error!("publish_draw_clear_all_paths: Error sending command: {e:?}");
        }
    }

    pub fn publish_drawing_mode(&self, mode: DrawingMode) {
        log::debug!("publish_drawing_mode: {:?}", mode);
        let res = self
            .service_command_tx
            .send(RoomServiceCommand::PublishDrawingMode(mode));
        if let Err(e) = res {
            log::error!("publish_drawing_mode: Error sending command: {e:?}");
        }
    }

    /// Publishes an audio track to the room. Blocks until complete.
    pub fn publish_audio_track(
        &self,
        sample_rate: u32,
        sample_rx: mpsc::UnboundedReceiver<Vec<i16>>,
    ) -> Result<(), RoomServiceError> {
        log::info!("publish_audio_track with sample_rate: {}", sample_rate);
        let res = self
            .service_command_tx
            .send(RoomServiceCommand::PublishAudioTrack {
                sample_rate,
                sample_rx,
            });
        if let Err(e) = res {
            return Err(RoomServiceError::PublishTrack(format!(
                "Failed to send command: {e:?}"
            )));
        }
        let res = self.service_command_res_rx.recv_timeout(COMMAND_TIMEOUT);
        match res {
            Ok(RoomServiceCommandResult::Success) => Ok(()),
            Ok(RoomServiceCommandResult::Failure) => Err(RoomServiceError::PublishTrack(
                "Failed to publish audio track".to_string(),
            )),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => Err(RoomServiceError::Timeout),
            Err(e) => Err(RoomServiceError::PublishTrack(format!(
                "Failed to receive result: {e:?}"
            ))),
        }
    }

    /// Unpublishes the audio track from the room.
    pub fn unpublish_audio_track(&self) {
        log::info!("unpublish_audio_track");
        let res = self
            .service_command_tx
            .send(RoomServiceCommand::UnpublishAudioTrack);
        if let Err(e) = res {
            log::error!("unpublish_audio_track: Failed to send command: {e:?}");
        }
    }

    /// Mutes the audio track.
    pub fn mute_audio_track(&self) {
        log::info!("mute_audio_track");
        let res = self
            .service_command_tx
            .send(RoomServiceCommand::MuteAudioTrack);
        if let Err(e) = res {
            log::error!("mute_audio_track: Failed to send command: {e:?}");
        }
    }

    /// Publishes a camera video track. Blocks until complete.
    pub fn publish_camera_track(&self, width: u32, height: u32) -> Result<(), RoomServiceError> {
        log::info!("publish_camera_track: {width}x{height}");
        let res = self
            .service_command_tx
            .send(RoomServiceCommand::PublishCameraTrack { width, height });
        if let Err(e) = res {
            return Err(RoomServiceError::PublishTrack(format!(
                "Failed to send command: {e:?}"
            )));
        }
        let res = self.service_command_res_rx.recv_timeout(COMMAND_TIMEOUT);
        match res {
            Ok(RoomServiceCommandResult::Success) => Ok(()),
            Ok(RoomServiceCommandResult::Failure) => Err(RoomServiceError::PublishTrack(
                "Failed to publish camera track".to_string(),
            )),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => Err(RoomServiceError::Timeout),
            Err(e) => Err(RoomServiceError::PublishTrack(format!(
                "Failed to receive result: {e:?}"
            ))),
        }
    }

    /// Unpublishes the camera track from the room.
    pub fn unpublish_camera_track(&self) {
        log::info!("unpublish_camera_track");
        let res = self
            .service_command_tx
            .send(RoomServiceCommand::UnpublishCameraTrack);
        if let Err(e) = res {
            log::error!("unpublish_camera_track: Failed to send command: {e:?}");
        }
    }

    /// Unpublishes the screen share track from the room.
    pub fn unpublish_screen_share_track(&self) {
        log::info!("unpublish_screen_share_track");
        let res = self
            .service_command_tx
            .send(RoomServiceCommand::UnpublishScreenShareTrack);
        if let Err(e) = res {
            log::error!("unpublish_screen_share_track: Failed to send command: {e:?}");
        }
    }

    /// Publishes a mouse click event to the room.
    pub fn publish_mouse_click(&self, data: MouseClickData) {
        log::debug!("publish_mouse_click: {data:?}");
        let res = self
            .service_command_tx
            .send(RoomServiceCommand::PublishMouseClick(data));
        if let Err(e) = res {
            log::error!("publish_mouse_click: Failed to send command: {e:?}");
        }
    }

    /// Publishes mouse visibility state to the room.
    pub fn publish_mouse_visible(&self, data: MouseVisibleData) {
        log::debug!("publish_mouse_visible: {data:?}");
        let res = self
            .service_command_tx
            .send(RoomServiceCommand::PublishMouseVisible(data));
        if let Err(e) = res {
            log::error!("publish_mouse_visible: Failed to send command: {e:?}");
        }
    }

    /// Publishes a keystroke event to the room.
    pub fn publish_keystroke(&self, data: KeystrokeData) {
        log::debug!("publish_keystroke: {data:?}");
        let res = self
            .service_command_tx
            .send(RoomServiceCommand::PublishKeystroke(data));
        if let Err(e) = res {
            log::error!("publish_keystroke: Failed to send command: {e:?}");
        }
    }

    /// Publishes a wheel/scroll event to the room.
    pub fn publish_wheel_event(&self, data: WheelDelta) {
        log::debug!("publish_wheel_event: {data:?}");
        let res = self
            .service_command_tx
            .send(RoomServiceCommand::PublishWheelEvent(data));
        if let Err(e) = res {
            log::error!("publish_wheel_event: Failed to send command: {e:?}");
        }
    }

    /// Publishes an add to clipboard event to the room.
    pub fn publish_add_to_clipboard(&self, data: AddToClipboardData) {
        log::debug!("publish_add_to_clipboard");
        let res = self
            .service_command_tx
            .send(RoomServiceCommand::PublishAddToClipboard(data));
        if let Err(e) = res {
            log::error!("publish_add_to_clipboard: Failed to send command: {e:?}");
        }
    }

    /// Publishes a paste from clipboard event to the room.
    pub fn publish_paste_from_clipboard(&self, data: PasteFromClipboardData) {
        log::debug!("publish_paste_from_clipboard");
        let res = self
            .service_command_tx
            .send(RoomServiceCommand::PublishPasteFromClipboard(data));
        if let Err(e) = res {
            log::error!("publish_paste_from_clipboard: Failed to send command: {e:?}");
        }
    }

    /// Publishes a click animation event to the room.
    pub fn publish_click_animation(&self, point: ClientPoint) {
        log::debug!("publish_click_animation: {point:?}");
        let res = self
            .service_command_tx
            .send(RoomServiceCommand::PublishClickAnimation(point));
        if let Err(e) = res {
            log::error!("publish_click_animation: Failed to send command: {e:?}");
        }
    }

    /// Retrieves the camera video source buffer.
    pub fn get_camera_buffer_source(&self) -> Option<NativeVideoSource> {
        log::info!("get_camera_buffer_source");
        let inner = self.inner.camera_buffer_source.lock().unwrap();
        inner.clone()
    }

    /// Returns a shared reference to the participants map.
    pub fn participants(&self) -> Arc<std::sync::RwLock<HashMap<String, ParticipantInfo>>> {
        self.inner.participants.clone()
    }

    /// Creates and sets a camera buffer manager for the local participant.
    pub fn local_camera_buffer_manager(&self) -> Arc<VideoBufferManager> {
        let mut participants = self.inner.participants.write().unwrap();
        let info = participants
            .get_mut("local")
            .expect("local participant info not found"); // This should never happen.
        info.camera_buffers()
    }

    /// Returns the remote screen share buffer if available.
    pub fn screen_share_buffer(&self) -> Option<Arc<VideoBufferManager>> {
        let buffer = self.inner.remote_screen_share.buffer.lock().unwrap();
        buffer.clone()
    }

    /// Returns whether the local audio track is currently muted.
    pub fn is_audio_muted(&self) -> bool {
        if let Ok(participants) = self.inner.participants.read() {
            if let Some(local) = participants.get("local") {
                return local.muted();
            }
        }
        true
    }

    /// Builds a snapshot of all participants for forwarding to Tauri.
    pub fn participants_snapshot(&self) -> Vec<socket_lib::CoreParticipantState> {
        build_participants_snapshot(&self.inner.participants)
    }

    /// Unmutes the audio track.
    pub fn unmute_audio_track(&self) {
        log::info!("unmute_audio_track");
        let res = self
            .service_command_tx
            .send(RoomServiceCommand::UnmuteAudioTrack);
        if let Err(e) = res {
            log::error!("unmute_audio_track: Failed to send command: {e:?}");
        }
    }
}

/// Builds a deduplicated snapshot of participant states from the internal HashMap.
/// Each user may have multiple LiveKit participants (audio, video, camera);
/// this groups them by user ID and uses the audio participant's mute status.
fn build_participants_snapshot(
    participants: &Arc<std::sync::RwLock<HashMap<String, ParticipantInfo>>>,
) -> Vec<socket_lib::CoreParticipantState> {
    let guard = participants.read().unwrap();
    let mut seen: HashMap<String, socket_lib::CoreParticipantState> = HashMap::new();

    for info in guard.values() {
        let identity = info.identity();
        // Identity format: "room:<roomId>:<userId>:<trackType>"
        let parts: Vec<&str> = identity.split(':').collect();
        if parts.len() < 4 {
            continue;
        }
        let user_id = parts[2];
        let track_type = parts[3];

        let entry =
            seen.entry(user_id.to_string())
                .or_insert_with(|| socket_lib::CoreParticipantState {
                    identity: identity.to_string(),
                    name: info.name().to_string(),
                    connected: true,
                    muted: false,
                    has_camera: false,
                    is_screensharing: false,
                });

        if track_type == "audio" {
            entry.muted = info.muted();
        }

        entry.has_camera = entry.has_camera || info.camera_active();
        entry.is_screensharing = entry.is_screensharing || info.is_screensharing();
    }

    seen.into_values().collect()
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
/// * `PublishCursorPosition` - Publishes cursor position data to the room
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
    audio_processor: SharedProcessor,
) {
    let mut stats_task: Option<tokio::task::JoinHandle<()>> = None;
    let mut audio_publisher: Option<AudioPublisher> = None;

    while let Some(command) = service_rx.recv().await {
        log::debug!("room_service_commands: Received command {command:?}");
        match command {
            // TODO: Break this into create room and publish track commands
            RoomServiceCommand::CreateRoom {
                token,
                video_token,
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
                {
                    let mut inner_video_room = inner.video_room.lock().await;
                    if let Some(video_room) = inner_video_room.take() {
                        log::warn!("room_service_commands: Video room already exists, killing it.");
                        if let Err(e) = video_room.close().await {
                            log::error!("room_service_commands: Failed to close video room: {e:?}");
                        }
                    }
                }

                // Clear participants when joining a new room
                {
                    let mut participants = inner.participants.write().unwrap();
                    participants.clear();
                }

                let url = livekit_server_url.clone();

                let connect_result = Room::connect(&url, &token, RoomOptions::default()).await;
                let (room, rx) = match connect_result {
                    Ok((room, rx)) => (room, rx),
                    Err(e) => {
                        log::error!("room_service_commands: Failed to connect to room {:?}", e);
                        let res = tx.send(RoomServiceCommandResult::Failure);
                        if let Err(e) = res {
                            log::error!("room_service_commands: Failed to send result: {e:?}");
                        }
                        continue;
                    }
                };

                if let Some(task) = stats_task.take() {
                    task.abort();
                }
                stats_task = Some(tokio::spawn(crate::livekit::stats::stats_loop(
                    inner.clone(),
                )));

                let user_sid = room.local_participant().sid().as_str().to_string();
                let user_identity = room.local_participant().identity().as_str().to_string();
                let user_name = room.local_participant().name();

                // Insert local participant into participants map
                {
                    let mut participants = inner.participants.write().unwrap();
                    participants.insert(
                        "local".to_string(),
                        ParticipantInfo::new(user_identity, user_name, false, false),
                    );
                }

                // Connect video room for screen share publishing
                let video_participant_sid =
                    match Room::connect(&url, &video_token, RoomOptions::default()).await {
                        Ok((video_room, _video_rx)) => {
                            let sid = video_room.local_participant().sid().as_str().to_string();
                            let mut inner_video_room = inner.video_room.lock().await;
                            *inner_video_room = Some(video_room);
                            sid
                        }
                        Err(e) => {
                            log::error!(
                                "room_service_commands: Failed to connect video room: {e:?}"
                            );
                            String::new()
                        }
                    };

                /* Spawn thread for handling livekit data events. */
                tokio::spawn(handle_room_events(
                    rx,
                    event_loop_proxy,
                    user_sid,
                    video_participant_sid,
                    inner.participants.clone(),
                    inner.mixer.clone(),
                    RemoteScreenShare {
                        buffer: inner.remote_screen_share.buffer.clone(),
                        stop_tx: inner.remote_screen_share.stop_tx.clone(),
                        publisher_sid: inner.remote_screen_share.publisher_sid.clone(),
                    },
                    inner.connection_quality.clone(),
                ));

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
                let inner_video_room = inner.video_room.lock().await;
                if inner_video_room.is_none() {
                    log::error!("room_service_commands: Video room doesn't exist.");
                    let res = tx.send(RoomServiceCommandResult::Failure);
                    if let Err(e) = res {
                        log::error!("room_service_commands: Failed to send result: {e:?}");
                    }
                    continue;
                }
                let room = inner_video_room.as_ref().unwrap();

                let buffer_source = NativeVideoSource::new(VideoResolution { width, height }, true);
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

                {
                    let mut inner_room = inner.room.lock().await;
                    if let Some(room) = inner_room.take() {
                        if let Err(e) = room.close().await {
                            log::error!("room_service_commands: Failed to close room: {e:?}");
                        }
                    }
                }
                {
                    let mut inner_video_room = inner.video_room.lock().await;
                    if let Some(video_room) = inner_video_room.take() {
                        if let Err(e) = video_room.close().await {
                            log::error!("room_service_commands: Failed to close video room: {e:?}");
                        }
                    }
                }

                // Clean up screen share buffer source
                {
                    let mut inner_buffer_source = inner.buffer_source.lock().unwrap();
                    inner_buffer_source.take();
                }

                // Clean up camera buffer source
                {
                    let mut inner_buffer_source = inner.camera_buffer_source.lock().unwrap();
                    inner_buffer_source.take();
                }

                // Clean up remote screen share resources
                {
                    let mut stop_tx_guard = inner.remote_screen_share.stop_tx.lock().unwrap();
                    if let Some(tx) = stop_tx_guard.take() {
                        let _ = tx.send(());
                    }
                }

                // Drop the participants
                {
                    let mut participants = inner.participants.write().unwrap();
                    participants.clear();
                }

                {
                    inner.remote_screen_share.buffer.lock().unwrap().take();
                    inner
                        .remote_screen_share
                        .publisher_sid
                        .lock()
                        .unwrap()
                        .take();
                }
            }
            RoomServiceCommand::PublishCursorPosition(x, y, _pointer) => {
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
                    log::error!("room_service_commands: Failed to publish cursor position: {e:?}");
                }
                log::debug!(
                    "Published cursor position with x: {x:?}, y: {y:?} to topic: {TOPIC_SHARER_LOCATION:?}"
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
                    let remote_participant = participant.1;
                    let sid = remote_participant.sid().as_str().to_string();
                    let identity = remote_participant.identity().as_str().to_string();
                    let name = remote_participant.name();

                    log::info!(
                        "room_service_commands: Participant: {} {:?}",
                        sid,
                        remote_participant
                    );

                    if insert_participant_if_absent(&inner.participants, &sid, &remote_participant)
                    {
                        log::info!("room_service_commands: participant added: {}", sid);
                    }

                    if let Err(e) = event_loop_proxy.send_event(UserEvent::ParticipantConnected(
                        ParticipantData {
                            name,
                            identity,
                            sid,
                        },
                    )) {
                        log::error!(
                            "room_service_commands: Failed to send participant connected event: {e:?}"
                        );
                    }
                }

                let snapshot = build_participants_snapshot(&inner.participants);
                if let Err(e) =
                    event_loop_proxy.send_event(UserEvent::ParticipantsSnapshot(snapshot))
                {
                    log::error!(
                        "room_service_commands: Failed to send participants snapshot: {e:?}"
                    );
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
            RoomServiceCommand::PublishDrawStart(point) => {
                let room = inner.room.lock().await;
                if room.is_none() {
                    log::warn!("room_service_commands: Room doesn't exist");
                    continue;
                }
                let room = room.as_ref().unwrap();
                let local_participant = room.local_participant();
                let event = ClientEvent::DrawStart(point);
                let payload = serde_json::to_vec(&event).unwrap();
                let res = local_participant
                    .publish_data(DataPacket {
                        payload,
                        reliable: true,
                        topic: Some(TOPIC_DRAW.to_string()),
                        ..Default::default()
                    })
                    .await;

                if let Err(e) = res {
                    log::error!("room_service_commands: Failed to publish draw start: {e:?}");
                }
            }
            RoomServiceCommand::PublishDrawAddPoint(point) => {
                let room = inner.room.lock().await;
                if room.is_none() {
                    log::warn!("room_service_commands: Room doesn't exist");
                    continue;
                }
                let room = room.as_ref().unwrap();
                let local_participant = room.local_participant();
                let event = ClientEvent::DrawAddPoint(point);
                let payload = serde_json::to_vec(&event).unwrap();
                let res = local_participant
                    .publish_data(DataPacket {
                        payload,
                        reliable: false,
                        topic: Some(TOPIC_DRAW.to_string()),
                        ..Default::default()
                    })
                    .await;

                if let Err(e) = res {
                    log::error!("room_service_commands: Failed to publish draw add point: {e:?}");
                }
            }
            RoomServiceCommand::PublishDrawEnd(point) => {
                let room = inner.room.lock().await;
                if room.is_none() {
                    log::warn!("room_service_commands: Room doesn't exist");
                    continue;
                }
                let room = room.as_ref().unwrap();
                let local_participant = room.local_participant();
                let event = ClientEvent::DrawEnd(point);
                let payload = serde_json::to_vec(&event).unwrap();
                let res = local_participant
                    .publish_data(DataPacket {
                        payload,
                        reliable: true,
                        topic: Some(TOPIC_DRAW.to_string()),
                        ..Default::default()
                    })
                    .await;

                if let Err(e) = res {
                    log::error!("room_service_commands: Failed to publish draw end: {e:?}");
                }
            }
            RoomServiceCommand::PublishDrawClearPaths(path_ids) => {
                let room = inner.room.lock().await;
                if room.is_none() {
                    log::warn!("room_service_commands: Room doesn't exist");
                    continue;
                }
                let room = room.as_ref().unwrap();
                let local_participant = room.local_participant();

                // Send individual DrawClearPath events for each path ID
                for path_id in path_ids {
                    let event = ClientEvent::DrawClearPath { path_id };
                    let payload = serde_json::to_vec(&event).unwrap();
                    let res = local_participant
                        .publish_data(DataPacket {
                            payload,
                            reliable: true,
                            topic: Some(TOPIC_DRAW.to_string()),
                            ..Default::default()
                        })
                        .await;

                    if let Err(e) = res {
                        log::error!(
                            "room_service_commands: Failed to publish draw clear path {}: {e:?}",
                            path_id
                        );
                    }
                }
            }
            RoomServiceCommand::PublishDrawClearAllPaths => {
                let room = inner.room.lock().await;
                if room.is_none() {
                    log::warn!("room_service_commands: Room doesn't exist");
                    continue;
                }
                let room = room.as_ref().unwrap();
                let local_participant = room.local_participant();
                let event = ClientEvent::DrawClearAllPaths;
                let payload = serde_json::to_vec(&event).unwrap();
                let res = local_participant
                    .publish_data(DataPacket {
                        payload,
                        reliable: true,
                        topic: Some(TOPIC_DRAW.to_string()),
                        ..Default::default()
                    })
                    .await;

                if let Err(e) = res {
                    log::error!(
                        "room_service_commands: Failed to publish draw clear all paths: {e:?}"
                    );
                }
            }
            RoomServiceCommand::PublishDrawingMode(mode) => {
                let room = inner.room.lock().await;
                if room.is_none() {
                    log::warn!("room_service_commands: Room doesn't exist");
                    continue;
                }
                let room = room.as_ref().unwrap();
                let local_participant = room.local_participant();
                let event = ClientEvent::DrawingMode(mode);
                let payload = serde_json::to_vec(&event).unwrap();
                let res = local_participant
                    .publish_data(DataPacket {
                        payload,
                        reliable: true,
                        topic: Some(TOPIC_DRAW.to_string()),
                        ..Default::default()
                    })
                    .await;

                if let Err(e) = res {
                    log::error!("room_service_commands: Failed to publish drawing mode: {e:?}");
                }
            }
            RoomServiceCommand::PublishAudioTrack {
                sample_rate,
                sample_rx,
            } => {
                let inner_room = inner.room.lock().await;
                if inner_room.is_none() {
                    log::error!("room_service_commands: Room doesn't exist for PublishAudioTrack");
                    let _ = tx.send(RoomServiceCommandResult::Failure);
                    continue;
                }
                let room = inner_room.as_ref().unwrap();

                match AudioPublisher::publish(room, sample_rate, sample_rx, audio_processor.clone())
                    .await
                {
                    Ok(publisher) => {
                        audio_publisher = Some(publisher);
                        log::info!("room_service_commands: Audio track published");
                        let _ = tx.send(RoomServiceCommandResult::Success);
                    }
                    Err(e) => {
                        log::error!("room_service_commands: Failed to publish audio track: {e}");
                        let _ = tx.send(RoomServiceCommandResult::Failure);
                    }
                }
            }
            RoomServiceCommand::UnpublishAudioTrack => {
                if let Some(publisher) = audio_publisher.take() {
                    let inner_room = inner.room.lock().await;
                    if let Some(room) = inner_room.as_ref() {
                        publisher.unpublish(room).await;
                    }
                }
                log::info!("room_service_commands: Audio track unpublished");
            }
            RoomServiceCommand::MuteAudioTrack => {
                if let Some(publisher) = audio_publisher.as_ref() {
                    publisher.mute();
                    if let Ok(mut participants) = inner.participants.write() {
                        if let Some(local) = participants.get_mut("local") {
                            local.set_muted(true);
                        }
                    }

                    let snapshot = build_participants_snapshot(&inner.participants);
                    if let Err(e) =
                        event_loop_proxy.send_event(UserEvent::ParticipantsSnapshot(snapshot))
                    {
                        log::error!(
                            "room_service_commands: Failed to send participants snapshot: {e:?}"
                        );
                    }

                    log::info!("room_service_commands: Audio track muted");
                } else {
                    log::warn!("room_service_commands: No audio track to mute");
                }
            }
            RoomServiceCommand::UnmuteAudioTrack => {
                if let Some(publisher) = audio_publisher.as_ref() {
                    publisher.unmute();
                    if let Ok(mut participants) = inner.participants.write() {
                        if let Some(local) = participants.get_mut("local") {
                            local.set_muted(false);
                        }
                    }

                    let snapshot = build_participants_snapshot(&inner.participants);
                    if let Err(e) =
                        event_loop_proxy.send_event(UserEvent::ParticipantsSnapshot(snapshot))
                    {
                        log::error!(
                            "room_service_commands: Failed to send participants snapshot: {e:?}"
                        );
                    }

                    log::info!("room_service_commands: Audio track unmuted");
                } else {
                    log::warn!("room_service_commands: No audio track to unmute");
                }
            }
            RoomServiceCommand::PublishCameraTrack { width, height } => {
                let inner_room = inner.room.lock().await;
                if inner_room.is_none() {
                    log::error!("room_service_commands: Room doesn't exist for PublishCameraTrack");
                    let _ = tx.send(RoomServiceCommandResult::Failure);
                    continue;
                }
                let room = inner_room.as_ref().unwrap();

                let buffer_source =
                    NativeVideoSource::new(VideoResolution { width, height }, false);
                let track = LocalVideoTrack::create_video_track(
                    CAMERA_TRACK_NAME,
                    RtcVideoSource::Native(buffer_source.clone()),
                );

                let res = room
                    .local_participant()
                    .publish_track(
                        LocalTrack::Video(track),
                        TrackPublishOptions {
                            source: TrackSource::Camera,
                            video_codec: VideoCodec::VP8,
                            simulcast: true,
                            video_encoding: Some(VideoEncoding {
                                max_bitrate: CAMERA_MAX_BITRATE,
                                max_framerate: CAMERA_MAX_FRAMERATE,
                            }),
                            ..Default::default()
                        },
                    )
                    .await;

                if let Err(e) = res {
                    log::error!("room_service_commands: Failed to publish camera track: {e:?}");
                    let _ = tx.send(RoomServiceCommandResult::Failure);
                    continue;
                }

                let mut inner_buffer_source = inner.camera_buffer_source.lock().unwrap();
                *inner_buffer_source = Some(buffer_source);

                // Mark local camera as active before building snapshot
                // to avoid the race where the capture thread hasn't written
                // its first frame yet.
                {
                    let mut participants = inner.participants.write().unwrap();
                    let info = participants
                        .get_mut("local")
                        .expect("local participant info not found");
                    info.camera_buffers().set_inactive(false);
                }

                log::info!("room_service_commands: Camera track published");
                let _ = tx.send(RoomServiceCommandResult::Success);
            }
            RoomServiceCommand::UnpublishCameraTrack => {
                let inner_room = inner.room.lock().await;
                if let Some(room) = inner_room.as_ref() {
                    // Find and unpublish camera track
                    let local_participant = room.local_participant();
                    for (sid, publication) in local_participant.track_publications() {
                        if publication.name() == CAMERA_TRACK_NAME {
                            log::info!("room_service_commands: Unpublishing camera track: {}", sid);
                            let res = local_participant.unpublish_track(&sid).await;
                            if let Err(e) = res {
                                log::error!(
                                    "room_service_commands: Failed to unpublish camera track: {e:?}"
                                );
                            }
                            break;
                        }
                    }
                }

                let mut inner_buffer_source = inner.camera_buffer_source.lock().unwrap();
                *inner_buffer_source = None;

                // Mark local camera as inactive before building snapshot
                // to avoid sending active camera when we disable it.
                {
                    let mut participants = inner.participants.write().unwrap();
                    if let Some(info) = participants.get_mut("local") {
                        info.camera_buffers().set_inactive(true);
                    }
                }

                log::info!("room_service_commands: Camera track unpublished");
            }
            RoomServiceCommand::UnpublishScreenShareTrack => {
                let inner_video_room = inner.video_room.lock().await;
                if let Some(room) = inner_video_room.as_ref() {
                    // Find and unpublish screen share track
                    let local_participant = room.local_participant();
                    for (sid, publication) in local_participant.track_publications() {
                        if publication.name() == VIDEO_TRACK_NAME {
                            log::info!(
                                "room_service_commands: Unpublishing screen share track: {}",
                                sid
                            );
                            let res = local_participant.unpublish_track(&sid).await;
                            if let Err(e) = res {
                                log::error!(
                                    "room_service_commands: Failed to unpublish screen share track: {e:?}"
                                );
                            }
                            break;
                        }
                    }
                }

                let mut inner_buffer_source = inner.buffer_source.lock().unwrap();
                *inner_buffer_source = None;

                log::info!("room_service_commands: Screen share track unpublished");
            }
            RoomServiceCommand::PublishMouseClick(data) => {
                let inner_room = inner.room.lock().await;
                if inner_room.is_none() {
                    log::warn!("room_service_commands: Room doesn't exist for PublishMouseClick");
                    continue;
                }
                let room = inner_room.as_ref().unwrap();
                let local_participant = room.local_participant();

                let event = ClientEvent::MouseClick(data);
                let payload = serde_json::to_vec(&event).unwrap();
                let res = local_participant
                    .publish_data(DataPacket {
                        payload,
                        reliable: true,
                        topic: None,
                        ..Default::default()
                    })
                    .await;

                if let Err(e) = res {
                    log::error!("room_service_commands: Failed to publish mouse click: {e:?}");
                }
            }
            RoomServiceCommand::PublishMouseVisible(data) => {
                let inner_room = inner.room.lock().await;
                if inner_room.is_none() {
                    log::warn!("room_service_commands: Room doesn't exist for PublishMouseVisible");
                    continue;
                }
                let room = inner_room.as_ref().unwrap();
                let local_participant = room.local_participant();

                let event = ClientEvent::MouseVisible(data);
                let payload = serde_json::to_vec(&event).unwrap();
                let res = local_participant
                    .publish_data(DataPacket {
                        payload,
                        reliable: true,
                        topic: None,
                        ..Default::default()
                    })
                    .await;

                if let Err(e) = res {
                    log::error!("room_service_commands: Failed to publish mouse visible: {e:?}");
                }
            }
            RoomServiceCommand::PublishKeystroke(data) => {
                let inner_room = inner.room.lock().await;
                if inner_room.is_none() {
                    log::warn!("room_service_commands: Room doesn't exist for PublishKeystroke");
                    continue;
                }
                let room = inner_room.as_ref().unwrap();
                let local_participant = room.local_participant();

                let event = ClientEvent::Keystroke(data);
                let payload = serde_json::to_vec(&event).unwrap();
                let res = local_participant
                    .publish_data(DataPacket {
                        payload,
                        reliable: true,
                        topic: None,
                        ..Default::default()
                    })
                    .await;

                if let Err(e) = res {
                    log::error!("room_service_commands: Failed to publish keystroke: {e:?}");
                }
            }
            RoomServiceCommand::PublishWheelEvent(data) => {
                let inner_room = inner.room.lock().await;
                if inner_room.is_none() {
                    log::warn!("room_service_commands: Room doesn't exist for PublishWheelEvent");
                    continue;
                }
                let room = inner_room.as_ref().unwrap();
                let local_participant = room.local_participant();

                let event = ClientEvent::WheelEvent(data);
                let payload = serde_json::to_vec(&event).unwrap();
                let res = local_participant
                    .publish_data(DataPacket {
                        payload,
                        reliable: true,
                        topic: None,
                        ..Default::default()
                    })
                    .await;

                if let Err(e) = res {
                    log::error!("room_service_commands: Failed to publish wheel event: {e:?}");
                }
            }
            RoomServiceCommand::PublishAddToClipboard(data) => {
                let inner_room = inner.room.lock().await;
                if inner_room.is_none() {
                    log::warn!(
                        "room_service_commands: Room doesn't exist for PublishAddToClipboard"
                    );
                    continue;
                }
                let room = inner_room.as_ref().unwrap();
                let local_participant = room.local_participant();

                let event = ClientEvent::AddToClipboard(data);
                let payload = serde_json::to_vec(&event).unwrap();
                let res = local_participant
                    .publish_data(DataPacket {
                        payload,
                        reliable: true,
                        topic: None,
                        ..Default::default()
                    })
                    .await;

                if let Err(e) = res {
                    log::error!("room_service_commands: Failed to publish add to clipboard: {e:?}");
                }
            }
            RoomServiceCommand::PublishPasteFromClipboard(data) => {
                let inner_room = inner.room.lock().await;
                if inner_room.is_none() {
                    log::warn!(
                        "room_service_commands: Room doesn't exist for PublishPasteFromClipboard"
                    );
                    continue;
                }
                let room = inner_room.as_ref().unwrap();
                let local_participant = room.local_participant();

                let event = ClientEvent::PasteFromClipboard(data);
                let payload = serde_json::to_vec(&event).unwrap();
                let res = local_participant
                    .publish_data(DataPacket {
                        payload,
                        reliable: true,
                        topic: None,
                        ..Default::default()
                    })
                    .await;

                if let Err(e) = res {
                    log::error!(
                        "room_service_commands: Failed to publish paste from clipboard: {e:?}"
                    );
                }
            }
            RoomServiceCommand::PublishClickAnimation(point) => {
                let inner_room = inner.room.lock().await;
                if inner_room.is_none() {
                    log::warn!(
                        "room_service_commands: Room doesn't exist for PublishClickAnimation"
                    );
                    continue;
                }
                let room = inner_room.as_ref().unwrap();
                let local_participant = room.local_participant();

                let event = ClientEvent::ClickAnimation(point);
                let payload = serde_json::to_vec(&event).unwrap();
                let res = local_participant
                    .publish_data(DataPacket {
                        payload,
                        reliable: true,
                        topic: Some(TOPIC_DRAW.to_string()),
                        ..Default::default()
                    })
                    .await;

                if let Err(e) = res {
                    log::error!("room_service_commands: Failed to publish click animation: {e:?}");
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

/// Represents a drawing path point with both coordinates and path identifier.
///
/// This structure combines a 2D point with a path ID to track which drawing
/// path the point belongs to, enabling multiple simultaneous drawing paths.
#[derive(Debug, Serialize, Deserialize, Clone, Copy)]
pub struct DrawPathPoint {
    /// The 2D coordinates of the point
    pub point: ClientPoint,
    /// The unique identifier for the drawing path this point belongs to
    pub path_id: u64,
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

/// Drawing mode - specifies the type of drawing operation or disabled state.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(tag = "type", content = "settings")]
pub enum DrawingMode {
    /// Drawing mode is disabled
    Disabled,
    /// Standard drawing mode with its settings
    Draw(DrawSettings),
    /// Click animation mode
    ClickAnimation,
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
    /// Drawing mode change event (disabled, draw, or click animation)
    DrawingMode(DrawingMode),
    /// Drawing started at a point with a path identifier
    DrawStart(DrawPathPoint),
    /// Add a point to the current in-progress drawing
    DrawAddPoint(ClientPoint),
    /// Drawing ended at a point
    DrawEnd(ClientPoint),
    /// Clear a specific drawing path
    DrawClearPath { path_id: u64 },
    /// Clear all drawing paths
    DrawClearAllPaths,
    /// Click animation at a point
    ClickAnimation(ClientPoint),
}

async fn handle_room_events(
    mut receiver: mpsc::UnboundedReceiver<RoomEvent>,
    event_loop_proxy: EventLoopProxy<UserEvent>,
    user_sid: String,
    video_participant_sid: String,
    participants: Arc<std::sync::RwLock<HashMap<String, ParticipantInfo>>>,
    mixer: audio::mixer::MixerHandle,
    remote_screen_share: RemoteScreenShare,
    connection_quality: Arc<std::sync::Mutex<Option<ConnectionQuality>>>,
) {
    while let Some(msg) = receiver.recv().await {
        match msg {
            RoomEvent::DataReceived {
                payload,
                topic,
                kind: _,
                participant,
            } => {
                // participant_in_control uses raw UTF-8 SID, not JSON. Handle before deserialize.
                // TODO(@konsalex): Maybe follow a JSON  type
                // type, payload approach to be easier to work with?
                if topic.as_deref() == Some(TOPIC_PARTICIPANT_IN_CONTROL) {
                    if let Ok(sid_str) = std::str::from_utf8(&payload) {
                        let in_control = sid_str == user_sid;
                        if let Err(e) = event_loop_proxy
                            .send_event(UserEvent::LocalParticipantInControl(in_control))
                        {
                            log::error!("handle_room_events: Failed to send LocalParticipantInControl: {e:?}");
                        }
                    } else {
                        log::warn!(
                            "handle_room_events: participant_in_control payload is not valid UTF-8"
                        );
                    }
                    continue;
                }

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
                    ClientEvent::DrawingMode(drawing_mode) => {
                        event_loop_proxy.send_event(UserEvent::DrawingMode(drawing_mode, sid))
                    }
                    ClientEvent::DrawStart(draw_path_point) => event_loop_proxy.send_event(
                        UserEvent::DrawStart(draw_path_point.point, draw_path_point.path_id, sid),
                    ),
                    ClientEvent::DrawAddPoint(point) => {
                        event_loop_proxy.send_event(UserEvent::DrawAddPoint(point, sid))
                    }
                    ClientEvent::DrawEnd(point) => {
                        event_loop_proxy.send_event(UserEvent::DrawEnd(point, sid))
                    }
                    ClientEvent::DrawClearPath { path_id } => {
                        event_loop_proxy.send_event(UserEvent::DrawClearPath(path_id, sid))
                    }
                    ClientEvent::DrawClearAllPaths => {
                        event_loop_proxy.send_event(UserEvent::DrawClearAllPaths(sid))
                    }
                    ClientEvent::ClickAnimation(point) => event_loop_proxy
                        .send_event(UserEvent::ClickAnimationFromParticipant(point, sid)),
                    ClientEvent::RemoteControlEnabled(data) => {
                        event_loop_proxy.send_event(UserEvent::SharerControlEnabled(data.enabled))
                    }
                    _ => Ok(()),
                };
                if let Err(e) = res {
                    log::error!("handle_room_events: Failed to send message: {e:?}");
                }
            }
            RoomEvent::ParticipantConnected(participant) => {
                let sid = participant.sid().as_str().to_string();
                let identity = participant.identity().as_str().to_string();
                let name = participant.name();

                log::info!("handle_room_events: Participant connected: {}", sid);

                if !insert_participant_if_absent(&participants, &sid, &participant) {
                    continue;
                }

                if let Err(e) =
                    event_loop_proxy.send_event(UserEvent::ParticipantConnected(ParticipantData {
                        name,
                        identity: identity.clone(),
                        sid,
                    }))
                {
                    log::error!(
                        "handle_room_events: Failed to send participant connected event: {e:?}"
                    );
                }

                let snapshot = build_participants_snapshot(&participants);
                if let Err(e) =
                    event_loop_proxy.send_event(UserEvent::ParticipantsSnapshot(snapshot))
                {
                    log::error!("handle_room_events: Failed to send participants snapshot: {e:?}");
                }
            }
            RoomEvent::ParticipantDisconnected(participant) => {
                let sid = participant.sid().as_str().to_string();
                let identity = participant.identity().as_str().to_string();
                let name = participant.name();

                log::info!("handle_room_events: Participant disconnected: {}", sid);

                // Stop streams and remove from HashMap
                let any_camera_active_after = {
                    let mut participants_guard = participants.write().unwrap();
                    if let Some(info) = participants_guard.get_mut(&sid) {
                        info.stop_audio_stream();
                        info.stop_camera_stream();
                    }
                    participants_guard.remove(&sid);
                    participants_guard.values().any(|info| info.camera_active())
                };

                if !any_camera_active_after {
                    if let Err(e) = event_loop_proxy.send_event(UserEvent::CloseCameraWindow) {
                        log::error!(
                            "handle_room_events: Failed to send CloseCameraWindow event: {e:?}"
                        );
                    }
                }

                if let Err(e) = event_loop_proxy.send_event(UserEvent::ParticipantDisconnected(
                    ParticipantData {
                        name,
                        identity,
                        sid,
                    },
                )) {
                    log::error!(
                        "handle_room_events: Failed to send participant disconnected event: {e:?}"
                    );
                }

                let snapshot = build_participants_snapshot(&participants);
                if let Err(e) =
                    event_loop_proxy.send_event(UserEvent::ParticipantsSnapshot(snapshot))
                {
                    log::error!("handle_room_events: Failed to send participants snapshot: {e:?}");
                }
            }
            RoomEvent::TrackPublished {
                publication,
                participant,
            } => {
                log::info!(
                    "handle_room_events: Track published: {} ({:?}) from {}",
                    publication.name(),
                    publication.source(),
                    participant.sid()
                );
            }
            RoomEvent::ActiveSpeakersChanged { speakers } => {
                log::trace!("handle_room_events: Active speakers changed");
                let mut participants_guard = participants.write().unwrap();

                // First, set all participants to not speaking
                for info in participants_guard.values_mut() {
                    info.set_is_speaking(false);
                }

                // Then set active speakers to speaking
                for speaker in speakers {
                    let sid = speaker.sid().as_str().to_string();
                    if let Some(info) = participants_guard.get_mut(&sid) {
                        info.set_is_speaking(true);
                    }
                }
            }
            RoomEvent::TrackMuted {
                participant,
                publication,
            } => {
                if publication.kind() == livekit::track::TrackKind::Audio {
                    let sid = participant.sid().as_str().to_string();
                    log::info!("handle_room_events: Audio track muted for {}", sid);

                    {
                        let mut participants_guard = participants.write().unwrap();
                        if let Some(info) = participants_guard.get_mut(&sid) {
                            info.set_muted(true);
                        }
                    }

                    let snapshot = build_participants_snapshot(&participants);
                    if let Err(e) =
                        event_loop_proxy.send_event(UserEvent::ParticipantsSnapshot(snapshot))
                    {
                        log::error!(
                            "handle_room_events: Failed to send participants snapshot: {e:?}"
                        );
                    }
                }
            }
            RoomEvent::TrackUnmuted {
                participant,
                publication,
            } => {
                if publication.kind() == livekit::track::TrackKind::Audio {
                    let sid = participant.sid().as_str().to_string();
                    log::info!("handle_room_events: Audio track unmuted for {}", sid);

                    {
                        let mut participants_guard = participants.write().unwrap();
                        if let Some(info) = participants_guard.get_mut(&sid) {
                            info.set_muted(false);
                        }
                    }

                    let snapshot = build_participants_snapshot(&participants);
                    if let Err(e) =
                        event_loop_proxy.send_event(UserEvent::ParticipantsSnapshot(snapshot))
                    {
                        log::error!(
                            "handle_room_events: Failed to send participants snapshot: {e:?}"
                        );
                    }
                }
            }
            RoomEvent::TrackSubscribed {
                track,
                publication,
                participant,
            } => {
                log::info!(
                    "handle_room_events: Track subscribed from {}: {} ({:?}) {:?} {:?}",
                    participant.identity(),
                    track.name(),
                    track.kind(),
                    track,
                    publication,
                );

                let participant_sid = participant.sid().as_str().to_string();

                if participant_sid == video_participant_sid {
                    log::debug!("handle_room_events: Skipping track subscribed event from video participant");
                    continue;
                }

                if insert_participant_if_absent(&participants, &participant_sid, &participant) {
                    log::info!(
                        "handle_room_events: Creating participant {} from track subscription",
                        participant_sid
                    );
                }

                match track {
                    livekit::track::RemoteTrack::Audio(audio_track) => {
                        let participant_identity = participant.identity().to_string();
                        log::info!(
                            "handle_room_events: Setting up audio stream for participant: {}",
                            participant_identity
                        );

                        let handle = crate::livekit::audio::play_remote_audio_track(
                            audio_track,
                            mixer.clone(),
                            &participant_identity,
                        );

                        let mut participants_guard = participants.write().unwrap();
                        if let Some(info) = participants_guard.get_mut(&participant_sid) {
                            info.set_audio_handle(handle);
                        }
                    }
                    livekit::track::RemoteTrack::Video(video_track) => match publication.source() {
                        TrackSource::Screenshare => {
                            log::info!(
                                "handle_room_events: Setting up screen share stream: {} from {}",
                                video_track.name(),
                                participant_sid,
                            );

                            {
                                let mut stop_tx_guard = remote_screen_share.stop_tx.lock().unwrap();
                                if let Some(tx) = stop_tx_guard.take() {
                                    log::info!(
                                        "handle_room_events: Stopping existing screen share task"
                                    );
                                    let _ = tx.send(());
                                }
                            }

                            let manager = {
                                let mut buffer_guard = remote_screen_share.buffer.lock().unwrap();
                                if let Some(existing) = buffer_guard.as_ref() {
                                    existing.clone()
                                } else {
                                    let new = Arc::new(VideoBufferManager::new());
                                    *buffer_guard = Some(new.clone());
                                    new
                                }
                            };

                            *remote_screen_share.publisher_sid.lock().unwrap() =
                                Some(participant_sid.clone());

                            let (stop_tx, stop_rx) = mpsc::unbounded_channel();
                            *remote_screen_share.stop_tx.lock().unwrap() = Some(stop_tx);

                            tokio::spawn(process_video_stream(
                                video_track,
                                manager,
                                stop_rx,
                                format!("screenshare_{}", participant_sid),
                                false,
                                Some(event_loop_proxy.clone()),
                            ));

                            // Derive the audio participant identity from the video participant identity
                            // e.g. "room:...:video" → "room:...:audio"
                            let sharer_name = participant.name().to_string();
                            let sharer_sid = {
                                let video_identity = participant.identity().as_str().to_string();
                                let audio_identity = video_identity
                                    .strip_suffix(":video")
                                    .map(|prefix| format!("{prefix}:audio"));
                                if let Some(audio_id) = audio_identity {
                                    let guard = participants.read().unwrap();
                                    crate::livekit::participant::find_sid_by_identity(
                                        &guard, &audio_id,
                                    )
                                    .unwrap_or_else(|| participant_sid.clone())
                                } else {
                                    participant_sid.clone()
                                }
                            };
                            if let Err(e) =
                                event_loop_proxy.send_event(UserEvent::OpenScreenShareWindow {
                                    sid: Some(sharer_sid),
                                    name: Some(sharer_name),
                                })
                            {
                                log::error!(
                                        "handle_room_events: Failed to send OpenScreenShareWindow event: {e:?}"
                                    );
                            }
                        }
                        TrackSource::Camera => {
                            log::info!(
                                "handle_room_events: Setting up camera stream for participant: {}",
                                participant_sid
                            );

                            if let Err(e) = event_loop_proxy.send_event(UserEvent::OpenCamera) {
                                log::error!(
                                    "handle_room_events: Failed to send OpenCamera event: {e:?}"
                                );
                            }

                            let (stop_tx, stop_rx) = mpsc::unbounded_channel();

                            let manager = {
                                let mut participants_guard = participants.write().unwrap();
                                let info = participants_guard
                                    .get_mut(&participant_sid)
                                    .expect("Participant should exist");
                                info.set_camera_stop_tx(stop_tx);
                                info.camera_buffers()
                            };

                            tokio::spawn(process_video_stream(
                                video_track,
                                manager,
                                stop_rx,
                                participant_sid.clone(),
                                true,
                                None,
                            ));
                        }
                        source => {
                            log::info!(
                                "handle_room_events: Ignoring non-camera video track: {} ({:?})",
                                video_track.name(),
                                source
                            );
                        }
                    },
                }
            }
            RoomEvent::TrackUnsubscribed {
                track,
                publication,
                participant,
            } => {
                log::info!(
                    "handle_room_events: Track unsubscribed from {}: {} ({:?})",
                    participant.identity(),
                    track.name(),
                    track.kind()
                );

                let participant_sid = participant.sid().as_str().to_string();

                if participant_sid == video_participant_sid {
                    log::debug!("handle_room_events: Skipping track unsubscribed event from video participant");
                    continue;
                }

                match track {
                    livekit::track::RemoteTrack::Video(_) => {
                        if publication.source() == TrackSource::Camera {
                            log::info!(
                                "handle_room_events: Stopping camera stream for participant: {}",
                                participant_sid
                            );

                            let any_camera_active = {
                                let mut participants_guard = participants.write().unwrap();
                                if let Some(info) = participants_guard.get_mut(&participant_sid) {
                                    info.stop_camera_stream();
                                }
                                participants_guard.values().any(|info| info.camera_active())
                            };
                            if !any_camera_active {
                                if let Err(e) =
                                    event_loop_proxy.send_event(UserEvent::CloseCameraWindow)
                                {
                                    log::error!(
                                        "handle_room_events: Failed to send CloseCameraWindow event: {e:?}"
                                    );
                                }
                            }
                        } else if publication.source() == TrackSource::Screenshare {
                            let unsub_sid = participant.sid().as_str().to_string();
                            log::info!(
                                "handle_room_events: Screen share unsubscribed from {}",
                                unsub_sid,
                            );

                            // Only clean up if this is the current publisher
                            let is_current = {
                                let sid_guard = remote_screen_share.publisher_sid.lock().unwrap();
                                sid_guard.as_deref() == Some(unsub_sid.as_str())
                            };

                            if !is_current {
                                log::info!(
                                    "handle_room_events: Ignoring unsub from non-current publisher {}",
                                    unsub_sid,
                                );
                                continue;
                            }

                            // Send stop signal
                            {
                                let mut stop_tx_guard = remote_screen_share.stop_tx.lock().unwrap();
                                if let Some(tx) = stop_tx_guard.take() {
                                    let _ = tx.send(());
                                }
                            }

                            // Clear publisher SID (keep the buffer for reuse)
                            {
                                remote_screen_share.publisher_sid.lock().unwrap().take();
                            }

                            // Close the screen share window
                            if let Err(e) =
                                event_loop_proxy.send_event(UserEvent::CloseScreenShareWindow)
                            {
                                log::error!("handle_room_events: Failed to send CloseScreenShareWindow event: {e:?}");
                            }
                        }
                    }
                    livekit::track::RemoteTrack::Audio(_) => {
                        log::info!(
                            "handle_room_events: Stopping audio stream for participant: {}",
                            participant_sid
                        );

                        let mut participants_guard = participants.write().unwrap();
                        if let Some(info) = participants_guard.get_mut(&participant_sid) {
                            info.stop_audio_stream();
                        }
                    }
                }
            }
            RoomEvent::TrackUnpublished {
                publication,
                participant,
            } => {
                log::info!(
                    "handle_room_events: Track unpublished from {}: {} ({:?})",
                    participant.identity(),
                    publication.name(),
                    publication.kind()
                );

                // if publication.kind() == livekit::track::TrackKind::Video
                //     && publication.name() == CAMERA_TRACK_NAME
                // {
                //     let participant_sid = participant.sid().as_str().to_string();
                //     log::info!(
                //         "handle_room_events: Clearing camera buffers for participant: {}",
                //         participant_sid
                //     );

                //     let mut participants_guard = participants.lock().unwrap();
                //     if let Some(info) = participants_guard.get_mut(&participant_sid) {
                //         info.clear_camera_buffers();
                //     }
                // }
            }
            RoomEvent::ConnectionQualityChanged {
                quality,
                participant,
            } => {
                if participant.sid().as_str() == user_sid {
                    log::info!("Connection quality changed: {:?}", quality);
                    *connection_quality.lock().unwrap() = Some(quality);
                }
            }
            _ => {
                log::info!("message: {:?}", msg);
            }
        }
    }
    log::info!("handle_room_events: ended")
}
