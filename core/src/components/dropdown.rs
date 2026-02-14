//! Reusable dropdown menu component for iced.
//!
//! Provides a trigger button, menu panel with items & dividers, and an overlay
//! system for dismissing the menu by clicking outside.  The component is generic
//! over the message type so it can be used from any window.
//!
//! ## Usage
//!
//! ```ignore
//! // 1. Define items
//! const ITEMS: &[DropdownItemDef] = &[
//!     DropdownItemDef { label: "Option A", icon: ICON_COG },
//! ];
//!
//! // 2. Trigger button (in your header)
//! let trigger = dropdown_trigger_button(ICON_COG, state.dropdown_open, Msg::Toggle);
//!
//! // 3. Menu panel
//! let menu = dropdown_menu(ITEMS, &[], Msg::ItemClicked);
//!
//! // 4. Wrap in overlay (handles backdrop dismiss)
//! dropdown_overlay(base_element, menu, Msg::Dismiss, top_offset, right_pad)
//! ```

use iced::widget::{button, column, container, row, stack, svg, text, Space};
use iced::{Alignment, Background, Border, Color, Length, Padding, Shadow, Theme};
use iced_wgpu::core::widget::text as text_widget;

use crate::components::fonts::GEIST_MEDIUM;
use crate::windows::colors::ColorToken;
use crate::windows::shadows::ShadowToken;

/// Definition for a single dropdown menu item.
pub struct DropdownItemDef {
    /// Display label shown in the menu row.
    pub label: &'static str,
    /// SVG icon bytes (embedded at compile time).
    pub icon: &'static [u8],
}

/// Build a dropdown trigger button with an SVG icon.
///
/// Design specs (approx):
/// - Width 44, height 24, radius 32
/// - Default: transparent bg (no border)
/// - Hover / active (dropdown open): Slate400 bg, white 20% border
/// - Contains 20×20 icon
pub fn dropdown_trigger_button<'a, Message: Clone + 'a>(
    icon_data: &'static [u8],
    is_open: bool,
    on_toggle: Message,
) -> iced::Element<'a, Message, Theme, iced::Renderer> {
    let icon_handle = svg::Handle::from_memory(icon_data);
    let icon = svg(icon_handle)
        .width(Length::Fixed(20.0))
        .height(Length::Fixed(20.0))
        .style(|_theme: &Theme, _status| svg::Style {
            color: Some(Color::WHITE),
        });

    button(
        container(icon)
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill),
    )
    .width(Length::Fixed(44.0))
    .height(Length::Fixed(24.0))
    .on_press(on_toggle)
    .padding(0)
    .style(move |_theme: &Theme, status| {
        let slate400 = ColorToken::Slate400.to_color();

        // Show bg only when hovered, pressed, or the dropdown is open
        let (bg, border_color, border_width) = match status {
            button::Status::Hovered => (
                Some(Background::Color(slate400)),
                Color::from_rgba(1.0, 1.0, 1.0, 0.2),
                1.0,
            ),
            button::Status::Pressed => (
                Some(Background::Color(slate400)),
                Color::from_rgba(1.0, 1.0, 1.0, 0.2),
                1.0,
            ),
            _ => {
                if is_open {
                    (
                        Some(Background::Color(slate400)),
                        Color::from_rgba(1.0, 1.0, 1.0, 0.2),
                        1.0,
                    )
                } else {
                    (None, Color::TRANSPARENT, 0.0)
                }
            }
        };

        button::Style {
            background: bg,
            border: Border {
                color: border_color,
                width: border_width,
                radius: 32.0.into(),
            },
            text_color: Color::WHITE,
            shadow: Shadow::default(),
            snap: false,
        }
    })
    .into()
}

/// Build the dropdown menu panel.
///
/// Renders `items` as a group, an optional divider, then `secondary_items`.
/// Each item click invokes `on_item_click(index)` where indices are contiguous
/// across both groups (secondary items start at `items.len()`).
pub fn dropdown_menu<'a, Message: Clone + 'a>(
    items: &[DropdownItemDef],
    secondary_items: &[DropdownItemDef],
    on_item_click: impl Fn(usize) -> Message + 'a,
) -> iced::Element<'a, Message, Theme, iced::Renderer> {
    let mut elements: Vec<iced::Element<'a, Message, Theme, iced::Renderer>> = Vec::new();

    // Primary items
    for (i, def) in items.iter().enumerate() {
        elements.push(dropdown_menu_item(def.label, def.icon, on_item_click(i)));
    }

    // Divider + secondary items
    if !secondary_items.is_empty() {
        elements.push(dropdown_divider());
        let offset = items.len();
        for (i, def) in secondary_items.iter().enumerate() {
            elements.push(dropdown_menu_item(
                def.label,
                def.icon,
                on_item_click(offset + i),
            ));
        }
    }

    container(column(elements).width(Length::Fixed(248.0)))
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
        })
        .into()
}

/// Wrap a base element with a dropdown overlay (dismiss backdrop + positioned menu).
///
/// Builds a 3-layer stack:
/// 1. `base` — the normal window content
/// 2. Dismiss backdrop — invisible full-size button that emits `on_dismiss`
/// 3. Positioned menu — anchored at `top_offset` from the top, `right_padding` from the right
pub fn dropdown_overlay<'a, Message: Clone + 'a>(
    base: iced::Element<'a, Message, Theme, iced::Renderer>,
    menu: iced::Element<'a, Message, Theme, iced::Renderer>,
    on_dismiss: Message,
    top_offset: f32,
    right_padding: f32,
) -> iced::Element<'a, Message, Theme, iced::Renderer> {
    // Invisible full-size button that catches clicks outside the menu
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

    // Position the menu at top-right using spacers
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

    // Stack: base → dismiss backdrop → dropdown panel
    // The dropdown panel sits on top of the backdrop so its buttons
    // receive clicks before the dismiss layer.
    stack![base, dismiss_backdrop, dropdown_positioned].into()
}

// ── Internals ───────────────────────────────────────────────────────────────

/// Build a single dropdown menu item.
fn dropdown_menu_item<'a, Message: Clone + 'a>(
    label: &'static str,
    icon_data: &'static [u8],
    on_press: Message,
) -> iced::Element<'a, Message, Theme, iced::Renderer> {
    // Icon (16×16 with gray stroke color)
    let icon_handle = svg::Handle::from_memory(icon_data);
    let icon = svg(icon_handle)
        .width(Length::Fixed(16.0))
        .height(Length::Fixed(16.0))
        .style(|_theme: &Theme, _status| svg::Style {
            color: Some(ColorToken::Gray400.to_color()),
        });

    // Label text
    let label_text = text(label)
        .size(14)
        .color(Color::WHITE)
        .font(GEIST_MEDIUM)
        .wrapping(text_widget::Wrapping::None);

    let content_row = row![icon, label_text].spacing(8).align_y(Alignment::Center);

    // Wrap in a styled container, then in a button for click handling
    let content = container(content_row)
        .width(Length::Fill)
        .padding(Padding::from([8, 10]))
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

/// Build a horizontal divider for the dropdown menu.
fn dropdown_divider<'a, Message: 'a>() -> iced::Element<'a, Message, Theme, iced::Renderer> {
    container(
        container(Space::new().height(Length::Fixed(1.0)).width(Length::Fill)).style(
            |_theme: &Theme| container::Style {
                background: Some(Background::Color(ColorToken::Slate500.to_color())),
                ..Default::default()
            },
        ),
    )
    .padding(Padding::from([4, 0]))
    .width(Length::Fill)
    .into()
}
