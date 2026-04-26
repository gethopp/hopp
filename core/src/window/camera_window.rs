//! Camera window with interactive Iced UI rendered via wgpu.
//!
//! This module implements a standalone window for the camera/video call view,
//! using winit for the window, wgpu for the GPU surface, and iced for
//! the interactive widget tree (buttons, text, layout).
//!
//! UI styling is ported from the iced-poc video call window:
//! - Geist font family (Regular + Medium)
//! - Tailwind color tokens (Slate, Gray, Green, Orange, Red, Lime)
//! - Shadow tokens for consistent depth
//! - Header control buttons via `split_button` (solid colors + optional dropdowns)
//! - Responsive participant grid with name labels and speaking indicators

use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::thread::JoinHandle;
use std::time::Duration;

use iced::widget::{
    button, column, container, mouse_area, row, shader, stack, svg, text, tooltip, Space,
};
use iced::{
    gradient, Alignment, Background, Border, Color, Length, Padding, Pixels, Radians, Shadow,
    Size as IcedSize,
};
use iced_wgpu::core::mouse;
use iced_wgpu::graphics::Viewport;
use iced_winit::core::renderer::Style;
use iced_winit::core::time::Instant;
use iced_winit::core::{window, Event, Size, Theme};
use iced_winit::runtime::user_interface::Cache;
use iced_winit::runtime::UserInterface;
use iced_winit::{conversion, Clipboard};
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoopProxy};
use winit::keyboard::ModifiersState;
use winit::window::{Window, WindowAttributes, WindowId};

use thiserror::Error;

use crate::audio::capturer::list_audio_inputs;
use crate::camera::capturer::CameraCapturer;
use crate::components::fonts::{self as fonts_mod, GEIST_MEDIUM, GEIST_REGULAR, ICONS_FONT};
use crate::components::split_button::{split_button, split_button_dropdown_wrap, SplitButtonItem};
use crate::components::toast::{self, ToastPosition, ToastState};
use crate::graphics::graphics_window_context::{ContextManager, GraphicsWindowContextError};
use crate::graphics::yuv_renderer::YuvVideoProgram;
use crate::livekit::participant::ParticipantInfo;
use crate::livekit::video::VideoBufferManager;
use crate::windows::colors::ColorToken;
use crate::windows::shadows::ShadowToken;
use crate::UserEvent;
use socket_lib::CameraStartMessage;

/// Initial camera window dimensions (logical pixels).
const CAMERA_WINDOW_WIDTH: f64 = 1035.0;
const CAMERA_WINDOW_HEIGHT: f64 = 555.0;

/// Minimum camera window dimensions.
const CAMERA_WINDOW_MIN_WIDTH: f64 = 100.0;
const CAMERA_WINDOW_MIN_HEIGHT: f64 = 100.0;

use std::sync::mpsc;

/// Target redraw interval: 30 FPS
const REDRAW_INTERVAL: Duration = Duration::from_millis(1_000 / 30);

pub enum RedrawCommand {
    Stop,
}

fn spawn_redraw_thread(
    redraw_rx: mpsc::Receiver<RedrawCommand>,
    window: Arc<Window>,
) -> JoinHandle<()> {
    std::thread::spawn(move || loop {
        match redraw_rx.recv_timeout(REDRAW_INTERVAL) {
            Ok(RedrawCommand::Stop) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => window.request_redraw(),
        }
    })
}

const CONTENT_PADDING: f32 = 12.0;

// Grid layout constants
const MIN_TILE_SIZE: f32 = 80.0;
const TILE_SPACING: f32 = 16.0;
const MIN_GRID_PADDING: f32 = 8.0;

// Header height: split_button (20px) + padding top (12)
const HEADER_HEIGHT: f32 = 20.0 + CONTENT_PADDING;

// Window size thresholds for small window styling
const SMALL_WIDTH_THRESHOLD: f32 = 450.0;
const SMALL_HEIGHT_THRESHOLD: f32 = 600.0;
const COMPACT_WIDTH_THRESHOLD: f32 = 300.0;
const HIDE_NAME_TILE_THRESHOLD: f32 = 150.0;

const COMPACT_TOP_INSET: f32 = 24.0;

const ICON_MICROPHONE_ON: char = '\u{F105}';
const ICON_MICROPHONE_OFF: char = '\u{F106}';
const ICON_SCREEN_SHARE: char = '\u{F102}';
const ICON_VIDEO: char = '\u{F101}';
const ICON_PHONE_OFF: char = '\u{F103}';

const ICON_EYE_ON_SVG: &[u8] = include_bytes!("../../resources/icons/EyeOn.svg");
const ICON_EYE_OFF_SVG: &[u8] = include_bytes!("../../resources/icons/EyeOff.svg");

const AVATAR_SIZE: f32 = 130.0;
const AVATAR_RADIUS: f32 = 20.0;
const AVATAR_FONT_SIZE: f32 = 42.0;
const AVATAR_LETTER_SPACING: f32 = -3.0;

#[derive(Error, Debug)]
pub enum CameraWindowError {
    #[error("Failed to create window")]
    WindowCreation,
    #[error("Failed to create wgpu surface")]
    SurfaceCreation,
    #[error("No suitable GPU adapter found")]
    AdapterRequest,
    #[error("Failed to request GPU device")]
    DeviceRequest,
}

#[derive(Debug, Clone)]
pub enum CameraMessage {
    MicToggle,
    ScreenShare,
    VideoToggle,
    EndCall,
    ToggleSelfVisibility,
    /// Mouse entered or left the local participant tile (for hover-only chrome).
    LocalTileHover(bool),
    CameraDropdownToggle,
    CameraDropdownDismiss,
    SelectCamera(String),
    MicDropdownToggle,
    MicDropdownDismiss,
    SelectMic(String),
}

