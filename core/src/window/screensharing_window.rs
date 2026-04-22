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

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant as StdInstant};

use iced::widget::{canvas, column, container, row, shader, stack, text, Space};
use iced::{
    gradient, Alignment, Background, Border, Color, Length, Padding, Pixels, Radians, Rectangle,
    Shadow, Vector,
};
use iced_core::clipboard::Kind;
use iced_wgpu::core::mouse;
use iced_wgpu::graphics::Viewport;
use iced_winit::core::renderer::Style;
use iced_winit::core::time::Instant;
use iced_winit::core::{window, Event, Size, Theme};
use iced_winit::runtime::user_interface::Cache;
use iced_winit::runtime::UserInterface;
use iced_winit::{conversion, Clipboard};
use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::{Key, ModifiersState};
#[cfg(not(target_os = "macos"))]
use winit::window::{CursorIcon, CustomCursor};
use winit::window::{Window, WindowAttributes, WindowId};

use thiserror::Error;

use fontdb::Database;
use resvg::{tiny_skia, usvg};

use crate::components::dropdown::{dropdown_overlay, dropdown_trigger_button, DropdownItemDef};
use crate::components::fonts::{self as fonts_mod, GEIST_MEDIUM, GEIST_REGULAR};
use crate::components::segmented_control::{
    self as seg_ctrl_mod, SegmentedButton, SegmentedControlAnim,
};
use crate::graphics::graphics_context::click_animation::ClickAnimationRenderer;
use crate::graphics::graphics_context::participant::{ParticipantError, ParticipantsManager};
use crate::graphics::graphics_window_context::{ContextManager, GraphicsWindowContextError};
use crate::graphics::yuv_renderer::YuvVideoProgram;
use crate::utils::clock;
use crate::utils::geometry::{Extent, Position};
use crate::windows::colors::ColorToken;

use super::aspect_ratio::{
    calculate_max_window_size, default_window_size, min_window_size, AspectRatioEnforcer,
    WindowConstant,
};

pub fn screensharing_window_attributes() -> WindowAttributes {
    let (init_w, init_h) = default_window_size();
    let (min_w, min_h) = min_window_size();
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
            .with_transparent(true)
    };
    attrs
}

/// Available screen area detected at runtime by probing with a temporary window.
/// This replaces hardcoded OS chrome offsets (menubar, taskbar, dock) with
/// actual values from the window manager.
#[derive(Debug)]
struct ScreenArea {
    /// Top-left position of the available area in logical pixels.
    position: Position,
    /// Available dimensions in logical pixels.
    extent: Extent,
}

/// Probe the available screen area by creating a temporary borderless, maximized,
/// invisible window. The window manager will constrain it to the usable area
/// (excluding menubar, dock, taskbar), letting us read back the true offsets.
/// Retries for up to 100ms if the window manager returns zero dimensions.
/// Falls back to monitor size with hardcoded OS chrome offsets on failure.
fn probe_available_screen_area(event_loop: &ActiveEventLoop) -> ScreenArea {
    let start = StdInstant::now();
    let timeout = Duration::from_millis(200);

    let attrs = WindowAttributes::default()
        .with_decorations(false)
        .with_maximized(true)
        .with_transparent(true)
        .with_visible(false);

    // On windows set the decorations in order to align the bottom of the window with the taskbar
    #[cfg(target_os = "windows")]
    let attrs = attrs.with_decorations(true);

    let Some(window) = event_loop.create_window(attrs).ok() else {
        log::warn!("probe_available_screen_area: failed to create probe window, using defaults");
        return default_screen_area_from_hardcoded();
    };

    let mut area = fallback_screen_area_from_monitor(&window);
    let scale = window.scale_factor();

    // On macOS, read monitor origin to detect if WM has placed the window yet.
    #[cfg(target_os = "macos")]
    let monitor_origin_y: f64 = window
        .current_monitor()
        .map(|m| {
            let p: winit::dpi::LogicalPosition<f64> = m.position().to_logical(m.scale_factor());
            p.y
        })
        .unwrap_or(0.0);

    loop {
        let inner: winit::dpi::LogicalSize<f64> = window.inner_size().to_logical(scale);
        let pos: Option<winit::dpi::LogicalPosition<f64>> =
            window.outer_position().ok().map(|p| p.to_logical(scale));

        if let Some(pos) = pos.filter(|_| inner.width > 0.0 && inner.height > 0.0) {
            area = ScreenArea {
                position: Position { x: pos.x, y: pos.y },
                extent: Extent {
                    width: inner.width,
                    height: inner.height,
                },
            };

            // On macOS, position must differ from the monitor origin
            // to confirm the WM accounted for the menu bar.
            #[cfg(target_os = "macos")]
            let settled = (pos.y - monitor_origin_y).abs() >= 1.0;
            #[cfg(not(target_os = "macos"))]
            let settled = true;

            if settled {
                // On windows the probing window returns wrong origin, by
                // locking it to the monitor's origin we ensure that we get the
                // proper dims.
                if let Some(monitor) = window.current_monitor() {
                    let mon_pos: winit::dpi::LogicalPosition<f64> =
                        monitor.position().to_logical(monitor.scale_factor());
                    let dy = mon_pos.y - area.position.y;
                    if dy > 0.0 {
                        area.position.y = mon_pos.y;
                        area.extent.height -= dy;
                    }
                }

                let elapsed = start.elapsed();
                log::info!(
                    "probe_available_screen_area: position=({:.1}, {:.1}), size={:.1}x{:.1} (took {:.1?})",
                    area.position.x,
                    area.position.y,
                    area.extent.width,
                    area.extent.height,
                    elapsed
                );
                break;
            }
        }

        if start.elapsed() >= timeout {
            let elapsed = start.elapsed();
            log::warn!(
                "probe_available_screen_area: area {:?} after {:.1?}",
                area,
                elapsed
            );

            // On macOS, if position never moved from monitor origin,
            // the WM didn't account for the menu bar. Apply 35px offset.
            #[cfg(target_os = "macos")]
            if (area.position.y - monitor_origin_y).abs() < 1.0 {
                area.position.y += 35.0;
                area.extent.height -= 35.0;
                log::info!("probe_available_screen_area: applied 35px macOS menu bar offset");
            }

            break;
        }

        std::thread::sleep(Duration::from_millis(10));
    }

    area
}

