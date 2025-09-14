pub mod room_service;

pub mod input {
    pub mod keyboard;
    pub mod mouse;
}

pub mod capture {
    pub mod capturer;
}

pub mod graphics {
    pub mod graphics_context;

    #[cfg(target_os = "windows")]
    pub mod direct_composition;
}

pub mod utils {
    pub mod geometry;
    pub mod svg_renderer;
}

pub(crate) mod overlay_window;

use capture::capturer::{poll_stream, Capturer};
use graphics::graphics_context::GraphicsContext;
use input::keyboard::{KeyboardController, KeyboardLayout};
use input::mouse::CursorController;
use log::{debug, error};
use overlay_window::OverlayWindow;
use room_service::RoomService;
use socket_lib::{
    AvailableContentMessage, CaptureContent, CursorSocket, Message, ScreenShareMessage,
    SentryMetadata,
};
use std::fmt;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use thiserror::Error;
use utils::geometry::{Extent, Frame};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalPosition, LogicalSize};
use winit::error::EventLoopError;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop, EventLoopProxy};
use winit::monitor::MonitorHandle;

#[cfg(target_os = "macos")]
use winit::platform::macos::{EventLoopBuilderExtMacOS, WindowExtMacOS};

#[cfg(target_os = "windows")]
use winit::platform::windows::WindowExtWindows;

use winit::window::{WindowAttributes, WindowLevel};

use crate::overlay_window::DisplayInfo;

// Constants for magic numbers
/// Initial size for the overlay window (width and height in logical pixels)
const OVERLAY_WINDOW_INITIAL_SIZE: f64 = 1.0;

/// Timeout in seconds for socket message reception
const SOCKET_MESSAGE_TIMEOUT_SECONDS: u64 = 30;

/// Process exit code for errors
const PROCESS_EXIT_CODE_ERROR: i32 = 1;
const STREAM_FAILURE_EXIT_CODE: i32 = 2;

#[derive(Error, Debug)]
pub enum ServerError {
    #[error("Livekit room service not found")]
    RoomServiceNotFound,
    #[error("Failed to create Livekit room")]
    RoomCreationError,
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

pub fn get_window_attributes() -> WindowAttributes {
    WindowAttributes::default()
        .with_title("Overlay window")
        .with_window_level(WindowLevel::AlwaysOnTop)
        .with_decorations(false)
        .with_transparent(true)
        .with_inner_size(LogicalSize::new(
            OVERLAY_WINDOW_INITIAL_SIZE,
            OVERLAY_WINDOW_INITIAL_SIZE,
        ))
        .with_content_protected(true)
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
    textures_path: String,
    // The arc is needed because we move the object to the
    // thread that checks if the stream has failed.
    //screen_capturer: Arc<Mutex<ScreenCapturer>>,
    screen_capturer: Arc<Mutex<Capturer>>,
    _screen_capturer_events: Option<JoinHandle<()>>,
    socket: CursorSocket,
    room_service: Option<RoomService>,
    event_loop_proxy: EventLoopProxy<UserEvent>,
}

#[derive(Error, Debug)]
pub enum ApplicationError {
    #[error("Failed to create room service: {0}")]
    RoomServiceError(#[from] std::io::Error),
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
        socket: CursorSocket,
        event_loop_proxy: EventLoopProxy<UserEvent>,
    ) -> Result<Self, ApplicationError> {
        let screencapturer = Arc::new(Mutex::new(Capturer::new(event_loop_proxy.clone())));

        Ok(Self {
            remote_control: None,
            textures_path: input.textures_path,
            screen_capturer: screencapturer.clone(),
            _screen_capturer_events: Some(std::thread::spawn(move || poll_stream(screencapturer))),
            socket,
            room_service: None,
            event_loop_proxy,
        })
    }

