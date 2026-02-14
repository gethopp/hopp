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

use iced::widget::{column, container, image as image_widget, row, stack, text, Space};
use iced::{
    gradient, Alignment, Background, Border, Color, ContentFit, Length, Padding, Pixels, Radians,
    Rectangle,
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
#[cfg(not(target_os = "macos"))]
use winit::window::{CursorIcon, CustomCursor};
use winit::window::{Window, WindowAttributes, WindowId};

use thiserror::Error;

use fontdb::Database;
use resvg::{tiny_skia, usvg};

use crate::components::dropdown::{self as dropdown_mod, DropdownItemDef};
use crate::components::fonts::{self as fonts_mod, GEIST_MEDIUM, GEIST_REGULAR};
use crate::components::segmented_control::{
    self as seg_ctrl_mod, SegmentedButton, SegmentedControlAnim,
};
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

const ICON_COG: &[u8] = include_bytes!("../resources/icons/cog.svg");
const ICON_WAND: &[u8] = include_bytes!("../resources/icons/wand.svg");
const ICON_PENCIL: &[u8] = include_bytes!("../resources/icons/pencil.svg");
const ICON_CLICKER: &[u8] = include_bytes!("../resources/icons/clicker.svg");
const CURSOR_ICON_POINTER: &[u8] =
    include_bytes!("../resources/icons/local-participant-cursor.svg");
const CURSOR_ICON_PENCIL: &[u8] = include_bytes!("../resources/icons/local-participant-pencil.svg");
const PARTICIPANT1_IMG: &[u8] = include_bytes!("../resources/icons/participant1.png");
const PARTICIPANT2_IMG: &[u8] = include_bytes!("../resources/icons/participant2.png");

// ── Segmented control buttons ────────────────────────────────────────────────
const SEGMENTED_BUTTONS: &[SegmentedButton] = &[
    SegmentedButton {
        id: "control",
        icon: ICON_WAND,
        description: Some("Remote control"),
    },
    SegmentedButton {
        id: "draw",
        icon: ICON_PENCIL,
        description: Some("Draw"),
    },
    SegmentedButton {
        id: "point",
        icon: ICON_CLICKER,
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

// ── Dropdown menu item definitions ──────────────────────────────────────────

const DROPDOWN_ITEMS: &[DropdownItemDef] = &[
    DropdownItemDef {
        label: "Participant 1",
        icon: ICON_COG,
    },
    DropdownItemDef {
        label: "Participant 2",
        icon: ICON_COG,
    },
];

/// Items shown below the divider in the dropdown menu.
const DROPDOWN_ITEMS_SECONDARY: &[DropdownItemDef] = &[DropdownItemDef {
    label: "Interesting button",
    icon: ICON_COG,
}];

// ── Iced messages ───────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum ScreensharingMessage {
    TabSelected(&'static str),
    ToggleDropdown,
    DismissDropdown,
    DropdownItemClicked(usize),
}

// ── Application state for the screensharing UI ─────────────────────────────

struct ScreensharingState {
    active_tab: &'static str,
    dropdown_open: bool,
    /// Animation state for the segmented-control indicator slide.
    tab_anim: Option<SegmentedControlAnim>,
    /// 0 = participant1, 1 = participant2
    current_participant: usize,
    /// Cached image handle — created once per participant switch, reused every frame.
    participant_handle: image_widget::Handle,
    /// Cached image width/height ratio (iw/ih) — avoids decoding during resize.
    img_aspect: f64,
}

impl Default for ScreensharingState {
    fn default() -> Self {
        let (handle, aspect) = participant_handle_and_aspect(PARTICIPANT1_IMG);
        Self {
            active_tab: SEGMENTED_BUTTONS[0].id,
            dropdown_open: false,
            tab_anim: None,
            current_participant: 0,
            participant_handle: handle,
            img_aspect: aspect,
        }
    }
}

/// Compute window dimensions and aspect ratio from image bytes.
/// Returns (window_width, window_height, aspect_ratio).
fn image_window_geometry(img_bytes: &[u8], target_width: f64) -> (f64, f64, f64) {
    use image::GenericImageView;
    let img = image::load_from_memory(img_bytes).expect("load participant image");
    let (iw, ih) = img.dimensions();
    let img_aspect = iw as f64 / ih as f64;

    let content_w = target_width - (CONTENT_PADDING as f64 * 2.0);
    let content_h = content_w / img_aspect;
    let win_w = target_width;
    let win_h = content_h + HEADER_CHROME_HEIGHT as f64 + CONTENT_PADDING as f64;
    (win_w, win_h, win_w / win_h)
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

/// Decode participant image to an opaque RGBA handle and return (handle, img_aspect).
fn participant_handle_and_aspect(img_bytes: &'static [u8]) -> (image_widget::Handle, f64) {
    use image::GenericImageView;
    match image::load_from_memory(img_bytes) {
        Ok(img) => {
            let (iw, ih) = img.dimensions();
            let aspect = iw as f64 / ih as f64;
            let mut rgba = img.to_rgba8();
            for px in rgba.pixels_mut() {
                px.0[3] = 255; // fully opaque
            }
            let handle =
                image_widget::Handle::from_rgba(rgba.width(), rgba.height(), rgba.into_raw());
            (handle, aspect)
        }
        Err(_) => (image_widget::Handle::from_bytes(img_bytes), 16.0 / 9.0),
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
    resized: bool,
    /// True when the mouse cursor is inside the participant image area.
    mouse_in_participant_area: bool,
    #[cfg(target_os = "macos")]
    ns_cursor_pointer: objc2::rc::Retained<objc2_app_kit::NSCursor>,
    #[cfg(target_os = "macos")]
    ns_cursor_pencil: objc2::rc::Retained<objc2_app_kit::NSCursor>,
    #[cfg(not(target_os = "macos"))]
    custom_cursor_pointer: winit::window::CustomCursor,
    #[cfg(not(target_os = "macos"))]
    custom_cursor_pencil: winit::window::CustomCursor,
}

impl ScreensharingWindow {
    /// Create a new screensharing window with wgpu surface and iced renderer.
    pub fn new(event_loop: &ActiveEventLoop) -> Result<Self, ScreensharingWindowError> {
        log::info!("ScreensharingWindow::new");

        // ── Create winit window ──────────────────────────────────────────
        let (init_w, init_h, _) =
            image_window_geometry(PARTICIPANT1_IMG, SCREENSHARING_WINDOW_WIDTH);
        let (min_w, min_h, _) =
            image_window_geometry(PARTICIPANT1_IMG, SCREENSHARING_WINDOW_MIN_WIDTH);

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
            power_preference: wgpu::PowerPreference::LowPower,
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
            alpha_mode: if cfg!(target_os = "macos") {
                wgpu::CompositeAlphaMode::PostMultiplied
            } else {
                wgpu::CompositeAlphaMode::Opaque
            },
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
            resized: false,
            mouse_in_participant_area: false,
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
    pub fn handle_window_event(&mut self, event: WindowEvent) {
        // Participant area event gating: update cursor-in-rect state and log input
        // when inside the participant image area. All events still flow to iced.
        let scale_factor = self.window.scale_factor() as f32;
        let rect = self.participant_image_rect();

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
                if inside && !was_inside {
                    let pct_x = (logical_x - rect.x) / rect.width;
                    let pct_y = (logical_y - rect.y) / rect.height;
                    log::warn!(
                        "ScreensharingWindow: cursor entered participant area at ({:.3}, {:.3})",
                        pct_x,
                        pct_y
                    );
                } else if !inside && was_inside {
                    log::warn!("ScreensharingWindow: cursor left participant area");
                }
            }
            // Reset when cursor leaves the window — winit skips the final
            // CursorMoved when the cursor exits quickly.
            WindowEvent::CursorLeft { .. } => {
                if self.mouse_in_participant_area {
                    self.mouse_in_participant_area = false;
                    self.update_cursor();
                    log::warn!("ScreensharingWindow: cursor left participant area (CursorLeft)");
                }
            }
            // Also reset when the window loses focus so stale state doesn't
            // linger while the user interacts with another window.
            WindowEvent::Focused(false) => {
                if self.mouse_in_participant_area {
                    self.mouse_in_participant_area = false;
                    self.update_cursor();
                    log::warn!(
                        "ScreensharingWindow: cursor left participant area (window unfocused)"
                    );
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                if self.mouse_in_participant_area {
                    let (pct_x, pct_y) = match &self.cursor {
                        mouse::Cursor::Available(pos) => (
                            (pos.x - rect.x) / rect.width,
                            (pos.y - rect.y) / rect.height,
                        ),
                        _ => (0.0, 0.0),
                    };
                    log::warn!(
                        "ScreensharingWindow: [participant_area] mouse button {:?} {:?} at ({:.3}, {:.3})",
                        button,
                        state,
                        pct_x,
                        pct_y
                    );
                } else {
                    log::warn!(
                        "ScreensharingWindow: [outside] mouse {:?} {:?} ignored",
                        button,
                        state
                    );
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                if self.mouse_in_participant_area {
                    log::warn!(
                        "ScreensharingWindow: [participant_area] scroll delta {:?}",
                        delta
                    );
                } else {
                    log::warn!("ScreensharingWindow: [outside] scroll ignored");
                }
            }
            WindowEvent::KeyboardInput {
                event: key_event, ..
            } => {
                if self.mouse_in_participant_area {
                    log::warn!(
                        "ScreensharingWindow: [participant_area] key {:?} {:?}",
                        key_event.logical_key,
                        key_event.state
                    );
                } else {
                    log::warn!(
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

            // Tick animation; keep requesting redraws while it runs.
            seg_ctrl_mod::tick_animation(&mut self.state.tab_anim);
            if seg_ctrl_mod::animation_running(&self.state.tab_anim) {
                self.window.request_redraw();
            }
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

    fn view(
        state: &ScreensharingState,
    ) -> iced::Element<'_, ScreensharingMessage, Theme, iced::Renderer> {
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

        let dropdown_btn = dropdown_mod::dropdown_trigger_button(
            ICON_COG,
            state.dropdown_open,
            ScreensharingMessage::ToggleDropdown,
        );

        // ── Header: stack-based layout so the segmented control is truly
        //    centered across the full window width, independent of name/cog widths.
        //    Layer 1: name on the left, cog on the right
        //    Layer 2: segmented control absolutely centered
        let header_ends = row![
            Space::new().width(Length::Fixed(68.0)), // Space for native macOS traffic lights
            Space::new().width(Length::Fixed(0.0)),  // gap before name
            name_label,
            Space::new().width(Length::Fill),
            dropdown_btn,
            Space::new().width(Length::Fixed(2.0)), // Right spacing for cog alignment
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

        // ── Content area (participant image) ─────────────────────────────
        let participant_img = image_widget(state.participant_handle.clone())
            .width(Length::Fill)
            .height(Length::Fill)
            .content_fit(ContentFit::Cover);

        let content_area = container(
            container(
                container(participant_img)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .center_x(Length::Fill)
                    .center_y(Length::Fill),
            )
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

        let outer_frame = container(main_content)
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
            });

        // ── Dropdown overlay (conditionally shown via stack) ────────────
        if state.dropdown_open {
            // The header height is approximately CONTENT_PADDING*2 + button_height
            let dropdown_top_offset = CONTENT_PADDING * 2.0 + 12.0;

            let menu = dropdown_mod::dropdown_menu(
                DROPDOWN_ITEMS,
                DROPDOWN_ITEMS_SECONDARY,
                ScreensharingMessage::DropdownItemClicked,
            );

            // Align dropdown right edge with cog button: header right padding + gap after cog
            let dropdown_right_padding = HEADER_RIGHT_PADDING + 2.0;

            dropdown_mod::dropdown_overlay(
                outer_frame.into(),
                menu,
                ScreensharingMessage::DismissDropdown,
                dropdown_top_offset,
                dropdown_right_padding,
            )
        } else {
            outer_frame.into()
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
                    "ScreensharingWindow: dropdown_open = {}",
                    self.state.dropdown_open
                );
            }
            ScreensharingMessage::DismissDropdown => {
                self.state.dropdown_open = false;
                log::info!("ScreensharingWindow: dropdown dismissed");
            }
            ScreensharingMessage::DropdownItemClicked(index) => {
                if matches!(index, 0 | 1) {
                    let img_bytes = if index == 0 {
                        PARTICIPANT1_IMG
                    } else {
                        PARTICIPANT2_IMG
                    };
                    self.state.current_participant = index;
                    let (handle, aspect) = participant_handle_and_aspect(img_bytes);
                    self.state.participant_handle = handle;
                    self.state.img_aspect = aspect;
                    let (w, h, _) = image_window_geometry(img_bytes, SCREENSHARING_WINDOW_WIDTH);
                    #[cfg(target_os = "macos")]
                    {
                        // Update the OS-enforced aspect ratio + min size for the new image.
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
                }
                self.state.dropdown_open = false;
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
                        alpha_mode: if cfg!(target_os = "macos") {
                            wgpu::CompositeAlphaMode::PostMultiplied
                        } else {
                            wgpu::CompositeAlphaMode::Opaque
                        },
                        view_formats: vec![],
                        desired_maximum_frame_latency: 2,
                    },
                );
                // Always sync viewport to the actual surface size so the
                // scissor rect can never exceed the render target.
                self.viewport = Viewport::with_physical_size(
                    Size::new(size.width, size.height),
                    self.window.scale_factor() as f32,
                );
            }
            self.resized = false;
        }

        let output = match self.surface.get_current_texture() {
            Ok(output) => output,
            Err(e) => {
                log::error!("ScreensharingWindow::redraw: failed to get texture: {e:?}");
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
        if seg_ctrl_mod::animation_running(&self.state.tab_anim) {
            self.window.request_redraw();
        }
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

        log::info!(
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
