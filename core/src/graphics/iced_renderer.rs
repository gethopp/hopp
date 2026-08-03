use std::sync::Arc;

use crate::components::fonts as fonts_mod;
use crate::graphics::graphics_window_context::ContextManager;
use crate::utils::geometry::Position;
use crate::{SelectionMode, SelectionOverlayState};
use iced::Renderer;
use iced::{Font, Pixels};
use iced_wgpu::graphics::Viewport;
use iced_winit::core::mouse;
use iced_winit::core::{renderer, time::Instant, window, Event, Theme};
use iced_winit::{
    conversion,
    core::Size,
    runtime::{user_interface, UserInterface},
    Clipboard,
};
use winit::event::WindowEvent;
use winit::keyboard::ModifiersState;
use winit::window::Window;

#[path = "iced_canvas.rs"]
mod iced_canvas;
use iced_canvas::{Message, OverlaySurface};

use super::click_animation::ClickAnimationRenderer;
use super::participant::ParticipantsManager;

#[derive(Clone, Copy)]
pub(crate) struct DrawArgs<'a> {
    pub(crate) frame: &'a wgpu::SurfaceTexture,
    pub(crate) view: &'a wgpu::TextureView,
    pub(crate) participants: &'a ParticipantsManager,
    pub(crate) click_animation_renderer: &'a ClickAnimationRenderer,
    pub(crate) position_translator: &'a dyn Fn(Position) -> Position,
    pub(crate) screen_selection: Option<SelectionOverlayState>,
    pub(crate) window_focused: bool,
}

pub(crate) struct IcedRenderer {
    renderer: Renderer,
    viewport: Viewport,
    clipboard: Clipboard,
    overlay_surface: OverlaySurface,
    cursor: mouse::Cursor,
    cache: Option<user_interface::Cache>,
}

impl std::fmt::Debug for IcedRenderer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "IcedRenderer")
    }
}

impl IcedRenderer {
    pub(crate) fn new(context_manager: &ContextManager, window: &Arc<Window>) -> Self {
        let engine = context_manager.overlay_context.engine.clone();
        let physical_size = window.inner_size();
        let viewport = Viewport::with_physical_size(
            Size::new(physical_size.width, physical_size.height),
            window.scale_factor() as f32,
        );
        let clipboard = Clipboard::connect(window.clone());
        let overlay_surface = OverlaySurface::new();
        fonts_mod::load_fonts();
        let wgpu_renderer = iced_wgpu::Renderer::new(engine, Font::default(), Pixels::from(16));
        let renderer = Renderer::Primary(wgpu_renderer);
        Self {
            renderer,
            viewport,
            clipboard,
            overlay_surface,
            cursor: mouse::Cursor::Unavailable,
            cache: None,
        }
    }

    pub(crate) fn reset(&mut self, engine: iced_wgpu::Engine) {
        self.renderer = Renderer::Primary(iced_wgpu::Renderer::new(
            engine,
            Font::default(),
            Pixels::from(16),
        ));
    }

    pub(crate) fn resize(&mut self, new_size: winit::dpi::PhysicalSize<u32>, scale_factor: f64) {
        self.viewport = Viewport::with_physical_size(
            Size::new(new_size.width, new_size.height),
            scale_factor as f32,
        );
    }

    pub(crate) fn handle_screen_selection_event(
        &mut self,
        event: &WindowEvent,
        scale_factor: f32,
        screen_selection: SelectionOverlayState,
        window_focused: bool,
    ) -> (bool, Option<SelectionMode>) {
        let Some(iced_event) =
            conversion::window_event(event.clone(), scale_factor, ModifiersState::default())
        else {
            return (false, None);
        };

        match &iced_event {
            Event::Mouse(iced::mouse::Event::CursorMoved { position }) => {
                self.cursor = mouse::Cursor::Available(*position);
            }
            Event::Mouse(iced::mouse::Event::CursorLeft) => {
                self.cursor = mouse::Cursor::Unavailable;
            }
            _ => {}
        }

        let mut messages = Vec::new();
        let mut interface = UserInterface::build(
            OverlaySurface::screen_selection_view(screen_selection, window_focused),
            self.viewport.logical_size(),
            self.cache.take().unwrap_or_default(),
            &mut self.renderer,
        );
        let (_, statuses) = interface.update(
            &[iced_event],
            self.cursor,
            &mut self.renderer,
            &mut self.clipboard,
            &mut messages,
        );
        self.cache = Some(interface.into_cache());

        let selected_mode = messages.into_iter().next().map(|message| match message {
            Message::SetSelectionMode(mode) => mode,
        });
        let captured = statuses.contains(&iced::event::Status::Captured);

        (captured, selected_mode)
    }

    pub(crate) fn draw(&mut self, args: DrawArgs) {
        let DrawArgs {
            frame,
            view,
            participants,
            click_animation_renderer,
            position_translator,
            screen_selection,
            window_focused,
        } = args;

        let mut interface = UserInterface::build(
            self.overlay_surface.view(
                participants,
                click_animation_renderer,
                position_translator,
                screen_selection,
                window_focused,
            ),
            self.viewport.logical_size(),
            self.cache.take().unwrap_or_default(),
            &mut self.renderer,
        );

        let (_, _) = interface.update(
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
            &renderer::Style::default(),
            self.cursor,
        );
        self.cache = Some(interface.into_cache());

        let wgpu_renderer = match &mut self.renderer {
            Renderer::Primary(renderer) => renderer,
            _ => unreachable!(),
        };
        wgpu_renderer.present(None, frame.texture.format(), view, &self.viewport);
    }
}
