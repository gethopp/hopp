pub mod audio {
    pub mod capturer;
    #[cfg(target_os = "macos")]
    pub mod device_monitor;
    pub mod mixer;
    pub mod player;
}

pub mod livekit {
    pub mod audio;
    pub mod participant;
    pub mod stats;
    pub mod video;
}

pub mod room_service;

pub mod input {
    pub mod clipboard;
    pub mod keyboard;
    pub mod mouse;
}

pub mod camera {
    pub mod capturer;
    pub mod stream;
}

pub mod capture {
    pub mod capturer;
}

pub mod graphics {
    pub mod graphics_context;
    pub mod yuv_buffer;
    pub mod yuv_renderer;

    #[cfg(target_os = "windows")]
    pub mod direct_composition;
}

pub mod utils {
    pub mod clock;
    pub mod geometry;
    pub mod svg_renderer;
}

pub(crate) mod window {
    pub(crate) mod camera_window;
    pub(crate) mod screensharing_window;
    pub(crate) mod stats_window;
    pub(crate) mod vibrancy;
}
pub(crate) mod components;
pub(crate) mod overlay_window;
pub(crate) mod window_manager;
pub(crate) mod windows;

use camera::capturer::{poll_camera_stream, CameraCapturer};
use capture::capturer::{poll_stream, Capturer};
use graphics::graphics_context::GraphicsContext;
use image::GenericImageView;
use input::clipboard::ClipboardController;
use input::keyboard::{KeyboardController, KeyboardLayout};
use input::mouse::CursorController;
use log::{debug, error};
use overlay_window::OverlayWindow;
use room_service::RoomService;
use socket_lib::{
    AvailableContentMessage, CallStartMessage, CameraStartMessage, CaptureContent, Message,
    ScreenShareMessage, SentryMetadata, SocketSender,
};
use std::fmt;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use thiserror::Error;
use utils::geometry::{Extent, Frame};
use window::camera_window::CameraWindow;
use window::screensharing_window::ScreensharingWindow;
use window::stats_window::StatsWindow;
use winit::application::ApplicationHandler;
use winit::error::EventLoopError;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop, EventLoopProxy};
use winit::monitor::MonitorHandle;

use window::screensharing_window::ScreenShareInputEvent;

#[cfg(target_os = "macos")]
use winit::platform::macos::EventLoopBuilderExtMacOS;

use crate::overlay_window::DisplayInfo;
use crate::room_service::DrawingMode;
use crate::utils::geometry::Position;

/// Process exit code for errors
const PROCESS_EXIT_CODE_ERROR: i32 = 1;
#[cfg(debug_assertions)]
const SOCKET_MESSAGE_TIMEOUT_SECONDS: u64 = 300;
#[cfg(not(debug_assertions))]
const SOCKET_MESSAGE_TIMEOUT_SECONDS: u64 = 30;
const STREAM_FAILURE_EXIT_CODE: i32 = 2;

#[derive(Error, Debug)]
pub enum ServerError {
    #[error("Livekit room service not found")]
    RoomServiceNotFound,
    #[error("Failed to create Livekit room")]
    RoomCreationError,
    #[error("Failed to publish track")]
    PublishTrackError,
    #[error("Failed to set overlay window fullscreen")]
    FullscreenError,
    #[error("Failed to create stream for screen share")]
    StreamCreationError,
    #[error("Failed to get stream extent for screen share")]
    StreamExtentError,
    #[error("Failed to create overlay window")]
    WindowCreationError,
    #[error("Failed to set cursor hittest for overlay window")]
    CursorHittestError,
    #[error("Failed to create graphics context")]
    GfxCreationError,
    #[error("Failed to create cursor controller, accessibility permissions needed")]
    CursorControllerCreationError,
}

/// Encapsulates the active remote control session components.
///
/// This struct manages all the components needed for an active remote control session,
/// including graphics rendering, input simulation, and window management. It's created
/// when a screen sharing session begins and destroyed when it ends.
///
/// # Fields
///
/// * `gfx` - Graphics context for rendering cursors and visual feedback
/// * `cursor_controller` - Handles mouse movement, clicks, and cursor visualization
/// * `keyboard_controller` - Manages keyboard input simulation
///
/// # Lifetime
///
/// The lifetime parameter `'a` ensures that the graphics context and cursor controller
/// don't outlive the underlying window resources they depend on.
struct RemoteControl<'a> {
    gfx: GraphicsContext<'a>,
    cursor_controller: CursorController,
    keyboard_controller: KeyboardController<KeyboardLayout>,
    clipboard_controller: Option<ClipboardController>,
    pencil_cursor: winit::window::CustomCursor,
}

impl<'a> RemoteControl<'a> {
    /// Renders a complete frame by updating cursors, hiding inactive ones, clearing expired paths, and drawing.
    ///
    /// # Returns
    /// Vector of cleared path IDs from auto-clear
    pub fn render_frame(&mut self) -> Vec<u64> {
        self.cursor_controller
            .update_cursors(self.gfx.participants_manager_mut());
        self.cursor_controller.hide_inactive_cursors();
        let cleared_path_ids = self.gfx.participants_manager_mut().update_auto_clear();
        let translator = self
            .cursor_controller
            .get_overlay_window()
            .create_position_translator();
        self.gfx.draw(&translator);
        cleared_path_ids
    }
}

/// The main application struct that manages the entire remote desktop control session.
///
/// This struct coordinates all aspects of the remote desktop system, including screen capture,
/// overlay window management, input handling, and communication with remote clients. It serves
/// as the primary entry point for managing remote desktop sessions.
///
/// # Architecture
///
/// The application follows an event-driven architecture where:
/// - Screen capture runs in a separate thread
/// - Socket communication handles messages the main tauri app
/// - Event loop processes commands received from the socket and the livekit room and system events
/// - Remote control components are created/destroyed based on session state
///
/// # Fields
///
/// * `remote_control` - Optional active remote control session (None when not sharing)
/// * `textures_path` - Path to texture resources for cursor and UI rendering
/// * `screen_capturer` - Thread-safe screen capture system wrapped in Arc<Mutex>
/// * `_screen_capturer_events` - Handle to the screen capture event polling thread
/// * `socket` - Local socket for communication with the main tauri app
/// * `room_service` - object for interacting with the livekit room and its async thread
/// * `event_loop_proxy` - Proxy for sending events to the main event loop
///
/// # Lifecycle
///
/// 1. **Initialization**: Created with configuration and socket connection
/// 2. **Available Content**: Provides list of screens/windows that can be shared
/// 3. **Screen Sharing**: Creates overlay window and starts capture when session begins
/// 4. **Active Session**: Handles input events and renders cursor feedback
/// 5. **Cleanup**: Destroys overlay window and stops capture when session ends
///
/// # Thread Safety
///
/// The application is designed to work across multiple threads:
/// - Main thread: Event loop and UI operations
/// - Capture thread: Screen capture and streaming
/// - Socket thread: Message handling from clients
/// - Room service: WebRTC communication
///
/// # Error Handling
///
/// Operations return `Result<(), ServerError>` for proper error propagation.
/// Critical errors may trigger session reset or application termination.
pub struct Application<'a> {
    remote_control: Option<RemoteControl<'a>>,
    // TODO: remove me
    textures_path: String,
    // The arc is needed because we move the object to the
    // thread that checks if the stream has failed.
    //screen_capturer: Arc<Mutex<ScreenCapturer>>,
    screen_capturer: Arc<Mutex<Capturer>>,
    _screen_capturer_events: Option<JoinHandle<()>>,
    socket: SocketSender,
    room_service: Option<RoomService>,
    event_loop_proxy: EventLoopProxy<UserEvent>,
    local_drawing: LocalDrawing,
    window_manager: Option<window_manager::WindowManager>,
    audio_capturer: audio::capturer::Capturer,
    audio_player: audio::player::Player,
    camera_capturer: Arc<Mutex<CameraCapturer>>,
    _camera_capturer_events: Option<JoinHandle<()>>,
    camera_window: Option<CameraWindow>,
    screensharing_window: Option<ScreensharingWindow>,
    stats_window: Option<StatsWindow>,
}

// window: winit window
// window_state: buttons pressed etc

#[derive(Error, Debug)]
pub enum ApplicationError {
    #[error("Failed to create room service: {0}")]
    RoomServiceError(#[from] std::io::Error),
    #[error("Failed to create audio player: {0}")]
    AudioPlayerError(String),
}

#[derive(Debug)]
struct LocalDrawing {
    enabled: bool,
    permanent: bool,
    left_mouse_pressed: bool,
    current_path_id: u64,
    last_cursor_position: Option<Position>,
    previous_controllers_enabled: bool,
    cursor_set_times: u32,
}

impl LocalDrawing {
    fn reset(&mut self) {
        self.enabled = false;
        self.permanent = false;
        self.left_mouse_pressed = false;
        self.current_path_id = 0;
        self.last_cursor_position = None;
        self.previous_controllers_enabled = false;
    }
}

impl fmt::Display for LocalDrawing {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "LocalDrawing: enabled: {} permanent: {} left_mouse_pressed: {} current_path_id: {} last_cursor_position: {:?}  previous_controllers_enabled: {}", self.enabled, self.permanent, self.left_mouse_pressed, self.current_path_id, self.last_cursor_position, self.previous_controllers_enabled)
    }
}

