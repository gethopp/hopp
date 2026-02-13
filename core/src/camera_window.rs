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
//! - Pill-shaped control buttons with solid/gradient backgrounds
//! - Responsive participant grid with name labels and speaking indicators

use std::sync::Arc;

use iced::widget::{button, column, container, row, svg, text, Space};
use iced::{
    gradient, Alignment, Background, Border, Color, Length, Padding, Pixels, Radians, Shadow,
    Size as IcedSize,
};
use iced_wgpu::core::mouse;
use iced_wgpu::graphics::{Shell, Viewport};
use iced_wgpu::Engine;
use iced_winit::core::renderer::Style;
use iced_winit::core::time::Instant;
use iced_winit::core::{window, Event, Size, Theme};
use iced_winit::runtime::user_interface::Cache;
use iced_winit::runtime::UserInterface;
use iced_winit::{conversion, Clipboard};
use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::ModifiersState;
use winit::window::{Window, WindowAttributes, WindowId};

use thiserror::Error;

use crate::components::fonts::{self as fonts_mod, GEIST_MEDIUM, GEIST_REGULAR};
use crate::windows::colors::ColorToken;
use crate::windows::shadows::ShadowToken;

// ── Window dimensions ───────────────────────────────────────────────────────

/// Initial camera window dimensions (logical pixels).
const CAMERA_WINDOW_WIDTH: f64 = 1035.0;
const CAMERA_WINDOW_HEIGHT: f64 = 555.0;

/// Minimum camera window dimensions.
const CAMERA_WINDOW_MIN_WIDTH: f64 = 400.0;
const CAMERA_WINDOW_MIN_HEIGHT: f64 = 400.0;

// ── Layout constants (matching iced-poc main.rs) ─────────────────────────────

const CONTENT_PADDING: f32 = 12.0;

// Grid layout constants
const MIN_TILE_SIZE: f32 = 80.0;
const TILE_SPACING: f32 = 16.0;
const MIN_GRID_PADDING: f32 = 16.0;

// Header height: button (44/1.5) + padding top/bottom (12*2)
const HEADER_HEIGHT: f32 = (44.0 / 1.5) + (CONTENT_PADDING * 2.0);

// Window size thresholds for small window styling
const SMALL_WIDTH_THRESHOLD: f32 = 450.0;
const SMALL_HEIGHT_THRESHOLD: f32 = 600.0;

// Geist font family imported from crate::components::fonts

// ── SVG icon bytes embedded at compile time ─────────────────────────────────

const ICON_MICROPHONE: &[u8] = include_bytes!("../resources/icons/microphone.svg");
const ICON_SCREEN_SHARE: &[u8] = include_bytes!("../resources/icons/screen-share.svg");
const ICON_VIDEO: &[u8] = include_bytes!("../resources/icons/video.svg");
const ICON_PHONE_OFF: &[u8] = include_bytes!("../resources/icons/phone-off.svg");

// ── Participant data ────────────────────────────────────────────────────────

struct Participant {
    name: &'static str,
    is_speaking: bool,
}

const PARTICIPANTS: &[Participant] = &[
    Participant {
        name: "Iason P.",
        is_speaking: false,
    },
    Participant {
        name: "Costa A.",
        is_speaking: false,
    },
    Participant {
        name: "Κωσταντινος Α.",
        is_speaking: false,
    },
    Participant {
        name: "Ιασων",
        is_speaking: false,
    },
    Participant {
        name: "陽翔 Haruto",
        is_speaking: true,
    },
    Participant {
        name: "عبد المنعم",
        is_speaking: false,
    },
];

// ── Errors ──────────────────────────────────────────────────────────────────

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

// ── Iced messages ───────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum CameraMessage {
    MicToggle,
    ScreenShare,
    VideoToggle,
    EndCall,
}

// ── Application state for the camera UI ────────────────────────────────────