    fn get_available_content(&mut self) -> Vec<CaptureContent> {
        let mut screen_capturer = self.screen_capturer.lock().unwrap();
        let res = screen_capturer.get_available_content();

        if let Err(e) = res {
            log::error!("get_available_content: Error getting available content: {e:?}");
            return vec![];
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
        screenshare_input: ScreenShareMessage,
        monitors: Vec<MonitorHandle>,
        event_loop: &ActiveEventLoop,
    ) -> Result<(), ServerError> {
        log::info!(
            "screenshare: resolution: {:?} content: {} accessibility_permission: {}",
            screenshare_input.resolution,
            screenshare_input.content,
            screenshare_input.accessibility_permission
        );
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
        let res = room_service.create_room(
            screenshare_input.token,
            extent.width as u32,
            extent.height as u32,
            self.event_loop_proxy.clone(),
        );
        if let Err(error) = res {
            log::error!("screenshare: error creating room: {error:?}");
            drop(screen_capturer);
            self.stop_screenshare();
            return Err(ServerError::RoomCreationError);
        }
        log::info!("screenshare: room created");

        let buffer_source = room_service.get_buffer_source();
        screen_capturer.set_buffer_source(buffer_source);

        let monitor = screen_capturer.get_selected_monitor(&monitors, screenshare_input.content.id);
        drop(screen_capturer);

        let res = self.create_overlay_window(
            monitor,
            event_loop,
            screenshare_input.accessibility_permission,
        );
        if let Err(e) = res {
            self.stop_screenshare();
            log::error!("screenshare: error creating overlay window: {e:?}");
            return Err(e);
        }

        /* We want to add the participants that already exist in the cursor controller list. */
        self.room_service.as_ref().unwrap().iterate_participants();

        Ok(())
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
        if let Some(room_service) = self.room_service.as_mut() {
            room_service.destroy_room();
        }
        drop(screen_capturer);
        self.destroy_overlay_window();
    }

    fn create_overlay_window(
        &mut self,
        selected_monitor: MonitorHandle,
        event_loop: &ActiveEventLoop,
        accessibility_permission: bool,
    ) -> Result<(), ServerError> {
        log::info!("create_overlay_window: selected_monitor: {selected_monitor:?} {accessibility_permission}",);
        let attributes = get_window_attributes();
        let window = match event_loop.create_window(attributes) {
            Ok(window) => window,
            Err(_error) => {
                return Err(ServerError::WindowCreationError);
            }
        };

        #[cfg(target_os = "linux")]
        {
            /* This is needed for getting the system picker for screen sharing. */
            let _ = window.request_inner_size(selected_monitor.size().clone());
        }

        let res = window.set_cursor_hittest(false);
        if let Err(_error) = res {
            return Err(ServerError::CursorHittestError);
        }

        #[cfg(target_os = "windows")]
        {
            window.set_skip_taskbar(true);
        }

        #[cfg(target_os = "macos")]
        {
            window.set_has_shadow(false);
        }

        window.set_visible(true);
        let monitor_position = selected_monitor.position();
        window.set_outer_position(LogicalPosition::new(monitor_position.x, monitor_position.y));

        let res = set_fullscreen(&window, selected_monitor.clone());
        if let Err(error) = res {
            log::error!("create_overlay_window: Error setting fullscreen {error:?}");
            return Err(ServerError::FullscreenError);
        }

        let window_position = match window.outer_position() {
            Ok(position) => position,
            Err(error) => {
                log::error!("create_overlay_window: Error getting window position {error:?} using monitor's");
                selected_monitor.position()
            }
        };

        let window_size = window.inner_size();

        let mut graphics_context = match GraphicsContext::new(
            window,
            self.textures_path.clone(),
            selected_monitor.scale_factor(),
        ) {
            Ok(context) => context,
            Err(error) => {
                log::error!("create_overlay_window: Error creating graphics context {error:?}");
                return Err(ServerError::GfxCreationError);
            }
        };

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

        let cursor_controller = CursorController::new(
            &mut graphics_context,
            overlay_window.clone(),
            self.event_loop_proxy.clone(),
            accessibility_permission,
        );
        if let Err(error) = cursor_controller {
            log::error!("create_overlay_window: Error creating cursor controller {error:?}");
            return Err(ServerError::CursorControllerCreationError);
        }

        self.remote_control = Some(RemoteControl {
            gfx: graphics_context,
            cursor_controller: cursor_controller.unwrap(),
            keyboard_controller: KeyboardController::<KeyboardLayout>::new(),
        });

        #[cfg(target_os = "linux")]
        {
            /* We can't support the overlay surface on linux yet. */
            self.remote_control = None;
        }

        Ok(())
    }

    fn destroy_overlay_window(&mut self) {
        log::info!("destroy_overlay_window");
        self.remote_control = None;
    }

    /// Resets the application state after a session ends or encounters an error.
    ///
    /// This method performs comprehensive cleanup and state reset:
    /// - Stops active screen sharing sessions
    /// - Destroys overlay windows
    /// - Cleans up LiveKit room
    /// - Restarts screen capturer if needed
    /// - Uploads telemetry data to monitoring systems
    ///
    /// # Usage
    ///
    /// This function is called when:
    /// - The user ends a remote desktop session
    /// - An error occurs that requires session reset
    /// - The client disconnects unexpectedly
    ///
    /// # Error Handling
    ///
    /// If the screen capturer is in an invalid state, this method will:
    /// 1. Perform manual cleanup of overlay window and room service
    /// 2. Create a new screen capturer instance
    /// 3. Restart the capture event polling thread
    ///
    /// # Side Effects
    ///
    /// - Uploads "Ending call" event to Sentry for telemetry
    /// - May create new threads for screen capture polling
    /// - Resets all session-specific state to initial values
    fn reset_state(&mut self) {
        let capturer_valid = {
            let screen_capturer = self.screen_capturer.lock();
            screen_capturer.is_ok()
        };
        if capturer_valid {
            self.stop_screenshare();
        } else {
            log::warn!("reset_state: Screen capturer is not valid");
            self.destroy_overlay_window();
            if let Some(room_service) = self.room_service.as_mut() {
                room_service.destroy_room();
            }

            /* Restart the screen capturer. */
            self.screen_capturer =
                Arc::new(Mutex::new(Capturer::new(self.event_loop_proxy.clone())));
            let screen_capturer_clone = self.screen_capturer.clone();

            /*
             * The previous screen capturer is invalid so we can stop the polling thread,
             * this should be unlikely to happen to happen.
             * Therefore we can have thread running but not doing anything.
             */
            self._screen_capturer_events = Some(std::thread::spawn(move || {
                poll_stream(screen_capturer_clone)
            }));
        }

        // Upload logs to sentry when ending call.
        sentry_utils::upload_logs_event("Ending call".to_string());
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
                if self.remote_control.is_none() {
                    log::warn!("user_event: remote control is none cursor position");
                    return;
                }
                let remote_control = &mut self.remote_control.as_mut().unwrap();
                remote_control.cursor_controller.cursor_move_controller(
                    x as f64,
                    y as f64,
                    sid.as_str(),
                );
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
                cursor_controller.set_controller_visible(visible, sid.as_str());
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
                let content = self.get_available_content();
                if content.is_empty() {
                    log::error!("user_event: No available content");
                    sentry_utils::upload_logs_event("No available content".to_string());
                }
                let res =
                    self.socket
                        .send_message(Message::AvailableContent(AvailableContentMessage {
                            content,
                        }));
                if res.is_err() {
                    log::error!(
                        "user_event: Error sending available content: {:?}",
                        res.err()
                    );
                }
            }
            UserEvent::ScreenShare(data) => {
                log::debug!("user_event: Screen share: {data:?}");
                let monitors = event_loop
                    .available_monitors()
                    .collect::<Vec<MonitorHandle>>();

                let result_message = match self.screenshare(data, monitors, event_loop) {
                    Ok(_) => Ok(()),
                    Err(e) => {
                        log::error!("user_event: Screen share failed: {e:?}");
                        sentry_utils::upload_logs_event("Screen share failed".to_string());
                        Err(e.to_string())
                    }
                };

                if let Err(e) = self
                    .socket
                    .send_message(Message::StartScreenShareResult(result_message))
                {
                    error!("user_event: Error sending start screen share result: {e:?}");
                }
            }
            UserEvent::StopScreenShare => {
                self.stop_screenshare();
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
                    .publish_sharer_location(x, y, true);
            }
            UserEvent::ResetState => {
                debug!("user_event: Resetting state");
                self.reset_state();
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
                if self.remote_control.is_none() {
                    log::warn!("user_event: remote control is none participant connected");
                    return;
                }
                let remote_control = &mut self.remote_control.as_mut().unwrap();
                if let Err(e) = remote_control.cursor_controller.add_controller(
                    &mut remote_control.gfx,
                    participant.sid,
                    participant.name,
                ) {
                    log::error!(
                        "user_event: Participant connected: Error adding controller: {e:?}"
                    );
                }
            }
            UserEvent::ParticipantDisconnected(participant) => {
                log::debug!("user_event: Participant disconnected: {participant:?}");
                if self.remote_control.is_none() {
                    log::warn!("user_event: remote control is none participant disconnected");
                    return;
                }
                let remote_control = &mut self.remote_control.as_mut().unwrap();
                remote_control
                    .cursor_controller
                    .remove_controller(participant.sid.as_str());
            }
            UserEvent::LivekitServerUrl(url) => {
                log::debug!("user_event: Livekit server url: {url}");
                let room_service = RoomService::new(url, self.event_loop_proxy.clone());
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
            UserEvent::SentryMetadata(sentry_metadata) => {
                log::debug!("user_event: Sentry metadata: {sentry_metadata:?}");
                sentry_utils::init_metadata(
                    sentry_metadata.user_email,
                    sentry_metadata.app_version,
                );
            }
        }
    }

