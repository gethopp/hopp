use std::sync::Arc;

use crate::components::fonts as fonts_mod;
use crate::graphics::graphics_window_context::ContextManager;
use crate::screen_selection::ScreenSelectionUi;
use crate::utils::geometry::Position;
use iced::Renderer;
use iced::{Font, Pixels};
use iced_wgpu::graphics::Viewport;
use iced_winit::core::mouse;
use iced_winit::core::{renderer, time::Instant, window, Event, Theme};
use iced_winit::{
    core::Size,
    runtime::{user_interface, UserInterface},
    Clipboard,
};
use winit::dpi::PhysicalPosition;
use winit::window::Window;

#[path = "iced_canvas.rs"]
mod iced_canvas;
use iced_canvas::{Message as SelectionMessage, OverlaySurface};

use super::click_animation::ClickAnimationRenderer;
use super::participant::ParticipantsManager;

pub use iced_canvas::Message as ScreenSelectionMessage;

#[derive(Clone, Copy)]
pub struct DrawArgs<'a> {
    pub frame: &'a wgpu::SurfaceTexture,
    pub view: &'a wgpu::TextureView,
    pub participants: &'a ParticipantsManager,
    pub click_animation_renderer: &'a ClickAnimationRenderer,
    pub position_translator: &'a dyn Fn(Position) -> Position,
    pub screen_selection: Option<&'a ScreenSelectionUi>,
    pub window_focused: bool,
    pub marker_content: Option<iced::Rectangle>,
}

pub struct IcedRenderer {
    renderer: Renderer,
    viewport: Viewport,
    clipboard: Clipboard,
    overlay_surface: OverlaySurface,
    cursor: mouse::Cursor,
}

impl std::fmt::Debug for IcedRenderer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "IcedRenderer")
    }
}

impl IcedRenderer {
    pub fn new(
        context_manager: &ContextManager,
        window: &Arc<Window>,
        texture_path: &String,
    ) -> Self {
        let engine = context_manager.overlay_context.engine.clone();
        let physical_size = window.inner_size();
        let viewport = Viewport::with_physical_size(
            Size::new(physical_size.width, physical_size.height),
            window.scale_factor() as f32,
        );
        let clipboard = Clipboard::connect(window.clone());
        let overlay_surface = OverlaySurface::new(texture_path);
        fonts_mod::load_fonts();
        let wgpu_renderer = iced_wgpu::Renderer::new(engine, Font::default(), Pixels::from(16));
        let renderer = Renderer::Primary(wgpu_renderer);
        Self {
            renderer,
            viewport,
            clipboard,
            overlay_surface,
            cursor: mouse::Cursor::Unavailable,
        }
    }

    pub fn reset(&mut self, engine: iced_wgpu::Engine) {
        self.renderer = Renderer::Primary(iced_wgpu::Renderer::new(
            engine,
            Font::default(),
            Pixels::from(16),
        ));
    }

    pub fn resize(&mut self, new_size: winit::dpi::PhysicalSize<u32>, scale_factor: f64) {
        self.viewport = Viewport::with_physical_size(
            Size::new(new_size.width, new_size.height),
            scale_factor as f32,
        );
    }

    pub fn set_cursor_position(&mut self, position: PhysicalPosition<f64>, scale_factor: f64) {
        let logical = iced::Point::new(
            (position.x / scale_factor) as f32,
            (position.y / scale_factor) as f32,
        );
        self.cursor = mouse::Cursor::Available(logical);
    }

    /// Runs a left-click through the selection UI and returns iced messages.
    pub fn handle_selection_click(
        &mut self,
        selection: &ScreenSelectionUi,
        participants: &ParticipantsManager,
        click_animation_renderer: &ClickAnimationRenderer,
    ) -> Vec<SelectionMessage> {
        let mut messages = Vec::new();
        let identity = |p: Position| p;
        let mut interface = UserInterface::build(
            self.overlay_surface.view(
                participants,
                click_animation_renderer,
                &identity,
                Some(selection),
                true,
                None,
            ),
            self.viewport.logical_size(),
            user_interface::Cache::default(),
            &mut self.renderer,
        );

        let events = [
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
        ];
        let _ = interface.update(
            &events,
            self.cursor,
            &mut self.renderer,
            &mut self.clipboard,
            &mut messages,
        );
        messages
    }

    pub fn draw(&mut self, args: DrawArgs) {
        let DrawArgs {
            frame,
            view,
            participants,
            click_animation_renderer,
            position_translator,
            screen_selection,
            window_focused,
            marker_content,
        } = args;

        let mut interface = UserInterface::build(
            self.overlay_surface.view(
                participants,
                click_animation_renderer,
                position_translator,
                screen_selection,
                window_focused,
                marker_content,
            ),
            self.viewport.logical_size(),
            user_interface::Cache::default(),
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

        let wgpu_renderer = match &mut self.renderer {
            Renderer::Primary(renderer) => renderer,
            _ => unreachable!(),
        };
        wgpu_renderer.present(None, frame.texture.format(), view, &self.viewport);
    }
}