#[derive(Debug)]
struct CameraState {
    // Viewport logical size for responsive layout
    viewport_size: IcedSize,
}

impl Default for CameraState {
    fn default() -> Self {
        Self {
            viewport_size: IcedSize::new(CAMERA_WINDOW_WIDTH as f32, CAMERA_WINDOW_HEIGHT as f32),
        }
    }
}

// ── Button background types (from iced-poc control_button pattern) ──────────

enum ButtonBackground {
    Solid(ColorToken),
    Gradient { top: Color, bottom: Color },
}

// ── CameraWindow ────────────────────────────────────────────────────────────

pub struct CameraWindow {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    _queue: wgpu::Queue,
    format: wgpu::TextureFormat,
    _engine: Engine,
    renderer: iced::Renderer,
    viewport: Viewport,
    cache: Option<Cache>,
    clipboard: Clipboard,
    cursor: mouse::Cursor,
    modifiers: ModifiersState,
    state: CameraState,
    resized: bool,
}

impl CameraWindow {
    /// Create a new camera window with wgpu surface and iced renderer.
    pub fn new(event_loop: &ActiveEventLoop) -> Result<Self, CameraWindowError> {
        log::info!("CameraWindow::new");

        // ── Create winit window ──────────────────────────────────────────
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
            use winit::platform::macos::WindowAttributesExtMacOS;
            attrs
                .with_title_hidden(true)
                .with_titlebar_transparent(true)
                .with_fullsize_content_view(true)
        };

        let window = event_loop.create_window(attrs).map_err(|e| {
            log::error!("CameraWindow: failed to create window: {e:?}");
            CameraWindowError::WindowCreation
        })?;
        let window = Arc::new(window);