struct CameraState {
    // TODO: why do we keep state for this instead of reusing the viewport_size from the CameraWindow?
    viewport_size: IcedSize,
    /// Local camera on/off state, updated from StartCamera/StopCamera handlers.
    camera_active: bool,
    /// When true, the local participant tile is hidden (floating control restores it).
    self_hidden: bool,
    /// True while the pointer is over the local tile (show hide-self control).
    local_tile_hovered: bool,
    /// Window narrower than `COMPACT_WIDTH_THRESHOLD` hides header, name labels, etc.
    is_compact: bool,
    /// Last min-height applied via `set_min_inner_size` (used to avoid redundant calls).
    compact_min_height: f64,
    toast: Option<ToastState>,
    camera_dropdown_open: bool,
    available_cameras: Vec<socket_lib::CameraDevice>,
    selected_camera_name: Option<String>,
    mic_dropdown_open: bool,
    available_mics: Vec<(String, bool)>,
    selected_mic_name: Option<String>,
}

impl Default for CameraState {
    fn default() -> Self {
        Self {
            viewport_size: IcedSize::new(CAMERA_WINDOW_WIDTH as f32, CAMERA_WINDOW_HEIGHT as f32),
            camera_active: false,
            self_hidden: false,
            local_tile_hovered: false,
            is_compact: false,
            compact_min_height: CAMERA_WINDOW_MIN_HEIGHT,
            toast: None,
            camera_dropdown_open: false,
            available_cameras: Vec::new(),
            selected_camera_name: None,
            mic_dropdown_open: false,
            available_mics: Vec::new(),
            selected_mic_name: None,
        }
    }
}

// ── CameraWindow ────────────────────────────────────────────────────────────

pub struct CameraWindow {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    format: wgpu::TextureFormat,
    alpha_mode: wgpu::CompositeAlphaMode,
    renderer: iced::Renderer,
    viewport: Viewport,
    cache: Option<Cache>,
    clipboard: Clipboard,
    cursor: mouse::Cursor,
    modifiers: ModifiersState,
    state: CameraState,
    participants: Arc<RwLock<HashMap<String, ParticipantInfo>>>,
    event_loop_proxy: EventLoopProxy<UserEvent>,
    redraw_tx: mpsc::Sender<RedrawCommand>,
    redraw_thread: Option<JoinHandle<()>>,
}

pub fn camera_window_attributes() -> WindowAttributes {
    let attrs = WindowAttributes::default()
        .with_title("Hopp Camera")
        .with_inner_size(winit::dpi::LogicalSize::new(
            CAMERA_WINDOW_WIDTH,
            CAMERA_WINDOW_HEIGHT,
        ))
        .with_min_inner_size(winit::dpi::LogicalSize::new(
            CAMERA_WINDOW_MIN_WIDTH,
            CAMERA_WINDOW_MIN_HEIGHT,
        ))
        .with_resizable(true);

    #[cfg(target_os = "macos")]
    let attrs = {
        use winit::{
            platform::macos::WindowAttributesExtMacOS,
            window::{WindowButtons, WindowLevel},
        };
        attrs
            .with_title_hidden(true)
            .with_titlebar_transparent(true)
            .with_fullsize_content_view(true)
            .with_transparent(true)
            .with_window_level(WindowLevel::AlwaysOnTop)
            .with_enabled_buttons(WindowButtons::MINIMIZE)
    };
    attrs
}

impl CameraWindow {
    /// Create a new camera window with wgpu surface and iced renderer.
    pub fn new(
        context_manager: &ContextManager,
        event_loop: &ActiveEventLoop,
        participants: Arc<RwLock<HashMap<String, ParticipantInfo>>>,
        event_loop_proxy: EventLoopProxy<UserEvent>,
        active_mic_name: Option<String>,
    ) -> Result<Self, CameraWindowError> {
        log::info!("CameraWindow::new");

        // ── Create winit window ──────────────────────────────────────────
        let window = event_loop
            .create_window(camera_window_attributes())
            .map_err(|e| {
                log::error!("CameraWindow: failed to create window: {e:?}");
                CameraWindowError::WindowCreation
            })?;
        // Bring to front when window is created
        window.focus_window();
        let window = Arc::new(window);

        // ── wgpu setup ───────────────────────────────────────────────────
        let surface_info = context_manager
            .create_camera_surface(&window)
            .map_err(|e| match e {
                GraphicsWindowContextError::SurfaceCreation => CameraWindowError::SurfaceCreation,
                GraphicsWindowContextError::AdapterRequest => CameraWindowError::AdapterRequest,
                GraphicsWindowContextError::DeviceRequest => CameraWindowError::DeviceRequest,
            })?;
        let device = context_manager.camera_context.device.clone();
        let format = surface_info.format;
        let alpha_mode = surface_info.alpha_mode;
        let surface = surface_info.surface;
        let physical_size = window.inner_size();

        let wgpu_renderer = iced_wgpu::Renderer::new(
            context_manager.camera_context.engine.clone(),
            GEIST_REGULAR,
            Pixels::from(16),
        );

        // Load Geist font data into the global iced font system
        fonts_mod::load_fonts();

        let renderer = iced::Renderer::Primary(wgpu_renderer);

        let viewport = Viewport::with_physical_size(
            Size::new(physical_size.width.max(1), physical_size.height.max(1)),
            window.scale_factor() as f32,
        );
        let clipboard = Clipboard::connect(window.clone());

        #[cfg(target_os = "macos")]
        {
            super::vibrancy::apply_macos_vibrancy(&window, 8.0);
        }

        let logical = viewport.logical_size();
        let camera_active = participants
            .read()
            .ok()
            .and_then(|p| p.get("local").map(|info| info.camera_active()))
            .unwrap_or(false);
        let state = CameraState {
            viewport_size: IcedSize::new(logical.width as f32, logical.height as f32),
            camera_active,
            selected_mic_name: active_mic_name,
            ..Default::default()
        };

        let (redraw_tx, redraw_rx) = mpsc::channel();
        let redraw_thread = Some(spawn_redraw_thread(redraw_rx, window.clone()));

        Ok(Self {
            window: window.clone(),
            surface,
            device,
            format,
            alpha_mode,
            renderer,
            viewport,
            cache: Some(Cache::default()),
            clipboard,
            cursor: mouse::Cursor::Unavailable,
            modifiers: ModifiersState::default(),
            state,
            participants,
            event_loop_proxy,
            redraw_tx,
            redraw_thread,
        })
    }

    /// The winit `WindowId` for event routing.
    pub fn window_id(&self) -> WindowId {
        self.window.id()
    }

