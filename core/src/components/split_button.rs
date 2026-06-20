//! Split button: main action + optional chevron dropdown hit area (Iced).

use iced::widget::{button, column, container, row, stack, text, Space};
use iced::{Alignment, Background, Border, Color, Length, Padding, Shadow, Theme};
use iced_wgpu::core::widget::text as text_widget;

use crate::components::fonts::{GEIST_MEDIUM, GEIST_REGULAR, ICONS_FONT};
use crate::windows::colors::ColorToken;
use crate::windows::shadows::ShadowToken;

const ICON_CHEVRON_DOWN: char = '\u{F10A}';

/// A single dropdown item (dynamic label, no icon required).
#[derive(Debug, Clone)]
pub struct SplitButtonItem {
    pub label: String,
    pub selected: bool,
}

/// Build the split button element.
/// Accepts icon as char (icon font). Returns just the button.
/// `dropdown_open` highlights the chevron when the dropdown is visible.
pub fn split_button<'a, Message: Clone + 'a>(
    icon_char: char,
    bg: Color,
    on_main_press: Message,
    on_dropdown_toggle: Option<Message>,
    dropdown_open: bool,
) -> iced::Element<'a, Message, Theme, iced::Renderer> {
    let hover_bg = {
        let c = ColorToken::Gray600.to_color();
        Color::from_rgba(c.r, c.g, c.b, 0.30)
    };

    let icon = text(icon_char.to_string())
        .font(ICONS_FONT)
        .size(16.0)
        .color(Color::WHITE)
        .align_x(Alignment::Center)
        .align_y(Alignment::Center);

    let main_btn = button(
        container(icon)
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill),
    )
    .width(Length::Fixed(32.0))
    .height(Length::Fixed(22.0))
    .on_press(on_main_press.clone())
    .padding(Padding::from([1, 0]))
    .style(move |_theme: &Theme, status| hit_area_style(status, hover_bg));

    let has_dropdown = on_dropdown_toggle.is_some();

    let inner_row: iced::Element<'_, Message, Theme, iced::Renderer> =
        if let Some(dropdown_msg) = on_dropdown_toggle {
            let chevron = text(ICON_CHEVRON_DOWN.to_string())
                .font(ICONS_FONT)
                .size(14.0)
                .color(Color::WHITE)
                .align_x(Alignment::Center)
                .align_y(Alignment::Center);

            let dropdown_btn = button(
                container(chevron)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .center_x(Length::Fill)
                    .center_y(Length::Fill),
            )
            .width(Length::Fixed(22.0))
            .height(Length::Fixed(22.0))
            .on_press(dropdown_msg)
            .padding(0)
            .style(move |_theme: &Theme, status| {
                if dropdown_open {
                    hit_area_style(button::Status::Hovered, hover_bg)
                } else {
                    hit_area_style(status, hover_bg)
                }
            });

            row![main_btn, dropdown_btn].spacing(1).into()
        } else {
            main_btn.into()
        };

    let inner_layer = container(inner_row).padding(Padding::new(2.0));

    let total_width = if has_dropdown {
        2.0 + 32.0 + 1.0 + 22.0 + 2.0
    } else {
        2.0 + 32.0 + 2.0
    };
    let total_height = 2.0 + 22.0 + 2.0;

    let base_btn = button(Space::new())
        .width(Length::Fixed(total_width))
        .height(Length::Fixed(total_height))
        .on_press(on_main_press)
        .padding(0)
        .style(move |_theme: &Theme, _status| button::Style {
            background: Some(Background::Color(bg)),
            border: Border {
                color: Color::TRANSPARENT,
                width: 0.0,
                radius: 100.0.into(),
            },
            text_color: Color::WHITE,
            shadow: Shadow::default(),
            snap: false,
        });

    stack![base_btn, inner_layer].into()
}

fn hit_area_style(status: button::Status, hover_bg: Color) -> button::Style {
    let bg = match status {
        button::Status::Hovered => Some(Background::Color(hover_bg)),
        _ => None,
    };
    button::Style {
        background: bg,
        border: Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: 100.0.into(),
        },
        text_color: Color::WHITE,
        shadow: Shadow::default(),
        snap: false,
    }
}

