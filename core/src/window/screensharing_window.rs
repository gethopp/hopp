//! Screensharing window with interactive Iced UI rendered via wgpu.
//!
//! This module implements a standalone window for the screensharing controls,
//! using winit for the window, wgpu for the GPU surface, and iced for
//! the interactive widget tree (buttons, text, layout).
//!
//! UI styling is ported from the iced-poc sharing window:
//! - Geist font family (Regular + Medium)
//! - Tailwind color tokens (Slate, Gray, Green, Orange, Red)
//! - Shadow tokens for consistent depth
//! - Pill-shaped control buttons with solid/gradient backgrounds

use std::sync::Arc;
use std::time::{Duration, Instant as StdInstant};

use iced::widget::{canvas, column, container, row, shader, stack, text, Space};
use iced::{
    gradient, Alignment, Background, Border, Color, Length, Padding, Pixels, Radians, Rectangle,
};
use iced_core::clipboard::Kind;
use iced_wgpu::core::mouse;
use iced_wgpu::graphics::{Shell, Viewport};
use iced_wgpu::Engine;
use iced_winit::core::renderer::Style;
use iced_winit::core::time::Instant;
use iced_winit::core::{window, Event, Size, Theme};
use iced_winit::runtime::user_interface::Cache;
use iced_winit::runtime::UserInterface;
use iced_winit::{conversion, Clipboard};
use winit::event::{ElementState, WindowEvent};
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::{Key, ModifiersState};
#[cfg(not(target_os = "macos"))]
use winit::window::{CursorIcon, CustomCursor};
use winit::window::{Window, WindowAttributes, WindowId};

use thiserror::Error;

use fontdb::Database;
use resvg::{tiny_skia, usvg};

use crate::components::fonts::{self as fonts_mod, GEIST_MEDIUM, GEIST_REGULAR};
use crate::components::segmented_control::{
    self as seg_ctrl_mod, SegmentedButton, SegmentedControlAnim,
};
use crate::graphics::graphics_context::participant::{ParticipantError, ParticipantsManager};
use crate::graphics::yuv_renderer::YuvVideoProgram;
use crate::utils::geometry::Position;
use crate::windows::colors::ColorToken;
use crate::windows::shadows::ShadowToken;

/// Sizing constants
const SCREENSHARING_WINDOW_WIDTH: f64 = 600.0; // logical pixels
const SCREENSHARING_WINDOW_MIN_WIDTH: f64 = 500.0;
const CONTENT_PADDING: f32 = 12.0;
/// Header chrome height: 4px top pad + ~26px segmented control + 12px bottom pad
const HEADER_CHROME_HEIGHT: f32 = 42.0;
/// Header right padding (less than content so the cog sits closer to the window edge)
const HEADER_RIGHT_PADDING: f32 = 4.0;

/// Target redraw interval: 60 FPS
const REDRAW_INTERVAL: Duration = Duration::from_millis(1_000 / 60);
/// Dedicated renderer ID for the screensharing stream in YUV pipeline caches.
const SCREENSHARE_STREAM_ID: u64 = u64::MAX;

const ICON_COG: &[u8] = include_bytes!("../../resources/icons/cog.svg");

/// Icon font codepoints for segmented control (from icons-font).
const ICON_REMOTE_CONTROL: char = '\u{F107}';
const ICON_PEN: char = '\u{F109}';
const ICON_CLICK_POINTER: char = '\u{F108}';
const CURSOR_ICON_POINTER: &[u8] =
    include_bytes!("../../resources/icons/local-participant-cursor.svg");
const CURSOR_ICON_PENCIL: &[u8] =
    include_bytes!("../../resources/icons/local-participant-pencil.svg");

// ── Segmented control buttons ────────────────────────────────────────────────
const SEGMENTED_BUTTONS: &[SegmentedButton] = &[
    SegmentedButton {
        id: "control",
        icon_char: ICON_REMOTE_CONTROL,
        description: Some("Remote control"),
    },
    SegmentedButton {
        id: "draw",
        icon_char: ICON_PEN,
        description: Some("Draw"),
    },
    SegmentedButton {
        id: "point",
        icon_char: ICON_CLICK_POINTER,
        description: Some("Click animation"),
    },
];

#[derive(Error, Debug)]
pub enum ScreensharingWindowError {
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
pub enum ScreensharingMessage {
    TabSelected(&'static str),
}

// ── Input events to forward to room service ─────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ScreenShareTab {
    Control,
    Draw,
    Point,
}

#[derive(Debug)]
pub(crate) enum ScreenShareInputEvent {
    CursorMoved { x: f64, y: f64 },
    MouseClick(crate::room_service::MouseClickData),
    Scroll(crate::room_service::WheelDelta),
    KeyInput(crate::room_service::KeystrokeData),
    DrawStart { x: f64, y: f64, path_id: u64 },
    DrawAddPoint { x: f64, y: f64 },
    DrawEnd { x: f64, y: f64 },
    DrawClearAllPaths,
    DrawClearPaths(Vec<u64>),
    ClickAnimation { x: f64, y: f64 },
    TabChanged(ScreenShareTab),
}

// ── Application state for the screensharing UI ─────────────────────────────

struct ScreensharingState {
    active_tab: &'static str,
    /// Animation state for the segmented-control indicator slide.
    tab_anim: Option<SegmentedControlAnim>,
    /// Stream aspect ratio (width/height) — repurposed from img_aspect.
    img_aspect: f64,
    /// Last known stream dimensions for change detection.
    last_stream_width: u32,
    last_stream_height: u32,
    /// Left mouse button is currently held inside participant area during draw mode.
    left_mouse_pressed: bool,
    /// Monotonically increasing path ID for local drawing strokes.
    current_path_id: u64,
    /// Last cursor position (percentage 0.0-1.0) inside participant area.
    last_draw_cursor: Option<(f64, f64)>,
}

impl Default for ScreensharingState {
    fn default() -> Self {
        Self {
            active_tab: SEGMENTED_BUTTONS[0].id,
            tab_anim: None,
            img_aspect: 16.0 / 9.0,
            last_stream_width: 0,
            last_stream_height: 0,
            left_mouse_pressed: false,
            current_path_id: 0,
            last_draw_cursor: None,
        }
    }
}

/// Canvas overlay that draws remote participant cursors and drawing strokes
/// on top of the video content.
struct ParticipantOverlay<'a> {
    participants: &'a ParticipantsManager,
}

impl<'a, Message> canvas::Program<Message> for ParticipantOverlay<'a> {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &iced::Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let translate = |pos: Position| -> Position {
            Position {
                x: pos.x * bounds.width as f64,
                y: pos.y * bounds.height as f64,
            }
        };
        self.participants.draw(renderer, bounds, &translate)
    }
}