    pub fn focus_window(&self) {
        self.window.set_visible(true);
        self.window.focus_window();
    }

    /// Request a redraw of the camera window.
    pub fn request_redraw(&mut self) {
        self.window.request_redraw();
    }

    /// Take ownership of the redraw thread handle for later joining.
    pub fn take_redraw_thread(&mut self) -> Option<JoinHandle<()>> {
        if let Err(e) = self.redraw_tx.send(RedrawCommand::Stop) {
            log::error!("CameraWindow::take_redraw_thread: failed to send Stop: {e:?}");
        }
        self.redraw_thread.take()
    }

    /// Update the local camera active state. Call from StartCamera/StopCamera handlers.
    pub fn set_camera_active(&mut self, active: bool, device_name: Option<String>) {
        self.state.camera_active = active;
        if active {
            self.state.selected_camera_name = device_name;
        }
    }

    pub fn set_selected_mic_name(&mut self, name: Option<String>) {
        self.state.selected_mic_name = name;
    }

    /// Handle a winit `WindowEvent` — forward to iced and manage resize / redraw.
    pub fn handle_window_event(&mut self, event: WindowEvent) {
        let is_redraw = matches!(event, WindowEvent::RedrawRequested);

        // Process interactive events (mouse, keyboard, etc.) through the iced pipeline.
        // Skip RedrawRequested — redraw() builds its own UI internally, so processing
        // it here would double-build the widget tree on every frame.
        if !is_redraw {
            if let Some(iced_event) = conversion::window_event(
                event.clone(),
                self.window.scale_factor() as f32,
                self.modifiers,
            ) {
                if let Event::Mouse(mouse_event) = iced_event {
                    self.cursor = match mouse_event {
                        iced::mouse::Event::CursorMoved { position } => {
                            mouse::Cursor::Available(position)
                        }
                        iced::mouse::Event::CursorLeft => mouse::Cursor::Unavailable,
                        _ => self.cursor,
                    };
                }

                // Build user interface, process the event, and collect messages
                let mut messages: Vec<CameraMessage> = Vec::new();

                let cache = self.cache.take().unwrap_or_default();
                let mut interface = UserInterface::build(
                    Self::view(&self.state, &self.participants, true),
                    self.viewport.logical_size(),
                    cache,
                    &mut self.renderer,
                );

                let iced_event = conversion::window_event(
                    event.clone(),
                    self.window.scale_factor() as f32,
                    self.modifiers,
                );
                if let Some(ev) = iced_event {
                    let (_, statuses) = interface.update(
                        &[ev],
                        self.cursor,
                        &mut self.renderer,
                        &mut self.clipboard,
                        &mut messages,
                    );
                    let _ = statuses;
                }

                self.cache = Some(interface.into_cache());

                // Process collected messages
                for msg in messages {
                    self.update(msg);
                }
            }
        }

        // Handle winit-specific events
        match event {
            WindowEvent::ModifiersChanged(new_modifiers) => {
                self.modifiers = new_modifiers.state();
            }
            WindowEvent::Resized(new_size) => {
                if new_size.width > 0 && new_size.height > 0 {
                    self.viewport = Viewport::with_physical_size(
                        Size::new(new_size.width, new_size.height),
                        self.window.scale_factor() as f32,
                    );
                    let logical = self.viewport.logical_size();
                    self.state.viewport_size = IcedSize::new(logical.width, logical.height);

                    self.sync_compact_constraints();

                    // Configure surface inline instead of deferring
                    self.surface.configure(
                        &self.device,
                        &wgpu::SurfaceConfiguration {
                            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                            format: self.format,
                            width: new_size.width,
                            height: new_size.height,
                            present_mode: wgpu::PresentMode::AutoVsync,
                            alpha_mode: self.alpha_mode,
                            view_formats: vec![],
                            desired_maximum_frame_latency: 2,
                        },
                    );

                    self.request_redraw();
                }
            }
            WindowEvent::RedrawRequested => {
                self.redraw();
            }
            WindowEvent::CloseRequested => {
                self.window.set_visible(false);
            }
            _ => {}
        }
    }

    // ── View ─────────────────────────────────────────────────────────────