    fn resumed(&mut self, _event_loop: &ActiveEventLoop) {}

    // Once we get movement input from guest, we will call Window::request_redraw
    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: winit::window::WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => {
                event_loop.exit();
            }
            WindowEvent::RedrawRequested => {
                // render the cursor
                // The vertices should be in counter clockwise order because of the front face culling
                if self.remote_control.is_none() {
                    log::warn!("window_event: remote control is none redraw requested");
                    return;
                }
                let remote_control = &mut self.remote_control.as_mut().unwrap();
                let gfx = &mut remote_control.gfx;
                let cursor_controller = &mut remote_control.cursor_controller;
                gfx.draw(cursor_controller);
            }
            _ => {}
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
    ScreenShare(ScreenShareMessage),
    StopScreenShare,
    RequestRedraw,
    SharerPosition(f64, f64),
    ResetState,
    Tick(u128),
    ParticipantConnected(ParticipantData),
    ParticipantDisconnected(ParticipantData),
    LivekitServerUrl(String),
    ControllerTakesScreenShare,
    ParticipantInControl(String),
    SentryMetadata(SentryMetadata),
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
        let mut socket = CursorSocket::new_create(&socket_path).map_err(|e| {
            log::error!("Error creating socket: {e:?}");
            RenderLoopError::SocketError(e)
        })?;
        let socket_clone = socket.duplicate().map_err(|e| {
            log::error!("Error duplicating socket: {e:?}");
            RenderLoopError::SocketError(e)
        })?;

