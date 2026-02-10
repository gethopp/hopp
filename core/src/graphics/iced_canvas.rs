use iced::widget::canvas;
use iced::{mouse, Length, Rectangle, Theme};
use iced_wgpu::core::Element;

#[path = "marker.rs"]
mod marker;
use marker::Marker;

use crate::graphics::graphics_context::participant::ParticipantsManager;

#[derive(Debug, Clone, Copy, Hash, Eq, PartialEq)]
pub enum Message {}

pub struct OverlaySurfaceCanvas<'a> {
    marker: &'a Marker,
    participants: &'a ParticipantsManager,
}

impl<'a> std::fmt::Debug for OverlaySurfaceCanvas<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "OverlaySurfaceCanvas")
    }
}

impl<'a> OverlaySurfaceCanvas<'a> {
    pub fn new(marker: &'a Marker, participants: &'a ParticipantsManager) -> Self {
        Self {
            marker,
            participants,
        }
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
        geometries.extend(self.participants.draw(renderer, bounds));
        geometries
    }
}

pub struct OverlaySurface {
    marker: Marker,
}

impl OverlaySurface {
    pub fn new(texture_path: &String) -> Self {
        let marker = Marker::new(texture_path);
        Self { marker }
    }

    pub fn view<'a>(
        &'a mut self,
        participants: &'a ParticipantsManager,
    ) -> Element<'a, Message, Theme, iced::Renderer> {
        log::debug!("OverlaySurface::view");

        canvas(OverlaySurfaceCanvas::new(&self.marker, participants))
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }
}
