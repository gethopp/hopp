use std::sync::Arc;

use iced::widget::{canvas, container};
use iced::{Background, Color, Length, Rectangle, Theme};
use iced_wgpu::core::mouse;
use iced_wgpu::graphics::Viewport;
use iced_winit::core::renderer::Style;
use iced_winit::core::time::Instant;
use iced_winit::core::{window, Event, Size};
use iced_winit::runtime::user_interface::Cache;
use iced_winit::runtime::UserInterface;
use iced_winit::{conversion, Clipboard};
use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::window::{Window, WindowAttributes, WindowId};

use thiserror::Error;

use crate::components::fonts::{self as fonts_mod, GEIST_REGULAR};
use crate::graphics::graphics_context::participant::ParticipantsManager;
use crate::graphics::graphics_window_context::{ContextManager, GraphicsWindowContextError};
use crate::room_service::DrawingMode;
use crate::utils::geometry::Position;
use crate::window::drawing_helpers;

pub fn drawing_window_attributes() -> WindowAttributes {
    use winit::window::WindowLevel;
    let attrs = WindowAttributes::default()
        .with_title("Hopp Drawing")
        .with_decorations(false)
        .with_transparent(true)
        .with_content_protected(true)
        .with_window_level(WindowLevel::AlwaysOnTop);

    #[cfg(target_os = "macos")]
    let attrs = {
        use winit::platform::macos::WindowAttributesExtMacOS;
        attrs
            .with_title_hidden(true)
            .with_titlebar_transparent(true)
            .with_fullsize_content_view(true)
    };
    attrs
}

pub(crate) enum RedrawCommand {
    Activity,
    Stop,
}

fn spawn_redraw_thread(
    redraw_rx: std::sync::mpsc::Receiver<RedrawCommand>,
    window: Arc<Window>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let redraw_interval = std::time::Duration::from_millis(16);
        let inactivity_timeout = std::time::Duration::from_secs(15);
        let mut last_activity_time = std::time::Instant::now();

        loop {
            match redraw_rx.recv_timeout(redraw_interval) {
                Ok(RedrawCommand::Stop) | Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    break
                }
                Ok(RedrawCommand::Activity) => {
                    if last_activity_time.elapsed() < redraw_interval {
                        continue;
                    }
                    last_activity_time = std::time::Instant::now();
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            }

            if last_activity_time.elapsed() > inactivity_timeout {
                continue;
            }

            window.request_redraw();
        }
    })
}

#[derive(Error, Debug)]
pub enum DrawingWindowError {
    #[error("Failed to create window")]
    WindowCreation,
    #[error("Failed to create wgpu surface")]
    SurfaceCreation,
    #[error("No suitable GPU adapter found")]
    AdapterRequest,
    #[error("Failed to request GPU device")]
    DeviceRequest,
}

#[derive(Debug)]
pub(crate) enum DrawingWindowInputEvent {
    DrawStart { x: f64, y: f64, path_id: u64 },
    DrawAddPoint { x: f64, y: f64 },
    DrawEnd { x: f64, y: f64 },
    DrawClearAllPaths,
    DrawClearPaths(Vec<u64>),
    Escape,
}

#[derive(Debug, Clone)]
pub enum DrawingMessage {}

struct DrawingOverlay<'a> {
    participants: &'a ParticipantsManager,
}

impl<'a, Message> iced::widget::canvas::Program<Message> for DrawingOverlay<'a> {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &iced::Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<iced::widget::canvas::Geometry> {
        let translate = |pos: Position| -> Position {
            Position {
                x: pos.x * bounds.width as f64,
                y: pos.y * bounds.height as f64,
            }
        };
        self.participants.draw(renderer, bounds, &translate)
    }
}

pub struct DrawingWindow {
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
    left_mouse_pressed: bool,
    current_path_id: u64,
    last_cursor_position: Option<(f64, f64)>,
    draw_persist: bool,
    participants_manager: ParticipantsManager,
    redraw_thread: Option<std::thread::JoinHandle<()>>,
    redraw_tx: std::sync::mpsc::Sender<RedrawCommand>,
    #[cfg(target_os = "macos")]
    ns_cursor_pencil: objc2::rc::Retained<objc2_app_kit::NSCursor>,
    #[cfg(not(target_os = "macos"))]
    custom_cursor_pencil: winit::window::CustomCursor,
}