/// Rasterize SVG bytes to straight-alpha RGBA at the given pixel size.
fn rasterize_svg_to_rgba(svg_bytes: &[u8], px_size: u32) -> (Vec<u8>, u32, u32) {
    let fontdb = std::sync::Arc::new(Database::new());
    let usvg_options = usvg::Options {
        fontdb,
        ..Default::default()
    };
    let tree = usvg::Tree::from_data(svg_bytes, &usvg_options)
        .expect("rasterize_svg_to_rgba: failed to parse cursor SVG");
    let svg_size = tree.size();
    let max_dim = svg_size.width().max(svg_size.height());
    let scale = if max_dim > 0.0 {
        px_size as f32 / max_dim
    } else {
        1.0
    };
    let w = (svg_size.width() * scale).ceil().max(1.0) as u32;
    let h = (svg_size.height() * scale).ceil().max(1.0) as u32;
    let mut pixmap = tiny_skia::Pixmap::new(w, h).expect("rasterize_svg_to_rgba: pixmap");
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    // tiny_skia gives premultiplied RGBA — convert to straight alpha.
    let mut rgba = pixmap.data().to_vec();
    for px in rgba.chunks_exact_mut(4) {
        let a = px[3] as f32;
        if a > 0.0 && a < 255.0 {
            let inv = 255.0 / a;
            px[0] = (px[0] as f32 * inv).round().min(255.0) as u8;
            px[1] = (px[1] as f32 * inv).round().min(255.0) as u8;
            px[2] = (px[2] as f32 * inv).round().min(255.0) as u8;
        }
    }
    (rgba, w, h)
}

/// Create an `NSCursor` from RGBA pixel data, with `logical_size` in points
/// and `pixel_size` actual pixels.  The hotspot is given in **point** coords.
#[cfg(target_os = "macos")]
fn create_macos_cursor(
    rgba: &[u8],
    pixel_w: u32,
    pixel_h: u32,
    logical_w: f64,
    logical_h: f64,
    hotspot_x: f64,
    hotspot_y: f64,
) -> objc2::rc::Retained<objc2_app_kit::NSCursor> {
    use objc2::rc::Retained;
    use objc2::AnyThread;
    use objc2_app_kit::{NSBitmapImageRep, NSCursor, NSImage, NSImageRep};
    use objc2_foundation::{NSPoint, NSSize};

    unsafe {
        // Build NSBitmapImageRep with an empty buffer that we'll fill.
        let planes_ptr: *mut *mut u8 = std::ptr::null_mut();
        let rep: Retained<NSBitmapImageRep> = objc2::msg_send![
            NSBitmapImageRep::alloc(),
            initWithBitmapDataPlanes: planes_ptr,
            pixelsWide: pixel_w as isize,
            pixelsHigh: pixel_h as isize,
            bitsPerSample: 8_isize,
            samplesPerPixel: 4_isize,
            hasAlpha: true,
            isPlanar: false,
            colorSpaceName: objc2_app_kit::NSDeviceRGBColorSpace,
            bytesPerRow: (pixel_w * 4) as isize,
            bitsPerPixel: 32_isize
        ];

        // Copy pixel data into the rep's buffer.
        let bitmap_data: *mut u8 = objc2::msg_send![&rep, bitmapData];
        std::ptr::copy_nonoverlapping(rgba.as_ptr(), bitmap_data, rgba.len());

        // Wrap in NSImage and set the logical (point) size.
        let image = NSImage::new();
        let rep_as_imagerep: &NSImageRep =
            &*((&rep as &NSBitmapImageRep) as *const NSBitmapImageRep as *const NSImageRep);
        image.addRepresentation(rep_as_imagerep);
        image.setSize(NSSize::new(logical_w, logical_h));

        // Create cursor with hotspot in point coordinates.
        NSCursor::initWithImage_hotSpot(
            NSCursor::alloc(),
            &image,
            NSPoint::new(hotspot_x, hotspot_y),
        )
    }
}

// ── ScreensharingWindow ─────────────────────────────────────────────────────

pub struct ScreensharingWindow {
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
    state: ScreensharingState,
    /// True when the mouse cursor is inside the participant image area.
    mouse_in_participant_area: bool,
    screen_share_buffer: Arc<crate::livekit::video::VideoBufferManager>,
    participants_manager: ParticipantsManager,
    last_redraw: StdInstant,
    /// SID of the local participant (sharer), for updating local drawing state.
    local_participant_sid: Option<String>,
    #[cfg(target_os = "macos")]
    ns_cursor_pointer: objc2::rc::Retained<objc2_app_kit::NSCursor>,
    #[cfg(target_os = "macos")]
    ns_cursor_pencil: objc2::rc::Retained<objc2_app_kit::NSCursor>,
    #[cfg(not(target_os = "macos"))]
    custom_cursor_pointer: winit::window::CustomCursor,
    #[cfg(not(target_os = "macos"))]
    custom_cursor_pencil: winit::window::CustomCursor,
}