/// Build a fallback `ScreenArea` from the monitor attached to the given window,
/// subtracting hardcoded OS chrome heights.
fn fallback_screen_area_from_monitor(window: &Window) -> ScreenArea {
    let Some(monitor) = window.current_monitor() else {
        return default_screen_area_from_hardcoded();
    };
    let logical_size: winit::dpi::LogicalSize<f64> =
        monitor.size().to_logical(monitor.scale_factor());
    let logical_pos: winit::dpi::LogicalPosition<f64> =
        monitor.position().to_logical(monitor.scale_factor());

    let os_chrome_height = if cfg!(target_os = "macos") {
        30.0
    } else if cfg!(target_os = "windows") {
        70.0
    } else {
        0.0
    };

    ScreenArea {
        position: Position {
            x: logical_pos.x,
            y: logical_pos.y + os_chrome_height,
        },
        extent: Extent {
            width: logical_size.width,
            height: logical_size.height - os_chrome_height,
        },
    }
}

/// Last-resort fallback when no monitor info is available at all.
fn default_screen_area_from_hardcoded() -> ScreenArea {
    let os_chrome_height = if cfg!(target_os = "macos") {
        30.0
    } else if cfg!(target_os = "windows") {
        70.0
    } else {
        0.0
    };

    ScreenArea {
        position: Position {
            x: 0.0,
            y: os_chrome_height,
        },
        extent: Extent {
            width: WindowConstant::DEFAULT_WIDTH,
            height: 900.0,
        },
    }
}

const REDRAW_INTERVAL: Duration = Duration::from_millis(1_000 / 10);

pub enum RedrawCommand {
    ForceRedraw,
    Stop,
}

fn spawn_redraw_thread(
    redraw_rx: std::sync::mpsc::Receiver<RedrawCommand>,
    redraw_in_progress: Arc<AtomicBool>,
    window: Arc<Window>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || loop {
        match redraw_rx.recv_timeout(REDRAW_INTERVAL) {
            Ok(RedrawCommand::ForceRedraw) => {
                if !redraw_in_progress.load(Ordering::Acquire) {
                    window.request_redraw();
                }
            }
            Ok(RedrawCommand::Stop) | Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => window.request_redraw(),
        }
    })
}
/// Dedicated renderer ID for the screensharing stream in YUV pipeline caches.
const SCREENSHARE_STREAM_ID: u64 = u64::MAX;
/// Identity used for the local participant's drawing/cursor state.
const LOCAL_PARTICIPANT_IDENTITY: &str = "local";

const ICON_COG: &[u8] = include_bytes!("../../resources/icons/cog.svg");
const ICON_PENCIL_SVG: &[u8] = include_bytes!("../../resources/icons/pencil.svg");

/// Icon font codepoints for segmented control (from icons-font).
const ICON_REMOTE_CONTROL: char = '\u{F107}';
const ICON_PEN: char = '\u{F109}';
const ICON_CLICK_POINTER: char = '\u{F108}';
const CURSOR_ICON_POINTER: &[u8] =
    include_bytes!("../../resources/icons/local-participant-cursor.svg");
const CURSOR_ICON_PENCIL: &[u8] =
    include_bytes!("../../resources/icons/local-participant-pencil.svg");
const CURSOR_ICON_POINT: &[u8] =
    include_bytes!("../../resources/icons/local-participant-pointer.svg");

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
    ToggleDropdown,
    DismissDropdown,
    DropdownItemClicked(usize),
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
    DrawingModeChanged(crate::room_service::DrawingMode),
    AddToClipboard { is_copy: bool },
    PasteFromClipboard(Option<String>),
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
    /// Whether the settings dropdown is open.
    dropdown_open: bool,
    /// When true, drawn strokes persist until right-click; otherwise they fade out.
    draw_persist: bool,
    /// Whether the sharer currently allows remote control input.
    remote_control_allowed: bool,
    /// True after the user manually resizes the window; suppresses auto-maximize.
    user_has_resized: bool,
    /// Multi-click detection state.
    last_click_count: u32,
    last_click_button: u32,
    last_click_time: StdInstant,
    last_click_x: f32,
    last_click_y: f32,
    sharer_name: String,
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
            dropdown_open: false,
            draw_persist: false,
            remote_control_allowed: true,
            user_has_resized: false,
            last_click_count: 0,
            last_click_button: 0,
            last_click_time: StdInstant::now(),
            last_click_x: 0.0,
            last_click_y: 0.0,
            sharer_name: "Screen".to_string(),
        }
    }
}

/// Canvas overlay that draws remote participant cursors and drawing strokes
/// on top of the video content.
struct ParticipantOverlay<'a> {
    participants: &'a ParticipantsManager,
    click_animation_renderer: &'a ClickAnimationRenderer,
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
        let mut geometries = self.participants.draw(renderer, bounds, &translate);
        geometries.push(
            self.click_animation_renderer
                .draw(renderer, bounds, &translate),
        );
        geometries
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
    format: wgpu::TextureFormat,
    alpha_mode: wgpu::CompositeAlphaMode,
    renderer: iced::Renderer,
    viewport: Viewport,
    cache: Option<Cache>,
    clipboard: Clipboard,
    cursor: mouse::Cursor,
    modifiers: ModifiersState,
    state: ScreensharingState,
    /// Target size of a programmatic resize in flight (logical pixels).
    programmatic_resize_target: Option<(f64, f64)>,
    /// True when the mouse cursor is inside the participant image area.
    mouse_in_participant_area: bool,
    /// True when participant_in_control names the local participant (use OS cursor in control tab).
    local_participant_in_control: bool,
    screen_area: ScreenArea,
    screen_share_buffer: Arc<crate::livekit::video::VideoBufferManager>,
    participants_manager: ParticipantsManager,
    click_animation_renderer: ClickAnimationRenderer,
    last_rendered_frame_id: u64,
    redraw_in_progress: Arc<AtomicBool>,
    redraw_tx: std::sync::mpsc::Sender<RedrawCommand>,
    redraw_thread: Option<std::thread::JoinHandle<()>>,
    aspect_ratio_enforcer: AspectRatioEnforcer,
    #[cfg(target_os = "macos")]
    ns_cursor_pointer: objc2::rc::Retained<objc2_app_kit::NSCursor>,
    #[cfg(target_os = "macos")]
    ns_cursor_pencil: objc2::rc::Retained<objc2_app_kit::NSCursor>,
    #[cfg(target_os = "macos")]
    ns_cursor_point: objc2::rc::Retained<objc2_app_kit::NSCursor>,
    #[cfg(not(target_os = "macos"))]
    custom_cursor_pointer: winit::window::CustomCursor,
    #[cfg(not(target_os = "macos"))]
    custom_cursor_pencil: winit::window::CustomCursor,
    #[cfg(not(target_os = "macos"))]
    custom_cursor_point: winit::window::CustomCursor,
}