    /// Build the Iced widget tree for the camera window.
    ///
    /// Layout (matching iced-poc main.rs):
    /// - Outer container: macOS near-transparent (vibrancy); otherwise Zinc900
    /// - Header row: traffic-light space + centered controls + balance space
    /// - Responsive participant grid with name labels
    fn view<'a>(
        state: &'a CameraState,
        participants: &'a Arc<RwLock<HashMap<String, ParticipantInfo>>>,
        skip_buffer: bool,
    ) -> iced::Element<'a, CameraMessage, Theme, iced::Renderer> {
        // ── Control buttons ────────────────────────────────────────────────
        let is_muted = participants
            .read()
            .ok()
            .and_then(|p| p.get("local").map(|info| info.muted()))
            .unwrap_or(false);

        let mic_bg = if is_muted {
            ColorToken::Gray400.to_color()
        } else {
            ColorToken::Orange400.to_color()
        };
        let mic_icon = if is_muted {
            ICON_MICROPHONE_OFF
        } else {
            ICON_MICROPHONE_ON
        };
        let mic_button = split_button(
            mic_icon,
            mic_bg,
            CameraMessage::MicToggle,
            Some(CameraMessage::MicDropdownToggle),
            state.mic_dropdown_open,
        );

        let video_bg_color = if state.camera_active {
            ColorToken::Green400.to_color()
        } else {
            ColorToken::Gray400.to_color()
        };
        let video_button = split_button(
            ICON_VIDEO,
            video_bg_color,
            CameraMessage::VideoToggle,
            Some(CameraMessage::CameraDropdownToggle),
            state.camera_dropdown_open,
        );

        let screen_button = split_button(
            ICON_SCREEN_SHARE,
            ColorToken::Gray400.to_color(),
            CameraMessage::ScreenShare,
            None,
            false,
        );

        let end_call_button = split_button(
            ICON_PHONE_OFF,
            ColorToken::Red500.to_color(),
            CameraMessage::EndCall,
            None,
            false,
        );

        let controls = row![mic_button, video_button, screen_button, end_call_button].spacing(8);

        // ── Header with centered controls (space for native traffic lights)
        let header = row![
            Space::new().width(Length::Fixed(80.0)), // Space for native macOS traffic lights
            Space::new().width(Length::Fill),
            controls,
            Space::new().width(Length::Fill),
            Space::new().width(Length::Fixed(80.0)), // Balance spacing
        ]
        .padding(Padding {
            top: CONTENT_PADDING,
            right: CONTENT_PADDING,
            bottom: 0.0,
            left: CONTENT_PADDING,
        })
        .align_y(Alignment::Center);

        // ── Participant grid ─────────────────────────────────────────────
        let video_grid = create_participant_grid(
            state.viewport_size,
            participants,
            state.self_hidden,
            state.local_tile_hovered,
            state.is_compact,
            skip_buffer,
        );

        // ── Main layout ─────────────────────────────────────────────────
        let content = if state.is_compact {
            column![video_grid]
                .width(Length::Fill)
                .height(Length::Fill)
                .padding(Padding {
                    top: COMPACT_TOP_INSET,
                    right: 0.0,
                    bottom: 0.0,
                    left: 0.0,
                })
        } else {
            column![header, video_grid]
                .width(Length::Fill)
                .height(Length::Fill)
        };

        let base_inner = container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(|_theme: &Theme| {
                let bg = if cfg!(target_os = "macos") {
                    Color::from_rgba(0.0, 0.0, 0.0, 0.05)
                } else {
                    ColorToken::Zinc900.to_color()
                };
                container::Style {
                    background: Some(Background::Color(bg)),
                    border: Border {
                        radius: 10.0.into(),
                        ..Default::default()
                    },
                    ..Default::default()
                }
            })
            .clip(true);

        let base: iced::Element<'a, CameraMessage, Theme, iced::Renderer> =
            if state.camera_dropdown_open {
                let items: Vec<SplitButtonItem> = state
                    .available_cameras
                    .iter()
                    .map(|cam| {
                        let is_selected = match &state.selected_camera_name {
                            Some(name) => name == &cam.name,
                            None => cam.default,
                        };
                        SplitButtonItem {
                            label: cam.name.clone(),
                            selected: is_selected,
                        }
                    })
                    .collect();

                split_button_dropdown_wrap(
                    base_inner.into(),
                    &items,
                    CameraMessage::CameraDropdownDismiss,
                    |i| CameraMessage::SelectCamera(state.available_cameras[i].name.clone()),
                    HEADER_HEIGHT + 2.0,
                    state.viewport_size.width / 2.0 - 13.0,
                )
            } else if state.mic_dropdown_open {
                let items: Vec<SplitButtonItem> = state
                    .available_mics
                    .iter()
                    .map(|(name, is_default)| {
                        let is_selected = match &state.selected_mic_name {
                            Some(sel) => sel == name,
                            None => *is_default,
                        };
                        SplitButtonItem {
                            label: name.clone(),
                            selected: is_selected,
                        }
                    })
                    .collect();

                split_button_dropdown_wrap(
                    base_inner.into(),
                    &items,
                    CameraMessage::MicDropdownDismiss,
                    |i| CameraMessage::SelectMic(state.available_mics[i].0.clone()),
                    HEADER_HEIGHT + 2.0,
                    state.viewport_size.width / 2.0 + 44.0,
                )
            } else {
                base_inner.into()
            };

        let floating_show_btn = if state.self_hidden {
            Some(
                container(self_visibility_button(true))
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .align_x(Alignment::End)
                    .align_y(Alignment::End)
                    .padding(4.0)
                    .into(),
            )
        } else {
            None
        };

        let toast_position: ToastPosition = if state.viewport_size.width < 500.0 {
            ToastPosition {
                top: None,
                right: Some(15.0),
                bottom: Some(15.0),
                left: None,
            }
        } else {
            ToastPosition {
                top: Some(15.0),
                right: Some(15.0),
                bottom: None,
                left: None,
            }
        };

        let mut layers: Vec<iced::Element<'a, CameraMessage, Theme, iced::Renderer>> = vec![base];
        if let Some(floating) = floating_show_btn {
            layers.push(floating);
        }
        if let Some(toast_el) = toast::toast_view(&state.toast, Some(&toast_position)) {
            layers.push(toast_el);
        }

        stack(layers).into()
    }

    pub fn show_error_toast(&mut self, message: &str) {
        self.state.toast = Some(toast::show_toast(
            message.to_string(),
            3000,
            ToastPosition {
                top: Some(15.0),
                right: Some(15.0),
                bottom: None,
                left: None,
            },
        ));
    }

    /// Handle a camera UI message (state update).
    fn update(&mut self, message: CameraMessage) {
        match message {
            CameraMessage::MicToggle => {
                let is_muted = self
                    .participants
                    .read()
                    .ok()
                    .and_then(|p| p.get("local").map(|info| info.muted()))
                    .unwrap_or(false);

                let event = if is_muted {
                    UserEvent::UnmuteAudio
                } else {
                    UserEvent::MuteAudio
                };
                log::info!("CameraWindow: mic toggle -> {:?}", event);
                if let Err(e) = self.event_loop_proxy.send_event(event) {
                    log::error!("CameraWindow: failed to send mic toggle event: {e:?}");
                }
            }
            CameraMessage::ScreenShare => {
                log::info!("CameraWindow: screen share -> OpenContentPicker");
                if let Err(e) = self
                    .event_loop_proxy
                    .send_event(UserEvent::OpenContentPicker)
                {
                    log::error!("CameraWindow: failed to send OpenContentPicker event: {e:?}");
                }
            }
            CameraMessage::VideoToggle => {
                let event = if self.state.camera_active {
                    UserEvent::StopCamera
                } else {
                    UserEvent::StartCamera {
                        msg: CameraStartMessage { device_name: None },
                        from_socket: false,
                    }
                };
                log::info!("CameraWindow: video toggle -> {:?}", event);
                if let Err(e) = self.event_loop_proxy.send_event(event) {
                    log::error!("CameraWindow: failed to send camera event: {e:?}");
                }
            }
            CameraMessage::EndCall => {
                log::info!("CameraWindow: end call -> CallEnd");
                if let Err(e) = self.event_loop_proxy.send_event(UserEvent::CallEnd) {
                    log::error!("CameraWindow: failed to send CallEnd event: {e:?}");
                }
            }
            CameraMessage::ToggleSelfVisibility => {
                self.state.self_hidden = !self.state.self_hidden;
                if self.state.self_hidden {
                    self.state.local_tile_hovered = false;
                }
            }
            CameraMessage::LocalTileHover(hovered) => {
                self.state.local_tile_hovered = hovered;
            }
            CameraMessage::CameraDropdownToggle => {
                self.state.mic_dropdown_open = false;
                if !self.state.camera_dropdown_open {
                    self.state.available_cameras = CameraCapturer::list_devices();
                }
                self.state.camera_dropdown_open = !self.state.camera_dropdown_open;
            }
            CameraMessage::CameraDropdownDismiss => {
                self.state.camera_dropdown_open = false;
            }
            CameraMessage::SelectCamera(name) => {
                self.state.camera_dropdown_open = false;
                self.state.selected_camera_name = Some(name.clone());
                let msg = CameraStartMessage {
                    device_name: Some(name),
                };
                if let Err(e) = self.event_loop_proxy.send_event(UserEvent::StartCamera {
                    msg,
                    from_socket: false,
                }) {
                    log::error!("Failed to send StartCamera: {e:?}");
                }
            }
            CameraMessage::MicDropdownToggle => {
                self.state.camera_dropdown_open = false;
                if !self.state.mic_dropdown_open {
                    self.state.available_mics = list_audio_inputs();
                }
                self.state.mic_dropdown_open = !self.state.mic_dropdown_open;
            }
            CameraMessage::MicDropdownDismiss => {
                self.state.mic_dropdown_open = false;
            }
            CameraMessage::SelectMic(name) => {
                self.state.mic_dropdown_open = false;
                self.state.selected_mic_name = Some(name.clone());
                let msg = socket_lib::AudioCaptureMessage { device_name: name };
                if let Err(e) = self
                    .event_loop_proxy
                    .send_event(UserEvent::StartAudioCapture {
                        msg,
                        from_socket: false,
                    })
                {
                    log::error!("Failed to send StartAudioCapture: {e:?}");
                }
            }
        }
    }

    /// Recompute `is_compact` from the current viewport width and update the
    /// window's minimum inner size accordingly. Safe to call on every frame —
    /// the windowing-system call is skipped when nothing changed.
    fn sync_compact_constraints(&mut self) {
        let is_compact = self.state.viewport_size.width < COMPACT_WIDTH_THRESHOLD;
        let count = self
            .participants
            .read()
            .map(|p| p.len())
            .unwrap_or(1)
            .max(1);

        let min_h = if is_compact {
            (count as f64) * (MIN_TILE_SIZE as f64)
                + ((count - 1) as f64) * (TILE_SPACING as f64)
                + (MIN_GRID_PADDING as f64 * 2.0)
        } else {
            CAMERA_WINDOW_MIN_HEIGHT
        };

        if is_compact != self.state.is_compact || self.state.compact_min_height != min_h {
            self.state.is_compact = is_compact;
            self.state.compact_min_height = min_h;
            self.window
                .set_min_inner_size(Some(winit::dpi::LogicalSize::new(
                    CAMERA_WINDOW_MIN_WIDTH,
                    min_h,
                )));
        }
    }

    /// Perform a full redraw: build UI, draw, present.
    fn redraw(&mut self) {
        self.sync_compact_constraints();
        toast::tick_toast(&mut self.state.toast);

        let output = match self.surface.get_current_texture() {
            Ok(output) => output,
            Err(e) => {
                log::error!("CameraWindow::redraw: failed to get texture: {e:?}");
                return;
            }
        };
        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        // Build fresh interface from cache
        let cache = self.cache.take().unwrap_or_default();
        let mut interface = UserInterface::build(
            Self::view(&self.state, &self.participants, false),
            self.viewport.logical_size(),
            cache,
            &mut self.renderer,
        );

        // Send a redraw event to iced
        let _ = interface.update(
            &[Event::Window(
                window::Event::RedrawRequested(Instant::now()),
            )],
            self.cursor,
            &mut self.renderer,
            &mut self.clipboard,
            &mut Vec::new(),
        );

        // Draw the interface with white text for dark background
        interface.draw(
            &mut self.renderer,
            &Theme::Dark,
            &Style {
                text_color: Color::WHITE,
            },
            self.cursor,
        );
        self.cache = Some(interface.into_cache());

        // Present via wgpu
        let wgpu_renderer = match &mut self.renderer {
            iced::Renderer::Primary(r) => r,
            _ => unreachable!(),
        };
        let clear_color = if cfg!(target_os = "macos") {
            Some(Color::TRANSPARENT)
        } else {
            Some(ColorToken::Zinc900.to_color())
        };
        wgpu_renderer.present(clear_color, output.texture.format(), &view, &self.viewport);

        self.window.pre_present_notify();
        output.present();
    }
}