// TODO(@konsalex): Things looks stretched out when
// (for sure in non-retina displays) when we resize the window. This needs to be fixed.
// Example: https://share.cleanshot.com/fgYwrMBM
impl ScreensharingWindow {
    /// Create a new screensharing window with wgpu surface and iced renderer.
    pub fn new(
        event_loop: &ActiveEventLoop,
        screen_share_buffer: Arc<crate::livekit::video::VideoBufferManager>,
        participant_sid: Option<String>,
    ) -> Result<Self, ScreensharingWindowError> {
        log::info!("ScreensharingWindow::new");

        // ── Create winit window ──────────────────────────────────────────
        // Initial window: use 16:9 default aspect
        let default_aspect = 16.0 / 9.0;
        let content_w = SCREENSHARING_WINDOW_WIDTH - (CONTENT_PADDING as f64 * 2.0);
        let content_h = content_w / default_aspect;
        let init_w = SCREENSHARING_WINDOW_WIDTH;
        let init_h = content_h + HEADER_CHROME_HEIGHT as f64 + CONTENT_PADDING as f64;

        let min_content_w = SCREENSHARING_WINDOW_MIN_WIDTH - (CONTENT_PADDING as f64 * 2.0);
        let min_content_h = min_content_w / default_aspect;
        let min_w = SCREENSHARING_WINDOW_MIN_WIDTH;
        let min_h = min_content_h + HEADER_CHROME_HEIGHT as f64 + CONTENT_PADDING as f64;

        let attrs = WindowAttributes::default()
            .with_title("Hopp Screensharing")
            .with_inner_size(winit::dpi::LogicalSize::new(init_w, init_h))
            .with_resizable(true)
            .with_min_inner_size(winit::dpi::LogicalSize::new(min_w, min_h));

        #[cfg(target_os = "macos")]
        let attrs = {
            use winit::platform::macos::WindowAttributesExtMacOS;
            attrs
                .with_title_hidden(true)
                .with_titlebar_transparent(true)
                .with_fullsize_content_view(true)
                .with_transparent(true) // Enable transparency at creation for vibrancy
        };

        let window = event_loop.create_window(attrs).map_err(|e| {
            log::error!("ScreensharingWindow: failed to create window: {e:?}");
            ScreensharingWindowError::WindowCreation
        })?;
        let window = Arc::new(window);
        // Bring to front when window is created
        window.focus_window();

        // ── wgpu setup ───────────────────────────────────────────────────
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::PRIMARY,
            ..Default::default()
        });

        let surface = instance.create_surface(window.clone()).map_err(|e| {
            log::error!("ScreensharingWindow: failed to create surface: {e:?}");
            ScreensharingWindowError::SurfaceCreation
        })?;

        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::None,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
        }))
        .map_err(|e| {
            log::error!("ScreensharingWindow: failed to request adapter: {e:?}");
            ScreensharingWindowError::AdapterRequest
        })?;

        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default(),
            label: Some("ScreensharingWindow device"),
            memory_hints: wgpu::MemoryHints::default(),
            trace: wgpu::Trace::default(),
            experimental_features: wgpu::ExperimentalFeatures::disabled(),
        }))
        .map_err(|e| {
            log::error!("ScreensharingWindow: failed to request device: {e:?}");
            ScreensharingWindowError::DeviceRequest
        })?;

        let caps = surface.get_capabilities(&adapter);
        // iced 0.14 enables `web-colors` by default → GAMMA_CORRECTION = false.
        // Match iced's compositor: prefer a non-sRGB surface so raw sRGB colour
        // values pass through without an extra gamma encode.
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| !f.is_srgb())
            .unwrap_or(caps.formats[0]);

        let physical_size = window.inner_size();
        let surface_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: physical_size.width.max(1),
            height: physical_size.height.max(1),
            present_mode: wgpu::PresentMode::AutoVsync,
            alpha_mode: wgpu::CompositeAlphaMode::Auto,
            view_formats: vec![],
            desired_maximum_frame_latency: 0,
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

        #[cfg(target_os = "macos")]
        {
            // Apply macOS vibrancy (frosted glass) after wgpu surface is set up,
            // so the NSVisualEffectView sits behind the Metal layer.
            apply_macos_vibrancy(&window);
            // Gray out the green fullscreen/zoom traffic-light button.
            disable_macos_fullscreen_button(&window);
            // Lock the window frame aspect ratio so macOS enforces it for ALL
            // resize methods (drag, keyboard shortcuts, tiling, accessibility).
            set_macos_window_aspect_ratio(&window, init_w, init_h);
        }

        // Create custom cursors for the participant area.
        // Logical size: 30×30 points (matching the SVG viewBox).
        const CURSOR_LOGICAL_SIZE: f64 = 30.0;

        // TODO(@konsalex): Extract in core init, to avoid re-rasterizing the cursors
        // on every window creation.
        // On macOS, rasterize at 4× the logical size for maximum crispness,
        // then create native NSCursors with the point size set to 30×30.
        #[cfg(target_os = "macos")]
        let (ns_cursor_pointer, ns_cursor_pencil) = {
            let px = (CURSOR_LOGICAL_SIZE * 4.0).round() as u32;
            let (pointer_rgba, pw, ph) = rasterize_svg_to_rgba(CURSOR_ICON_POINTER, px);
            let (pencil_rgba, ew, eh) = rasterize_svg_to_rgba(CURSOR_ICON_PENCIL, px);
            let pointer = create_macos_cursor(
                &pointer_rgba,
                pw,
                ph,
                CURSOR_LOGICAL_SIZE,
                CURSOR_LOGICAL_SIZE,
                3.0,
                2.0,
            );
            let pencil = create_macos_cursor(
                &pencil_rgba,
                ew,
                eh,
                CURSOR_LOGICAL_SIZE,
                CURSOR_LOGICAL_SIZE,
                2.0,
                29.0,
            );
            (pointer, pencil)
        };

        // On non-macOS platforms, fall back to winit CustomCursor at 30px.
        #[cfg(not(target_os = "macos"))]
        let (custom_cursor_pointer, custom_cursor_pencil) = {
            let px = CURSOR_LOGICAL_SIZE as u32;
            let (pointer_rgba, pw, ph) = rasterize_svg_to_rgba(CURSOR_ICON_POINTER, px);
            let (pencil_rgba, ew, eh) = rasterize_svg_to_rgba(CURSOR_ICON_PENCIL, px);
            let pointer = event_loop.create_custom_cursor(
                CustomCursor::from_rgba(pointer_rgba, pw as u16, ph as u16, 3, 2)
                    .expect("create pointer cursor"),
            );
            let pencil = event_loop.create_custom_cursor(
                CustomCursor::from_rgba(pencil_rgba, ew as u16, eh as u16, 2, 29)
                    .expect("create pencil cursor"),
            );
            (pointer, pencil)
        };

        let mut participants_manager = ParticipantsManager::new();
        let local_participant_sid = if let Some(sid) = &participant_sid {
            if let Err(e) = participants_manager.add_participant(sid.clone(), sid, true) {
                log::warn!("ScreensharingWindow::new: failed to add participant {sid}: {e:?}");
            }
            Some(sid.clone())
        } else {
            None
        };

        let s = Self {
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
            state: ScreensharingState::default(),
            mouse_in_participant_area: false,
            screen_share_buffer,
            participants_manager,
            last_redraw: StdInstant::now(),
            local_participant_sid,
            #[cfg(target_os = "macos")]
            ns_cursor_pointer,
            #[cfg(target_os = "macos")]
            ns_cursor_pencil,
            #[cfg(not(target_os = "macos"))]
            custom_cursor_pointer,
            #[cfg(not(target_os = "macos"))]
            custom_cursor_pencil,
        };
        s.update_cursor();
        Ok(s)
    }

    /// The winit `WindowId` for event routing.
    pub fn window_id(&self) -> WindowId {
        self.window.id()
    }

    pub fn request_redraw(&self) {
        self.window.request_redraw();
    }

    pub fn add_participant(
        &mut self,
        sid: String,
        name: &str,
        auto_clear: bool,
    ) -> Result<(), ParticipantError> {
        self.participants_manager
            .add_participant(sid, name, auto_clear)
    }

    pub fn remove_participant(&mut self, sid: &str) {
        self.participants_manager.remove_participant(sid);
    }

    pub fn set_cursor_position(&mut self, sid: &str, position: Option<Position>) {
        self.participants_manager.set_cursor_position(sid, position);
    }

    pub fn draw_start(&mut self, sid: &str, point: Position, path_id: u64) {
        self.participants_manager.draw_start(sid, point, path_id);
    }

    pub fn draw_add_point(&mut self, sid: &str, point: Position) {
        self.participants_manager.draw_add_point(sid, point);
    }

    pub fn draw_end(&mut self, sid: &str, point: Position) {
        self.participants_manager.draw_end(sid, point);
    }

    pub fn draw_clear_path(&mut self, sid: &str, path_id: u64) {
        self.participants_manager.draw_clear_path(sid, path_id);
    }

    pub fn draw_clear_all_paths(&mut self, sid: &str) {
        self.participants_manager.draw_clear_all_paths(sid);
    }

    pub fn set_drawing_mode(&mut self, sid: &str, mode: crate::room_service::DrawingMode) {
        self.participants_manager.set_drawing_mode(sid, mode);
    }

    pub fn update_auto_clear(&mut self) -> Vec<u64> {
        self.participants_manager.update_auto_clear()
    }

    /// Returns the instant when the next redraw should occur.
    pub fn next_redraw_at(&self) -> StdInstant {
        self.last_redraw + REDRAW_INTERVAL
    }

    /// Update window cursor based on active tab and mouse position.
    ///
    /// - Outside participant area → always the OS default cursor.
    /// - Inside participant area + `draw` tab → pencil cursor.
    /// - Inside participant area + any other tab → pointer cursor.
    fn update_cursor(&self) {
        #[cfg(target_os = "macos")]
        {
            use objc2_app_kit::NSCursor;
            if !self.mouse_in_participant_area {
                unsafe { NSCursor::arrowCursor().set() };
            } else if self.state.active_tab == "draw" {
                unsafe { self.ns_cursor_pencil.set() };
            } else {
                unsafe { self.ns_cursor_pointer.set() };
            }
        }
        #[cfg(not(target_os = "macos"))]
        {
            if !self.mouse_in_participant_area {
                self.window
                    .set_cursor(winit::window::Cursor::Icon(CursorIcon::Default));
            } else if self.state.active_tab == "draw" {
                self.window.set_cursor(winit::window::Cursor::Custom(
                    self.custom_cursor_pencil.clone(),
                ));
            } else {
                self.window.set_cursor(winit::window::Cursor::Custom(
                    self.custom_cursor_pointer.clone(),
                ));
            }
        }
    }

    /// Bounding rectangle of the participant image area in logical pixels.
    fn participant_image_rect(&self) -> Rectangle {
        let logical = self.viewport.logical_size();
        Rectangle {
            x: CONTENT_PADDING,
            y: HEADER_CHROME_HEIGHT,
            width: logical.width - 2.0 * CONTENT_PADDING,
            height: logical.height - HEADER_CHROME_HEIGHT - CONTENT_PADDING,
        }
    }

    /// Handle a winit `WindowEvent` — forward to iced and manage resize / redraw.
    /// Returns an optional input event to be forwarded to room service.
    pub fn handle_window_event(&mut self, event: WindowEvent) -> Option<ScreenShareInputEvent> {
        // Participant area event gating: update cursor-in-rect state and capture input
        // when inside the participant image area. All events still flow to iced.
        let scale_factor = self.window.scale_factor() as f32;
        let rect = self.participant_image_rect();
        let mut input_event = None;

        match &event {
            WindowEvent::CursorMoved { position, .. } => {
                let logical_x = (position.x / scale_factor as f64) as f32;
                let logical_y = (position.y / scale_factor as f64) as f32;
                let inside = logical_x >= rect.x
                    && logical_x < rect.x + rect.width
                    && logical_y >= rect.y
                    && logical_y < rect.y + rect.height;
                let was_inside = self.mouse_in_participant_area;
                self.mouse_in_participant_area = inside;
                if was_inside != inside {
                    self.update_cursor();
                }
                if inside {
                    let pct_x = ((logical_x - rect.x) / rect.width) as f64;
                    let pct_y = ((logical_y - rect.y) / rect.height) as f64;
                    self.state.last_draw_cursor = Some((pct_x, pct_y));

                    if self.state.active_tab == "draw" && self.state.left_mouse_pressed {
                        if let Some(sid) = self.local_participant_sid.clone() {
                            self.participants_manager.draw_add_point(
                                &sid,
                                crate::utils::geometry::Position { x: pct_x, y: pct_y },
                            );
                        }
                        input_event =
                            Some(ScreenShareInputEvent::DrawAddPoint { x: pct_x, y: pct_y });
                    } else {
                        input_event =
                            Some(ScreenShareInputEvent::CursorMoved { x: pct_x, y: pct_y });
                    }

                    if !was_inside {
                        log::debug!(
                            "ScreensharingWindow: cursor entered participant area at ({:.3}, {:.3})",
                            pct_x,
                            pct_y
                        );
                    }
                } else if !inside && was_inside {
                    if self.state.active_tab == "draw" && self.state.left_mouse_pressed {
                        if let Some((lx, ly)) = self.state.last_draw_cursor {
                            if let Some(sid) = self.local_participant_sid.clone() {
                                self.participants_manager.draw_end(
                                    &sid,
                                    crate::utils::geometry::Position { x: lx, y: ly },
                                );
                            }
                            input_event = Some(ScreenShareInputEvent::DrawEnd { x: lx, y: ly });
                        }
                        self.state.left_mouse_pressed = false;
                    }
                    self.state.last_draw_cursor = None;
                    log::debug!("ScreensharingWindow: cursor left participant area");
                }
            }
            // Reset when cursor leaves the window — winit skips the final
            // CursorMoved when the cursor exits quickly.
            WindowEvent::CursorLeft { .. } => {
                if self.mouse_in_participant_area {
                    if self.state.active_tab == "draw" && self.state.left_mouse_pressed {
                        if let Some((lx, ly)) = self.state.last_draw_cursor {
                            if let Some(sid) = self.local_participant_sid.clone() {
                                self.participants_manager.draw_end(
                                    &sid,
                                    crate::utils::geometry::Position { x: lx, y: ly },
                                );
                            }
                            input_event = Some(ScreenShareInputEvent::DrawEnd { x: lx, y: ly });
                        }
                        self.state.left_mouse_pressed = false;
                    }
                    self.state.last_draw_cursor = None;
                    self.mouse_in_participant_area = false;
                    self.update_cursor();
                    log::debug!("ScreensharingWindow: cursor left participant area (CursorLeft)");
                }
            }
            // Also reset when the window loses focus so stale state doesn't
            // linger while the user interacts with another window.
            WindowEvent::Focused(false) => {
                if self.mouse_in_participant_area {
                    if self.state.active_tab == "draw" && self.state.left_mouse_pressed {
                        if let Some((lx, ly)) = self.state.last_draw_cursor {
                            if let Some(sid) = self.local_participant_sid.clone() {
                                self.participants_manager.draw_end(
                                    &sid,
                                    crate::utils::geometry::Position { x: lx, y: ly },
                                );
                            }
                            input_event = Some(ScreenShareInputEvent::DrawEnd { x: lx, y: ly });
                        }
                        self.state.left_mouse_pressed = false;
                    }
                    self.state.last_draw_cursor = None;
                    self.mouse_in_participant_area = false;
                    self.update_cursor();
                    log::debug!(
                        "ScreensharingWindow: cursor left participant area (window unfocused)"
                    );
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                if self.mouse_in_participant_area {
                    let (pct_x, pct_y) = match &self.cursor {
                        mouse::Cursor::Available(pos) => (
                            ((pos.x - rect.x) / rect.width) as f64,
                            ((pos.y - rect.y) / rect.height) as f64,
                        ),
                        _ => (0.0, 0.0),
                    };

                    let down = state.is_pressed();

                    if self.state.active_tab == "draw" {
                        match button {
                            winit::event::MouseButton::Left => {
                                if down {
                                    self.state.current_path_id += 1;
                                    self.state.left_mouse_pressed = true;
                                    if let Some(sid) = self.local_participant_sid.clone() {
                                        self.participants_manager.draw_start(
                                            &sid,
                                            crate::utils::geometry::Position { x: pct_x, y: pct_y },
                                            self.state.current_path_id,
                                        );
                                    }
                                    input_event = Some(ScreenShareInputEvent::DrawStart {
                                        x: pct_x,
                                        y: pct_y,
                                        path_id: self.state.current_path_id,
                                    });
                                } else {
                                    self.state.left_mouse_pressed = false;
                                    if let Some(sid) = self.local_participant_sid.clone() {
                                        self.participants_manager.draw_end(
                                            &sid,
                                            crate::utils::geometry::Position { x: pct_x, y: pct_y },
                                        );
                                    }
                                    input_event =
                                        Some(ScreenShareInputEvent::DrawEnd { x: pct_x, y: pct_y });
                                }
                            }
                            winit::event::MouseButton::Right => {
                                if down {
                                    if let Some(sid) = self.local_participant_sid.clone() {
                                        self.participants_manager.draw_clear_all_paths(&sid);
                                    }
                                    input_event = Some(ScreenShareInputEvent::DrawClearAllPaths);
                                }
                            }
                            _ => {}
                        }
                    } else if self.state.active_tab == "point" {
                        if matches!(button, winit::event::MouseButton::Left) && down {
                            input_event =
                                Some(ScreenShareInputEvent::ClickAnimation { x: pct_x, y: pct_y });
                        }
                    } else {
                        // control mode — existing behavior
                        let button_num = match button {
                            winit::event::MouseButton::Left => 0,
                            winit::event::MouseButton::Right => 1,
                            winit::event::MouseButton::Middle => 2,
                            winit::event::MouseButton::Back => 3,
                            winit::event::MouseButton::Forward => 4,
                            winit::event::MouseButton::Other(n) => *n as u32,
                        };

                        input_event = Some(ScreenShareInputEvent::MouseClick(
                            crate::room_service::MouseClickData {
                                x: pct_x,
                                y: pct_y,
                                button: button_num,
                                clicks: 1,
                                down,
                                shift: self.modifiers.shift_key(),
                                meta: self.modifiers.super_key(),
                                ctrl: self.modifiers.control_key(),
                                alt: self.modifiers.alt_key(),
                            },
                        ));
                    }

                    log::debug!(
                        "ScreensharingWindow: [participant_area] mouse button {:?} {:?} at ({:.3}, {:.3})",
                        button,
                        state,
                        pct_x,
                        pct_y
                    );
                } else {
                    log::debug!(
                        "ScreensharingWindow: [outside] mouse {:?} {:?} ignored",
                        button,
                        state
                    );
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                if self.mouse_in_participant_area && self.state.active_tab == "control" {
                    // Extract delta_x and delta_y from the scroll delta
                    let (delta_x, delta_y) = match delta {
                        winit::event::MouseScrollDelta::LineDelta(x, y) => (*x as f64, *y as f64),
                        winit::event::MouseScrollDelta::PixelDelta(pos) => (pos.x, pos.y),
                    };

                    input_event = Some(ScreenShareInputEvent::Scroll(
                        crate::room_service::WheelDelta {
                            deltaX: delta_x,
                            deltaY: delta_y,
                        },
                    ));

                    log::debug!(
                        "ScreensharingWindow: [participant_area] scroll delta {:?}",
                        delta
                    );
                } else {
                    log::debug!("ScreensharingWindow: [outside] scroll ignored");
                }
            }
            WindowEvent::KeyboardInput {
                event: key_event, ..
            } => {
                if self.mouse_in_participant_area && self.state.active_tab == "control" {
                    // Extract key string to match the format expected by keyboard.rs.
                    // We need strings like "Enter", "Tab", "a", "A", etc., not control characters.
                    use winit::keyboard::Key;
                    let mut meta = self.modifiers.super_key();
                    let key_str = match &key_event.logical_key {
                        // For character keys, use the character directly
                        Key::Character(s) => s.to_string(),
                        // For named keys (Enter, Tab, Escape, etc.), use the Debug format
                        // which produces strings like "Enter", "Tab", "ArrowLeft", etc.
                        // Space is a named key but maps to the actual space character in the keymap.
                        Key::Named(winit::keyboard::NamedKey::Space) => " ".to_string(),
                        Key::Named(winit::keyboard::NamedKey::Super) => {
                            meta = true;
                            "Meta".to_string()
                        }
                        Key::Named(named) => format!("{:?}", named),
                        // For dead keys, use the character if available, otherwise "Dead"
                        Key::Dead(ch) => {
                            if let Some(c) = ch {
                                c.to_string()
                            } else {
                                "Dead".to_string()
                            }
                        }
                        Key::Unidentified(_) => "Unidentified".to_string(),
                    };

                    let down = key_event.state.is_pressed();

                    input_event = Some(ScreenShareInputEvent::KeyInput(
                        crate::room_service::KeystrokeData {
                            key: vec![key_str.clone()],
                            meta,
                            ctrl: self.modifiers.control_key(),
                            shift: self.modifiers.shift_key(),
                            alt: self.modifiers.alt_key(),
                            down,
                        },
                    ));

                    log::debug!(
                        "ScreensharingWindow: [participant_area] key {:?} {:?}",
                        key_event.logical_key,
                        key_event.state
                    );
                } else {
                    log::debug!(
                        "ScreensharingWindow: [outside] key {:?} ignored",
                        key_event.logical_key
                    );
                }
            }
            _ => {}
        }

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
            let mut messages: Vec<ScreensharingMessage> = Vec::new();

            let cache = self.cache.take().unwrap_or_default();
            let mut interface = UserInterface::build(
                Self::view(
                    &self.state,
                    &self.screen_share_buffer,
                    &self.participants_manager,
                ),
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
                match &msg {
                    ScreensharingMessage::TabSelected(id) => {
                        let tab = match *id {
                            "draw" => ScreenShareTab::Draw,
                            "point" => ScreenShareTab::Point,
                            _ => ScreenShareTab::Control,
                        };
                        let mode = match tab {
                            ScreenShareTab::Draw => crate::room_service::DrawingMode::Draw(
                                crate::room_service::DrawSettings { permanent: false },
                            ),
                            ScreenShareTab::Point => {
                                crate::room_service::DrawingMode::ClickAnimation
                            }
                            ScreenShareTab::Control => crate::room_service::DrawingMode::Disabled,
                        };
                        if let Some(sid) = self.local_participant_sid.clone() {
                            self.participants_manager.set_drawing_mode(&sid, mode);
                        }
                        self.state.left_mouse_pressed = false;
                        input_event = Some(ScreenShareInputEvent::TabChanged(tab));
                    }
                }
                self.update(msg);
            }

            // Tick animation; keep requesting redraws while it runs.
            seg_ctrl_mod::tick_animation(&mut self.state.tab_anim);
        }

        // Handle winit-specific events
        match event {
            WindowEvent::ModifiersChanged(new_modifiers) => {
                self.modifiers = new_modifiers.state();
            }
            WindowEvent::Resized(new_size) => {
                if new_size.width > 0 && new_size.height > 0 {
                    // Re-apply aspect ratio on macOS.  Winit's window delegate
                    // calls setResizeIncrements: at the start/end of every live
                    // resize, which clears NSWindow.aspectRatio (the two are
                    // mutually exclusive per Apple docs).  We must restore it on
                    // each Resized event so the OS enforces the ratio for all
                    // subsequent drag movements.
                    #[cfg(target_os = "macos")]
                    {
                        let content_w = SCREENSHARING_WINDOW_WIDTH - (CONTENT_PADDING as f64 * 2.0);
                        let content_h = content_w / self.state.img_aspect;
                        let w = SCREENSHARING_WINDOW_WIDTH;
                        let h = content_h + HEADER_CHROME_HEIGHT as f64 + CONTENT_PADDING as f64;
                        set_macos_window_aspect_ratio(&self.window, w, h);
                    }
                    self.surface.configure(
                        &self.device,
                        &wgpu::SurfaceConfiguration {
                            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                            format: self.format,
                            width: new_size.width,
                            height: new_size.height,
                            present_mode: wgpu::PresentMode::AutoVsync,
                            alpha_mode: wgpu::CompositeAlphaMode::Auto,
                            view_formats: vec![],
                            desired_maximum_frame_latency: 0,
                        },
                    );
                    self.viewport = Viewport::with_physical_size(
                        Size::new(new_size.width, new_size.height),
                        self.window.scale_factor() as f32,
                    );
                    self.window.request_redraw();
                }
            }
            WindowEvent::RedrawRequested => {
                if self.last_redraw.elapsed() >= REDRAW_INTERVAL {
                    let cleared = self.redraw();
                    self.last_redraw = StdInstant::now();
                    if !cleared.is_empty() {
                        input_event = Some(ScreenShareInputEvent::DrawClearPaths(cleared));
                    }
                }
            }
            WindowEvent::CloseRequested => {
                self.window.set_visible(false);
            }
            WindowEvent::KeyboardInput { event, .. } => {
                // Only handle Copy/Paste/Cut events when the user is inside the
                // screen-sharing area, and they are in control mode.
                if !self.mouse_in_participant_area || self.state.active_tab != "control" {
                    return None;
                }

                if event.state == ElementState::Pressed {
                    let ctrl_or_cmd = if cfg!(target_os = "macos") {
                        self.modifiers.super_key()
                    } else {
                        self.modifiers.control_key()
                    };

                    if ctrl_or_cmd {
                        match event.logical_key.as_ref() {
                            // Will send a command to remote participant's screen
                            // to copy their selected text
                            Key::Character("c") => {
                                println!("Copy triggered!");
                            }
                            Key::Character("v") => {
                                println!("Paste triggered!");
                                // Get from clipboard manager the text
                                let clipboard_text =
                                    self.clipboard.read(Kind::Standard).unwrap_or_default();
                                println!("Clipboard text: {}", clipboard_text);
                            }
                            _ => {}
                        }
                    }
                }
            }
            _ => {}
        }

        input_event
    }

    fn view<'a>(
        state: &'a ScreensharingState,
        screen_share_buffer: &'a Arc<crate::livekit::video::VideoBufferManager>,
        participants: &'a ParticipantsManager,
    ) -> iced::Element<'a, ScreensharingMessage, Theme, iced::Renderer> {
        // ── Name label (left of header, after traffic lights) ───────────
        let name_label = container(
            text("Costa's Screen")
                .size(14)
                .color(Color::WHITE)
                .font(GEIST_MEDIUM),
        )
        .padding(Padding {
            top: 1.0,
            right: 0.0,
            bottom: 0.0,
            left: 0.0,
        });

        let seg_ctrl = seg_ctrl_mod::segmented_control(
            SEGMENTED_BUTTONS,
            state.active_tab,
            &state.tab_anim,
            ScreensharingMessage::TabSelected,
        );

        // ── Header: stack-based layout so the segmented control is truly
        //    centered across the full window width, independent of name width.
        //    Layer 1: name on the left
        //    Layer 2: segmented control absolutely centered
        let header_ends = row![
            Space::new().width(Length::Fixed(68.0)), // Space for native macOS traffic lights
            Space::new().width(Length::Fixed(0.0)),  // gap before name
            name_label,
            Space::new().width(Length::Fill),
        ]
        .align_y(Alignment::Center)
        .width(Length::Fill);

        let header_center = container(seg_ctrl)
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill);

        let header = container(stack![header_ends, header_center])
            .width(Length::Fill)
            .padding(Padding {
                top: 4.0,
                right: HEADER_RIGHT_PADDING,
                bottom: CONTENT_PADDING,
                left: CONTENT_PADDING,
            });

        // ── Content area (video stream) ──────────────────────────────────
        // Check if buffer has data by peeking at the latest frame
        let has_data = {
            let frame_lock = screen_share_buffer.latest_frame();
            let buf = frame_lock.lock().unwrap();
            buf.width > 0 && buf.height > 0
        };

        let video_content: iced::Element<'a, ScreensharingMessage, Theme, iced::Renderer> =
            if has_data {
                shader::<ScreensharingMessage, _>(YuvVideoProgram {
                    participant_id: SCREENSHARE_STREAM_ID,
                    buffer: screen_share_buffer.clone(),
                    corner_radius: 12.0,
                    stretch_to_fill: true,
                })
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
            } else {
                // Show placeholder when no data
                container(text(""))
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .style(|_theme: &Theme| container::Style {
                        background: Some(Background::Color(ColorToken::Slate800.to_color())),
                        ..Default::default()
                    })
                    .into()
            };

        let canvas_overlay: iced::Element<'a, ScreensharingMessage, Theme, iced::Renderer> =
            canvas(ParticipantOverlay { participants })
                .width(Length::Fill)
                .height(Length::Fill)
                .into();

        let layered_content = stack![video_content, canvas_overlay];

        let content_area = container(
            container(layered_content)
                .width(Length::Fill)
                .height(Length::Fill)
                .style(|_theme: &Theme| container::Style {
                    background: Some(Background::Color(ColorToken::Slate800.to_color())),
                    border: Border {
                        color: ColorToken::Slate600.to_color(),
                        width: 1.0,
                        radius: 12.0.into(),
                    },
                    ..Default::default()
                })
                .clip(true),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .padding(
            Padding::new(0.0)
                .left(CONTENT_PADDING)
                .right(CONTENT_PADDING)
                .bottom(CONTENT_PADDING),
        );

        let main_content = column![header, content_area]
            .width(Length::Fill)
            .height(Length::Fill);

        container(main_content)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(|_theme: &Theme| container::Style {
                // On macOS, layer a semi-transparent dark tint over the vibrancy
                // so the blur shows through but white text stays readable.
                // Tune: lower alpha → more vibrancy visible, higher → darker.
                background: if cfg!(target_os = "macos") {
                    // Dark semi-transparent overlay for the window chrome.
                    // Values are sRGB (non-sRGB surface, no GPU encoding).
                    Some(Background::Color(Color::from_rgba(0.31, 0.31, 0.35, 0.55)))
                } else {
                    Some(Background::Color(ColorToken::Slate600.to_color()))
                },
                ..Default::default()
            })
            .into()
    }

    /// Handle a screensharing UI message (state update).
    fn update(&mut self, message: ScreensharingMessage) {
        match message {
            ScreensharingMessage::TabSelected(id) => {
                self.state.tab_anim =
                    seg_ctrl_mod::start_animation(SEGMENTED_BUTTONS, self.state.active_tab, id);
                self.state.active_tab = id;
                self.update_cursor();
                log::info!("ScreensharingWindow: tab selected = {}", id);
            }
        }
    }

    /// Perform a full redraw: build UI, draw, present.
    /// Returns path IDs cleared by auto-expire during this frame.
    fn redraw(&mut self) -> Vec<u64> {
        // Check if stream dimensions changed and update window size
        {
            let frame_lock = self.screen_share_buffer.latest_frame();
            let buf = frame_lock.lock().unwrap();
            let stream_w = buf.width;
            let stream_h = buf.height;

            if stream_w > 0
                && stream_h > 0
                && (stream_w != self.state.last_stream_width
                    || stream_h != self.state.last_stream_height)
            {
                // Stream dimensions changed — update aspect ratio and resize window
                let aspect = stream_w as f64 / stream_h as f64;
                self.state.img_aspect = aspect;
                self.state.last_stream_width = stream_w;
                self.state.last_stream_height = stream_h;

                let content_w = SCREENSHARING_WINDOW_WIDTH - (CONTENT_PADDING as f64 * 2.0);
                let content_h = content_w / aspect;
                let w = SCREENSHARING_WINDOW_WIDTH;
                let h = content_h + HEADER_CHROME_HEIGHT as f64 + CONTENT_PADDING as f64;

                #[cfg(target_os = "macos")]
                {
                    // Update the OS-enforced aspect ratio + min size for the new stream.
                    set_macos_window_aspect_ratio(&self.window, w, h);

                    let saved_pos = self.window.outer_position();
                    let _ = self
                        .window
                        .request_inner_size(winit::dpi::LogicalSize::new(w, h));
                    // Restore top-left so the window doesn't jump vertically.
                    if let Ok(pos) = saved_pos {
                        self.window.set_outer_position(pos);
                    }
                }

                #[cfg(not(target_os = "macos"))]
                {
                    let _ = self
                        .window
                        .request_inner_size(winit::dpi::LogicalSize::new(w, h));
                }

                log::info!(
                    "ScreensharingWindow: stream dimensions changed to {}x{}, aspect={:.3}, window size={:.1}x{:.1}",
                    stream_w,
                    stream_h,
                    aspect,
                    w,
                    h
                );
            }
        }

        let output = match self.surface.get_current_texture() {
            Ok(output) => output,
            Err(e) => {
                log::error!("ScreensharingWindow::redraw: failed to get texture: {e:?}");
                return vec![];
            }
        };
        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        // Build fresh interface from cache
        let cache = self.cache.take().unwrap_or_default();
        let mut interface = UserInterface::build(
            Self::view(
                &self.state,
                &self.screen_share_buffer,
                &self.participants_manager,
            ),
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
        // On macOS, clear to transparent so the vibrancy effect shows through.
        let clear_color = if cfg!(target_os = "macos") {
            Some(Color::TRANSPARENT)
        } else {
            None
        };
        wgpu_renderer.present(clear_color, output.texture.format(), &view, &self.viewport);

        self.window.pre_present_notify();
        output.present();

        // Keep the redraw loop alive while the segmented-control indicator
        // is animating, so the slide plays smoothly even when no user input
        // events are arriving.
        seg_ctrl_mod::tick_animation(&mut self.state.tab_anim);

        self.participants_manager.update_auto_clear()
    }
}