impl<'a> Application<'a> {
    /// Creates a new Application instance with the specified configuration.
    ///
    /// This initializes all the core components needed for remote desktop control:
    /// - Screen capturer for capturing display content
    /// - Room service for interacting with the livekit room and its async thread
    /// - Event handling infrastructure
    ///
    /// # Arguments
    ///
    /// * `input` - Configuration including texture paths and LiveKit server URL
    /// * `socket` - Established socket connection for client communication
    /// * `event_loop_proxy` - Proxy for sending events to the main event loop
    ///
    /// # Returns
    ///
    /// Returns `Ok(Application)` on success, or `Err(ApplicationError)` if initialization fails.
    ///
    /// # Errors
    ///
    /// This function will return an error if:
    /// - Room service creation fails
    /// - Screen capturer initialization fails
    /// - Event loop proxy is invalid
    pub fn new(
        input: RenderLoopRunArgs,
        socket: SocketSender,
        event_loop_proxy: EventLoopProxy<UserEvent>,
    ) -> Result<Self, ApplicationError> {
        let screencapturer = Arc::new(Mutex::new(Capturer::new(event_loop_proxy.clone())));

        let audio_player = audio::player::Player::new(event_loop_proxy.clone())
            .map_err(ApplicationError::AudioPlayerError)?;

        let audio_capturer = audio::capturer::Capturer::new(event_loop_proxy.clone());

        let camera_capturer = Arc::new(Mutex::new(CameraCapturer::new()));
        let camera_capturer_clone = camera_capturer.clone();

        Ok(Self {
            remote_control: None,
            textures_path: input.textures_path,
            screen_capturer: screencapturer.clone(),
            _screen_capturer_events: Some(std::thread::spawn(move || poll_stream(screencapturer))),
            socket,
            room_service: None,
            event_loop_proxy,
            local_drawing: LocalDrawing {
                enabled: false,
                permanent: false,
                left_mouse_pressed: false,
                current_path_id: 0,
                last_cursor_position: None,
                previous_controllers_enabled: false,
                cursor_set_times: 0,
            },
            window_manager: None,
            audio_capturer,
            audio_player,
            camera_capturer: camera_capturer.clone(),
            _camera_capturer_events: Some(std::thread::spawn(move || {
                poll_camera_stream(camera_capturer_clone)
            })),
            camera_window: None,
            screensharing_window: None,
            stats_window: None,
        })
    }

    fn get_available_content(&mut self, event_loop: &ActiveEventLoop) -> Vec<CaptureContent> {
        let mut screen_capturer = self.screen_capturer.lock().unwrap();
        let res = screen_capturer.get_available_content();

        if let Err(e) = res {
            log::error!("get_available_content: Error getting available content: {e:?}");
            return vec![];
        }

        if let Some(wm) = self.window_manager.as_mut() {
            let _ = wm.update(event_loop);
        }

        res.unwrap()
    }

    /// Initiates a screen sharing session with the specified configuration.
    ///
    /// This method sets up the complete screen sharing pipeline:
    /// 1. Calculates optimal streaming resolution using aspect fitting
    /// 2. Creates a livekit room for real-time communication
    /// 3. Starts screen capture on the selected monitor
    /// 4. Creates overlay window for cursor visualization
    ///
    /// # Arguments
    ///
    /// * `screenshare_input` - Configuration including content selection and resolution
    /// * `monitors` - Available monitors for screen capture
    /// * `event_loop` - Active event loop for window creation
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on successful setup, or `Err(ServerError)` if any step fails.
    ///
    /// # Side Effects
    ///
    /// On success, this method:
    /// - Starts screen capture in a background thread
    /// - Creates a maximized transparent overlay window
    /// - Initializes cursor and keyboard controllers
    /// - Begins streaming captured content via LiveKit
    fn screenshare(
        &mut self,
        event_loop: &ActiveEventLoop,
        screenshare_input: ScreenShareMessage,
        monitors: Vec<MonitorHandle>,
    ) -> Result<(), ServerError> {
        log::info!(
            "screenshare: resolution: {:?} content: {} accessibility_permission: {}",
            screenshare_input.resolution,
            screenshare_input.content,
            screenshare_input.accessibility_permission,
        );

        self.stop_screenshare();
        self.close_screensharing_window();

        let mut screen_capturer = self.screen_capturer.lock().unwrap();
        /*
         * In order to not rely on the buffer source to exist before starting the room
         * we start the stream first and we lazy initialize the stream buffer and the
         * capture buffer.
         *
         * Then using the stream extent we can create the room and create the buffer source,
         * which we set in the Stream.
         */
        let res = screen_capturer.start_capture(
            screenshare_input.content,
            Extent {
                width: screenshare_input.resolution.width,
                height: screenshare_input.resolution.height,
            },
            !screenshare_input.accessibility_permission,
        );
        if let Err(error) = res {
            log::error!("screenshare: error starting capture: {error:?}");
            return Err(ServerError::StreamCreationError);
        }

        let extent = screen_capturer.get_stream_extent();
        if extent.width == 0. || extent.height == 0. {
            drop(screen_capturer);
            self.stop_screenshare();
            return Err(ServerError::StreamExtentError);
        }

        if self.room_service.is_none() {
            drop(screen_capturer);
            self.stop_screenshare();
            return Err(ServerError::RoomServiceNotFound);
        }

        let room_service = self.room_service.as_mut().unwrap();
        let res = room_service.publish_track(extent.width as u32, extent.height as u32);
        if let Err(error) = res {
            log::error!("screenshare: error publishing track: {error:?}");
            drop(screen_capturer);
            self.stop_screenshare();
            return Err(ServerError::PublishTrackError);
        }
        log::info!("screenshare: track published");
        let buffer_source = room_service.get_buffer_source();
        screen_capturer.set_buffer_source(buffer_source);

        let monitor = screen_capturer.get_selected_monitor(&monitors, screenshare_input.content.id);
        drop(screen_capturer);

        let res = self.create_overlay_window(
            event_loop,
            monitor,
            screenshare_input.accessibility_permission,
        );
        if let Err(e) = res {
            self.stop_screenshare();
            log::error!("screenshare: error creating overlay window: {e:?}");
            return Err(e);
        }

        /* We want to add the participants that already exist in the cursor controller list. */
        let room_service = self.room_service.as_ref().unwrap();
        room_service.iterate_participants();

        {
            let participants = room_service.participants();
            let mut guard = participants.write().unwrap();
            if let Some(local) = guard.get_mut("local") {
                local.set_is_screensharing(true);
            }
        }

        Ok(())
    }

    fn stop_camera(&mut self) {
        log::info!("stop_camera");
        {
            let mut capturer = self.camera_capturer.lock().unwrap();
            capturer.stop_capture();
        }
        if let Some(room_service) = self.room_service.as_ref() {
            room_service.unpublish_camera_track();
            let participants = room_service.participants();
            let guard = participants.read().unwrap();
            if let Some(info) = guard.get("local") {
                info.camera_buffers().set_inactive(true);
            }
        }
    }

    fn stop_mic(&mut self) {
        log::info!("stop_mic");
        self.audio_capturer.stop_capture();
        if let Some(room_service) = self.room_service.as_ref() {
            room_service.unpublish_audio_track();
        }
    }

    fn open_screensharing_window(
        &mut self,
        event_loop: &ActiveEventLoop,
        buffer: Arc<crate::livekit::video::VideoBufferManager>,
        participant_sid: Option<String>,
        participant_name: Option<String>,
    ) {
        match ScreensharingWindow::new(event_loop, buffer, participant_sid, participant_name) {
            Ok(win) => self.screensharing_window = Some(win),
            Err(e) => {
                log::error!("Failed to open screensharing window: {e:?}");
                return;
            }
        }
        if self.stats_window.is_none() {
            match StatsWindow::new(event_loop) {
                Ok(w) => self.stats_window = Some(w),
                Err(e) => log::error!("Failed to open stats window: {e:?}"),
            }
        }
    }

    fn close_screensharing_window(&mut self) {
        log::info!("close_screensharing_window");
        self.screensharing_window = None;
        self.stats_window = None;
    }

    fn stop_screenshare(&mut self) {
        log::info!("stop_screenshare");
        let screen_capturer = self.screen_capturer.lock();
        if let Err(e) = screen_capturer {
            log::error!("stop_screenshare: Error locking screen capturer: {e:?}");
            return;
        }
        let mut screen_capturer = screen_capturer.unwrap();
        screen_capturer.stop_capture();
        drop(screen_capturer);
        if let Some(room_service) = self.room_service.as_ref() {
            room_service.unpublish_screen_share_track();
            let participants = room_service.participants();
            let mut guard = participants.write().unwrap();
            if let Some(local) = guard.get_mut("local") {
                local.set_is_screensharing(false);
            }
        }
        self.destroy_overlay_window();
    }