impl Drop for CameraWindow {
    fn drop(&mut self) {
        let _ = self.redraw_thread.take();
    }
}

// ── Styling helper functions (ported from iced-poc main.rs) ─────────────────

/// Truncate a name to a maximum of 16 characters, adding "..." if truncated.
fn truncate_name(name: &str) -> String {
    let chars: Vec<char> = name.chars().collect();
    if chars.len() > 16 {
        chars[..16].iter().collect::<String>() + "..."
    } else {
        name.to_string()
    }
}

/// Create a name label badge.
///
/// Variants
/// - **Speaking**: Lime800→Lime950 gradient
/// - **Default**: Slate950→Slate900 gradient
/// - **Muted**: Slate800→Slate700 gradient + `ICON_MICROPHONE_OFF` (Slate400)
fn name_label<'a>(
    name: &str,
    is_small_window: bool,
    is_speaking: bool,
    is_muted: bool,
) -> iced::Element<'a, CameraMessage, Theme, iced::Renderer> {
    let font_size = if is_small_window { 12.0 } else { 16.0 };
    let padding = if is_small_window {
        Padding::from([4, 10])
    } else {
        Padding::from([6, 16])
    };

    let display_name = truncate_name(name);

    // Icon uses the same point size as the name so the row height matches the unmuted
    // single-line label (a larger icon glyph would grow the pill vertically).
    let name_el = text(display_name)
        .size(font_size)
        .color(Color::WHITE)
        .font(GEIST_MEDIUM)
        .align_y(Alignment::Center);

    let label_row: iced::Element<'a, CameraMessage, Theme, iced::Renderer> = if is_muted {
        let mic = text(ICON_MICROPHONE_OFF.to_string())
            .font(ICONS_FONT)
            .size(font_size)
            .color(ColorToken::Slate400.to_color())
            .align_y(Alignment::Center);
        row![mic, Space::new().width(Length::Fixed(6.0)), name_el]
            .spacing(0.0)
            .align_y(Alignment::Center)
            .into()
    } else {
        row![name_el].spacing(0.0).align_y(Alignment::Center).into()
    };

    container(label_row)
        .padding(padding)
        .style(move |_theme: &Theme| {
            // Linear gradient from top to bottom (180deg = PI radians)
            let grad = if is_muted {
                gradient::Linear::new(Radians(std::f32::consts::PI))
                    .add_stop(0.0, ColorToken::Slate800.to_color())
                    .add_stop(1.0, ColorToken::Slate700.to_color())
            } else if is_speaking {
                gradient::Linear::new(Radians(std::f32::consts::PI))
                    .add_stop(0.0, ColorToken::Lime800.to_color())
                    .add_stop(1.0, ColorToken::Lime950.to_color())
            } else {
                gradient::Linear::new(Radians(std::f32::consts::PI))
                    .add_stop(0.0, ColorToken::Slate950.to_color())
                    .add_stop(1.0, ColorToken::Slate900.to_color())
            };

            container::Style {
                background: Some(Background::Gradient(grad.into())),
                border: Border {
                    color: Color::from_rgba(1.0, 1.0, 1.0, 0.3), // White with 30% opacity
                    width: 1.0,
                    radius: 20.0.into(),
                },
                shadow: ShadowToken::Xl.to_shadow(),
                ..Default::default()
            }
        })
        .into()
}

