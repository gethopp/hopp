use iced::widget::{button, canvas, column, container, image, row, text, Space};
use iced::{mouse, Alignment, Background, Border, Color, ContentFit, Length, Padding, Rectangle, Theme};
use iced_wgpu::core::Element;

#[path = "marker.rs"]
mod marker;
use marker::Marker;

use crate::components::fonts::GEIST_REGULAR;
use crate::graphics::graphics_context::click_animation::ClickAnimationRenderer;
use crate::graphics::graphics_context::participant::ParticipantsManager;
use crate::screen_selection::{ScreenSelectionItemUi, ScreenSelectionTab, ScreenSelectionUi};
use crate::utils::geometry::Position;

#[derive(Debug, Clone)]
pub enum Message {
    SelectTab(ScreenSelectionTab),
    /// Clicking a tile immediately starts sharing that source.
    ShareItem(usize),
    Cancel,
}

pub struct OverlaySurfaceCanvas<'a> {
    marker: &'a Marker,
    participants: &'a ParticipantsManager,
    click_animation_renderer: &'a ClickAnimationRenderer,
    position_translator: &'a dyn Fn(Position) -> Position,
    marker_content: Option<Rectangle>,
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
        marker_content: Option<Rectangle>,
    ) -> Self {
        Self {
            marker,
            participants,
            click_animation_renderer,
            position_translator,
            marker_content,
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
        let mut geometries = vec![self.marker.draw(renderer, bounds, self.marker_content)];
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
        screen_selection: Option<&'a ScreenSelectionUi>,
        window_focused: bool,
        marker_content: Option<Rectangle>,
    ) -> Element<'a, Message, Theme, iced::Renderer> {
        if let Some(selection) = screen_selection {
            selection_view(selection, window_focused)
        } else {
            canvas(OverlaySurfaceCanvas::new(
                &self.marker,
                participants,
                click_animation_renderer,
                position_translator,
                marker_content,
            ))
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
        }
    }
}

fn selection_view<'a>(
    selection: &'a ScreenSelectionUi,
    window_focused: bool,
) -> Element<'a, Message, Theme, iced::Renderer> {
    if !window_focused {
        return Space::new().width(Length::Fill).height(Length::Fill).into();
    }

    let scrim = Color::from_rgba(0.0, 0.0, 0.0, 0.55);
    let card_bg = Color::from_rgb(0.14, 0.14, 0.16);
    let tile_bg = Color::from_rgb(0.20, 0.20, 0.22);
    let tile_selected = Color::from_rgb(0.18, 0.32, 0.55);
    let accent = Color::from_rgb(0.20, 0.55, 0.95);
    let text_primary = Color::from_rgb(0.96, 0.96, 0.97);
    let text_muted = Color::from_rgb(0.70, 0.70, 0.74);
    let tab_track = Color::from_rgb(0.22, 0.22, 0.25);

    let screens_tab = segment_tab(
        "Entire Screen",
        selection.tab == ScreenSelectionTab::Screens,
        accent,
        text_primary,
        text_muted,
        Message::SelectTab(ScreenSelectionTab::Screens),
    );
    let windows_label = if selection.windows_supported {
        "Window"
    } else {
        "Window (unavailable)"
    };
    let windows_tab = segment_tab(
        windows_label,
        selection.tab == ScreenSelectionTab::Windows,
        accent,
        text_primary,
        text_muted,
        Message::SelectTab(ScreenSelectionTab::Windows),
    );

    let tabs = container(
        row![screens_tab, windows_tab]
            .spacing(4.0)
            .padding(4.0)
            .width(Length::Fill),
    )
    .width(Length::Fill)
    .style(move |_theme: &Theme| container::Style {
        background: Some(Background::Color(tab_track)),
        border: Border {
            radius: 10.0.into(),
            ..Default::default()
        },
        ..Default::default()
    });

    let mut tiles = column![].spacing(10.0).width(Length::Fill);
    if selection.items.is_empty() {
        let empty_message = if selection.tab == ScreenSelectionTab::Windows {
            selection
                .windows_hint
                .as_deref()
                .unwrap_or("No windows available to share")
        } else {
            "No screens available"
        };
        tiles = tiles.push(
            text(empty_message)
                .size(15.0)
                .color(text_muted)
                .font(GEIST_REGULAR),
        );
    } else {
        let mut row_tiles = row![].spacing(10.0).width(Length::Fill);
        for (index, item) in selection.items.iter().enumerate() {
            let tile = share_tile(
                item,
                index == selection.selected_index,
                tile_bg,
                tile_selected,
                accent,
                text_primary,
                text_muted,
                index,
            );
            row_tiles = row_tiles.push(tile);
            if index % 2 == 1 {
                tiles = tiles.push(row_tiles);
                row_tiles = row![].spacing(10.0).width(Length::Fill);
            }
        }
        if selection.items.len() % 2 == 1 {
            row_tiles = row_tiles.push(Space::new().width(Length::Fill).height(Length::Shrink));
            tiles = tiles.push(row_tiles);
        }
    }

    let cancel = button(
        text("Cancel")
            .size(15.0)
            .color(text_primary)
            .font(GEIST_REGULAR),
    )
    .padding(Padding::from([10.0, 18.0]))
    .style(move |_theme: &Theme, status| {
        let hovered = matches!(status, iced::widget::button::Status::Hovered);
        iced::widget::button::Style {
            background: Some(Background::Color(if hovered {
                Color::from_rgb(0.28, 0.28, 0.30)
            } else {
                Color::from_rgb(0.24, 0.24, 0.26)
            })),
            border: Border {
                radius: 8.0.into(),
                ..Default::default()
            },
            text_color: text_primary,
            ..Default::default()
        }
    })
    .on_press(Message::Cancel);

    let actions = row![
        text("Click a screen or window to start sharing")
            .size(13.0)
            .color(text_muted)
            .font(GEIST_REGULAR),
        Space::new().width(Length::Fill).height(Length::Shrink),
        cancel,
    ]
    .spacing(10.0)
    .align_y(Alignment::Center)
    .width(Length::Fill);

    let body = column![
        text("Choose what to share")
            .size(22.0)
            .color(text_primary)
            .font(GEIST_REGULAR),
        text("Your screen or a single application window")
            .size(14.0)
            .color(text_muted)
            .font(GEIST_REGULAR),
        tabs,
        tiles,
        actions,
    ]
    .spacing(16.0)
    .max_width(640.0);

    let card = container(body)
        .padding(Padding::from([28.0, 28.0]))
        .style(move |_theme: &Theme| container::Style {
            background: Some(Background::Color(card_bg)),
            border: Border {
                radius: 14.0.into(),
                width: 1.0,
                color: Color::from_rgba(1.0, 1.0, 1.0, 0.08),
            },
            ..Default::default()
        });

    container(card)
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(Alignment::Center)
        .align_y(Alignment::Center)
        .style(move |_theme: &Theme| container::Style {
            background: Some(Background::Color(scrim)),
            ..Default::default()
        })
        .into()
}