impl DrawingWindow {
    pub fn new(
        context_manager: &ContextManager,
        event_loop: &ActiveEventLoop,
        permanent: bool,
        position: Option<winit::dpi::LogicalPosition<f64>>,
    ) -> Result<Self, DrawingWindowError> {
        log::info!("DrawingWindow::new");

        let mut window_attributes = drawing_window_attributes();
        window_attributes = window_attributes.with_maximized(true);
        if let Some(pos) = position {
            window_attributes = window_attributes.with_position(pos);
        }

        let window = event_loop.create_window(window_attributes).map_err(|e| {
            log::error!("DrawingWindow: failed to create window: {e:?}");
            DrawingWindowError::WindowCreation
        })?;
        let window = Arc::new(window);

        window.focus_window();

        #[cfg(target_os = "macos")]
        {
            use objc2::rc::Retained;
            use objc2_app_kit::NSView;
            use raw_window_handle::{HasWindowHandle, RawWindowHandle};
            if let Ok(raw_handle) = window.window_handle() {
                if let RawWindowHandle::AppKit(handle) = raw_handle.as_raw() {
                    let ns_view: Option<Retained<NSView>> =
                        unsafe { Retained::retain(handle.ns_view.as_ptr().cast()) };
                    if let Some(ns_window) = ns_view.and_then(|v| v.window()) {
                        ns_window.disableCursorRects();
                    }
                }
            }
        }

        let surface_info =
            context_manager
                .create_drawing_surface(&window)
                .map_err(|e| match e {
                    GraphicsWindowContextError::SurfaceCreation => {
                        DrawingWindowError::SurfaceCreation
                    }
                    GraphicsWindowContextError::AdapterRequest => {
                        DrawingWindowError::AdapterRequest
                    }
                    GraphicsWindowContextError::DeviceRequest => DrawingWindowError::DeviceRequest,
                })?;
        let device = context_manager.drawing_context.device.clone();
        let format = surface_info.format;
        let alpha_mode = surface_info.alpha_mode;
        let surface = surface_info.surface;
        let physical_size = window.inner_size();

        let wgpu_renderer = iced_wgpu::Renderer::new(
            context_manager.drawing_context.engine.clone(),
            GEIST_REGULAR,
            iced::Pixels::from(16),
        );

        fonts_mod::load_fonts();

        let renderer = iced::Renderer::Primary(wgpu_renderer);

        let viewport = Viewport::with_physical_size(
            Size::new(physical_size.width.max(1), physical_size.height.max(1)),
            window.scale_factor() as f32,
        );
        let clipboard = Clipboard::connect(window.clone());

        let px = (drawing_helpers::CURSOR_LOGICAL_SIZE * 4.0).round() as u32;
        let (pencil_rgba, ew, eh) =
            drawing_helpers::rasterize_svg_to_rgba(drawing_helpers::CURSOR_ICON_PENCIL, px);

        #[cfg(target_os = "macos")]
        let ns_cursor_pencil = drawing_helpers::create_macos_cursor(
            &pencil_rgba,
            ew,
            eh,
            drawing_helpers::CURSOR_LOGICAL_SIZE,
            drawing_helpers::CURSOR_LOGICAL_SIZE,
            2.0,
            29.0,
        );

        #[cfg(not(target_os = "macos"))]
        let custom_cursor_pencil = event_loop.create_custom_cursor(
            winit::window::CustomCursor::from_rgba(pencil_rgba, ew as u16, eh as u16, 2, 29)
                .expect("create pencil cursor"),
        );

        let mut participants_manager = ParticipantsManager::new();
        if let Err(e) = participants_manager.add_participant(
            drawing_helpers::LOCAL_PARTICIPANT_IDENTITY.to_string(),
            drawing_helpers::LOCAL_PARTICIPANT_IDENTITY,
            true,
            DrawingMode::Draw(crate::room_service::DrawSettings { permanent }),
        ) {
            log::warn!("DrawingWindow::new: failed to add local participant: {e:?}");
        }

        let (redraw_tx, redraw_rx) = std::sync::mpsc::channel();
        let redraw_thread = Some(spawn_redraw_thread(redraw_rx, window.clone()));

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
            left_mouse_pressed: false,
            current_path_id: 0,
            last_cursor_position: None,
            draw_persist: permanent,
            participants_manager,
            redraw_thread,
            redraw_tx,
            #[cfg(target_os = "macos")]
            ns_cursor_pencil,
            #[cfg(not(target_os = "macos"))]
            custom_cursor_pencil,
        };
        Ok(s)
    }

    pub fn window_id(&self) -> WindowId {
        self.window.id()
    }

    fn signal_activity(&self) {
        let _ = self.redraw_tx.send(RedrawCommand::Activity);
    }

    pub fn take_redraw_thread(&mut self) -> Option<std::thread::JoinHandle<()>> {
        if let Err(e) = self.redraw_tx.send(RedrawCommand::Stop) {
            log::error!("DrawingWindow::take_redraw_thread: failed to send Stop: {e:?}");
        }
        self.redraw_thread.take()
    }

    pub fn hide(&self) {
        self.window.set_visible(false);
    }

    pub fn set_draw_persist(&mut self, permanent: bool) {
        self.draw_persist = permanent;
        self.participants_manager.set_drawing_mode(
            drawing_helpers::LOCAL_PARTICIPANT_IDENTITY,
            crate::room_service::DrawingMode::Draw(crate::room_service::DrawSettings { permanent }),
        );
    }

    fn set_default_cursor(&self) {
        #[cfg(target_os = "macos")]
        {
            use objc2_app_kit::NSCursor;
            NSCursor::arrowCursor().set();
        }
        #[cfg(not(target_os = "macos"))]
        {
            self.window.set_cursor(winit::window::Cursor::Icon(
                winit::window::CursorIcon::Default,
            ));
        }
    }

    fn set_pencil_cursor(&self) {
        #[cfg(target_os = "macos")]
        {
            self.ns_cursor_pencil.set();
        }
        #[cfg(not(target_os = "macos"))]
        {
            self.window.set_cursor(winit::window::Cursor::Custom(
                self.custom_cursor_pencil.clone(),
            ));
        }
    }

    fn view<'a>(
        participants: &'a ParticipantsManager,
    ) -> iced::Element<'a, DrawingMessage, Theme, iced::Renderer> {
        let canvas_overlay = canvas(DrawingOverlay { participants })
            .width(Length::Fill)
            .height(Length::Fill);

        container(canvas_overlay)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(|_theme: &Theme| container::Style {
                background: Some(Background::Color(Color::TRANSPARENT)),
                ..Default::default()
            })
            .into()
    }

    pub fn handle_window_event(&mut self, event: WindowEvent) -> Option<DrawingWindowInputEvent> {
        let mut input_event = None;
        let scale_factor = self.window.scale_factor() as f32;

        match &event {
            WindowEvent::CursorMoved { position, .. } => {
                let physical_size = self.window.inner_size();
                let pct_x = (position.x / physical_size.width as f64).clamp(0.0, 1.0);
                let pct_y = (position.y / physical_size.height as f64).clamp(0.0, 1.0);
                self.last_cursor_position = Some((pct_x, pct_y));

                if self.left_mouse_pressed {
                    self.participants_manager.draw_add_point(
                        drawing_helpers::LOCAL_PARTICIPANT_IDENTITY,
                        Position { x: pct_x, y: pct_y },
                    );
                    input_event =
                        Some(DrawingWindowInputEvent::DrawAddPoint { x: pct_x, y: pct_y });
                    self.signal_activity();
                }
            }
            WindowEvent::CursorEntered { .. } => {
                log::info!("drawing_window: cursor entered");
                self.set_pencil_cursor();
            }
            WindowEvent::CursorLeft { .. } => {
                if self.left_mouse_pressed {
                    if let Some((lx, ly)) = self.last_cursor_position {
                        self.participants_manager.draw_end(
                            drawing_helpers::LOCAL_PARTICIPANT_IDENTITY,
                            Position { x: lx, y: ly },
                        );
                        input_event = Some(DrawingWindowInputEvent::DrawEnd { x: lx, y: ly });
                    }
                    self.left_mouse_pressed = false;
                    self.signal_activity();
                }
                self.set_default_cursor();
            }
            WindowEvent::Focused(true) => {
                self.set_pencil_cursor();
            }
            WindowEvent::Focused(false) => {
                if self.left_mouse_pressed {
                    if let Some((lx, ly)) = self.last_cursor_position {
                        self.participants_manager.draw_end(
                            drawing_helpers::LOCAL_PARTICIPANT_IDENTITY,
                            Position { x: lx, y: ly },
                        );
                        input_event = Some(DrawingWindowInputEvent::DrawEnd { x: lx, y: ly });
                    }
                    self.left_mouse_pressed = false;
                    self.signal_activity();
                }
                self.set_default_cursor();
            }
            WindowEvent::MouseInput { state, button, .. } => {
                self.set_pencil_cursor();
                let (pct_x, pct_y) = match &self.cursor {
                    mouse::Cursor::Available(pos) => {
                        let physical_size = self.window.inner_size();
                        let logical_width = physical_size.width as f64 / scale_factor as f64;
                        let logical_height = physical_size.height as f64 / scale_factor as f64;
                        (
                            (pos.x as f64 / logical_width).clamp(0.0, 1.0),
                            (pos.y as f64 / logical_height).clamp(0.0, 1.0),
                        )
                    }
                    _ => (0.0, 0.0),
                };

                if *button == winit::event::MouseButton::Left {
                    if state.is_pressed() {
                        self.current_path_id += 1;
                        self.left_mouse_pressed = true;
                        self.participants_manager.draw_start(
                            drawing_helpers::LOCAL_PARTICIPANT_IDENTITY,
                            Position { x: pct_x, y: pct_y },
                            self.current_path_id,
                        );
                        input_event = Some(DrawingWindowInputEvent::DrawStart {
                            x: pct_x,
                            y: pct_y,
                            path_id: self.current_path_id,
                        });
                        self.signal_activity();
                    } else {
                        self.left_mouse_pressed = false;
                        self.participants_manager.draw_end(
                            drawing_helpers::LOCAL_PARTICIPANT_IDENTITY,
                            Position { x: pct_x, y: pct_y },
                        );
                        input_event = Some(DrawingWindowInputEvent::DrawEnd { x: pct_x, y: pct_y });
                        self.signal_activity();
                    }
                } else if *button == winit::event::MouseButton::Right
                    && state.is_pressed()
                    && self.draw_persist
                {
                    self.participants_manager
                        .draw_clear_all_paths(drawing_helpers::LOCAL_PARTICIPANT_IDENTITY);
                    input_event = Some(DrawingWindowInputEvent::DrawClearAllPaths);
                    self.signal_activity();
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if event.state.is_pressed() {
                    if let Key::Named(NamedKey::Escape) = event.logical_key {
                        input_event = Some(DrawingWindowInputEvent::Escape);
                    }
                }
            }
            WindowEvent::Resized(new_size) => {
                if new_size.width > 0 && new_size.height > 0 {
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
                    self.viewport = Viewport::with_physical_size(
                        Size::new(new_size.width, new_size.height),
                        self.window.scale_factor() as f32,
                    );
                    self.signal_activity();
                }
            }
            WindowEvent::RedrawRequested => {
                let cleared = self.redraw();
                if !cleared.is_empty() {
                    input_event = Some(DrawingWindowInputEvent::DrawClearPaths(cleared));
                }
            }
            WindowEvent::CloseRequested => {
                self.window.set_visible(false);
            }
            _ => {}
        }

        if !matches!(event, WindowEvent::RedrawRequested) {
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

                let mut messages: Vec<DrawingMessage> = Vec::new();

                let cache = self.cache.take().unwrap_or_default();
                let mut interface = UserInterface::build(
                    Self::view(&self.participants_manager),
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
                    let (_, _) = interface.update(
                        &[ev],
                        self.cursor,
                        &mut self.renderer,
                        &mut self.clipboard,
                        &mut messages,
                    );
                }

                self.cache = Some(interface.into_cache());
            }
        }

        if let WindowEvent::ModifiersChanged(new_modifiers) = event {
            self.modifiers = new_modifiers.state();
        }

        input_event
    }

    fn redraw(&mut self) -> Vec<u64> {
        self.participants_manager.hide_inactive_cursors();

        let cleared = self.participants_manager.update_auto_clear();

        let output = match self.surface.get_current_texture() {
            Ok(output) => output,
            Err(e) => {
                log::error!("DrawingWindow::redraw: failed to get texture: {e:?}");
                return cleared;
            }
        };

        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let cache = self.cache.take().unwrap_or_default();
        let mut interface = UserInterface::build(
            Self::view(&self.participants_manager),
            self.viewport.logical_size(),
            cache,
            &mut self.renderer,
        );

        let _ = interface.update(
            &[Event::Window(
                window::Event::RedrawRequested(Instant::now()),
            )],
            self.cursor,
            &mut self.renderer,
            &mut self.clipboard,
            &mut Vec::new(),
        );

        interface.draw(
            &mut self.renderer,
            &Theme::Dark,
            &Style {
                text_color: Color::WHITE,
            },
            self.cursor,
        );
        self.cache = Some(interface.into_cache());

        let wgpu_renderer = match &mut self.renderer {
            iced::Renderer::Primary(r) => r,
            _ => unreachable!(),
        };
        wgpu_renderer.present(
            Some(Color::TRANSPARENT),
            output.texture.format(),
            &view,
            &self.viewport,
        );

        self.window.pre_present_notify();
        output.present();

        cleared
    }
}