fn get_initials(name: &str) -> Vec<String> {
    let parts: Vec<&str> = name.split_whitespace().collect();
    match parts.len() {
        0 => vec!["🤷".to_string()],
        1 => parts[0]
            .chars()
            .next()
            .map(|c| vec![c.to_uppercase().to_string()])
            .unwrap_or_default(),
        _ => {
            let first = parts[0]
                .chars()
                .next()
                .map(|c| c.to_uppercase().to_string())
                .unwrap_or_default();
            let last = parts
                .last()
                .unwrap()
                .chars()
                .next()
                .map(|c| c.to_uppercase().to_string())
                .unwrap_or_default();
            vec![first, last]
        }
    }
}

fn initials_avatar<'a>(
    name: &str,
    tile_size: f32,
) -> iced::Element<'a, CameraMessage, Theme, iced::Renderer> {
    let scale = ((tile_size - 16.0).max(40.0) / AVATAR_SIZE).min(1.0);
    let avatar_size = AVATAR_SIZE * scale;
    let font_size = AVATAR_FONT_SIZE * scale;
    let radius = AVATAR_RADIUS * scale;
    let letter_spacing = AVATAR_LETTER_SPACING * scale;
    let initials = get_initials(name);

    let initials_row: iced::Element<'a, CameraMessage, Theme, iced::Renderer> = {
        let mut r = row![];
        for (i, ch) in initials.iter().enumerate() {
            if i > 0 {
                r = r.push(Space::new().width(Length::Fixed(letter_spacing)));
            }
            r = r.push(
                text(ch.clone())
                    .size(font_size)
                    .color(Color::WHITE)
                    .font(GEIST_MEDIUM),
            );
        }
        r.align_y(Alignment::Center).into()
    };

    let inner = container(initials_row)
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .style(move |_theme: &Theme| container::Style {
            background: Some(Background::Color(ColorToken::Violet800.to_color())),
            border: Border {
                radius: radius.into(),
                ..Default::default()
            },
            ..Default::default()
        });

    // Inner shadow approximation: white border overlay
    // iced Shadow has no inset mode; approximate with a semi-transparent white border
    // https://discourse.iced.rs/t/advanced-widget-rendering-styling/896
    let inner_shadow_overlay = container(Space::new())
        .width(Length::Fill)
        .height(Length::Fill)
        .style(move |_theme: &Theme| container::Style {
            border: Border {
                color: Color::from_rgba(1.0, 1.0, 1.0, 0.08),
                width: 1.0,
                radius: radius.into(),
            },
            ..Default::default()
        });

    container(iced::widget::stack![inner, inner_shadow_overlay])
        .width(Length::Fixed(avatar_size))
        .height(Length::Fixed(avatar_size))
        .style(move |_theme: &Theme| container::Style {
            border: Border {
                radius: radius.into(),
                ..Default::default()
            },
            shadow: Shadow {
                color: Color::from_rgba(1.0, 1.0, 1.0, 0.03),
                offset: iced::Vector::new(0.0, 4.0),
                blur_radius: 32.0,
            },
            ..Default::default()
        })
        .into()
}

/// Tile corner radius for participant cards (Figma: 6px).
const TILE_RADIUS: f32 = 6.0;

