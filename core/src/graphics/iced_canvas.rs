use iced::widget::canvas::{stroke, Path, Stroke};
use iced::widget::{button, canvas, column, container, stack, text, Space};
use iced::{
    mouse, Alignment, Background, Border, Color, Length, Padding, Point, Rectangle, Shadow, Size,
    Theme,
};
use iced_wgpu::core::Element;

#[path = "marker.rs"]
mod marker;
use marker::Marker;

use crate::components::fonts::GEIST_REGULAR;
use crate::graphics::graphics_context::click_animation::ClickAnimationRenderer;
use crate::graphics::graphics_context::participant::ParticipantsManager;
use crate::utils::geometry::{Frame, Position};
use crate::{SelectionMode, SelectionOverlayState};

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

struct SelectionBorderCanvas {
    frame: Frame,
}

impl<Message> canvas::Program<Message> for SelectionBorderCanvas {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &iced::Renderer,
        _theme: &iced::Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut canvas_frame = canvas::Frame::new(renderer, bounds.size());
        let border = Path::rounded_rectangle(
            Point::new(self.frame.origin_x as f32, self.frame.origin_y as f32),
            Size::new(
                self.frame.extent.width as f32,
                self.frame.extent.height as f32,
            ),
            10.0.into(),
        );
        canvas_frame.stroke(
            &border,
            Stroke {
                style: stroke::Style::Solid(Color::from_rgba(0.28, 0.12, 0.58, 0.98)),
                width: 4.0,
                ..Stroke::default()
            },
        );
        vec![canvas_frame.into_geometry()]
    }
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
        screen_selection: Option<SelectionOverlayState>,
        window_focused: bool,
    ) -> Element<'a, Message, Theme, iced::Renderer> {
        if let Some(screen_selection) = screen_selection {
            Self::screen_selection_view(screen_selection, window_focused)
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
        screen_selection: SelectionOverlayState,
        window_focused: bool,
    ) -> Element<'static, Message, Theme, iced::Renderer> {
        let SelectionOverlayState { mode, border_frame } = screen_selection;
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
                    text("Move your cursor to the display you'd like to share (or use the arrows), then click or press Enter.")
                        .size(18.0)
                        .color(Color::from_rgb(0.89, 0.84, 0.98))
                        .font(GEIST_REGULAR),
                    text("To share a window instead, click \"Share Window\" in the top-right, then click the window you'd like to share. Press ESC to cancel.")
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
                        .then_some(Background::Color(Color::from_rgba(0.1, 0.1, 0.1, 0.00))),
                    ..Default::default()
                })
                .into()
        };

        let toggle = button(
            text("Share Window")
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
                (true, button::Status::Pressed) => {
                    Some(Background::Color(Color::from_rgba(0.20, 0.08, 0.42, 0.98)))
                }
                (false, button::Status::Pressed) => {
                    Some(Background::Color(Color::from_rgba(0.09, 0.10, 0.12, 0.98)))
                }
                (true, button::Status::Hovered) => {
                    Some(Background::Color(Color::from_rgba(0.36, 0.18, 0.68, 0.98)))
                }
                (false, button::Status::Hovered) => {
                    Some(Background::Color(Color::from_rgba(0.22, 0.23, 0.26, 0.98)))
                }
                (true, _) => Some(Background::Color(Color::from_rgba(0.28, 0.12, 0.58, 0.98))),
                (false, _) => Some(Background::Color(Color::from_rgba(0.13, 0.14, 0.16, 0.96))),
            };

            button::Style {
                background,
                border: Border {
                    color: if window_selected {
                        Color::from_rgba(0.78, 0.64, 1.0, 1.0)
                    } else {
                        Color::from_rgba(1.0, 1.0, 1.0, 0.55)
                    },
                    width: 1.0,
                    radius: 8.0.into(),
                },
                text_color: Color::WHITE,
                shadow: Shadow::default(),
                snap: false,
            }
        });

        let toggle_layer: Element<'static, Message, Theme, iced::Renderer> =
            if cfg!(target_os = "macos") && window_focused {
                container(toggle)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .align_x(Alignment::End)
                    .align_y(Alignment::Start)
                    .padding(Padding {
                        top: 24.0,
                        right: 24.0,
                        bottom: 0.0,
                        left: 0.0,
                    })
                    .into()
            } else {
                Space::new().into()
            };

        if let Some(frame) = border_frame {
            stack![
                content,
                canvas(SelectionBorderCanvas { frame })
                    .width(Length::Fill)
                    .height(Length::Fill),
                toggle_layer
            ]
            .into()
        } else {
            stack![content, toggle_layer].into()
        }
    }
}
