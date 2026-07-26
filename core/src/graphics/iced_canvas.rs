use iced::widget::{button, canvas, column, container, stack, text, Space};
use iced::{
    mouse, Alignment, Background, Border, Color, Length, Padding, Rectangle, Shadow, Theme,
};
use iced_wgpu::core::Element;

#[path = "marker.rs"]
mod marker;
use marker::Marker;

use crate::components::fonts::GEIST_REGULAR;
use crate::graphics::graphics_context::click_animation::ClickAnimationRenderer;
use crate::graphics::graphics_context::participant::ParticipantsManager;
use crate::utils::geometry::Position;
use crate::SelectionMode;

#[derive(Debug, Clone, Copy)]
pub enum Message {
    SetSelectionMode(SelectionMode),
}

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
        let mut geometries = vec![self.marker.draw(renderer, bounds, self.position_translator)];
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
        screen_selection: Option<SelectionMode>,
        window_focused: bool,
    ) -> Element<'a, Message, Theme, iced::Renderer> {
        if let Some(mode) = screen_selection {
            Self::screen_selection_view(mode, window_focused)
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

    pub fn screen_selection_view(
        mode: SelectionMode,
        window_focused: bool,
    ) -> Element<'static, Message, Theme, iced::Renderer> {
        let window_selected = mode == SelectionMode::Window;
        let content: Element<'static, Message, Theme, iced::Renderer> = if mode
            == SelectionMode::Screen
            && window_focused
        {
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
            container(Space::new())
                .width(Length::Fill)
                .height(Length::Fill)
                .style(move |_theme: &Theme| container::Style {
                    background: window_selected
                        .then_some(Background::Color(Color::from_rgba(0.08, 0.05, 0.20, 0.25))),
                    ..Default::default()
                })
                .into()
        };

        let toggle = button(
            text("Window")
                .size(14.0)
                .color(Color::WHITE)
                .font(GEIST_REGULAR),
        )
        .on_press(Message::SetSelectionMode(if window_selected {
            SelectionMode::Screen
        } else {
            SelectionMode::Window
        }))
        .padding(Padding::from([8.0, 16.0]))
        .style(move |_theme: &Theme, status| {
            let background = match (window_selected, status) {
                (_, button::Status::Pressed) => {
                    Some(Background::Color(Color::from_rgba(0.20, 0.08, 0.42, 0.98)))
                }
                (true, button::Status::Hovered) => {
                    Some(Background::Color(Color::from_rgba(0.36, 0.18, 0.68, 0.98)))
                }
                (false, button::Status::Hovered) => {
                    Some(Background::Color(Color::from_rgba(0.16, 0.10, 0.30, 0.95)))
                }
                (true, _) => Some(Background::Color(Color::from_rgba(0.28, 0.12, 0.58, 0.98))),
                (false, _) => None,
            };

            button::Style {
                background,
                border: Border {
                    color: Color::from_rgba(1.0, 1.0, 1.0, if window_selected { 0.8 } else { 0.3 }),
                    width: 1.0,
                    radius: 8.0.into(),
                },
                text_color: Color::WHITE,
                shadow: Shadow::default(),
                snap: false,
            }
        });

        let toggle_layer = container(toggle)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(Alignment::End)
            .align_y(Alignment::Start)
            .padding(Padding {
                top: 24.0,
                right: 24.0,
                bottom: 0.0,
                left: 0.0,
            });

        stack![content, toggle_layer].into()
    }
}