fn self_visibility_button<'a>(
    is_hidden: bool,
) -> iced::Element<'a, CameraMessage, Theme, iced::Renderer> {
    let icon_data = if is_hidden {
        ICON_EYE_ON_SVG
    } else {
        ICON_EYE_OFF_SVG
    };
    let tooltip_text = if is_hidden {
        "Show your camera"
    } else {
        "Hide yourself"
    };

    let icon_handle = svg::Handle::from_memory(icon_data);
    let slate300 = ColorToken::Slate300.to_color();
    let icon = svg(icon_handle)
        .width(Length::Fixed(14.0))
        .height(Length::Fixed(14.0))
        .style(move |_theme: &Theme, _status| svg::Style {
            color: Some(slate300),
        });

    let btn = button(
        container(icon)
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill),
    )
    .width(Length::Fixed(38.0))
    .height(Length::Fixed(26.0))
    .on_press(CameraMessage::ToggleSelfVisibility)
    .padding(Padding::from([6.0, 12.0]))
    .style(move |_theme: &Theme, status| {
        let bg_color = match status {
            button::Status::Hovered => ColorToken::Slate600.to_color(),
            button::Status::Pressed => ColorToken::Slate800.to_color(),
            _ => ColorToken::Slate700.to_color(),
        };
        button::Style {
            background: Some(Background::Color(bg_color)),
            border: Border {
                color: Color::from_rgba(1.0, 1.0, 1.0, 0.3),
                width: 1.0,
                radius: 19.0.into(),
            },
            text_color: Color::WHITE,
            shadow: Shadow::default(),
            snap: false,
        }
    });

    let tooltip_content = container(text(tooltip_text).size(12).color(Color::WHITE))
        .padding(Padding::from([4.0, 8.0]))
        .style(|_theme: &Theme| container::Style {
            background: Some(Background::Color(ColorToken::Gray600.to_color())),
            border: Border {
                color: Color::from_rgba(1.0, 1.0, 1.0, 0.15),
                width: 1.0,
                radius: 6.0.into(),
            },
            shadow: ShadowToken::Xs.to_shadow(),
            ..Default::default()
        });

    let tip_pos = if is_hidden {
        tooltip::Position::Left
    } else {
        tooltip::Position::Top
    };

    tooltip(btn, tooltip_content, tip_pos)
        .gap(1)
        .snap_within_viewport(true)
        .into()
}

/// Create a participant card tile.
///
struct ParticipantCardProps<'a> {
    pub(super) participant_id: u64,
    pub(super) name: &'a str,
    pub(super) is_speaking: bool,
    pub(super) is_muted: bool,
    pub(super) buffers: Arc<VideoBufferManager>,
    pub(super) tile_size: f32,
    pub(super) is_small_window: bool,
    pub(super) is_local: bool,
    pub(super) local_tile_hovered: bool,
    pub(super) hide_name: bool,
    pub(super) skip_buffer: bool,
}

/// If the participant has camera buffers, renders GPU-accelerated video via the
/// shader widget. Otherwise, falls back to a solid Slate900 placeholder.
/// - Overlaid name label at bottom-left
/// - Shadow on outer container
/// - Clipped with rounded corners (8px radius)
fn participant_card<'a>(
    props: ParticipantCardProps<'_>,
) -> iced::Element<'a, CameraMessage, Theme, iced::Renderer> {
    let ParticipantCardProps {
        participant_id,
        name,
        is_speaking,
        is_muted,
        buffers,
        tile_size,
        is_small_window,
        is_local,
        local_tile_hovered,
        hide_name,
        skip_buffer,
    } = props;

    // Adjust padding based on window size
    let overlay_padding = if is_small_window { 8.0 } else { 14.0 };
    let name_owned = name.to_string();

    // Background layer: GPU video if buffer exists and is active, Slate900 fallback otherwise
    let bg_element: iced::Element<'a, CameraMessage, Theme, iced::Renderer> =
        if buffers.is_inactive() {
            container(initials_avatar(name, tile_size))
                .width(Length::Fill)
                .height(Length::Fill)
                .center_x(Length::Fill)
                .center_y(Length::Fill)
                .style(move |_theme: &Theme| container::Style {
                    background: Some(Background::Color(ColorToken::Slate900.to_color())),
                    border: Border {
                        color: Color::TRANSPARENT,
                        width: 0.0,
                        radius: TILE_RADIUS.into(),
                    },
                    ..Default::default()
                })
                .into()
        } else {
            // Render video when stream is active
            let video_program = YuvVideoProgram {
                participant_id,
                buffer: buffers,
                corner_radius: TILE_RADIUS,
                stretch_to_fill: false,
                skip_upload: skip_buffer,
                mirror: is_local,
            };
            let video_bg: iced::widget::Shader<CameraMessage, _> = shader(video_program)
                .width(Length::Fill)
                .height(Length::Fill);
            video_bg.into()
        };
    // Overlay with name label at bottom-left
    let overlay = container(
        column![
            Space::new().height(Length::Fill),
            name_label(&name_owned, is_small_window, is_speaking, is_muted),
        ]
        .width(Length::Fill),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .padding(overlay_padding);

    let eye_button_overlay: Option<iced::Element<'a, CameraMessage, Theme, iced::Renderer>> =
        if is_local && local_tile_hovered {
            Some(
                container(self_visibility_button(false))
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .align_x(Alignment::End)
                    .align_y(Alignment::End)
                    .padding(4.0)
                    .into(),
            )
        } else {
            None
        };

    let mut tile_layers: Vec<iced::Element<'a, CameraMessage, Theme, iced::Renderer>> =
        vec![bg_element];
    if !hide_name {
        tile_layers.push(overlay.into());
    }
    if let Some(eye_btn) = eye_button_overlay {
        tile_layers.push(eye_btn);
    }

    // Clipped content: video + name label (+ local hide control), masked to rounded corners.
    let clipped_content = container(stack(tile_layers))
        .width(Length::Fill)
        .height(Length::Fill)
        .style(move |_theme: &Theme| container::Style {
            border: Border {
                radius: TILE_RADIUS.into(),
                ..Default::default()
            },
            ..Default::default()
        })
        .clip(true);

    // Border overlay drawn ON TOP of video so the shader can't cover it.
    let border_frame = container(Space::new())
        .width(Length::Fill)
        .height(Length::Fill)
        .style(move |_theme: &Theme| {
            let border_color = if hide_name && is_speaking {
                ColorToken::Green400.to_color()
            } else {
                Color::from_rgba(0.0, 0.0, 0.0, 0.45)
            };
            container::Style {
                border: Border {
                    color: border_color,
                    width: 1.0,
                    radius: TILE_RADIUS.into(),
                },
                ..Default::default()
            }
        });

    // Stack: clipped video below, border on top.
    let tile = container(iced::widget::stack![clipped_content, border_frame])
        .width(Length::Fixed(tile_size))
        .height(Length::Fixed(tile_size))
        .style(move |_theme: &Theme| container::Style {
            border: Border {
                radius: TILE_RADIUS.into(),
                ..Default::default()
            },
            shadow: Shadow {
                color: Color::from_rgba(0.0, 0.0, 0.0, 0.1),
                offset: iced::Vector::new(0.0, 0.0),
                blur_radius: 7.0,
            },
            ..Default::default()
        });

    if is_local {
        mouse_area(tile)
            .on_enter(CameraMessage::LocalTileHover(true))
            .on_exit(CameraMessage::LocalTileHover(false))
            .into()
    } else {
        tile.into()
    }
}