fn share_tile<'a>(
    item: &'a ScreenSelectionItemUi,
    selected: bool,
    tile_bg: Color,
    tile_selected: Color,
    accent: Color,
    text_primary: Color,
    text_muted: Color,
    index: usize,
) -> Element<'a, Message, Theme, iced::Renderer> {
    let preview: Element<'a, Message, Theme, iced::Renderer> =
        if let Some(handle) = item.thumbnail.clone() {
            container(
                image(handle)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .content_fit(ContentFit::Cover),
            )
            .width(Length::Fill)
            .height(Length::Fixed(110.0))
            .style(move |_theme: &Theme| container::Style {
                background: Some(Background::Color(Color::from_rgb(0.10, 0.10, 0.12))),
                border: Border {
                    radius: 8.0.into(),
                    ..Default::default()
                },
                ..Default::default()
            })
            .into()
        } else {
            container(
                text("No preview")
                    .size(12.0)
                    .color(text_muted)
                    .font(GEIST_REGULAR),
            )
            .width(Length::Fill)
            .height(Length::Fixed(110.0))
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .style(move |_theme: &Theme| container::Style {
                background: Some(Background::Color(Color::from_rgb(0.10, 0.10, 0.12))),
                border: Border {
                    radius: 8.0.into(),
                    ..Default::default()
                },
                ..Default::default()
            })
            .into()
        };

    let mut labels = column![
        text(&item.title)
            .size(14.0)
            .color(text_primary)
            .font(GEIST_REGULAR),
    ]
    .spacing(2.0)
    .width(Length::Fill);

    if let Some(subtitle) = item.subtitle.as_deref() {
        labels = labels.push(
            text(subtitle)
                .size(12.0)
                .color(text_muted)
                .font(GEIST_REGULAR),
        );
    }

    let content = column![preview, labels].spacing(8.0).width(Length::Fill);

    button(content)
        .padding(Padding::from([10.0, 10.0]))
        .width(Length::Fill)
        .style(move |_theme: &Theme, status| {
            let hovered = matches!(status, iced::widget::button::Status::Hovered);
            iced::widget::button::Style {
                background: Some(Background::Color(if selected {
                    tile_selected
                } else if hovered {
                    Color::from_rgb(0.24, 0.24, 0.27)
                } else {
                    tile_bg
                })),
                border: Border {
                    radius: 10.0.into(),
                    width: if selected || hovered { 2.0 } else { 1.0 },
                    color: if selected || hovered {
                        accent
                    } else {
                        Color::from_rgba(1.0, 1.0, 1.0, 0.06)
                    },
                },
                text_color: text_primary,
                ..Default::default()
            }
        })
        .on_press(Message::ShareItem(index))
        .into()
}

fn segment_tab<'a>(
    label: &'a str,
    active: bool,
    accent: Color,
    text_primary: Color,
    text_muted: Color,
    message: Message,
) -> Element<'a, Message, Theme, iced::Renderer> {
    button(
        text(label)
            .size(14.0)
            .color(if active { text_primary } else { text_muted })
            .font(GEIST_REGULAR),
    )
    .padding(Padding::from([8.0, 12.0]))
    .width(Length::Fill)
    .style(move |_theme: &Theme, _status| iced::widget::button::Style {
        background: if active {
            Some(Background::Color(Color::from_rgb(0.30, 0.30, 0.34)))
        } else {
            None
        },
        border: Border {
            radius: 8.0.into(),
            width: if active { 1.0 } else { 0.0 },
            color: if active {
                accent
            } else {
                Color::TRANSPARENT
            },
        },
        text_color: if active { text_primary } else { text_muted },
        ..Default::default()
    })
    .on_press(message)
    .into()
}