        // ── wgpu setup ───────────────────────────────────────────────────
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::PRIMARY,
            ..Default::default()
        });

        let surface = instance.create_surface(window.clone()).map_err(|e| {
            log::error!("CameraWindow: failed to create surface: {e:?}");
            CameraWindowError::SurfaceCreation
        })?;

        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
        }))
        .map_err(|e| {
            log::error!("CameraWindow: failed to request adapter: {e:?}");
            CameraWindowError::AdapterRequest
        })?;

        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default(),
            label: Some("CameraWindow device"),
            memory_hints: wgpu::MemoryHints::default(),
            trace: wgpu::Trace::default(),
            experimental_features: wgpu::ExperimentalFeatures::disabled(),
        }))
        .map_err(|e| {
            log::error!("CameraWindow: failed to request device: {e:?}");
            CameraWindowError::DeviceRequest
        })?;

        let caps = surface.get_capabilities(&adapter);
        let format = caps.formats[0];

        let physical_size = window.inner_size();
        let surface_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: physical_size.width.max(1),
            height: physical_size.height.max(1),
            present_mode: wgpu::PresentMode::AutoVsync,
            alpha_mode: wgpu::CompositeAlphaMode::Opaque,
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &surface_config);

        // ── Iced renderer with Geist fonts ───────────────────────────────
        let engine = Engine::new(
            &adapter,
            device.clone(),
            queue.clone(),
            format,
            Some(iced_wgpu::graphics::Antialiasing::MSAAx4),
            Shell::headless(),
        );
        let wgpu_renderer =
            iced_wgpu::Renderer::new(engine.clone(), GEIST_REGULAR, Pixels::from(16));

        // Load Geist font data into the global iced font system
        fonts_mod::load_fonts();

        let renderer = iced::Renderer::Primary(wgpu_renderer);

        let viewport = Viewport::with_physical_size(
            Size::new(physical_size.width.max(1), physical_size.height.max(1)),
            window.scale_factor() as f32,
        );
        let clipboard = Clipboard::connect(window.clone());

        let logical = viewport.logical_size();
        let mut state = CameraState::default();
        state.viewport_size = IcedSize::new(logical.width as f32, logical.height as f32);

        Ok(Self {
            window,
            surface,
            device,
            _queue: queue,
            format,
            _engine: engine,
            renderer,
            viewport,
            cache: Some(Cache::default()),
            clipboard,
            cursor: mouse::Cursor::Unavailable,
            modifiers: ModifiersState::default(),
            state,
            resized: false,
        })
    }

    /// The winit `WindowId` for event routing.
    pub fn window_id(&self) -> WindowId {
        self.window.id()
    }

    /// Handle a winit `WindowEvent` — forward to iced and manage resize / redraw.
    pub fn handle_window_event(&mut self, event: WindowEvent) {
        // Convert winit event to iced event
        if let Some(iced_event) = conversion::window_event(
            event.clone(),
            self.window.scale_factor() as f32,
            self.modifiers,
        ) {
            match iced_event {
                Event::Mouse(mouse_event) => {
                    self.cursor = match mouse_event {
                        iced::mouse::Event::CursorMoved { position } => {
                            mouse::Cursor::Available(position)
                        }
                        iced::mouse::Event::CursorLeft => mouse::Cursor::Unavailable,
                        _ => self.cursor,
                    };
                }
                _ => {}
            }

            // Build user interface, process the event, and collect messages
            let mut messages: Vec<CameraMessage> = Vec::new();

            let cache = self.cache.take().unwrap_or_default();
            let mut interface = UserInterface::build(
                Self::view(&self.state),
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
                    self.state.viewport_size =
                        IcedSize::new(logical.width as f32, logical.height as f32);
                    self.resized = true;
                    self.window.request_redraw();
                }
            }
            WindowEvent::RedrawRequested => {
                self.redraw();
            }
            WindowEvent::CloseRequested => {
                self.window.set_visible(false);
            }
            _ => {
                // Request redraw for any event that might change UI state
                self.window.request_redraw();
            }
        }
    }

    // ── View ─────────────────────────────────────────────────────────────

    /// Build the Iced widget tree for the camera window.
    ///
    /// Layout (matching iced-poc main.rs):
    /// - Outer container: Slate600 bg, white 50% border, 18px radius
    /// - Header row: traffic-light space + centered controls + balance space
    /// - Responsive participant grid with name labels
    fn view(state: &CameraState) -> iced::Element<'_, CameraMessage, Theme, iced::Renderer> {
        // ── Control buttons (matching iced-poc control_button) ────────────
        let mic_button = control_button(
            ICON_MICROPHONE,
            ButtonBackground::Solid(ColorToken::Orange500),
            CameraMessage::MicToggle,
        );

        let screen_button = control_button(
            ICON_SCREEN_SHARE,
            ButtonBackground::Solid(ColorToken::Gray400),
            CameraMessage::ScreenShare,
        );

        let video_button = control_button(
            ICON_VIDEO,
            ButtonBackground::Solid(ColorToken::Green400),
            CameraMessage::VideoToggle,
        );

        // Red gradient from Figma: #FB2C36 (top) -> #C10007 (bottom)
        let end_call_button = control_button(
            ICON_PHONE_OFF,
            ButtonBackground::Gradient {
                top: Color::from_rgb(251.0 / 255.0, 44.0 / 255.0, 54.0 / 255.0), // #FB2C36
                bottom: Color::from_rgb(193.0 / 255.0, 0.0 / 255.0, 7.0 / 255.0), // #C10007
            },
            CameraMessage::EndCall,
        );

        let controls = row![mic_button, screen_button, video_button, end_call_button].spacing(8);

        // ── Header with centered controls (space for native traffic lights)
        let header = row![
            Space::new().width(Length::Fixed(80.0)), // Space for native macOS traffic lights
            Space::new().width(Length::Fill),
            controls,
            Space::new().width(Length::Fill),
            Space::new().width(Length::Fixed(80.0)), // Balance spacing
        ]
        .padding(Padding::new(CONTENT_PADDING))
        .align_y(Alignment::Center);

        // ── Participant grid ─────────────────────────────────────────────
        let video_grid = create_participant_grid(state.viewport_size);

        // ── Main layout ─────────────────────────────────────────────────
        let content = column![header, video_grid]
            .width(Length::Fill)
            .height(Length::Fill);

        // ── Outer frame (matching iced-poc: Slate600 bg, white 50% border, 18px radius)
        container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(|_theme: &Theme| {
                let border_color = Color::from_rgba(1.0, 1.0, 1.0, 0.5);

                container::Style {
                    background: Some(Background::Color(ColorToken::Slate600.to_color())),
                    border: Border {
                        color: border_color,
                        width: 1.0,
                        radius: 18.0.into(),
                    },
                    ..Default::default()
                }
            })
            .into()
    }

    /// Handle a camera UI message (state update).
    fn update(&mut self, message: CameraMessage) {
        match message {
            CameraMessage::MicToggle => {
                log::info!("CameraWindow: mic toggle requested");
            }
            CameraMessage::ScreenShare => {
                log::info!("CameraWindow: screen share requested");
            }
            CameraMessage::VideoToggle => {
                log::info!("CameraWindow: video toggle requested");
            }
            CameraMessage::EndCall => {
                log::info!("CameraWindow: end call requested");
            }
        }
    }

    /// Perform a full redraw: build UI, draw, present.
    fn redraw(&mut self) {
        if self.resized {
            let size = self.window.inner_size();
            if size.width > 0 && size.height > 0 {
                self.surface.configure(
                    &self.device,
                    &wgpu::SurfaceConfiguration {
                        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                        format: self.format,
                        width: size.width,
                        height: size.height,
                        present_mode: wgpu::PresentMode::AutoVsync,
                        alpha_mode: wgpu::CompositeAlphaMode::Opaque,
                        view_formats: vec![],
                        desired_maximum_frame_latency: 2,
                    },
                );
            }
            self.resized = false;
        }

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
            Self::view(&self.state),
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
        wgpu_renderer.present(None, output.texture.format(), &view, &self.viewport);

        self.window.pre_present_notify();
        output.present();
    }
}