        let event_loop_proxy = self.event_loop.create_proxy();
        /*
         * Thread for processing messages from the tauri app.
         */
        std::thread::spawn(move || loop {
            let message = match socket.receive_message_with_timeout(std::time::Duration::from_secs(
                SOCKET_MESSAGE_TIMEOUT_SECONDS,
            )) {
                Ok(message) => message,
                Err(e) => {
                    /* When the listener has been disconnected we terminate the process. */
                    log::error!("RenderEventLoop::run Error receiving message: {e:?}");
                    let res = event_loop_proxy.send_event(UserEvent::Terminate);
                    if res.is_err() {
                        log::error!(
                            "RenderEventLoop::run Error sending terminate event: {:?}",
                            res.err()
                        );
                    }

                    /* We want to make sure the process is terminated. */
                    std::thread::sleep(std::time::Duration::from_secs(1));
                    std::process::exit(PROCESS_EXIT_CODE_ERROR);
                }
            };
            let user_event = match message {
                Message::GetAvailableContent => UserEvent::GetAvailableContent,
                Message::StartScreenShare(screen_share_message) => {
                    UserEvent::ScreenShare(screen_share_message)
                }
                Message::StopScreenshare => UserEvent::StopScreenShare,
                Message::Reset => UserEvent::ResetState,
                Message::ControllerCursorEnabled(enabled) => {
                    UserEvent::ControllerCursorEnabled(enabled)
                }
                // Ping is on purpose empty. We use it only for stopping the above receive to timeout.
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
        });

        let proxy = self.event_loop.create_proxy();
        let mut application = Application::new(input, socket_clone, proxy)?;
        self.event_loop.run_app(&mut application).map_err(|e| {
            log::error!("Error running application: {e:?}");
            RenderLoopError::EventLoopError(e)
        })
    }
}

#[derive(Error, Debug)]
enum FullscreenError {
    #[error("Failed to get raw window handle")]
    #[cfg(target_os = "macos")]
    GetRawWindowHandleError,
    #[error("Failed to get NSView")]
    #[cfg(target_os = "macos")]
    GetNSViewError,
    #[error("Failed to get NSWindow")]
    #[cfg(target_os = "macos")]
    GetNSWindowError,
    #[error("Failed to get raw window handle")]
    #[cfg(target_os = "macos")]
    FailedToGetRawWindowHandle,
}

fn set_fullscreen(
    window: &winit::window::Window,
    selected_monitor: MonitorHandle,
) -> Result<(), FullscreenError> {
    log::info!("set_fullscreen: {selected_monitor:?}");
    #[cfg(target_os = "macos")]
    {
        /* WA for putting the window in the right place. */
        window.set_maximized(true);
        window.set_simple_fullscreen(true);

        use objc2::rc::Retained;
        use objc2_app_kit::NSMainMenuWindowLevel;
        use objc2_app_kit::NSView;
        use raw_window_handle::HasWindowHandle;
        use raw_window_handle::RawWindowHandle;

        let raw_handle = window
            .window_handle()
            .map_err(|_| FullscreenError::GetRawWindowHandleError)?;
        if let RawWindowHandle::AppKit(handle) = raw_handle.as_raw() {
            let view = handle.ns_view.as_ptr();
            let ns_view: Option<Retained<NSView>> = unsafe { Retained::retain(view.cast()) };
            if ns_view.is_none() {
                return Err(FullscreenError::GetNSViewError);
            }
            let ns_view = ns_view.unwrap();
            let ns_window = ns_view.window();
            if ns_window.is_none() {
                return Err(FullscreenError::GetNSWindowError);
            }
            let ns_window = ns_window.unwrap();
            /* This is a hack to make the overlay window to appear above the main menu. */
            ns_window.setLevel(NSMainMenuWindowLevel + 1);
            return Ok(());
        }
        Err(FullscreenError::FailedToGetRawWindowHandle)
    }
    #[cfg(any(target_os = "windows", target_os = "linux"))]
    {
        use winit::window::Fullscreen;

        window.set_fullscreen(Some(Fullscreen::Borderless(Some(selected_monitor))));

        Ok(())
    }
}