/// When the dropdown is open, wrap the full window content with
/// the dropdown menu + dismiss backdrop. Handles all dropdown
/// rendering internally — the consumer just passes items + callbacks.
pub fn split_button_dropdown_wrap<'a, Message: Clone + 'a>(
    base: iced::Element<'a, Message, Theme, iced::Renderer>,
    items: &[SplitButtonItem],
    on_dismiss: Message,
    on_select: impl Fn(usize) -> Message + 'a,
    top_offset: f32,
    right_padding: f32,
) -> iced::Element<'a, Message, Theme, iced::Renderer> {
    let mut elements: Vec<iced::Element<'a, Message, Theme, iced::Renderer>> = Vec::new();
    for (i, item) in items.iter().enumerate() {
        elements.push(split_button_menu_item(
            item.label.clone(),
            item.selected,
            on_select(i),
        ));
    }

    let menu = container(column(elements).width(Length::Fixed(248.0)))
        .padding(Padding::from([4, 4]))
        .style(|_theme: &Theme| container::Style {
            background: Some(Background::Color(ColorToken::Slate700.to_color())),
            border: Border {
                color: Color::from_rgba(1.0, 1.0, 1.0, 0.15),
                width: 1.0,
                radius: 8.0.into(),
            },
            shadow: ShadowToken::Xl.to_shadow(),
            ..Default::default()
        });

    let dismiss_backdrop = button(Space::new().width(Length::Fill).height(Length::Fill))
        .width(Length::Fill)
        .height(Length::Fill)
        .on_press(on_dismiss)
        .padding(0)
        .style(|_theme: &Theme, _status| button::Style {
            background: None,
            border: Border {
                color: Color::TRANSPARENT,
                width: 0.0,
                radius: 0.0.into(),
            },
            text_color: Color::TRANSPARENT,
            shadow: Shadow::default(),
            snap: false,
        });

    let dropdown_positioned = container(column![
        Space::new().height(Length::Fixed(top_offset)),
        row![
            Space::new().width(Length::Fill),
            menu,
            Space::new().width(Length::Fixed(right_padding)),
        ]
    ])
    .width(Length::Fill)
    .height(Length::Fill);

    stack![base, dismiss_backdrop, dropdown_positioned].into()
}

const MENU_LABEL_MAX_CHARS: usize = 30;

// TODO(@konsalex): When we upgrade to iced 0.15 (currently dev), we will be able to use native ellipsis
fn truncate_with_ellipsis(s: &str, max_chars: usize) -> String {
    let char_count = s.chars().count();
    if char_count <= max_chars {
        return s.to_string();
    }
    let truncated: String = s.chars().take(max_chars.saturating_sub(1)).collect();
    format!("{truncated}\u{2026}")
}

fn split_button_menu_item<'a, Message: Clone + 'a>(
    label: String,
    selected: bool,
    on_press: Message,
) -> iced::Element<'a, Message, Theme, iced::Renderer> {
    let check: iced::Element<'a, Message, Theme, iced::Renderer> = if selected {
        text("\u{2713}")
            .size(13)
            .color(Color::WHITE)
            .font(GEIST_MEDIUM)
            .width(Length::Fixed(14.0))
            .into()
    } else {
        Space::new()
            .width(Length::Fixed(14.0))
            .height(Length::Shrink)
            .into()
    };

    let display_label = truncate_with_ellipsis(&label, MENU_LABEL_MAX_CHARS);

    let label_text = text(display_label)
        .size(12)
        .color(Color::WHITE)
        .font(GEIST_REGULAR)
        .wrapping(text_widget::Wrapping::None);

    let clipped_label = container(label_text).width(Length::Fill).clip(true);

    let content_row = row![check, clipped_label]
        .spacing(6)
        .align_y(Alignment::Center);

    let content = container(content_row)
        .width(Length::Fill)
        .padding(Padding::from([4, 8]))
        .style(|_theme: &Theme| container::Style {
            background: None,
            border: Border {
                color: Color::TRANSPARENT,
                width: 0.0,
                radius: 6.0.into(),
            },
            ..Default::default()
        });

    button(content)
        .width(Length::Fill)
        .on_press(on_press)
        .padding(Padding::from([1, 6]))
        .style(|_theme: &Theme, status| {
            let bg = match status {
                button::Status::Hovered => {
                    Some(Background::Color(Color::from_rgba(1.0, 1.0, 1.0, 0.08)))
                }
                button::Status::Pressed => {
                    Some(Background::Color(Color::from_rgba(1.0, 1.0, 1.0, 0.04)))
                }
                _ => None,
            };
            button::Style {
                background: bg,
                border: Border {
                    color: Color::TRANSPARENT,
                    width: 0.0,
                    radius: 6.0.into(),
                },
                text_color: Color::WHITE,
                shadow: Shadow::default(),
                snap: false,
            }
        })
        .into()
}