/// Apply macOS vibrancy (frosted glass) effect to the window.
///
/// Uses `NSVisualEffectView` with the HUDWindow material for a dark translucent
/// background that blends with the desktop, following Apple's
/// [Human Interface Guidelines for materials](https://developer.apple.com/design/human-interface-guidelines/materials).
#[cfg(target_os = "macos")]
fn apply_macos_vibrancy(window: &Window) {
    use objc2::rc::Retained;
    use objc2::{msg_send, MainThreadMarker, MainThreadOnly};
    use objc2_app_kit::{
        NSAutoresizingMaskOptions, NSView, NSVisualEffectBlendingMode, NSVisualEffectMaterial,
        NSVisualEffectState, NSVisualEffectView, NSWindowOrderingMode,
    };
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    let Some(mtm) = MainThreadMarker::new() else {
        log::warn!("apply_macos_vibrancy: not on main thread, skipping");
        return;
    };

    let Ok(raw_handle) = window.window_handle() else {
        log::warn!("apply_macos_vibrancy: failed to get window handle");
        return;
    };

    let RawWindowHandle::AppKit(handle) = raw_handle.as_raw() else {
        log::warn!("apply_macos_vibrancy: not an AppKit handle");
        return;
    };

    unsafe {
        // Get the content view (winit's NSView) from the raw window handle.
        let ns_view: Option<Retained<NSView>> = Retained::retain(handle.ns_view.as_ptr().cast());
        let Some(ns_view) = ns_view else {
            log::warn!("apply_macos_vibrancy: failed to retain NSView");
            return;
        };

        // The content view's superview is the window's frame view (NSThemeFrame).
        // We insert the vibrancy view there, *behind* the content view, so the
        // wgpu Metal layer renders on top and transparent areas show vibrancy.
        // Adding a subview directly to the content view doesn't work because
        // subviews render ON TOP of the parent's CAMetalLayer, hiding iced UI.
        let Some(frame_view) = ns_view.superview() else {
            log::warn!("apply_macos_vibrancy: content view has no superview");
            return;
        };

        let bounds = frame_view.bounds();

        // Create NSVisualEffectView with HUDWindow material — the semantic
        // replacement for the deprecated .ultraDark material (macOS 10.14+).
        let vibrancy_view: Retained<NSVisualEffectView> =
            msg_send![NSVisualEffectView::alloc(mtm), initWithFrame: bounds];

        vibrancy_view.setMaterial(NSVisualEffectMaterial(13)); // HUDWindow
        vibrancy_view.setBlendingMode(NSVisualEffectBlendingMode(0)); // BehindWindow
        vibrancy_view.setState(NSVisualEffectState(1)); // Active (always dark, like ultraDark)
        vibrancy_view.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewWidthSizable
                | NSAutoresizingMaskOptions::ViewHeightSizable,
        );

        // Insert into the frame view behind the content view. We do NOT
        // replace the content view or call setOpaque/setBackgroundColor —
        // the window was created with with_transparent(true) which handles
        // that safely. This keeps winit's internal state intact.
        frame_view.addSubview_positioned_relativeTo(
            &vibrancy_view,
            NSWindowOrderingMode::Below,
            Some(&ns_view),
        );
    }

    log::info!("apply_macos_vibrancy: vibrancy applied successfully");
}

