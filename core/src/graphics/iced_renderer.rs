use std::sync::Arc;

use iced::Renderer;
use iced::{Font, Pixels};
use iced_wgpu::{
    core::mouse,
    graphics::{Shell, Viewport},
    Engine,
};
use iced_winit::core::{renderer, time::Instant, window, Event, Theme};
use iced_winit::{
    core::Size,
    runtime::{user_interface, UserInterface},
    Clipboard,
};
use winit::window::Window;

#[path = "iced_canvas.rs"]
mod iced_canvas;
use iced_canvas::OverlaySurface;

use crate::graphics::graphics_context::draw::Draw;
use crate::utils::geometry::Position;

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
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        adapter: &wgpu::Adapter,
        window: &Arc<Window>,
        texture_path: &String,
    ) -> Self {
        let engine = Engine::new(
            adapter,
            device.clone(),
            queue.clone(),
            format,
            None, // TODO: I might need to change this
            Shell::headless(),
        );
        let physical_size = window.inner_size();
        let viewport = Viewport::with_physical_size(
            Size::new(physical_size.width, physical_size.height),
            window.scale_factor() as f32,
        );
        let clipboard = Clipboard::connect(window.clone());
        let overlay_surface = OverlaySurface::new(texture_path);
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

    pub fn draw(&mut self, frame: &wgpu::SurfaceTexture, view: &wgpu::TextureView) {
        let mut interface = UserInterface::build(
            self.overlay_surface.view(),
            self.viewport.logical_size(),
            user_interface::Cache::default(),
            &mut self.renderer,
        );

        let (state, _) = interface.update(
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
            _ => panic!("Expected primary renderer"),
        };
        wgpu_renderer.present(None, frame.texture.format(), view, &self.viewport);
    }

    pub fn add_draw_participant(&mut self, sid: String, color: &str) {
        self.overlay_surface.add_draw_participant(sid, color);
    }

    pub fn remove_draw_participant(&mut self, sid: &str) {
        self.overlay_surface.remove_draw_participant(sid);
    }

    pub fn set_drawing_mode(&mut self, sid: &str, mode: crate::room_service::DrawingMode) {
        self.overlay_surface.set_drawing_mode(sid, mode);
    }

    pub fn draw_start(&mut self, sid: &str, point: Position, path_id: u64) {
        self.overlay_surface.draw_start(sid, point, path_id);
    }

    pub fn draw_add_point(&mut self, sid: &str, point: Position) {
        self.overlay_surface.draw_add_point(sid, point);
    }

    pub fn draw_end(&mut self, sid: &str, point: Position) {
        self.overlay_surface.draw_end(sid, point);
    }

    pub fn draw_clear_path(&mut self, sid: &str, path_id: u64) {
        self.overlay_surface.draw_clear_path(sid, path_id);
    }

    pub fn draw_clear_all_paths(&mut self, sid: &str) {
        self.overlay_surface.draw_clear_all_paths(sid);
    }
}