// ── Styling helper functions (ported from iced-poc main.rs) ─────────────────

/// Create a control button with an SVG icon.
///
/// Matches the iced-poc main.rs `control_button` function exactly:
/// - SVG icon at 24/1.5 = 16px
/// - Button width 60/1.5 = 40px, height 44/1.5 ≈ 29.3px
/// - Pill-shaped with 10px radius
/// - Solid or gradient background with hover/press states
fn control_button(
    icon_data: &'static [u8],
    bg: ButtonBackground,
    message: CameraMessage,
) -> iced::Element<'static, CameraMessage, Theme, iced::Renderer> {
    let icon_handle = svg::Handle::from_memory(icon_data);
    let icon = svg(icon_handle)
        .width(Length::Fixed(24.0 / 1.5))
        .height(Length::Fixed(24.0 / 1.5));

    button(
        container(icon)
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill),
    )
    .width(Length::Fixed(60.0 / 1.5))
    .height(Length::Fixed(44.0 / 1.5))
    .on_press(message)
    .padding(0)
    .style(move |_theme: &Theme, status| {
        let background = match &bg {
            ButtonBackground::Solid(color_token) => {
                let base_color = color_token.to_color();
                let adjusted_color = match status {
                    button::Status::Hovered => Color::from_rgba(
                        (base_color.r + 0.1).min(1.0),
                        (base_color.g + 0.1).min(1.0),
                        (base_color.b + 0.1).min(1.0),
                        base_color.a,
                    ),
                    button::Status::Pressed => Color::from_rgba(
                        (base_color.r - 0.1).max(0.0),
                        (base_color.g - 0.1).max(0.0),
                        (base_color.b - 0.1).max(0.0),
                        base_color.a,
                    ),
                    _ => base_color,
                };
                Background::Color(adjusted_color)
            }
            ButtonBackground::Gradient { top, bottom } => {
                let (adjusted_top, adjusted_bottom) = match status {
                    button::Status::Hovered => (
                        Color::from_rgba(
                            (top.r + 0.1).min(1.0),
                            (top.g + 0.1).min(1.0),
                            (top.b + 0.1).min(1.0),
                            top.a,
                        ),
                        Color::from_rgba(
                            (bottom.r + 0.1).min(1.0),
                            (bottom.g + 0.1).min(1.0),
                            (bottom.b + 0.1).min(1.0),
                            bottom.a,
                        ),
                    ),
                    button::Status::Pressed => (
                        Color::from_rgba(
                            (top.r - 0.1).max(0.0),
                            (top.g - 0.1).max(0.0),
                            (top.b - 0.1).max(0.0),
                            top.a,
                        ),
                        Color::from_rgba(
                            (bottom.r - 0.1).max(0.0),
                            (bottom.g - 0.1).max(0.0),
                            (bottom.b - 0.1).max(0.0),
                            bottom.a,
                        ),
                    ),
                    _ => (*top, *bottom),
                };
                // Gradient from top to bottom (180deg = PI radians)
                let grad = gradient::Linear::new(Radians(std::f32::consts::PI))
                    .add_stop(0.0, adjusted_top)
                    .add_stop(1.0, adjusted_bottom);
                Background::Gradient(grad.into())
            }
        };

        button::Style {
            background: Some(background),
            border: Border {
                color: Color::TRANSPARENT,
                width: 0.0,
                radius: 10.0.into(),
            },
            text_color: Color::WHITE,
            shadow: Shadow::default(),
            snap: false,
        }
    })
    .into()
}