/// Disable (gray out) the green fullscreen/zoom traffic-light button on macOS.
#[cfg(target_os = "macos")]
fn disable_macos_fullscreen_button(window: &Window) {
    use objc2::rc::Retained;
    use objc2_app_kit::{NSView, NSWindowButton};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    let Ok(raw_handle) = window.window_handle() else {
        log::warn!("disable_macos_fullscreen_button: failed to get window handle");
        return;
    };

    let RawWindowHandle::AppKit(handle) = raw_handle.as_raw() else {
        log::warn!("disable_macos_fullscreen_button: not an AppKit handle");
        return;
    };

    unsafe {
        let ns_view: Option<Retained<NSView>> = Retained::retain(handle.ns_view.as_ptr().cast());
        let Some(ns_view) = ns_view else {
            log::warn!("disable_macos_fullscreen_button: failed to retain NSView");
            return;
        };
        let Some(ns_window) = ns_view.window() else {
            log::warn!("disable_macos_fullscreen_button: NSView has no window");
            return;
        };

        if let Some(zoom_btn) = ns_window.standardWindowButton(NSWindowButton::ZoomButton) {
            zoom_btn.setEnabled(false);
            log::info!("disable_macos_fullscreen_button: zoom button disabled");
        }
    }
}

/// Set the **frame-level** aspect ratio on the NSWindow.
///
/// Uses the safe `NSWindow::setAspectRatio()` binding which calls the
/// Objective-C `setAspectRatio:` property setter.  This constrains the window
/// **frame** ratio — macOS enforces it for ALL resize methods (drag, keyboard
/// shortcuts, tiling, accessibility).
///
/// Also sets `NSWindow.minSize` directly (frame-level, not content-level)
/// to match the aspect ratio, mirroring the proven Swift implementation.
#[cfg(target_os = "macos")]
fn set_macos_window_aspect_ratio(window: &Window, width: f64, height: f64) {
    use objc2::rc::Retained;
    use objc2_app_kit::NSView;
    use objc2_foundation::NSSize;
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    let Ok(raw_handle) = window.window_handle() else {
        log::warn!("set_macos_window_aspect_ratio: failed to get window handle");
        return;
    };

    let RawWindowHandle::AppKit(handle) = raw_handle.as_raw() else {
        log::warn!("set_macos_window_aspect_ratio: not an AppKit handle");
        return;
    };

    unsafe {
        let ns_view: Option<Retained<NSView>> = Retained::retain(handle.ns_view.as_ptr().cast());
        let Some(ns_view) = ns_view else {
            log::warn!("set_macos_window_aspect_ratio: failed to retain NSView");
            return;
        };
        let Some(ns_window) = ns_view.window() else {
            log::warn!("set_macos_window_aspect_ratio: NSView has no window");
            return;
        };

        // Use the safe generated binding — avoids any msg_send! encoding pitfalls.
        let aspect = NSSize::new(width, height);
        ns_window.setAspectRatio(aspect);

        // Also set min size at the frame level (same ratio).
        let min_w = SCREENSHARING_WINDOW_MIN_WIDTH;
        let min_h = min_w * (height / width);
        ns_window.setMinSize(NSSize::new(min_w, min_h));

        log::warn!(
            "set_macos_window_aspect_ratio: aspect={:.1}x{:.1}, minSize={:.1}x{:.1}",
            width,
            height,
            min_w,
            min_h,
        );
    }
}

fn status_label(
    label: &'static str,
    is_active: bool,
) -> iced::Element<'static, ScreensharingMessage, Theme, iced::Renderer> {
    container(text(label).size(14).color(Color::WHITE).font(GEIST_MEDIUM))
        .padding(Padding::from([6, 16]))
        .style(move |_theme: &Theme| {
            // Linear gradient from top to bottom (180deg = PI radians)
            let grad = if is_active {
                // Active: Lime gradient (from name_label speaking state)
                gradient::Linear::new(Radians(std::f32::consts::PI))
                    .add_stop(0.0, ColorToken::Lime800.to_color())
                    .add_stop(1.0, ColorToken::Lime950.to_color())
            } else {
                // Inactive: Slate gradient (from name_label default state)
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
