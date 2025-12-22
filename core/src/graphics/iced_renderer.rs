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

#[path = "iced_marker.rs"]
mod iced_marker;
use iced_marker::Marker;

pub struct IcedRenderer {
    renderer: Renderer,
    viewport: Viewport,
    clipboard: Clipboard,
    marker: Marker,
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
        let marker = Marker::new(texture_path);
        let wgpu_renderer = iced_wgpu::Renderer::new(engine, Font::default(), Pixels::from(16));
        let renderer = Renderer::Primary(wgpu_renderer);
        Self {
            renderer,
            viewport,
            clipboard,
            marker,
            cursor: mouse::Cursor::Unavailable,
        }
    }

    pub fn draw(&mut self, frame: &wgpu::SurfaceTexture, view: &wgpu::TextureView) {
        let mut interface = UserInterface::build(
            self.marker.view(),
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
}