    fn create_overlay_window(
        &mut self,
        event_loop: &ActiveEventLoop,
        selected_monitor: MonitorHandle,
        accessibility_permission: bool,
    ) -> Result<(), ServerError> {
        log::info!("create_overlay_window: selected_monitor: {selected_monitor:?} {accessibility_permission}",);

        let window = self
            .window_manager
            .as_mut()
            .ok_or(ServerError::WindowCreationError)?
            .show_window(&selected_monitor, event_loop)
            .map_err(|e| {
                log::error!("create_overlay_window: Error showing window: {:?}", e);
                ServerError::from(e)
            })?;

        let window_size = window.inner_size();
        let window_outer_position = window.outer_position();

        let mut graphics_context = match GraphicsContext::new(
            window,
            self.textures_path.clone(),
            selected_monitor.scale_factor(),
            self.event_loop_proxy.clone(),
        ) {
            Ok(context) => context,
            Err(error) => {
                log::error!("create_overlay_window: Error creating graphics context {error:?}");
                return Err(ServerError::GfxCreationError);
            }
        };

        // Add local participant to draw manager with auto-clear enabled
        graphics_context
            .add_participant("local".to_string(), "Me ", true)
            .map_err(|e| {
                log::error!(
                    "create_overlay_window: Failed to create local participant cursor: {e}"
                );
                ServerError::GfxCreationError
            })?;

        // Load pencil cursor image once during window creation
        let pencil_path = format!("{}/pencil.png", self.textures_path);
        let pencil_image = image::open(&pencil_path).map_err(|e| {
            log::error!("create_overlay_window: Failed to load pencil.png: {e:?}");
            ServerError::GfxCreationError
        })?;
        let mut rgba = pencil_image.to_rgba8();
        for pixel in rgba.chunks_exact_mut(4) {
            let a = pixel[3] as f32 / 255.0;
            pixel[0] = (pixel[0] as f32 * a) as u8;
            pixel[1] = (pixel[1] as f32 * a) as u8;
            pixel[2] = (pixel[2] as f32 * a) as u8;
        }
        let (width, height) = pencil_image.dimensions();
        let hotspot_x = 0; // Pencil tip at top-left
        let hotspot_y = height.saturating_sub(1); // Bottom of image (pencil tip)

        let custom_cursor_source = winit::window::CustomCursor::from_rgba(
            rgba.into_raw(),
            width as u16,
            height as u16,
            hotspot_x as u16,
            hotspot_y as u16,
        )
        .map_err(|e| {
            log::error!("create_overlay_window: Failed to create cursor source: {e:?}");
            ServerError::GfxCreationError
        })?;

        let pencil_cursor = event_loop.create_custom_cursor(custom_cursor_source);

        /* Hardcode window frame to zero as we only support displays for now.*/
        let window_frame = Frame::default();
        let scaled = {
            #[cfg(target_os = "macos")]
            {
                true
            }
            #[cfg(any(target_os = "windows", target_os = "linux"))]
            {
                false
            }
        };

        let monitor_position = selected_monitor.position();

        let window_position = match window_outer_position {
            Ok(position) => position,
            Err(error) => {
                log::error!("create_overlay_window: Error getting window position {error:?} using monitor's");
                selected_monitor.position()
            }
        };

        let overlay_window = Arc::new(OverlayWindow::new(
            window_frame,
            Extent {
                width: window_size.width as f64,
                height: window_size.height as f64,
            },
            window_position,
            DisplayInfo {
                display_extent: Extent {
                    width: selected_monitor.size().width as f64,
                    height: selected_monitor.size().height as f64,
                },
                display_position: monitor_position,
                display_scale: selected_monitor.scale_factor(),
            },
            scaled,
        ));

        log::info!("create_overlay_window: overlay_window created {overlay_window}");

        let redraw_sender = graphics_context.redraw_sender();
        let clock = graphics_context.clock();
        let cursor_controller = CursorController::new(
            overlay_window.clone(),
            redraw_sender,
            self.event_loop_proxy.clone(),
            accessibility_permission,
            clock,
        );
        if let Err(error) = cursor_controller {
            log::error!("create_overlay_window: Error creating cursor controller {error:?}");
            return Err(ServerError::CursorControllerCreationError);
        }

        let clipboard_controller = match ClipboardController::new() {
            Ok(controller) => Some(controller),
            Err(error) => {
                log::error!("create_overlay_window: Error creating clipboard controller {error:?}");
                None
            }
        };
        self.remote_control = Some(RemoteControl {
            gfx: graphics_context,
            cursor_controller: cursor_controller.unwrap(),
            keyboard_controller: KeyboardController::<KeyboardLayout>::new(),
            clipboard_controller,
            pencil_cursor,
        });

        // Reset local drawing state on start of screenshare.
        self.local_drawing.reset();

        #[cfg(target_os = "linux")]
        {
            /* We can't support the overlay surface on linux yet. */
            self.remote_control = None;
        }

        Ok(())
    }

    fn destroy_overlay_window(&mut self) {
        log::info!("destroy_overlay_window");
        if let Some(wm) = self.window_manager.as_mut() {
            wm.hide_active_window();
        }
        self.remote_control = None;
    }
}

impl Drop for Application<'_> {
    fn drop(&mut self) {
        let screen_capturer = self.screen_capturer.lock();
        if let Err(e) = screen_capturer {
            log::error!("Error locking screen capturer: {e:?}");
            return;
        }
        let mut screen_capturer = screen_capturer.unwrap();
        screen_capturer.stop_capture();
        screen_capturer.stop_runtime_stream_handler();
        if let Some(screen_capturer_events) = self._screen_capturer_events.take() {
            screen_capturer_events.join().unwrap();
        }
    }
}

#[derive(Debug, Clone)]
pub struct ScrollDelta {
    pub x: f64,
    pub y: f64,
}