pub struct ScreensharingWindowConfig {
    pub screen_share_buffer: Arc<crate::livekit::video::VideoBufferManager>,
    pub participants: Vec<(String, String, bool)>,
    pub draw_persist: bool,
    pub last_mode: Option<socket_lib::StoredMode>,
    pub redraw_rx: std::sync::mpsc::Receiver<RedrawCommand>,
    pub redraw_tx: std::sync::mpsc::Sender<RedrawCommand>,
}

impl ScreensharingWindow {
    /// Create a new screensharing window with wgpu surface and iced renderer.
    pub fn new(
        context_manager: &ContextManager,
        event_loop: &ActiveEventLoop,
        config: ScreensharingWindowConfig,
    ) -> Result<Self, ScreensharingWindowError> {
        log::info!("ScreensharingWindow::new");

        let ScreensharingWindowConfig {
            screen_share_buffer,
            participants,
            draw_persist,
            last_mode,
            redraw_rx,
            redraw_tx,
        } = config;

        let screen_area = probe_available_screen_area(event_loop);

        let window = event_loop
            .create_window(screensharing_window_attributes())
            .map_err(|e| {
                log::error!("ScreensharingWindow: failed to create window: {e:?}");
                ScreensharingWindowError::WindowCreation
            })?;
        let window = Arc::new(window);
        // Bring to front when window is created
        window.focus_window();

        // ── wgpu setup ───────────────────────────────────────────────────
        let surface_info = context_manager
            .create_screensharing_surface(&window)
            .map_err(|e| match e {
                GraphicsWindowContextError::SurfaceCreation => {
                    ScreensharingWindowError::SurfaceCreation
                }
                GraphicsWindowContextError::AdapterRequest => {
                    ScreensharingWindowError::AdapterRequest
                }
                GraphicsWindowContextError::DeviceRequest => {
                    ScreensharingWindowError::DeviceRequest
                }
            })?;
        let device = context_manager.screensharing_context.device.clone();
        let format = surface_info.format;
        let alpha_mode = surface_info.alpha_mode;
        log::info!("ScreensharingWindow: selected alpha_mode: {:?}", alpha_mode);
        let surface = surface_info.surface;
        let physical_size = window.inner_size();

        // ── Iced renderer with Geist fonts ───────────────────────────────
        let wgpu_renderer = iced_wgpu::Renderer::new(
            context_manager.screensharing_context.engine.clone(),
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
        super::vibrancy::apply_macos_vibrancy(&window, 8.0);

        let aspect_ratio_enforcer = AspectRatioEnforcer::new(&window);

        // Create custom cursors for the participant area.
        // Logical size: 30×30 points (matching the SVG viewBox).
        const CURSOR_LOGICAL_SIZE: f64 = 30.0;

        // TODO(@konsalex): Extract in core init, to avoid re-rasterizing the cursors
        // on every window creation.
        // On macOS, rasterize at 4× the logical size for maximum crispness,
        // then create native NSCursors with the point size set to 30×30.
        #[cfg(target_os = "macos")]
        let (ns_cursor_pointer, ns_cursor_pencil, ns_cursor_point) = {
            let px = (CURSOR_LOGICAL_SIZE * 4.0).round() as u32;
            let (pointer_rgba, pw, ph) = rasterize_svg_to_rgba(CURSOR_ICON_POINTER, px);
            let (pencil_rgba, ew, eh) = rasterize_svg_to_rgba(CURSOR_ICON_PENCIL, px);
            let (point_rgba, pt_w, pt_h) = rasterize_svg_to_rgba(CURSOR_ICON_POINT, px);
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
            let point = create_macos_cursor(
                &point_rgba,
                pt_w,
                pt_h,
                CURSOR_LOGICAL_SIZE,
                CURSOR_LOGICAL_SIZE,
                2.0,
                4.0,
            );
            (pointer, pencil, point)
        };

        // On non-macOS platforms, fall back to winit CustomCursor at 30px.
        #[cfg(not(target_os = "macos"))]
        let (custom_cursor_pointer, custom_cursor_pencil, custom_cursor_point) = {
            let point_cursor_hotspot = (2.0, 4.0);
            let px = CURSOR_LOGICAL_SIZE as u32;
            let (pointer_rgba, pw, ph) = rasterize_svg_to_rgba(CURSOR_ICON_POINTER, px);
            let (pencil_rgba, ew, eh) = rasterize_svg_to_rgba(CURSOR_ICON_PENCIL, px);
            let (point_rgba, pt_w, pt_h) = rasterize_svg_to_rgba(CURSOR_ICON_POINT, px);
            let pointer = event_loop.create_custom_cursor(
                CustomCursor::from_rgba(pointer_rgba, pw as u16, ph as u16, 3, 2)
                    .expect("create pointer cursor"),
            );
            let pencil = event_loop.create_custom_cursor(
                CustomCursor::from_rgba(pencil_rgba, ew as u16, eh as u16, 2, 29)
                    .expect("create pencil cursor"),
            );
            let point = event_loop.create_custom_cursor(
                CustomCursor::from_rgba(
                    point_rgba,
                    pt_w as u16,
                    pt_h as u16,
                    point_cursor_hotspot.0 as u16,
                    point_cursor_hotspot.1 as u16,
                )
                .expect("create point-mode cursor"),
            );
            (pointer, pencil, point)
        };

        let mut participants_manager = ParticipantsManager::new();
        for (identity, name, _) in &participants {
            if let Err(e) = participants_manager.add_participant(
                identity.clone(),
                name,
                false,
                crate::room_service::DrawingMode::Any,
            ) {
                log::warn!("ScreensharingWindow::new: failed to add participant {identity}: {e:?}");
            }
        }
        // Always add a local participant for the controller's own drawing/cursor state.
        if let Err(e) = participants_manager.add_participant(
            LOCAL_PARTICIPANT_IDENTITY.to_string(),
            LOCAL_PARTICIPANT_IDENTITY,
            true,
            crate::room_service::DrawingMode::Disabled,
        ) {
            log::warn!("ScreensharingWindow::new: failed to add local participant: {e:?}");
        }
        let sharer_first_name = participants
            .iter()
            .find(|(_, _, is_screensharing)| *is_screensharing)
            .map(|(_, name, _)| name.as_str())
            .and_then(|n| n.split_whitespace().next())
            .unwrap_or("Screen")
            .to_string();
        let redraw_in_progress = Arc::new(AtomicBool::new(false));
        let redraw_thread = spawn_redraw_thread(
            redraw_rx,
            Arc::clone(&redraw_in_progress),
            Arc::clone(&window),
        );
        let s = Self {
            window,
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
            state: {
                let (initial_tab, initial_mode) = match &last_mode {
                    Some(socket_lib::StoredMode::Draw { .. }) => (
                        "draw",
                        crate::room_service::DrawingMode::Draw(crate::room_service::DrawSettings {
                            permanent: draw_persist,
                        }),
                    ),
                    Some(socket_lib::StoredMode::ClickAnimation) => {
                        ("point", crate::room_service::DrawingMode::ClickAnimation)
                    }
                    _ => (
                        SEGMENTED_BUTTONS[0].id,
                        crate::room_service::DrawingMode::Disabled,
                    ),
                };
                participants_manager.set_drawing_mode(LOCAL_PARTICIPANT_IDENTITY, initial_mode);
                ScreensharingState {
                    sharer_name: sharer_first_name,
                    draw_persist,
                    active_tab: initial_tab,
                    ..Default::default()
                }
            },
            screen_area,
            programmatic_resize_target: None,
            mouse_in_participant_area: false,
            local_participant_in_control: false,
            screen_share_buffer,
            participants_manager,
            click_animation_renderer: ClickAnimationRenderer::new(clock::default_clock()),
            last_rendered_frame_id: 0,
            redraw_in_progress,
            redraw_tx,
            redraw_thread: Some(redraw_thread),
            aspect_ratio_enforcer,
            #[cfg(target_os = "macos")]
            ns_cursor_pointer,
            #[cfg(target_os = "macos")]
            ns_cursor_pencil,
            #[cfg(target_os = "macos")]
            ns_cursor_point,
            #[cfg(not(target_os = "macos"))]
            custom_cursor_pointer,
            #[cfg(not(target_os = "macos"))]
            custom_cursor_pencil,
            #[cfg(not(target_os = "macos"))]
            custom_cursor_point,
        };
        s.update_cursor();
        Ok(s)
    }

    /// The winit `WindowId` for event routing.
    pub fn window_id(&self) -> WindowId {
        self.window.id()
    }

    pub fn focus_window(&self) {
        if self.window.is_visible() == Some(false) {
            self.window.set_visible(true);
        }
        self.window.focus_window();
    }

    pub fn hide(&self) {
        self.window.set_visible(false);
    }

    pub fn add_participant(
        &mut self,
        identity: String,
        name: &str,
        auto_clear: bool,
    ) -> Result<(), ParticipantError> {
        self.participants_manager.add_participant(
            identity,
            name,
            auto_clear,
            crate::room_service::DrawingMode::Disabled,
        )
    }

    pub fn remove_participant(&mut self, identity: &str) {
        self.participants_manager.remove_participant(identity);
    }

    pub fn set_cursor_position(&mut self, identity: &str, position: Option<Position>) {
        self.participants_manager
            .set_cursor_position(identity, position);
    }

    pub fn draw_start(&mut self, identity: &str, point: Position, path_id: u64) {
        self.participants_manager
            .draw_start(identity, point, path_id);
    }

    pub fn draw_add_point(&mut self, identity: &str, point: Position) {
        self.participants_manager.draw_add_point(identity, point);
    }

    pub fn draw_end(&mut self, identity: &str, point: Position) {
        self.participants_manager.draw_end(identity, point);
    }

    pub fn draw_clear_path(&mut self, identity: &str, path_id: u64) {
        self.participants_manager.draw_clear_path(identity, path_id);
    }

    pub fn draw_clear_all_paths(&mut self, identity: &str) {
        self.participants_manager.draw_clear_all_paths(identity);
    }

    pub fn set_drawing_mode(&mut self, identity: &str, mode: crate::room_service::DrawingMode) {
        self.participants_manager.set_drawing_mode(identity, mode);
    }

    pub fn drawing_mode(&self) -> crate::room_service::DrawingMode {
        match self.state.active_tab {
            "draw" => crate::room_service::DrawingMode::Draw(crate::room_service::DrawSettings {
                permanent: self.state.draw_persist,
            }),
            "point" => crate::room_service::DrawingMode::ClickAnimation,
            _ => crate::room_service::DrawingMode::Disabled,
        }
    }

    pub fn trigger_click_animation(&mut self, position: Position) {
        self.click_animation_renderer
            .enable_click_animation(position);
    }

    pub fn set_remote_control_allowed(&mut self, allowed: bool) {
        self.state.remote_control_allowed = allowed;
    }

    /// Update the window for a new sharer: refresh the display name and swap
    /// the redraw channel so the newly spawned `process_video_stream` can
    /// drive redraws. Returns the old redraw thread handle for deferred join.
    pub fn update_window_with_new_sharer(
        &mut self,
        participants: &[(String, String, bool)],
        new_rx: std::sync::mpsc::Receiver<RedrawCommand>,
        new_tx: std::sync::mpsc::Sender<RedrawCommand>,
    ) -> Option<std::thread::JoinHandle<()>> {
        let sharer_first_name = participants
            .iter()
            .find(|(_, _, is_screensharing)| *is_screensharing)
            .map(|(_, name, _)| name.as_str())
            .and_then(|n| n.split_whitespace().next())
            .unwrap_or("Screen")
            .to_string();
        // Reset ScreensharingState fields
        self.state.sharer_name = sharer_first_name;
        self.state.current_path_id = 0;
        self.state.remote_control_allowed = true;
        self.state.left_mouse_pressed = false;
        self.state.last_click_count = 0;
        self.state.last_click_button = 0;
        self.state.last_click_time = StdInstant::now();
        self.state.last_click_x = 0.0;
        self.state.last_click_y = 0.0;

        // Reset window-level state
        self.local_participant_in_control = false;

        // Reset redraw thread
        let old_handle = self.take_redraw_thread();
        self.redraw_thread = Some(spawn_redraw_thread(
            new_rx,
            Arc::clone(&self.redraw_in_progress),
            Arc::clone(&self.window),
        ));
        self.redraw_tx = new_tx;
        self.last_rendered_frame_id = 0;

        old_handle
    }

    /// Update local control ownership and refresh cursor. When true, control tab uses OS cursor.
    pub fn set_local_participant_in_control(&mut self, in_control: bool) {
        self.local_participant_in_control = in_control;
        self.update_cursor();
    }

    /// Compute multi-click count using the same logic as Chromium.
    /// Coordinates are in logical pixels so the distance threshold is
    /// resolution-independent.
    fn get_mouse_click_count(&self, x: f32, y: f32, button: u32) -> u32 {
        const DOUBLE_CLICK_TIME_MS: u64 = 500;
        const DOUBLE_CLICK_RANGE: f32 = 4.0;

        let prev = &self.state;
        if prev.last_click_count == 0 {
            return 1;
        }
        if prev.last_click_time.elapsed().as_millis() as u64 > DOUBLE_CLICK_TIME_MS {
            return 1;
        }
        if (x - prev.last_click_x).abs() > DOUBLE_CLICK_RANGE / 2.0 {
            return 1;
        }
        if (y - prev.last_click_y).abs() > DOUBLE_CLICK_RANGE / 2.0 {
            return 1;
        }
        if prev.last_click_button != button {
            return 1;
        }
        // On macOS and Windows keep counting; elsewhere cap at 3.
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        if prev.last_click_count >= 3 {
            return 1;
        }
        prev.last_click_count + 1
    }

    /// Update window cursor based on active tab, mouse position, and local control ownership.
    ///
    /// - Outside participant area → always the OS default cursor.
    /// - Inside participant area + `draw` tab → pencil cursor.
    /// - Inside participant area + `point` tab → click-pointer cursor (`local-participant-pointer.svg`).
    /// - Inside participant area + `control` tab + local in control → OS default cursor.
    /// - Inside participant area + other cases → hand cursor (`local-participant-cursor.svg`).
    fn update_cursor(&self) {
        #[cfg(target_os = "macos")]
        {
            use objc2_app_kit::NSCursor;
            if !self.mouse_in_participant_area {
                NSCursor::arrowCursor().set();
            } else if self.state.active_tab == "draw" {
                self.ns_cursor_pencil.set();
            } else if self.state.active_tab == "point" {
                self.ns_cursor_point.set();
            } else if self.state.active_tab == "control" && self.local_participant_in_control {
                NSCursor::arrowCursor().set();
            } else {
                self.ns_cursor_pointer.set();
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
            } else if self.state.active_tab == "point" {
                self.window.set_cursor(winit::window::Cursor::Custom(
                    self.custom_cursor_point.clone(),
                ));
            } else if self.state.active_tab == "control" && self.local_participant_in_control {
                self.window
                    .set_cursor(winit::window::Cursor::Icon(CursorIcon::Default));
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
            x: WindowConstant::PADDING,
            y: WindowConstant::HEADER_HEIGHT,
            width: logical.width - 2.0 * WindowConstant::PADDING,
            height: logical.height - WindowConstant::HEADER_HEIGHT - WindowConstant::PADDING,
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
                let inside = !self.state.dropdown_open
                    && logical_x >= rect.x
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
                        self.participants_manager.draw_add_point(
                            LOCAL_PARTICIPANT_IDENTITY,
                            crate::utils::geometry::Position { x: pct_x, y: pct_y },
                        );
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
                            self.participants_manager.draw_end(
                                LOCAL_PARTICIPANT_IDENTITY,
                                crate::utils::geometry::Position { x: lx, y: ly },
                            );
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
                            self.participants_manager.draw_end(
                                LOCAL_PARTICIPANT_IDENTITY,
                                crate::utils::geometry::Position { x: lx, y: ly },
                            );
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
                            self.participants_manager.draw_end(
                                LOCAL_PARTICIPANT_IDENTITY,
                                crate::utils::geometry::Position { x: lx, y: ly },
                            );
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
                                    self.participants_manager.draw_start(
                                        LOCAL_PARTICIPANT_IDENTITY,
                                        crate::utils::geometry::Position { x: pct_x, y: pct_y },
                                        self.state.current_path_id,
                                    );
                                    input_event = Some(ScreenShareInputEvent::DrawStart {
                                        x: pct_x,
                                        y: pct_y,
                                        path_id: self.state.current_path_id,
                                    });
                                } else {
                                    self.state.left_mouse_pressed = false;
                                    self.participants_manager.draw_end(
                                        LOCAL_PARTICIPANT_IDENTITY,
                                        crate::utils::geometry::Position { x: pct_x, y: pct_y },
                                    );
                                    input_event =
                                        Some(ScreenShareInputEvent::DrawEnd { x: pct_x, y: pct_y });
                                }
                            }
                            winit::event::MouseButton::Right => {
                                if down && self.state.draw_persist {
                                    self.participants_manager
                                        .draw_clear_all_paths(LOCAL_PARTICIPANT_IDENTITY);
                                    input_event = Some(ScreenShareInputEvent::DrawClearAllPaths);
                                }
                            }
                            _ => {}
                        }
                    } else if self.state.active_tab == "point" {
                        if matches!(button, winit::event::MouseButton::Left) && down {
                            self.click_animation_renderer.enable_click_animation(
                                crate::utils::geometry::Position { x: pct_x, y: pct_y },
                            );
                            input_event =
                                Some(ScreenShareInputEvent::ClickAnimation { x: pct_x, y: pct_y });
                        }
                    } else if self.state.remote_control_allowed {
                        // control mode
                        // Use the Web/MDN MouseEvent.button convention
                        // (the receiving side interprets these values):
                        // 0=left, 1=middle, 2=right, 3=back, 4=forward
                        let button_num = match button {
                            winit::event::MouseButton::Left => 0,
                            winit::event::MouseButton::Middle => 1,
                            winit::event::MouseButton::Right => 2,
                            winit::event::MouseButton::Back => 3,
                            winit::event::MouseButton::Forward => 4,
                            winit::event::MouseButton::Other(n) => *n as u32,
                        };

                        // Compute multi-click count on press using logical
                        // pixel positions (same approach as Chromium).
                        let logical_x = match &self.cursor {
                            mouse::Cursor::Available(pos) => pos.x,
                            _ => 0.0,
                        };
                        let logical_y = match &self.cursor {
                            mouse::Cursor::Available(pos) => pos.y,
                            _ => 0.0,
                        };
                        let clicks = if down {
                            let c = self.get_mouse_click_count(logical_x, logical_y, button_num);
                            self.state.last_click_count = c;
                            self.state.last_click_button = button_num;
                            self.state.last_click_time = StdInstant::now();
                            self.state.last_click_x = logical_x;
                            self.state.last_click_y = logical_y;
                            c
                        } else {
                            self.state.last_click_count
                        };

                        input_event = Some(ScreenShareInputEvent::MouseClick(
                            crate::room_service::MouseClickData {
                                x: pct_x,
                                y: pct_y,
                                button: button_num,
                                clicks,
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
                if self.mouse_in_participant_area
                    && self.state.active_tab == "control"
                    && self.state.remote_control_allowed
                {
                    // Extract delta_x and delta_y from the scroll delta.
                    // Always convert lines to pixels, it seems to work for now, maybe a better approach is
                    // to also forward the unit type (lines/pixels) to the sharer.
                    const LINE_HEIGHT_PX: f64 = 40.0;
                    let (delta_x, delta_y) = match delta {
                        winit::event::MouseScrollDelta::LineDelta(x, y) => {
                            (*x as f64 * LINE_HEIGHT_PX, *y as f64 * LINE_HEIGHT_PX)
                        }
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
                if self.mouse_in_participant_area
                    && self.state.active_tab == "control"
                    && self.state.remote_control_allowed
                {
                    // Extract key string to match the format expected by keyboard.rs.
                    // We need strings like "Enter", "Tab", "a", "A", etc., not control characters.
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

                    let ctrl_or_cmd = if cfg!(target_os = "macos") {
                        self.modifiers.super_key()
                    } else {
                        self.modifiers.control_key()
                    };

                    if ctrl_or_cmd && key_event.state.is_pressed() {
                        match key_str.as_str() {
                            "c" => {
                                input_event =
                                    Some(ScreenShareInputEvent::AddToClipboard { is_copy: true });
                            }
                            "x" => {
                                input_event =
                                    Some(ScreenShareInputEvent::AddToClipboard { is_copy: false });
                            }
                            "v" => {
                                let clipboard_text =
                                    self.clipboard.read(Kind::Standard).unwrap_or_default();
                                let data = if clipboard_text.is_empty() {
                                    None
                                } else {
                                    self.clipboard.write(Kind::Standard, String::new());
                                    Some(clipboard_text)
                                };
                                input_event = Some(ScreenShareInputEvent::PasteFromClipboard(data));
                            }
                            _ => {
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
                            }
                        }
                    } else {
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
                    }

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
        // TODO check if we have to throttle this, we might get too many mouse events
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
            let mut messages: Vec<ScreensharingMessage> = Vec::new();

            let cache = self.cache.take().unwrap_or_default();
            let mut interface = UserInterface::build(
                Self::view(
                    &self.state,
                    &self.screen_share_buffer,
                    &self.participants_manager,
                    &self.click_animation_renderer,
                    true,
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
                                crate::room_service::DrawSettings {
                                    permanent: self.state.draw_persist,
                                },
                            ),
                            ScreenShareTab::Point => {
                                crate::room_service::DrawingMode::ClickAnimation
                            }
                            ScreenShareTab::Control => crate::room_service::DrawingMode::Disabled,
                        };
                        if mode == crate::room_service::DrawingMode::Disabled
                            || mode == crate::room_service::DrawingMode::ClickAnimation
                        {
                            self.participants_manager
                                .draw_clear_all_paths(LOCAL_PARTICIPANT_IDENTITY);
                        }
                        self.participants_manager
                            .set_drawing_mode(LOCAL_PARTICIPANT_IDENTITY, mode.clone());
                        self.state.left_mouse_pressed = false;
                        input_event = Some(ScreenShareInputEvent::DrawingModeChanged(mode));
                    }
                    ScreensharingMessage::DropdownItemClicked(index) => {
                        let permanent = *index == 1;
                        let mode = crate::room_service::DrawingMode::Draw(
                            crate::room_service::DrawSettings { permanent },
                        );
                        self.participants_manager
                            .set_drawing_mode(LOCAL_PARTICIPANT_IDENTITY, mode.clone());
                        input_event = Some(ScreenShareInputEvent::DrawingModeChanged(mode));
                    }
                    _ => {}
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
                    let logical: winit::dpi::LogicalSize<f64> =
                        new_size.to_logical(self.window.scale_factor());

                    // Classify this resize event.
                    if let Some((target_w, target_h)) = self.programmatic_resize_target {
                        let matches_target = (logical.width - target_w).abs() < 2.0
                            && (logical.height - target_h).abs() < 2.0;
                        if matches_target {
                            self.programmatic_resize_target = None;
                            log::info!(
                                "ScreensharingWindow: programmatic resize landed at {:.1}x{:.1}",
                                logical.width,
                                logical.height
                            );
                        } else {
                            // Stale/reordered event while programmatic resize in flight — skip.
                            log::info!(
                                "ScreensharingWindow: ignoring stale resize {:.1}x{:.1} (waiting for {:.1}x{:.1})",
                                logical.width, logical.height, target_w, target_h
                            );
                        }
                    } else if self.state.last_stream_width == 0 {
                        // No stream data yet — all resizes are part of window creation.
                        log::info!(
                            "ScreensharingWindow: creation-phase resize to {:.1}x{:.1} (ignored)",
                            logical.width,
                            logical.height
                        );
                    } else {
                        let is_zoom = self.aspect_ratio_enforcer.is_zoomed(&self.window);

                        if is_zoom {
                            log::info!(
                                "ScreensharingWindow: zoom resize to {:.1}x{:.1} (logical)",
                                logical.width,
                                logical.height
                            );
                        } else {
                            self.state.user_has_resized = true;
                            log::info!(
                                "ScreensharingWindow: user resize to {:.1}x{:.1} (logical)",
                                logical.width,
                                logical.height
                            );
                        }

                        if let Some((target_w, target_h)) =
                            self.aspect_ratio_enforcer.correct_aspect_after_resize(
                                &self.window,
                                logical.width,
                                logical.height,
                                self.state.img_aspect,
                                self.screen_area.extent,
                                self.screen_area.position,
                            )
                        {
                            self.programmatic_resize_target = Some((target_w, target_h));
                            let _ = self.window.request_inner_size(winit::dpi::LogicalSize::new(
                                target_w, target_h,
                            ));
                            return None;
                        }
                    }

                    // Always reconfigure surface + viewport.
                    self.aspect_ratio_enforcer
                        .set_aspect_ratio(&self.window, self.state.img_aspect);
                    self.surface.configure(
                        &self.device,
                        &wgpu::SurfaceConfiguration {
                            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                            format: self.format,
                            width: new_size.width,
                            height: new_size.height,
                            present_mode: wgpu::PresentMode::Immediate,
                            alpha_mode: self.alpha_mode,
                            view_formats: vec![],
                            desired_maximum_frame_latency: 2,
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
                self.redraw_in_progress.store(true, Ordering::Release);
                let cleared = self.redraw();
                self.redraw_in_progress.store(false, Ordering::Release);
                if !cleared.is_empty() {
                    input_event = Some(ScreenShareInputEvent::DrawClearPaths(cleared));
                }
            }
            WindowEvent::CloseRequested => {
                self.window.set_visible(false);
            }
            _ => {}
        }

        input_event
    }

    fn view<'a>(
        state: &'a ScreensharingState,
        screen_share_buffer: &'a Arc<crate::livekit::video::VideoBufferManager>,
        participants: &'a ParticipantsManager,
        click_animation_renderer: &'a ClickAnimationRenderer,
        skip_buffer: bool,
    ) -> iced::Element<'a, ScreensharingMessage, Theme, iced::Renderer> {
        // ── Name label (left of header, after traffic lights) ───────────
        let name_label = container(
            text(format!("{}'s Screen", state.sharer_name))
                .size(14)
                .color(Color::WHITE)
                .font(GEIST_MEDIUM),
        )
        .padding(Padding {
            top: 2.0,
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
        let cog_button = dropdown_trigger_button(
            ICON_COG,
            state.dropdown_open,
            ScreensharingMessage::ToggleDropdown,
        );

        let traffic_light_spacer = if cfg!(target_os = "macos") { 68.0 } else { 0.0 };

        let header_ends = row![
            Space::new().width(Length::Fixed(traffic_light_spacer)),
            name_label,
            Space::new().width(Length::Fill),
            cog_button,
        ]
        .align_y(Alignment::Center)
        .width(Length::Fill);

        let header_center = container(seg_ctrl)
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill);

        let header_left_padding = if cfg!(target_os = "macos") {
            WindowConstant::PADDING
        } else {
            WindowConstant::HEADER_SIDE_PADDING
        };

        let header = container(stack![header_ends, header_center])
            .width(Length::Fill)
            .padding(Padding {
                top: 4.0,
                right: WindowConstant::HEADER_SIDE_PADDING,
                bottom: WindowConstant::PADDING,
                left: header_left_padding,
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
                    skip_upload: skip_buffer,
                    mirror: false,
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
            canvas(ParticipantOverlay {
                participants,
                click_animation_renderer,
            })
            .width(Length::Fill)
            .height(Length::Fill)
            .into();

        let remote_control_disabled_label: iced::Element<
            'a,
            ScreensharingMessage,
            Theme,
            iced::Renderer,
        > = if !state.remote_control_allowed && state.active_tab == "control" {
            container(
                container(
                    text("Remote control is disabled")
                        .size(12)
                        .font(GEIST_REGULAR)
                        .color(Color::from_rgba(0.0, 0.0, 0.0, 0.9)),
                )
                .padding(Padding {
                    top: 2.0,
                    right: 10.0,
                    bottom: 2.0,
                    left: 10.0,
                })
                .style(|_theme: &Theme| container::Style {
                    background: Some(Background::Gradient(
                        gradient::Linear::new(Radians(0.0))
                            .add_stop(
                                0.0,
                                Color::from_rgba(249.0 / 255.0, 250.0 / 255.0, 251.0 / 255.0, 0.6),
                            )
                            .add_stop(
                                1.0,
                                Color::from_rgba(153.0 / 255.0, 161.0 / 255.0, 175.0 / 255.0, 0.6),
                            )
                            .into(),
                    )),
                    border: Border {
                        radius: 100.0.into(),
                        color: Color::from_rgba(1.0, 1.0, 1.0, 0.8),
                        width: 1.0,
                    },
                    ..Default::default()
                }),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(Alignment::End)
            .align_y(Alignment::End)
            .padding(10.0)
            .into()
        } else {
            Space::new().into()
        };

        let layered_content = stack![video_content, canvas_overlay, remote_control_disabled_label];

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
                    shadow: Shadow {
                        color: Color::from_rgba(0.0, 0.0, 0.0, 0.20),
                        offset: Vector::new(0.0, 6.0),
                        blur_radius: 6.0,
                    },
                    ..Default::default()
                })
                .clip(true),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .padding(
            Padding::new(0.0)
                .left(WindowConstant::PADDING)
                .right(WindowConstant::PADDING)
                .bottom(WindowConstant::PADDING),
        );

        let main_content = column![header, content_area]
            .width(Length::Fill)
            .height(Length::Fill);

        let base: iced::Element<'a, ScreensharingMessage, Theme, iced::Renderer> =
            container(main_content)
                .width(Length::Fill)
                .height(Length::Fill)
                .style(|_theme: &Theme| container::Style {
                    background: if cfg!(target_os = "macos") {
                        Some(Background::Color(Color::from_rgba(0.31, 0.31, 0.45, 0.15)))
                    } else {
                        Some(Background::Color(ColorToken::Zinc900.to_color()))
                    },
                    border: Border {
                        radius: 10.0.into(),
                        ..Default::default()
                    },
                    ..Default::default()
                })
                .clip(true)
                .into();

        if state.dropdown_open {
            let items = [
                DropdownItemDef {
                    label: "Fade Out",
                    icon: ICON_PENCIL_SVG,
                    selected: !state.draw_persist,
                },
                DropdownItemDef {
                    label: "Persist Until Right Click",
                    icon: ICON_PENCIL_SVG,
                    selected: state.draw_persist,
                },
            ];
            let menu = crate::components::dropdown::dropdown_menu(
                &items,
                &[],
                ScreensharingMessage::DropdownItemClicked,
            );
            dropdown_overlay(
                base,
                menu,
                ScreensharingMessage::DismissDropdown,
                WindowConstant::HEADER_HEIGHT,
                WindowConstant::HEADER_SIDE_PADDING,
            )
        } else {
            base
        }
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
            ScreensharingMessage::ToggleDropdown => {
                self.state.dropdown_open = !self.state.dropdown_open;
                log::info!(
                    "ScreensharingWindow: dropdown toggled = {}",
                    self.state.dropdown_open
                );
            }
            ScreensharingMessage::DismissDropdown => {
                self.state.dropdown_open = false;
            }
            ScreensharingMessage::DropdownItemClicked(index) => {
                match index {
                    0 => self.state.draw_persist = false,
                    1 => self.state.draw_persist = true,
                    _ => {}
                }
                log::info!(
                    "ScreensharingWindow: draw_persist = {}",
                    self.state.draw_persist
                );
                self.state.dropdown_open = false;
            }
        }
    }

    /// Perform a full redraw: build UI, draw, present.
    /// Returns path IDs cleared by auto-expire during this frame.
    fn redraw(&mut self) -> Vec<u64> {
        self.redraw_inner()
    }

    fn redraw_inner(&mut self) -> Vec<u64> {
        // Check if stream dimensions changed and update window size
        let current_frame_id;
        let mut skip_buffer = false;
        {
            let frame_lock = self.screen_share_buffer.latest_frame();
            let buf = frame_lock.lock().unwrap();
            current_frame_id = buf.frame_id;

            // Reset the last_rendered_frame_id on the first frame, we do this because we might get
            // stale frame ids in the initial renders from the video buffer manager.
            if current_frame_id > 0
                && self.last_rendered_frame_id > 40
                && current_frame_id < self.last_rendered_frame_id - 40
            {
                log::info!(
                    "redraw_inner: stream restart detected (frame_id={current_frame_id}, last_rendered={}), resetting",
                    self.last_rendered_frame_id
                );
                self.last_rendered_frame_id = 0;
            }
            // Skip if we already rendered this frame (stale RedrawRequested)
            if current_frame_id > 0 && current_frame_id <= self.last_rendered_frame_id {
                log::warn!(
                    "redraw_inner: dropping redraw {current_frame_id} {}",
                    self.last_rendered_frame_id
                );
                skip_buffer = true;
            }

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

                let (width, height) = if !self.state.user_has_resized {
                    // Auto-maximize to fit monitor
                    calculate_max_window_size(self.screen_area.extent, aspect).unwrap_or_else(
                        || {
                            let content_w =
                                WindowConstant::DEFAULT_WIDTH - WindowConstant::SKELETON_W;
                            let content_h = content_w / aspect;
                            (
                                WindowConstant::DEFAULT_WIDTH,
                                content_h + WindowConstant::SKELETON_H,
                            )
                        },
                    )
                } else {
                    // User manually resized: keep current width, adjust height for new aspect
                    let current_size: winit::dpi::LogicalSize<f64> = self
                        .window
                        .inner_size()
                        .to_logical(self.window.scale_factor());
                    let content_w = current_size.width - WindowConstant::SKELETON_W;
                    let content_h = content_w / aspect;
                    (current_size.width, content_h + WindowConstant::SKELETON_H)
                };

                self.aspect_ratio_enforcer
                    .set_aspect_ratio(&self.window, aspect);

                let saved_pos = self.window.outer_position();
                self.programmatic_resize_target = Some((width, height));
                let _ = self
                    .window
                    .request_inner_size(winit::dpi::LogicalSize::new(width, height));

                if !self.state.user_has_resized {
                    self.window
                        .set_outer_position(winit::dpi::LogicalPosition::new(
                            self.screen_area.position.x,
                            self.screen_area.position.y,
                        ));
                } else if let Ok(pos) = saved_pos {
                    // Restore position so the window doesn't jump when aspect changes.
                    self.window.set_outer_position(pos);
                }

                log::info!(
                    "ScreensharingWindow: stream dimensions changed to {}x{}, aspect={:.3}, window size={:.1}x{:.1}",
                    stream_w,
                    stream_h,
                    aspect,
                    width,
                    height
                );

                // Don't render at the old size — wait for the Resized event to
                // reconfigure the surface and trigger a redraw at the correct size.
                return Vec::new();
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

        self.click_animation_renderer.update();
        self.participants_manager.hide_inactive_cursors();

        // Build fresh interface from cache
        let cache = self.cache.take().unwrap_or_default();
        let mut interface = UserInterface::build(
            Self::view(
                &self.state,
                &self.screen_share_buffer,
                &self.participants_manager,
                &self.click_animation_renderer,
                skip_buffer,
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
            Some(ColorToken::Zinc900.to_color())
        };
        wgpu_renderer.present(clear_color, output.texture.format(), &view, &self.viewport);

        self.window.pre_present_notify();
        output.present();

        // Keep the redraw loop alive while the segmented-control indicator
        // is animating, so the slide plays smoothly even when no user input
        // events are arriving.
        seg_ctrl_mod::tick_animation(&mut self.state.tab_anim);

        let cleared = self.participants_manager.update_auto_clear();

        if !skip_buffer {
            self.last_rendered_frame_id = current_frame_id;
        }
        cleared
    }
}

impl ScreensharingWindow {
    /// Sends the Stop command and extracts the redraw thread handle so it can
    /// be joined later (outside the main thread's event-loop turn) instead of
    /// in `Drop`, which would deadlock because `request_redraw()` blocks on the
    /// main thread.
    pub fn take_redraw_thread(&mut self) -> Option<std::thread::JoinHandle<()>> {
        if let Err(e) = self.redraw_tx.send(RedrawCommand::Stop) {
            log::error!("ScreensharingWindow::take_redraw_thread: failed to send Stop: {e:?}");
        }
        self.redraw_thread.take()
    }
}

impl Drop for ScreensharingWindow {
    fn drop(&mut self) {
        // If the thread wasn't already taken via take_redraw_thread(), send
        // Stop and detach — joining here would deadlock.
        if self.redraw_thread.is_some() {
            let _ = self.redraw_tx.send(RedrawCommand::Stop);
            log::warn!("ScreensharingWindow::drop: redraw thread not taken, detaching");
            drop(self.redraw_thread.take());
        }
    }
}
