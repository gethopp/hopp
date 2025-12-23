use iced::widget::canvas;
use iced::{mouse, Length, Rectangle, Theme};
use iced_wgpu::core::Element;

#[path = "marker.rs"]
mod marker;
use marker::Marker;

use crate::graphics::graphics_context::draw::DrawManager;
use crate::utils::geometry::Position;

#[derive(Debug, Clone, Copy, Hash, Eq, PartialEq)]
pub enum Message {}

pub struct OverlaySurfaceCanvas<'a> {
    marker: &'a Marker,
    draws: &'a DrawManager,
}

impl<'a> std::fmt::Debug for OverlaySurfaceCanvas<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "OverlaySurfaceCanvas")
    }
}

impl<'a> OverlaySurfaceCanvas<'a> {
    pub fn new(marker: &'a Marker, draws: &'a DrawManager) -> Self {
        Self { marker, draws }
    }
}

impl<'a, Message> canvas::Program<Message> for OverlaySurfaceCanvas<'a> {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &iced::Renderer,
        _theme: &iced::Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut geometries = vec![self.marker.draw(renderer, bounds)];
        geometries.extend(self.draws.draw(renderer, bounds));
        geometries
    }
}

pub struct OverlaySurface {
    marker: Marker,
    draws: DrawManager,
}

impl OverlaySurface {
    pub fn new(texture_path: &String) -> Self {
        let marker = Marker::new(texture_path);
        let draws = DrawManager::new();
        Self { marker, draws }
    }

    pub fn view(&mut self) -> Element<'_, Message, Theme, iced::Renderer> {
        self.draws.update();

        canvas(OverlaySurfaceCanvas::new(&self.marker, &self.draws))
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    pub fn add_draw_participant(&mut self, sid: String, color: &str) {
        self.draws.add_participant(sid, color);
    }

    pub fn remove_draw_participant(&mut self, sid: &str) {
        self.draws.remove_participant(sid);
    }

    pub fn set_drawing_mode(&mut self, sid: &str, mode: crate::room_service::DrawingMode) {
        self.draws.set_drawing_mode(sid, mode);
    }

    pub fn draw_start(&mut self, sid: &str, point: Position) {
        self.draws.draw_start(sid, point);
    }

    pub fn draw_add_point(&mut self, sid: &str, point: Position) {
        self.draws.draw_add_point(sid, point);
    }

    pub fn draw_end(&mut self, sid: &str, point: Position) {
        self.draws.draw_end(sid, point);
    }
}