impl<'a> ApplicationHandler<UserEvent> for Application<'a> {
    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: UserEvent) {
        match event {
            UserEvent::CursorPosition(x, y, sid) => {
                log::debug!("user_event: cursor position: {x} {y} {sid}");
                if let Some(remote_control) = &mut self.remote_control {
                    remote_control.cursor_controller.cursor_move_controller(
                        x as f64,
                        y as f64,
                        sid.as_str(),
                    );
                }
                if let Some(screensharing_window) = &mut self.screensharing_window {
                    screensharing_window.set_cursor_position(
                        sid.as_str(),
                        Some(Position {
                            x: x as f64,
                            y: y as f64,
                        }),
                    );
                    screensharing_window.request_redraw();
                }
            }
            UserEvent::MouseClick(data, sid) => {
                log::debug!("user_event: mouse click: {data:?} {sid}");
                if self.remote_control.is_none() {
                    log::warn!("user_event: remote control is none mouse click");
                    return;
                }
                let remote_control = &mut self.remote_control.as_mut().unwrap();
                remote_control
                    .cursor_controller
                    .mouse_click_controller(data, sid.as_str());
            }
            UserEvent::ControllerCursorEnabled(enabled) => {
                log::debug!("user_event: cursor enabled: {enabled:?}");
                if self.remote_control.is_none() {
                    log::warn!("user_event: remote control is none cursor enabled ");
                    return;
                }
                if self.room_service.is_none() {
                    log::warn!("user_event: room service is none cursor enabled");
                    return;
                }
                let remote_control = &mut self.remote_control.as_mut().unwrap();
                let cursor_controller = &mut remote_control.cursor_controller;
                cursor_controller.set_controllers_enabled(enabled);
                let keyboard_controller = &mut remote_control.keyboard_controller;
                keyboard_controller.set_enabled(enabled);
                self.room_service
                    .as_ref()
                    .unwrap()
                    .publish_controller_cursor_enabled(enabled);
            }
            UserEvent::ControllerCursorVisible(visible, sid) => {
                log::debug!("user_event: cursor visible: {visible:?} {sid}");
                if self.remote_control.is_none() {
                    log::warn!("user_event: remote control is none cursor visible");
                    return;
                }
                let remote_control = &mut self.remote_control.as_mut().unwrap();
                let cursor_controller = &mut remote_control.cursor_controller;
                cursor_controller.set_controller_pointer(visible, sid.as_str());
            }
            UserEvent::Keystroke(keystroke_data) => {
                log::debug!("user_event: keystroke: {keystroke_data:?}");
                if self.remote_control.is_none() {
                    log::warn!("user_event: remote control is none keystroke");
                    return;
                }
                let remote_control = &mut self.remote_control.as_mut().unwrap();
                let keyboard_controller = &mut remote_control.keyboard_controller;
                keyboard_controller.simulate_keystrokes(keystroke_data);
            }
            UserEvent::Scroll(delta, sid) => {
                log::debug!("user_event: scroll: {delta:?} {sid}");
                if self.remote_control.is_none() {
                    log::warn!("user_event: remote control is none scroll");
                    return;
                }
                let remote_control = &mut self.remote_control.as_mut().unwrap();
                let cursor_controller = &mut remote_control.cursor_controller;
                cursor_controller.scroll_controller(delta, sid.as_str());
            }
            UserEvent::Terminate => {
                log::info!("user_event: Client disconnected, terminating.");
                event_loop.exit();
            }
            UserEvent::GetAvailableContent => {
                log::debug!("user_event: Get available content");
                let content = self.get_available_content(event_loop);
                if content.is_empty() {
                    log::error!("user_event: No available content");
                    sentry_utils::upload_logs_event("No available content".to_string());
                }
                let res = self
                    .socket
                    .send(Message::AvailableContent(AvailableContentMessage {
                        content,
                    }));
                if res.is_err() {
                    log::error!(
                        "user_event: Error sending available content: {:?}",
                        res.err()
                    );
                }
            }
            UserEvent::CallStart(call_start) => {
                log::info!("user_event: CallStart");
                let result = if let Some(room_service) = self.room_service.as_ref() {
                    match room_service.create_room(
                        call_start.audio_token,
                        call_start.video_token,
                        self.event_loop_proxy.clone(),
                    ) {
                        Ok(_) => {
                            log::info!("user_event: Room created successfully");
                            room_service.iterate_participants();
                            Ok(())
                        }
                        Err(e) => {
                            log::error!("user_event: Failed to create room: {e:?}");
                            Err(e.to_string())
                        }
                    }
                } else {
                    log::error!("user_event: Room service not found for CallStart");
                    Err(ServerError::RoomServiceNotFound.to_string())
                };
                if let Err(e) = self.socket.send(Message::CallStartResult(result)) {
                    error!("user_event: Error sending CallStartResult: {e:?}");
                }
            }
            UserEvent::CallEnd => {
                log::info!("user_event: CallEnd");
                self.stop_mic();
                self.stop_camera();

                let capturer_valid = {
                    let screen_capturer = self.screen_capturer.lock();
                    screen_capturer.is_ok()
                };
                if capturer_valid {
                    self.stop_screenshare();
                } else {
                    log::warn!("user_event: CallEnd: screen capturer is not valid");
                    self.destroy_overlay_window();

                    self.screen_capturer =
                        Arc::new(Mutex::new(Capturer::new(self.event_loop_proxy.clone())));
                    let screen_capturer_clone = self.screen_capturer.clone();
                    self._screen_capturer_events = Some(std::thread::spawn(move || {
                        poll_stream(screen_capturer_clone)
                    }));
                }

                if let Some(room_service) = self.room_service.as_mut() {
                    room_service.destroy_room();
                }

                self.camera_window = None;
                self.screensharing_window = None;
                self.stats_window = None;

                if let Err(e) = self.socket.send(Message::CallEnded) {
                    log::error!("user_event: Error sending CallEnded: {e:?}");
                }

                sentry_utils::upload_logs_event("Ending call".to_string());
            }
            UserEvent::ScreenShare(data) => {
                log::debug!("user_event: Screen share: {data:?}");
                let monitors = event_loop
                    .available_monitors()
                    .collect::<Vec<MonitorHandle>>();

                let result_message = match self.screenshare(event_loop, data, monitors) {
                    Ok(_) => Ok(()),
                    Err(e) => {
                        log::error!("user_event: Screen share failed: {e:?}");
                        sentry_utils::upload_logs_event("Screen share failed".to_string());
                        Err(e.to_string())
                    }
                };

                if result_message.is_ok() {
                    if let Some(room_service) = self.room_service.as_ref() {
                        let snapshot = room_service.participants_snapshot();
                        let _ = self.socket.send(Message::ParticipantsSnapshot(snapshot));
                    }
                }

                if let Err(e) = self
                    .socket
                    .send(Message::StartScreenShareResult(result_message))
                {
                    error!("user_event: Error sending start screen share result: {e:?}");
                }
            }
            UserEvent::ScreenShareFrameReady => {
                if let Some(screensharing_window) = &self.screensharing_window {
                    screensharing_window.request_redraw();
                }
            }
            UserEvent::StopScreenShare => {
                self.stop_screenshare();
                if let Some(room_service) = self.room_service.as_ref() {
                    let snapshot = room_service.participants_snapshot();
                    let _ = self.socket.send(Message::ParticipantsSnapshot(snapshot));
                }
            }
            UserEvent::RequestRedraw => {
                log::trace!("user_event: Requesting redraw");
                if self.remote_control.is_none() {
                    log::warn!("user_event: remote control is none request redraw");
                    return;
                }
                let remote_control = &mut self.remote_control.as_mut().unwrap();
                let gfx = &mut remote_control.gfx;
                gfx.window().request_redraw();
            }
            UserEvent::SharerPosition(x, y) => {
                debug!("user_event: sharer position: {x} {y}");
                if self.room_service.is_none() {
                    log::warn!("user_event: room service is none sharer position");
                    return;
                }
                self.room_service
                    .as_ref()
                    .unwrap()
                    .publish_cursor_position(x, y, true);
            }
            UserEvent::Tick(time) => {
                debug!("user_event: Tick");
                if self.room_service.is_none() {
                    log::warn!("user_event: room service is none tick");
                    return;
                }
                self.room_service.as_ref().unwrap().tick_response(time);
            }
            UserEvent::ParticipantConnected(participant) => {
                log::debug!("user_event: Participant connected: {participant:?}");

                if let Some(remote_control) = &mut self.remote_control {
                    let sid = participant.sid.clone();
                    let name = participant.name.clone();

                    // Add participant to draw manager first (assigns color)
                    if let Err(e) = remote_control
                        .gfx
                        .add_participant(sid.clone(), &name, false)
                    {
                        log::error!("Failed to create cursor for participant {sid}: {e}");
                    } else {
                        // Then add to cursor controller for state tracking
                        remote_control.cursor_controller.add_controller(sid);
                    }
                }

                if let Some(screensharing_window) = &mut self.screensharing_window {
                    if let Err(e) = screensharing_window.add_participant(
                        participant.sid.clone(),
                        &participant.name,
                        false,
                    ) {
                        log::error!(
                            "user_event: failed to add participant to screensharing window: {e:?}"
                        );
                    }
                }
            }
            UserEvent::ParticipantDisconnected(participant) => {
                log::debug!("user_event: Participant disconnected: {participant:?}");

                if let Some(remote_control) = &mut self.remote_control {
                    remote_control
                        .cursor_controller
                        .remove_controller(participant.sid.as_str());
                    // Remove participant from draw manager
                    remote_control
                        .gfx
                        .remove_participant(participant.sid.as_str());
                }
                if let Some(screensharing_window) = &mut self.screensharing_window {
                    screensharing_window.remove_participant(participant.sid.as_str());
                }
            }
            UserEvent::ParticipantsSnapshot(snapshot) => {
                log::debug!("user_event: Participants snapshot: {snapshot:?}");
                let _ = self.socket.send(Message::ParticipantsSnapshot(snapshot));
            }
            UserEvent::LivekitServerUrl(url) => {
                log::debug!("user_event: Livekit server url: {url}");

                let room_service = RoomService::new(
                    url,
                    self.event_loop_proxy.clone(),
                    self.audio_player.mixer().clone(),
                    self.audio_player.processor(),
                );
                if room_service.is_err() {
                    log::error!(
                        "user_event: Error creating room service: {:?}",
                        room_service.err()
                    );
                    return;
                }
                log::debug!("user_event: Room service created: {room_service:?}");
                self.room_service = Some(room_service.unwrap());
            }
            UserEvent::ControllerTakesScreenShare => {
                log::info!("user_event: Controller takes screen share");
                self.stop_screenshare();
                if let Some(room_service) = self.room_service.as_ref() {
                    let snapshot = room_service.participants_snapshot();
                    let _ = self.socket.send(Message::ParticipantsSnapshot(snapshot));
                }
            }
            UserEvent::ParticipantInControl(participant) => {
                log::debug!("user_event: participant in control: {participant:?}");
                if self.room_service.is_none() {
                    log::warn!("user_event: room service is none participant in control");
                    return;
                }
                self.room_service
                    .as_ref()
                    .unwrap()
                    .publish_participant_in_control(participant);
            }
            UserEvent::LocalParticipantInControl(in_control) => {
                log::debug!("user_event: local participant in control: {in_control}");
                if let Some(screensharing_window) = &mut self.screensharing_window {
                    screensharing_window.set_local_participant_in_control(in_control);
                }
            }
            UserEvent::SentryMetadata(sentry_metadata) => {
                log::debug!("user_event: Sentry metadata: {sentry_metadata:?}");
                sentry_utils::init_metadata(
                    sentry_metadata.user_email,
                    sentry_metadata.app_version,
                );
            }
            UserEvent::AddToClipboard(add_to_clipboard_data) => {
                log::info!("user_event: Add to clipboard: {add_to_clipboard_data:?}");
                if self.remote_control.is_none() {
                    log::warn!("user_event: remote control is none add to clipboard");
                    return;
                }
                let remote_control = &mut self.remote_control.as_mut().unwrap();
                if remote_control.clipboard_controller.is_none() {
                    log::warn!("user_event: clipboard controller is none add to clipboard");
                    return;
                }
                let clipboard_controller =
                    &mut remote_control.clipboard_controller.as_ref().unwrap();
                clipboard_controller.add_to_clipboard(
                    add_to_clipboard_data.is_copy,
                    &mut remote_control.keyboard_controller,
                );
            }
            UserEvent::PasteFromClipboard(paste_from_clipboard_data) => {
                log::info!("user_event: Paste from clipboard");
                if self.remote_control.is_none() {
                    log::warn!("user_event: remote control is none paste from clipboard");
                    return;
                }
                let remote_control = &mut self.remote_control.as_mut().unwrap();
                if remote_control.clipboard_controller.is_none() {
                    log::warn!("user_event: clipboard controller is none paste from clipboard");
                    return;
                }
                let clipboard_controller =
                    &mut remote_control.clipboard_controller.as_mut().unwrap();
                clipboard_controller.paste_from_clipboard(
                    &mut remote_control.keyboard_controller,
                    paste_from_clipboard_data.data,
                );
            }
            UserEvent::DrawingMode(drawing_mode, sid) => {
                log::debug!("user_event: DrawingMode: {:?} {}", drawing_mode, sid);
                if let Some(remote_control) = &mut self.remote_control {
                    match &drawing_mode {
                        DrawingMode::Disabled => {
                            remote_control
                                .cursor_controller
                                .set_controller_pointer(false, sid.as_str());
                        }
                        _ => {
                            remote_control
                                .cursor_controller
                                .set_controller_pointer(true, sid.as_str());
                        }
                    }
                    remote_control
                        .gfx
                        .set_drawing_mode(sid.as_str(), drawing_mode.clone());
                }
                if let Some(screensharing_window) = &mut self.screensharing_window {
                    screensharing_window.set_drawing_mode(sid.as_str(), drawing_mode);
                }
            }
            UserEvent::DrawStart(point, path_id, sid) => {
                log::debug!("user_event: DrawStart: {:?} {} {}", point, path_id, sid);
                let pos = Position {
                    x: point.x,
                    y: point.y,
                };
                if let Some(remote_control) = &mut self.remote_control {
                    remote_control.gfx.draw_start(sid.as_str(), pos, path_id);
                    remote_control.cursor_controller.cursor_move_controller(
                        point.x,
                        point.y,
                        sid.as_str(),
                    );
                }
                if let Some(screensharing_window) = &mut self.screensharing_window {
                    screensharing_window.draw_start(sid.as_str(), pos, path_id);
                    screensharing_window.set_cursor_position(sid.as_str(), Some(pos));
                }
            }
            UserEvent::DrawAddPoint(point, sid) => {
                log::debug!("user_event: DrawAddPoint: {:?} {}", point, sid);
                let pos = Position {
                    x: point.x,
                    y: point.y,
                };
                if let Some(remote_control) = &mut self.remote_control {
                    remote_control.gfx.draw_add_point(sid.as_str(), pos);
                    remote_control.cursor_controller.cursor_move_controller(
                        point.x,
                        point.y,
                        sid.as_str(),
                    );
                }
                if let Some(screensharing_window) = &mut self.screensharing_window {
                    screensharing_window.draw_add_point(sid.as_str(), pos);
                    screensharing_window.set_cursor_position(sid.as_str(), Some(pos));
                }
            }
            UserEvent::DrawEnd(point, sid) => {
                log::debug!("user_event: DrawEnd: {:?} {}", point, sid);
                let pos = Position {
                    x: point.x,
                    y: point.y,
                };
                if let Some(remote_control) = &mut self.remote_control {
                    remote_control.gfx.draw_end(sid.as_str(), pos);
                    remote_control.cursor_controller.cursor_move_controller(
                        point.x,
                        point.y,
                        sid.as_str(),
                    );
                }
                if let Some(screensharing_window) = &mut self.screensharing_window {
                    screensharing_window.draw_end(sid.as_str(), pos);
                    screensharing_window.set_cursor_position(sid.as_str(), Some(pos));
                }
            }
            UserEvent::DrawClearPath(path_id, sid) => {
                log::debug!("user_event: DrawClearPath: {} {}", path_id, sid);
                if let Some(remote_control) = &mut self.remote_control {
                    remote_control.gfx.draw_clear_path(sid.as_str(), path_id);
                    remote_control.gfx.trigger_render();
                }
                if let Some(screensharing_window) = &mut self.screensharing_window {
                    screensharing_window.draw_clear_path(sid.as_str(), path_id);
                }
            }
            UserEvent::DrawClearAllPaths(sid) => {
                log::debug!("user_event: DrawClearAllPaths: {}", sid);
                if let Some(remote_control) = &mut self.remote_control {
                    remote_control.gfx.draw_clear_all_paths(sid.as_str());
                    remote_control.gfx.trigger_render();
                }
                if let Some(screensharing_window) = &mut self.screensharing_window {
                    screensharing_window.draw_clear_all_paths(sid.as_str());
                }
            }
            UserEvent::ClickAnimationFromParticipant(point, sid) => {
                log::debug!(
                    "user_event: ClickAnimationFromParticipant: {:?} {}",
                    point,
                    sid
                );
                if let Some(remote_control) = &mut self.remote_control {
                    remote_control.gfx.trigger_click_animation(Position {
                        x: point.x,
                        y: point.y,
                    });
                }
                if let Some(screensharing_window) = &mut self.screensharing_window {
                    screensharing_window.trigger_click_animation(Position {
                        x: point.x,
                        y: point.y,
                    });
                }
            }
            UserEvent::ListAudioDevices => {
                log::debug!("user_event: ListAudioDevices");
                let devices: Vec<socket_lib::AudioDevice> = self
                    .audio_capturer
                    .list_sources()
                    .into_iter()
                    .map(|(name, default)| socket_lib::AudioDevice { name, default })
                    .collect();
                if let Err(e) = self.socket.send(Message::AudioDeviceList(devices)) {
                    error!("user_event: Error sending audio device list: {e:?}");
                }
            }
            UserEvent::StartAudioCapture(msg) => {
                log::info!(
                    "user_event: StartAudioCapture device_name={}",
                    msg.device_name
                );
                let result = if self.audio_capturer.is_capturing() {
                    self.audio_capturer
                        .switch_device(Some(&msg.device_name))
                        .map(|_| ())
                } else {
                    (|| -> Result<(), String> {
                        let (sample_tx, sample_rx) = tokio::sync::mpsc::unbounded_channel();

                        let sample_rate = self
                            .audio_capturer
                            .start_capture(Some(&msg.device_name), sample_tx)?;

                        let room_service = self
                            .room_service
                            .as_ref()
                            .ok_or_else(|| "Room service not found".to_string())?;

                        room_service
                            .publish_audio_track(sample_rate, sample_rx)
                            .map_err(|e| format!("Failed to publish audio track: {e}"))?;

                        Ok(())
                    })()
                };

                if let Err(ref e) = result {
                    log::error!("user_event: StartAudioCapture failed: {e}");
                }

                if let Err(e) = self.socket.send(Message::StartAudioCaptureResult(result)) {
                    error!("user_event: Error sending StartAudioCaptureResult: {e:?}");
                }
            }
            UserEvent::StopAudioCapture => {
                log::info!("user_event: StopAudioCapture");
                self.stop_mic();
            }
            UserEvent::MuteAudio => {
                log::info!("user_event: MuteAudio");
                if let Some(room_service) = self.room_service.as_ref() {
                    room_service.mute_audio_track();
                }
            }
            UserEvent::UnmuteAudio => {
                log::info!("user_event: UnmuteAudio");
                if let Some(room_service) = self.room_service.as_ref() {
                    room_service.unmute_audio_track();
                }
            }
            UserEvent::ToggleMic => {
                log::info!("user_event: ToggleMic");
                if let Some(room_service) = self.room_service.as_ref() {
                    if room_service.is_audio_muted() {
                        room_service.unmute_audio_track();
                    } else {
                        room_service.mute_audio_track();
                    }
                }
            }
            UserEvent::ListCameras => {
                log::debug!("user_event: ListCameras");
                let devices = CameraCapturer::list_devices();
                if let Err(e) = self.socket.send(Message::CameraList(devices)) {
                    error!("user_event: Error sending camera list: {e:?}");
                }
            }
            UserEvent::StartCamera(msg) => {
                log::info!("user_event: StartCamera device='{:?}'", msg.device_name);
                let result = (|| -> Result<(), String> {
                    let room_service = self
                        .room_service
                        .as_ref()
                        .ok_or_else(|| "Room service not found".to_string())?;

                    let (width, height) = {
                        let mut capturer = self.camera_capturer.lock().unwrap();
                        capturer.start_capture(
                            msg.device_name.as_deref(),
                            self.socket.clone(),
                            room_service.local_camera_buffer_manager(),
                        )?
                    };

                    room_service
                        .publish_camera_track(width, height)
                        .map_err(|e| format!("Failed to publish camera track: {e}"))?;

                    if let Some(source) = room_service.get_camera_buffer_source() {
                        let capturer = self.camera_capturer.lock().unwrap();
                        capturer.set_buffer_source(source);
                    }

                    Ok(())
                })();

                if let Err(ref e) = result {
                    log::error!("user_event: StartCamera failed: {e}");
                }

                // Open camera window if camera started successfully and window is enabled
                if result.is_ok() && self.camera_window.is_none() {
                    if let Some(room_service) = self.room_service.as_ref() {
                        let participants = room_service.participants();
                        match CameraWindow::new(
                            event_loop,
                            participants,
                            self.event_loop_proxy.clone(),
                        ) {
                            Ok(cam) => {
                                log::info!("user_event: Camera window opened for local camera");
                                self.camera_window = Some(cam);
                            }
                            Err(e) => log::error!("Failed to open camera window: {e:?}"),
                        }
                    }
                }

                if result.is_ok() {
                    if let Some(cam) = &mut self.camera_window {
                        cam.set_camera_active(true);
                    }
                    if let Some(room_service) = self.room_service.as_ref() {
                        let snapshot = room_service.participants_snapshot();
                        let _ = self.socket.send(Message::ParticipantsSnapshot(snapshot));
                    }
                }

                if let Err(e) = self.socket.send(Message::StartCameraResult(result)) {
                    error!("user_event: Error sending StartCameraResult: {e:?}");
                }
            }
            UserEvent::StopCamera => {
                log::info!("user_event: StopCamera");
                self.stop_camera();
                if let Some(cam) = &mut self.camera_window {
                    cam.set_camera_active(false);
                }
                if let Some(room_service) = self.room_service.as_ref() {
                    let snapshot = room_service.participants_snapshot();
                    let _ = self.socket.send(Message::ParticipantsSnapshot(snapshot));
                    let participants = room_service.participants();
                    let any_active = {
                        let guard = participants.read().unwrap();
                        guard.values().any(|info| info.camera_active())
                    };
                    if !any_active {
                        self.camera_window = None;
                    }
                }
            }
            UserEvent::OpenCamera => {
                log::info!("user_event: OpenCamera");
                if self.camera_window.is_some() {
                    log::info!("user_event: Camera window already exists, skipping");
                    return;
                }
                if let Some(room_service) = self.room_service.as_ref() {
                    let participants = room_service.participants();
                    match CameraWindow::new(event_loop, participants, self.event_loop_proxy.clone())
                    {
                        Ok(cam) => self.camera_window = Some(cam),
                        Err(e) => log::error!("Failed to open camera window: {e:?}"),
                    }
                } else {
                    log::warn!("user_event: room service is none, cannot open camera window");
                }
            }
            UserEvent::OpenScreensharing => {
                log::info!("user_event: OpenScreensharing");
                let buffer = Arc::new(crate::livekit::video::VideoBufferManager::new());
                self.open_screensharing_window(event_loop, buffer, None, None);
            }
            UserEvent::OpenContentPicker => {
                log::info!("user_event: OpenContentPicker");
                if let Err(e) = self.socket.send(Message::OpenContentPicker) {
                    log::error!("user_event: Error sending OpenContentPicker: {e:?}");
                }
            }
            UserEvent::OpenScreenShareWindow {
                sid: participant_sid,
                name: participant_name,
            } => {
                log::info!("user_event: OpenScreenShareWindow");
                if self.screensharing_window.is_some() {
                    log::info!("user_event: Screensharing window already exists, skipping");
                    return;
                }
                // Stop any active local screenshare before opening remote view
                self.stop_screenshare();
                if let Some(room_service) = self.room_service.as_ref() {
                    if let Some(screen_share_buffer) = room_service.screen_share_buffer() {
                        self.open_screensharing_window(
                            event_loop,
                            screen_share_buffer,
                            participant_sid,
                            participant_name,
                        );
                    } else {
                        log::warn!("user_event: No screen share buffer available");
                    }
                } else {
                    log::warn!("user_event: Room service not available");
                }
            }
            UserEvent::CloseScreenShareWindow => {
                log::info!("user_event: CloseScreenShareWindow");
                self.close_screensharing_window();
            }
            UserEvent::CloseCameraWindow => {
                log::info!("user_event: CloseCameraWindow");
                self.camera_window = None;
            }
            UserEvent::BringWindowsToFront => {
                log::info!("user_event: BringWindowsToFront");
                let mut focused = false;
                if let Some(ss) = &self.screensharing_window {
                    ss.focus_window();
                    focused = true;
                }
                if let Some(cam) = &self.camera_window {
                    cam.focus_window();
                    focused = true;
                }
                if let Err(e) = self
                    .socket
                    .send(Message::BringWindowsToFrontResult(focused))
                {
                    log::error!("user_event: Error sending BringWindowsToFrontResult: {e:?}");
                }
            }
            // TODO(@konsalex): We need to rethink how to tackle this,
            // as a new-joiner in a Room will not have access to this
            UserEvent::SharerControlEnabled(enabled) => {
                if let Some(screensharing_window) = &mut self.screensharing_window {
                    screensharing_window.set_remote_control_allowed(enabled);
                }
            }
            UserEvent::DefaultOutputDeviceChanged => {
                log::info!("Default audio output device changed, reconnecting...");
                if let Err(e) = self.audio_player.mixer().reconnect() {
                    log::error!("Failed to reconnect audio output: {e}");
                }
            }
            UserEvent::DefaultInputDeviceChanged => {
                self.audio_capturer.handle_default_device_changed();
            }
            UserEvent::LocalDrawingEnabled(drawing_enabled) => {
                log::debug!("user_event: LocalDrawingEnabled: {:?}", drawing_enabled);
                if self.remote_control.is_none() {
                    log::warn!("user_event: remote control is none local drawing enabled");
                    return;
                }

                let remote_control = &mut self.remote_control.as_mut().unwrap();
                if !self.local_drawing.enabled {
                    let window = remote_control.gfx.window();

                    // Enable cursor hittest so we can receive mouse events
                    if let Err(e) = window.set_cursor_hittest(true) {
                        log::error!("user_event: Failed to enable cursor hittest: {e:?}");
                        return;
                    }

                    // Enable drawing mode
                    self.local_drawing.enabled = true;
                    self.local_drawing.permanent = drawing_enabled.permanent;

                    // Reset cursor set times counter
                    self.local_drawing.cursor_set_times = 0;

                    // Store the current controller state before disabling
                    self.local_drawing.previous_controllers_enabled =
                        remote_control.cursor_controller.is_controllers_enabled();

                    // Disable remote control
                    remote_control
                        .cursor_controller
                        .set_controllers_enabled(false);
                    remote_control.keyboard_controller.set_enabled(false);

                    let draw_mode = room_service::DrawingMode::Draw(room_service::DrawSettings {
                        permanent: drawing_enabled.permanent,
                    });
                    remote_control
                        .gfx
                        .set_drawing_mode("local", draw_mode.clone());

                    if let Some(room_service) = &self.room_service {
                        room_service.publish_drawing_mode(draw_mode);
                    }

                    log::info!(
                        "Local drawing mode enabled (permanent: {})",
                        drawing_enabled.permanent
                    );
                } else {
                    // Disable drawing mode
                    self.local_drawing.enabled = false;
                    self.local_drawing.left_mouse_pressed = false;
                    self.local_drawing.last_cursor_position = None;

                    // Clear all local drawing paths
                    remote_control.gfx.draw_clear_all_paths("local");

                    // Send LiveKit event to clear all paths
                    if let Some(room_service) = &self.room_service {
                        room_service.publish_draw_clear_all_paths();
                    }

                    let window = remote_control.gfx.window();

                    // Restore default cursor
                    window.set_cursor(winit::window::Cursor::Icon(
                        winit::window::CursorIcon::Default,
                    ));

                    // Disable cursor hittest
                    if let Err(e) = window.set_cursor_hittest(false) {
                        log::error!("user_event: Failed to disable cursor hittest: {e:?}");
                    }

                    // Restore remote control to previous state
                    remote_control
                        .cursor_controller
                        .set_controllers_enabled(self.local_drawing.previous_controllers_enabled);
                    remote_control
                        .keyboard_controller
                        .set_enabled(self.local_drawing.previous_controllers_enabled);

                    // Set drawing mode to disabled for local participant
                    remote_control
                        .gfx
                        .set_drawing_mode("local", room_service::DrawingMode::Disabled);

                    if let Some(room_service) = &self.room_service {
                        room_service.publish_drawing_mode(room_service::DrawingMode::Disabled);
                    }

                    log::info!("Local drawing mode disabled");

                    remote_control.gfx.trigger_render();
                }
            }
        }
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window_manager.is_none() {
            log::info!("Application::resumed: initializing WindowManager");
            match window_manager::WindowManager::new(event_loop) {
                Ok(wm) => self.window_manager = Some(wm),
                Err(e) => log::error!(
                    "Application::resumed: failed to initialize WindowManager: {:?}",
                    e
                ),
            }
        }
    }

    // Once we get movement input from guest, we will call Window::request_redraw
    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: winit::window::WindowId,
        event: WindowEvent,
    ) {
        // Route to camera window if it matches
        if let Some(camera) = &mut self.camera_window {
            if camera.window_id() == window_id {
                camera.handle_window_event(event);
                return;
            }
        }

        // Route to stats window if it matches
        if let Some(sw) = &mut self.stats_window {
            if sw.window_id() == window_id {
                sw.handle_window_event(event);
                return;
            }
        }

        // Route to screensharing window if it matches
        if let Some(screen_sharing_window) = &mut self.screensharing_window {
            if screen_sharing_window.window_id() == window_id {
                let input_event = screen_sharing_window.handle_window_event(event);
                // Mutable borrow on screen_sharing_window is dropped here
                if let Some(event) = input_event {
                    if let Some(rs) = &self.room_service {
                        match event {
                            ScreenShareInputEvent::CursorMoved { x, y } => {
                                rs.publish_cursor_position(x, y, true);
                            }
                            ScreenShareInputEvent::MouseClick(data) => {
                                rs.publish_mouse_click(data);
                            }
                            ScreenShareInputEvent::Scroll(data) => {
                                rs.publish_wheel_event(data);
                            }
                            ScreenShareInputEvent::KeyInput(data) => {
                                rs.publish_keystroke(data);
                            }
                            ScreenShareInputEvent::DrawStart { x, y, path_id } => {
                                rs.publish_draw_start(crate::room_service::DrawPathPoint {
                                    point: crate::room_service::ClientPoint { x, y },
                                    path_id,
                                });
                            }
                            ScreenShareInputEvent::DrawAddPoint { x, y } => {
                                rs.publish_draw_add_point(crate::room_service::ClientPoint {
                                    x,
                                    y,
                                });
                            }
                            ScreenShareInputEvent::DrawEnd { x, y } => {
                                rs.publish_draw_end(crate::room_service::ClientPoint { x, y });
                            }
                            ScreenShareInputEvent::DrawClearAllPaths => {
                                rs.publish_draw_clear_all_paths();
                            }
                            ScreenShareInputEvent::DrawClearPaths(ids) => {
                                rs.publish_draw_clear_paths(ids);
                            }
                            ScreenShareInputEvent::ClickAnimation { x, y } => {
                                rs.publish_click_animation(crate::room_service::ClientPoint {
                                    x,
                                    y,
                                });
                            }
                            ScreenShareInputEvent::AddToClipboard { is_copy } => {
                                rs.publish_add_to_clipboard(
                                    crate::room_service::AddToClipboardData { is_copy },
                                );
                            }
                            ScreenShareInputEvent::PasteFromClipboard(text) => match text {
                                Some(clipboard_text) => {
                                    let bytes = clipboard_text.as_bytes();
                                    const MAX_PACKET: usize = 15 * 1024;
                                    let total_packets =
                                        ((bytes.len() + MAX_PACKET - 1) / MAX_PACKET) as u64;
                                    for i in 0..total_packets {
                                        let start = (i as usize) * MAX_PACKET;
                                        let end = ((i as usize + 1) * MAX_PACKET).min(bytes.len());
                                        let chunk = bytes[start..end].to_vec();
                                        rs.publish_paste_from_clipboard(
                                            crate::room_service::PasteFromClipboardData {
                                                data: Some(crate::room_service::ClipboardPayload {
                                                    packet_id: i,
                                                    total_packets,
                                                    data: chunk,
                                                }),
                                            },
                                        );
                                    }
                                }
                                None => {
                                    rs.publish_paste_from_clipboard(
                                        crate::room_service::PasteFromClipboardData { data: None },
                                    );
                                }
                            },
                            ScreenShareInputEvent::DrawingModeChanged(mode) => {
                                if mode == crate::room_service::DrawingMode::Disabled
                                    || mode == crate::room_service::DrawingMode::ClickAnimation
                                {
                                    rs.publish_draw_clear_all_paths();
                                }
                                rs.publish_drawing_mode(mode);
                            }
                        }
                    }
                }
                return;
            }
        }

        match event {
            WindowEvent::CloseRequested => {
                event_loop.exit();
            }
            WindowEvent::RedrawRequested => {
                if self.remote_control.is_none() {
                    log::warn!("window_event: remote control is none redraw requested");
                    return;
                }
                let remote_control = &mut self.remote_control.as_mut().unwrap();

                if self.local_drawing.enabled && self.local_drawing.cursor_set_times < 500 {
                    let window = remote_control.gfx.window();
                    window.focus_window();
                    window.set_cursor_visible(false);
                    window.set_cursor_visible(true);
                    window.set_cursor(remote_control.pencil_cursor.clone());
                    self.local_drawing.cursor_set_times += 1;
                }

                // Render frame with cursor updates, auto-clear, and drawing
                let cleared_path_ids = remote_control.render_frame();

                // Publish cleared paths to room service
                if !cleared_path_ids.is_empty() && self.room_service.is_some() {
                    self.room_service
                        .as_ref()
                        .unwrap()
                        .publish_draw_clear_paths(cleared_path_ids);
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                if self.local_drawing.enabled {
                    if button == winit::event::MouseButton::Left {
                        if state == winit::event::ElementState::Pressed {
                            self.local_drawing.left_mouse_pressed = true;
                            // Start a new path if we have a cursor position
                            if let Some(position) = self.local_drawing.last_cursor_position {
                                if let Some(remote_control) = &mut self.remote_control {
                                    self.local_drawing.current_path_id += 1;
                                    remote_control.gfx.draw_start(
                                        "local",
                                        position,
                                        self.local_drawing.current_path_id,
                                    );
                                    remote_control.gfx.trigger_render();

                                    // Send LiveKit event
                                    if let Some(room_service) = &self.room_service {
                                        room_service.publish_draw_start(
                                            room_service::DrawPathPoint {
                                                point: room_service::ClientPoint {
                                                    x: position.x,
                                                    y: position.y,
                                                },
                                                path_id: self.local_drawing.current_path_id,
                                            },
                                        );
                                    }

                                    log::debug!(
                                        "Local draw_start at {:?} with path_id {}",
                                        position,
                                        self.local_drawing.current_path_id
                                    );
                                }
                            }
                        } else {
                            self.local_drawing.left_mouse_pressed = false;
                            // End the current path
                            if let Some(position) = self.local_drawing.last_cursor_position {
                                if let Some(remote_control) = &mut self.remote_control {
                                    remote_control.gfx.draw_end("local", position);
                                    remote_control.gfx.trigger_render();

                                    // Send LiveKit event
                                    if let Some(room_service) = &self.room_service {
                                        room_service.publish_draw_end(room_service::ClientPoint {
                                            x: position.x,
                                            y: position.y,
                                        });
                                    }

                                    log::debug!("Local draw_end at {:?}", position);
                                }
                            }
                        }
                    } else if button == winit::event::MouseButton::Right
                        && state == winit::event::ElementState::Pressed
                    {
                        if let Some(remote_control) = &mut self.remote_control {
                            // Clear all local drawing paths
                            remote_control.gfx.draw_clear_all_paths("local");
                            remote_control.gfx.trigger_render();

                            // Send LiveKit event to clear all paths
                            if let Some(room_service) = &self.room_service {
                                room_service.publish_draw_clear_all_paths();
                            }
                            log::debug!("Local draw_clear_all_paths on right click");
                        }
                    }
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                if self.local_drawing.enabled {
                    // Convert physical position to percentage (0.0–1.0)
                    let pos = if let Some(remote_control) = &self.remote_control {
                        let overlay_window = remote_control.cursor_controller.get_overlay_window();
                        let scale = overlay_window.get_display_scale();
                        overlay_window
                            .get_local_percentage_from_pixel(position.x / scale, position.y / scale)
                    } else {
                        Position {
                            x: position.x,
                            y: position.y,
                        }
                    };
                    self.local_drawing.last_cursor_position = Some(pos);

                    // If we're actively drawing, add a point
                    if self.local_drawing.left_mouse_pressed {
                        if let Some(remote_control) = &mut self.remote_control {
                            remote_control.gfx.draw_add_point("local", pos);
                            remote_control.gfx.trigger_render();

                            // Send LiveKit event
                            if let Some(room_service) = &self.room_service {
                                room_service.publish_draw_add_point(room_service::ClientPoint {
                                    x: pos.x,
                                    y: pos.y,
                                });
                            }
                        }
                    }
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if self.local_drawing.enabled && event.state == winit::event::ElementState::Pressed
                {
                    if let winit::keyboard::Key::Named(winit::keyboard::NamedKey::Escape) =
                        event.logical_key
                    {
                        // Disable drawing mode
                        let _ = self
                            .event_loop_proxy
                            .send_event(UserEvent::LocalDrawingEnabled(
                                socket_lib::DrawingEnabled { permanent: false },
                            ));
                        log::debug!("Escape pressed, disabling local drawing");
                    }
                }
            }
            WindowEvent::Resized(_size) => {
                if let Some(wm) = self.window_manager.as_mut() {
                    if wm.is_active_window(window_id) {
                        log::info!("window_event: active window resized {:?}", window_id);
                        if let Err(e) = wm.update(event_loop) {
                            log::error!("window_event: failed to update window manager: {:?}", e);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        // We use explicitly WaitUntil to avoid busy-looping
        // and super high CPU usage.
        // Source: https://stackoverflow.com/a/76105294
        use winit::event_loop::ControlFlow;

        let now = std::time::Instant::now();
        let mut next_redraw: Option<std::time::Instant> = None;

        // Handle camera window
        if let Some(camera) = &self.camera_window {
            let camera_next = camera.next_redraw_at();
            if now >= camera_next {
                camera.request_redraw();
            }
            next_redraw = Some(camera_next);
        }

        // Handle screensharing window
        if let Some(screensharing) = &self.screensharing_window {
            let ss_next = screensharing.next_redraw_at();
            if now >= ss_next {
                screensharing.request_redraw();
            }
            next_redraw = match next_redraw {
                Some(existing) => Some(existing.min(ss_next)),
                None => Some(ss_next),
            };
        }

        // Handle stats window
        if let Some(stats_win) = &mut self.stats_window {
            if let Some(rs) = &self.room_service {
                stats_win.update_stats(rs);
            }
            let sw_next = stats_win.next_redraw_at();
            if now >= sw_next {
                stats_win.request_redraw();
            }
            next_redraw = match next_redraw {
                Some(existing) => Some(existing.min(sw_next)),
                None => Some(sw_next),
            };
        }

        // Set control flow based on earliest next redraw
        if let Some(next) = next_redraw {
            event_loop.set_control_flow(ControlFlow::WaitUntil(next));
        } else {
            event_loop.set_control_flow(ControlFlow::Wait);
        }
    }
}

#[derive(Debug, Clone)]
pub struct KeystrokeData {
    key: String,
    meta: bool,
    shift: bool,
    ctrl: bool,
    alt: bool,
    down: bool,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct MouseClickData {
    x: f32,
    y: f32,
    button: u32,
    clicks: f32,
    down: bool,
    shift: bool,
    alt: bool,
    ctrl: bool,
    meta: bool,
}

#[derive(Debug, Clone)]
pub struct ParticipantData {
    pub name: String,
    pub identity: String,
    pub sid: String,
}

#[derive(Debug, Clone)]
pub enum UserEvent {
    CursorPosition(f32, f32, String),
    MouseClick(MouseClickData, String),
    ControllerCursorEnabled(bool),
    ControllerCursorVisible(bool, String),
    Keystroke(KeystrokeData),
    Scroll(ScrollDelta, String),
    GetAvailableContent,
    Terminate,
    CallStart(CallStartMessage),
    CallEnd,
    ScreenShare(ScreenShareMessage),
    StopScreenShare,
    ScreenShareFrameReady,
    RequestRedraw,
    SharerPosition(f64, f64),
    Tick(u128),
    ParticipantConnected(ParticipantData),
    ParticipantDisconnected(ParticipantData),
    ParticipantsSnapshot(Vec<socket_lib::CoreParticipantState>),
    LivekitServerUrl(String),
    ControllerTakesScreenShare,
    ParticipantInControl(String),
    LocalParticipantInControl(bool),
    SentryMetadata(SentryMetadata),
    AddToClipboard(room_service::AddToClipboardData),
    PasteFromClipboard(room_service::PasteFromClipboardData),
    DrawingMode(room_service::DrawingMode, String),
    DrawStart(room_service::ClientPoint, u64, String),
    DrawAddPoint(room_service::ClientPoint, String),
    DrawEnd(room_service::ClientPoint, String),
    DrawClearPath(u64, String),
    DrawClearAllPaths(String),
    ClickAnimationFromParticipant(room_service::ClientPoint, String),
    LocalDrawingEnabled(socket_lib::DrawingEnabled),
    ListAudioDevices,
    StartAudioCapture(socket_lib::AudioCaptureMessage),
    StopAudioCapture,
    MuteAudio,
    UnmuteAudio,
    ToggleMic,
    ListCameras,
    StartCamera(CameraStartMessage),
    StopCamera,
    OpenCamera,
    OpenScreensharing,
    OpenContentPicker,
    OpenScreenShareWindow {
        sid: Option<String>,
        name: Option<String>,
    },
    CloseScreenShareWindow,
    CloseCameraWindow,
    BringWindowsToFront,
    SharerControlEnabled(bool),
    DefaultOutputDeviceChanged,
    DefaultInputDeviceChanged,
}

pub struct RenderEventLoop {
    pub event_loop: EventLoop<UserEvent>,
}

pub struct RenderLoopRunArgs {
    pub textures_path: String,
}

impl fmt::Display for RenderLoopRunArgs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Textures path: {}", self.textures_path)
    }
}

#[derive(Error, Debug)]
pub enum RenderLoopError {
    #[error("Socket operation failed: {0}")]
    SocketError(#[from] std::io::Error),
    #[error("Event loop error: {0}")]
    EventLoopError(#[from] EventLoopError),
    #[error("Failed to create application: {0}")]
    ApplicationError(#[from] ApplicationError),
    #[error("Failed to get livekit server url")]
    LivekitServerUrlError,
}

impl Default for RenderEventLoop {
    fn default() -> Self {
        Self::new()
    }
}

impl RenderEventLoop {
    pub fn new() -> Self {
        let mut event_loop = EventLoop::<UserEvent>::with_user_event();

        #[cfg(target_os = "macos")]
        event_loop.with_activation_policy(winit::platform::macos::ActivationPolicy::Accessory);

        /* This is the beginning of the app, if this fails we can panic. */
        let event_loop = event_loop.build().expect("Failed to build event loop");

        Self { event_loop }
    }

    pub fn run(self, input: RenderLoopRunArgs, socket_path: String) -> Result<(), RenderLoopError> {
        log::info!("Starting RenderEventLoop");

        log::info!("Creating socket at path: {socket_path}");
        let (sender, event_socket) = socket_lib::listen(&socket_path).map_err(|e| {
            log::error!("Error creating socket: {e:?}");
            RenderLoopError::SocketError(e)
        })?;

        let event_loop_proxy = self.event_loop.create_proxy();
        /*
         * Thread for dispatching socket events to the winit event loop.
         */
        std::thread::spawn(move || {
            loop {
                let message =
                    match event_socket
                        .events
                        .recv_timeout(std::time::Duration::from_secs(
                            SOCKET_MESSAGE_TIMEOUT_SECONDS,
                        )) {
                        Ok(msg) => msg,
                        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                            log::error!(
                                "RenderEventLoop::run Socket message timeout, terminating."
                            );
                            let res = event_loop_proxy.send_event(UserEvent::Terminate);
                            if res.is_err() {
                                log::error!(
                                    "RenderEventLoop::run Error sending terminate event: {:?}",
                                    res.err()
                                );
                            }
                            std::thread::sleep(std::time::Duration::from_secs(1));
                            std::process::exit(PROCESS_EXIT_CODE_ERROR);
                        }
                        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                            log::error!(
                                "RenderEventLoop::run Socket event channel closed, terminating."
                            );
                            let res = event_loop_proxy.send_event(UserEvent::Terminate);
                            if res.is_err() {
                                log::error!(
                                    "RenderEventLoop::run Error sending terminate event: {:?}",
                                    res.err()
                                );
                            }
                            std::thread::sleep(std::time::Duration::from_secs(1));
                            std::process::exit(PROCESS_EXIT_CODE_ERROR);
                        }
                    };
                let user_event = match message {
                    Message::GetAvailableContent => UserEvent::GetAvailableContent,
                    Message::CallStart(call_start_message) => {
                        UserEvent::CallStart(call_start_message)
                    }
                    Message::CallEnd => UserEvent::CallEnd,
                    Message::StartScreenShare(screen_share_message) => {
                        UserEvent::ScreenShare(screen_share_message)
                    }
                    Message::StopScreenshare => UserEvent::StopScreenShare,
                    Message::ControllerCursorEnabled(enabled) => {
                        UserEvent::ControllerCursorEnabled(enabled)
                    }
                    Message::DrawingEnabled(permanent) => UserEvent::LocalDrawingEnabled(permanent),
                    Message::ListAudioDevices => UserEvent::ListAudioDevices,
                    Message::StartAudioCapture(msg) => UserEvent::StartAudioCapture(msg),
                    Message::StopAudioCapture => UserEvent::StopAudioCapture,
                    Message::MuteAudio => UserEvent::MuteAudio,
                    Message::UnmuteAudio => UserEvent::UnmuteAudio,
                    Message::ToggleMic => UserEvent::ToggleMic,
                    Message::ListCameras => UserEvent::ListCameras,
                    Message::StartCamera(msg) => UserEvent::StartCamera(msg),
                    Message::StopCamera => UserEvent::StopCamera,
                    Message::OpenCamera => UserEvent::OpenCamera,
                    Message::OpenScreensharing => UserEvent::OpenScreensharing,
                    Message::OpenScreenShareWindow => UserEvent::OpenScreenShareWindow {
                        sid: None,
                        name: None,
                    },
                    Message::CloseScreenShareWindow => UserEvent::CloseScreenShareWindow,
                    Message::BringWindowsToFront => UserEvent::BringWindowsToFront,
                    // Ping is on purpose empty. We use it only for keeping the connection alive.
                    Message::Ping => {
                        continue;
                    }
                    Message::LivekitServerUrl(url) => UserEvent::LivekitServerUrl(url),
                    Message::SentryMetadata(sentry_metadata) => {
                        UserEvent::SentryMetadata(sentry_metadata)
                    }
                    _ => {
                        log::error!("RenderEventLoop::run Unknown message: {message:?}");
                        continue;
                    }
                };
                let res = event_loop_proxy.send_event(user_event);
                if res.is_err() {
                    log::error!(
                        "RenderEventLoop::run Error sending user event: {:?}",
                        res.err()
                    );
                }
            }
        });

        let proxy = self.event_loop.create_proxy();
        let mut application = Application::new(input, sender, proxy)?;
        self.event_loop.run_app(&mut application).map_err(|e| {
            log::error!("Error running application: {e:?}");
            RenderLoopError::EventLoopError(e)
        })
    }
}