/// Truncate a name to a maximum of 10 characters, adding "..." if truncated.
fn truncate_name(name: &str) -> String {
    let chars: Vec<char> = name.chars().collect();
    if chars.len() > 10 {
        chars[..10].iter().collect::<String>() + "..."
    } else {
        name.to_string()
    }
}

/// Create a name label badge.
///
/// Uses gradient backgrounds with the Lime palette when speaking
/// or the Slate palette when not, matching the iced-poc name_label style.
fn name_label(
    name: &'static str,
    is_small_window: bool,
    is_speaking: bool,
) -> iced::Element<'static, CameraMessage, Theme, iced::Renderer> {
    let font_size = if is_small_window { 12 } else { 16 };
    let padding = if is_small_window {
        Padding::from([4, 10])
    } else {
        Padding::from([6, 16])
    };

    let display_name = truncate_name(name);

    container(
        text(display_name)
            .size(font_size)
            .color(Color::WHITE)
            .font(GEIST_MEDIUM),
    )
    .padding(padding)
    .style(move |_theme: &Theme| {
        // Linear gradient from top to bottom (180deg = PI radians)
        let grad = if is_speaking {
            // Speaking: Lime gradient
            gradient::Linear::new(Radians(std::f32::consts::PI))
                .add_stop(0.0, ColorToken::Lime800.to_color())
                .add_stop(1.0, ColorToken::Lime950.to_color())
        } else {
            // Not speaking: Slate gradient
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

/// Create a participant card tile.
///
/// Matches the iced-poc participant_card:
/// - Colored placeholder background (since we don't have real camera images yet)
/// - Overlaid name label at bottom-left
/// - Shadow on outer container
/// - Clipped with rounded corners
fn participant_card(
    name: &'static str,
    tile_size: f32,
    is_small_window: bool,
    is_speaking: bool,
    color_index: usize,
) -> iced::Element<'static, CameraMessage, Theme, iced::Renderer> {
    // Adjust padding based on window size
    let overlay_padding = if is_small_window { 8.0 } else { 14.0 };
    let tile_radius = 12.0;

    // Placeholder colors for participant tiles (varied Slate shades)
    let tile_colors = [
        ColorToken::Slate800,
        ColorToken::Slate700,
        ColorToken::Gray800,
        ColorToken::Slate800,
        ColorToken::Gray700,
        ColorToken::Slate700,
    ];
    let bg_color = tile_colors[color_index % tile_colors.len()].to_color();

    // Background layer (placeholder for camera feed)
    let bg_container = container(Space::new())
        .width(Length::Fill)
        .height(Length::Fill)
        .style(move |_theme: &Theme| container::Style {
            background: Some(Background::Color(bg_color)),
            border: Border {
                color: Color::TRANSPARENT,
                width: 0.0,
                radius: tile_radius.into(),
            },
            shadow: ShadowToken::Md.to_shadow(),
            ..Default::default()
        });

    // Overlay with name label at bottom-left
    let overlay = container(
        column![
            Space::new().height(Length::Fill),
            name_label(name, is_small_window, is_speaking),
        ]
        .width(Length::Fill),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .padding(overlay_padding);

    // Stack background and overlay — enforce 1:1 aspect ratio on the stack itself
    let stacked = container(iced::widget::stack![bg_container, overlay])
        .width(Length::Fixed(tile_size))
        .height(Length::Fixed(tile_size));

    // Outer container with shadow
    container(stacked)
        .width(Length::Fixed(tile_size))
        .height(Length::Fixed(tile_size))
        .style(move |_theme: &Theme| container::Style {
            background: None,
            shadow: ShadowToken::Xl.to_shadow(),
            ..Default::default()
        })
        .clip(true)
        .into()
}

/// Create the responsive participant grid.
///
/// Calculates optimal grid layout based on available size, maximizing tile size
/// while ensuring all participants are visible. Matches the iced-poc algorithm.
fn create_participant_grid(
    available_size: IcedSize,
) -> iced::Element<'static, CameraMessage, Theme, iced::Renderer> {
    let participant_count = PARTICIPANTS.len();
    if participant_count == 0 {
        return Space::new().into();
    }

    // Subtract header height from vertical space, then apply grid padding
    let available_width = available_size.width - (MIN_GRID_PADDING * 2.0);
    let available_height = (available_size.height - HEADER_HEIGHT) - (MIN_GRID_PADDING * 2.0);

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

    // Calculate actual grid dimensions
    let actual_cols = tiles_per_row.min(participant_count);
    let grid_content_width =
        (tile_size * actual_cols as f32) + (TILE_SPACING * (actual_cols - 1).max(0) as f32);
    let grid_content_height =
        (tile_size * num_rows as f32) + (TILE_SPACING * (num_rows - 1).max(0) as f32);

    // Calculate dynamic padding to center the grid within the grid area (below header)
    let grid_area_height = available_size.height - HEADER_HEIGHT;
    let h_padding = ((available_size.width - grid_content_width) / 2.0).max(MIN_GRID_PADDING);
    let v_padding = ((grid_area_height - grid_content_height) / 2.0).max(MIN_GRID_PADDING);

    // Create rows of participants
    let mut rows: Vec<iced::Element<'static, CameraMessage, Theme, iced::Renderer>> = Vec::new();
    let mut participants_iter = PARTICIPANTS.iter().enumerate();

    for _ in 0..num_rows {
        let mut row_tiles: Vec<iced::Element<'static, CameraMessage, Theme, iced::Renderer>> =
            Vec::new();

        for _ in 0..tiles_per_row {
            if let Some((idx, participant)) = participants_iter.next() {
                row_tiles.push(participant_card(
                    participant.name,
                    tile_size,
                    is_small_window,
                    participant.is_speaking,
                    idx,
                ));
            }
        }

        if !row_tiles.is_empty() {
            let participant_row = row(row_tiles).spacing(TILE_SPACING);
            rows.push(participant_row.into());
        }
    }

    // Create the grid column
    let grid = column(rows)
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
