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

use iced::widget::{column, container, row, stack, text, Space};
use iced::{gradient, Alignment, Background, Border, Color, Length, Padding, Pixels, Radians};
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

use crate::components::dropdown::{self as dropdown_mod, DropdownItemDef};
use crate::components::fonts::{self as fonts_mod, GEIST_MEDIUM, GEIST_REGULAR};
use crate::components::segmented_control::{
    self as seg_ctrl_mod, SegmentedButton, SegmentedControlAnim,
};
use crate::windows::colors::ColorToken;
use crate::windows::shadows::ShadowToken;

/// Sizing constants
const SCREENSHARING_WINDOW_WIDTH: f64 = 600.0; // logical pixels
const SCREENSHARING_WINDOW_HEIGHT: f64 = 350.0;
const SCREENSHARING_WINDOW_MIN_WIDTH: f64 = 600.0;
const CONTENT_PADDING: f32 = 12.0;

const ICON_COG: &[u8] = include_bytes!("../resources/icons/cog.svg");
const ICON_WAND: &[u8] = include_bytes!("../resources/icons/wand.svg");
const ICON_PENCIL: &[u8] = include_bytes!("../resources/icons/pencil.svg");

// ── Segmented control buttons ────────────────────────────────────────────────
const SEGMENTED_BUTTONS: &[SegmentedButton] = &[
    SegmentedButton {
        id: "magic",
        icon: ICON_WAND,
    },
    SegmentedButton {
        id: "wand",
        icon: ICON_WAND,
    },
    SegmentedButton {
        id: "draw",
        icon: ICON_PENCIL,
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
        label: "Fade out",
        icon: ICON_COG,
    },
    DropdownItemDef {
        label: "Persist until right click",
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

#[derive(Debug)]
struct ScreensharingState {
    sharing: bool,
    active_tab: &'static str,
    dropdown_open: bool,
    /// Animation state for the segmented-control indicator slide.
    tab_anim: Option<SegmentedControlAnim>,
}

impl Default for ScreensharingState {
    fn default() -> Self {
        Self {
            sharing: false,
            active_tab: SEGMENTED_BUTTONS[0].id,
            dropdown_open: false,
            tab_anim: None,
        }
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
}

impl ScreensharingWindow {
    /// Create a new screensharing window with wgpu surface and iced renderer.
    pub fn new(event_loop: &ActiveEventLoop) -> Result<Self, ScreensharingWindowError> {
        log::info!("ScreensharingWindow::new");

        // ── Create winit window ──────────────────────────────────────────
        let attrs = WindowAttributes::default()
            .with_title("Hopp Screensharing")
            .with_inner_size(winit::dpi::LogicalSize::new(
                SCREENSHARING_WINDOW_WIDTH,
                SCREENSHARING_WINDOW_HEIGHT,
            ))
            .with_resizable(true)
            .with_min_inner_size(winit::dpi::LogicalSize::new(
                SCREENSHARING_WINDOW_MIN_WIDTH,
                SCREENSHARING_WINDOW_HEIGHT,
            ));

        #[cfg(target_os = "macos")]
        let attrs = {
            use winit::platform::macos::WindowAttributesExtMacOS;
            attrs
                .with_title_hidden(true)
                .with_titlebar_transparent(true)
                .with_fullsize_content_view(true)
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
            state: ScreensharingState::default(),
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
                    self.viewport = Viewport::with_physical_size(
                        Size::new(new_size.width, new_size.height),
                        self.window.scale_factor() as f32,
                    );
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
        let name_label = text("Costa's Screen")
            .size(14)
            .color(Color::WHITE)
            .font(GEIST_MEDIUM);

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
            Space::new().width(Length::Fixed(72.0)), // Space for native macOS traffic lights
            Space::new().width(Length::Fixed(8.0)),  // gap before name
            name_label,
            Space::new().width(Length::Fill),
            dropdown_btn,
            Space::new().width(Length::Fixed(12.0)),
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
                right: CONTENT_PADDING,
                bottom: CONTENT_PADDING,
                left: CONTENT_PADDING,
            });

        // ── Content area (screen preview placeholder) ────────────────────
        let status_text = if state.sharing {
            "Screen sharing active"
        } else {
            "Ready to share your screen"
        };

        let status_badge = status_label(status_text, state.sharing);

        let content_area = container(
            container(
                container(status_badge)
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
                background: Some(Background::Color(ColorToken::Slate600.to_color())),
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

            // Align dropdown right edge with cog button: header padding + gap after cog
            let dropdown_right_padding = CONTENT_PADDING + 12.0;

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
                match index {
                    0 => {
                        log::info!("ScreensharingWindow: fade out selected");
                        self.state.sharing = !self.state.sharing;
                    }
                    1 => {
                        log::info!("ScreensharingWindow: persist until right click selected");
                    }
                    _ => {
                        log::info!(
                            "ScreensharingWindow: unknown dropdown item clicked = {}",
                            index
                        );
                    }
                }
                self.state.dropdown_open = false;
                log::info!("ScreensharingWindow: dropdown item clicked = {}", index);
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
        wgpu_renderer.present(None, output.texture.format(), &view, &self.viewport);

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