/// Hash an identity string to a u64 for GPU texture keying.
fn identity_to_id(identity: &str) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    identity.hash(&mut hasher);
    hasher.finish()
}

/// Create the responsive participant grid.
///
/// Calculates optimal grid layout based on available size, maximizing tile size
/// while ensuring all participants are visible. Matches the iced-poc algorithm.
fn create_participant_grid<'a>(
    available_size: IcedSize,
    participants: &'a Arc<RwLock<HashMap<String, ParticipantInfo>>>,
    self_hidden: bool,
    local_tile_hovered: bool,
    is_compact: bool,
    skip_buffer: bool,
) -> iced::Element<'a, CameraMessage, Theme, iced::Renderer> {
    let participants_guard = participants.read().unwrap();

    // Sort participants by name for stable ordering
    let mut sorted: Vec<(&String, &ParticipantInfo)> = participants_guard.iter().collect();
    sorted.sort_by(|a, b| a.1.name().cmp(b.1.name()));

    let sorted: Vec<(&String, &ParticipantInfo)> = if self_hidden {
        sorted
            .into_iter()
            .filter(|(identity, _)| identity.as_str() != "local")
            .collect()
    } else {
        sorted
    };

    // Uncomment to test with multiple participants
    // #[cfg(debug_assertions)]
    // {
    //     let cloned = sorted.clone();
    //     sorted.extend(cloned.iter().copied());
    //     sorted.extend(cloned.iter().copied());
    // }

    let participant_count = sorted.len();
    if participant_count == 0 {
        return Space::new().into();
    }

    // Subtract header height from vertical space, then apply grid padding
    let header_offset = if is_compact {
        COMPACT_TOP_INSET
    } else {
        HEADER_HEIGHT
    };
    let available_width = available_size.width - (MIN_GRID_PADDING * 2.0);
    let available_height = (available_size.height - header_offset) - (MIN_GRID_PADDING * 2.0);

    // Determine if this is a small window for styling purposes
    let is_small_window = available_size.width < SMALL_WIDTH_THRESHOLD
        || available_size.height < SMALL_HEIGHT_THRESHOLD;

    // Find the optimal grid configuration that maximizes tile size
    // while ensuring ALL participants are visible
    let mut best_tile_size = 0.0_f32;
    let mut best_cols = 1;
    let mut best_rows = participant_count;

    for cols in 1..=participant_count {
        let rows = (participant_count as f32 / cols as f32).ceil() as usize;

        // Calculate max tile size for this configuration
        let max_tile_width = (available_width - (TILE_SPACING * (cols - 1) as f32)) / cols as f32;
        let max_tile_height = (available_height - (TILE_SPACING * (rows - 1) as f32)) / rows as f32;

        // Tile size is constrained by both width and height (1:1 aspect ratio)
        let tile_size = max_tile_width.min(max_tile_height);

        // Always pick the configuration with the largest tile size that fits all participants
        if tile_size > 0.0 && tile_size > best_tile_size {
            best_tile_size = tile_size;
            best_cols = cols;
            best_rows = rows;
        }
    }

    // Apply minimum tile size (but don't exceed what fits)
    let tile_size = best_tile_size.max(MIN_TILE_SIZE);
    let tiles_per_row = best_cols;
    let num_rows = best_rows;
    let hide_name = tile_size < HIDE_NAME_TILE_THRESHOLD;

    // Calculate actual grid dimensions
    let actual_cols = tiles_per_row.min(participant_count);
    let grid_content_width =
        (tile_size * actual_cols as f32) + (TILE_SPACING * (actual_cols - 1).max(0) as f32);
    let grid_content_height =
        (tile_size * num_rows as f32) + (TILE_SPACING * (num_rows - 1).max(0) as f32);

    // Calculate dynamic padding to center the grid within the grid area (below header)
    let grid_area_height = available_size.height - header_offset;
    let h_padding = ((available_size.width - grid_content_width) / 2.0).max(MIN_GRID_PADDING);
    let v_padding = ((grid_area_height - grid_content_height) / 2.0).max(MIN_GRID_PADDING);

    // Create rows of participants
    let mut rows_vec: Vec<iced::Element<'a, CameraMessage, Theme, iced::Renderer>> = Vec::new();
    let mut participants_iter = sorted.iter();

    for _ in 0..num_rows {
        let mut row_tiles: Vec<iced::Element<'a, CameraMessage, Theme, iced::Renderer>> =
            Vec::new();

        for _ in 0..tiles_per_row {
            if let Some((identity, info)) = participants_iter.next() {
                let id = identity_to_id(identity);
                let camera_buffers = info.camera_buffers();
                let is_local = identity.as_str() == "local";
                row_tiles.push(participant_card(ParticipantCardProps {
                    participant_id: id,
                    name: info.name(),
                    is_speaking: info.is_speaking(),
                    is_muted: info.muted(),
                    buffers: camera_buffers,
                    tile_size,
                    is_small_window,
                    is_local,
                    local_tile_hovered,
                    hide_name,
                    skip_buffer,
                }));
            }
        }

        if !row_tiles.is_empty() {
            let participant_row = row(row_tiles).spacing(TILE_SPACING);
            rows_vec.push(participant_row.into());
        }
    }

    // Create the grid column
    let grid = column(rows_vec)
        .spacing(TILE_SPACING)
        .align_x(Alignment::Center);

    // Use dynamic padding to center the grid exactly
    container(grid)
        .width(Length::Fill)
        .height(Length::Fill)
        .padding(
            Padding::new(0.0)
                .top(v_padding)
                .bottom(v_padding)
                .left(h_padding)
                .right(h_padding),
        )
        .into()
}
