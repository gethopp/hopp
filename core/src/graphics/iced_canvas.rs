use iced::widget::{Space, canvas, column, container, text};
use iced::{Alignment, Background, Border, Color, Length, Padding, Rectangle, Theme, mouse};
use iced_wgpu::core::Element;

#[path = "marker.rs"]
mod marker;
use marker::Marker;

use crate::components::fonts::GEIST_REGULAR;
use crate::graphics::graphics_context::click_animation::ClickAnimationRenderer;
use crate::graphics::graphics_context::participant::ParticipantsManager;
use crate::utils::geometry::Position;

#[derive(Debug, Clone, Copy, Hash, Eq, PartialEq)]
pub enum Message {}

pub struct OverlaySurfaceCanvas<'a> {
    marker: &'a Marker,
    participants: &'a ParticipantsManager,
    click_animation_renderer: &'a ClickAnimationRenderer,
    position_translator: &'a dyn Fn(Position) -> Position,
}

impl<'a> std::fmt::Debug for OverlaySurfaceCanvas<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "OverlaySurfaceCanvas")
    }
}

impl<'a> OverlaySurfaceCanvas<'a> {
    pub fn new(
        marker: &'a Marker,
        participants: &'a ParticipantsManager,
        click_animation_renderer: &'a ClickAnimationRenderer,
        position_translator: &'a dyn Fn(Position) -> Position,
    ) -> Self {
        Self {
            marker,
            participants,
            click_animation_renderer,
            position_translator,
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
        geometries.extend(
            self.participants
                .draw(renderer, bounds, self.position_translator),
        );

        geometries.push(self.click_animation_renderer.draw(
            renderer,
            bounds,
            self.position_translator,
        ));

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
        click_animation_renderer: &'a ClickAnimationRenderer,
        position_translator: &'a dyn Fn(Position) -> Position,
        screen_selection: bool,
        window_focused: bool,
    ) -> Element<'a, Message, Theme, iced::Renderer> {
        if screen_selection {
            if !window_focused {
                return Space::new().width(Length::Fill).height(Length::Fill).into();
            }

            let card_background = Color::from_rgba(0.28, 0.12, 0.58, 0.98);
            let scrim_background = Color::from_rgba(0.08, 0.05, 0.20, 0.80);

            let box_text = column![
                text("Click anywhere to select this screen or press Enter")
                    .size(26.0)
                    .color(Color::from_rgb(0.98, 0.96, 1.0))
                    .font(GEIST_REGULAR),
                text("Move your cursor to the display you'd like to share (or use the arrows) and click. Press ESC to cancel.")
                    .size(18.0)
                    .color(Color::from_rgb(0.89, 0.84, 0.98))
                    .font(GEIST_REGULAR),
            ]
            .spacing(16.0)
            .max_width(460.0);

            let box_container = container(box_text)
                .padding(Padding::from([30.0, 40.0]))
                .style(move |_theme: &Theme| container::Style {
                    background: Some(Background::Color(card_background)),
                    border: Border {
                        radius: 16.0.into(),
                        ..Default::default()
                    },
                    ..Default::default()
                });

            container(box_container)
                .width(Length::Fill)
                .height(Length::Fill)
                .align_x(Alignment::Center)
                .align_y(Alignment::Center)
                .style(move |_theme: &Theme| container::Style {
                    background: Some(Background::Color(scrim_background)),
                    ..Default::default()
                })
                .into()
        } else {
            canvas(OverlaySurfaceCanvas::new(
                &self.marker,
                participants,
                click_animation_renderer,
                position_translator,
            ))
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
        }
    }
}
