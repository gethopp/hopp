//! Reusable segmented control widget for iced.
//!
//! A pill-shaped container with icon tabs. One tab is "active" at a time,
//! identified by a string ID. The active-tab indicator slides smoothly between
//! positions using a simple ease-out cubic animation driven by `std::time::Instant`.

use std::time::Instant;

use iced::widget::{button, container, row, stack, svg, Space};
use iced::{Background, Border, Color, Length, Shadow, Theme};

use crate::windows::colors::ColorToken;
use crate::windows::shadows::ShadowToken;

/// Fixed width for every tab so the layout does not shift on selection.
const TAB_WIDTH: f32 = 44.0;
/// Tab/indicator height.
const TAB_HEIGHT: f32 = 26.0;
/// Horizontal padding inside the outer container (left & right of the tabs row).
const OUTER_PAD_H: f32 = 0.0;
/// Duration of the slide animation in milliseconds.
const ANIM_DURATION_MS: u128 = 200;

/// Definition for a single button inside the segmented control.
pub struct SegmentedButton {
    /// Unique identifier for this button, returned via the `on_select` callback.
    pub id: &'static str,
    /// SVG icon bytes (embedded at compile time).
    pub icon: &'static [u8],
}

/// Tracks the sliding animation between tabs.
#[derive(Debug, Clone)]
pub struct SegmentedControlAnim {
    /// Tab index we are animating *from* (internal positional index).
    from_idx: usize,
    /// When the animation started.
    pub started_at: Instant,
}

/// Returns `true` when the animation is still in progress and the caller
/// should request another redraw.
pub fn animation_running(anim: &Option<SegmentedControlAnim>) -> bool {
    match anim {
        Some(a) => a.started_at.elapsed().as_millis() < ANIM_DURATION_MS,
        None => false,
    }
}

/// Call this when the user selects a new tab.  Returns `Some(anim)` to store in
/// the state so the indicator will animate from the old position.
///
/// `buttons` is needed to resolve the IDs to positional indices for animation.
pub fn start_animation(
    buttons: &[SegmentedButton],
    current_id: &str,
    new_id: &str,
) -> Option<SegmentedControlAnim> {
    if current_id == new_id {
        return None;
    }
    let from_idx = buttons.iter().position(|b| b.id == current_id).unwrap_or(0);
    Some(SegmentedControlAnim {
        from_idx,
        started_at: Instant::now(),
    })
}

/// Clear the animation once finished.
pub fn tick_animation(anim: &mut Option<SegmentedControlAnim>) {
    if let Some(a) = anim {
        if a.started_at.elapsed().as_millis() >= ANIM_DURATION_MS {
            *anim = None;
        }
    }
}

// ── Widget builder ───────────────────────────────────────────────────────────

/// Build a segmented control element.
///
/// * `buttons`    – slice of [`SegmentedButton`] definitions (icon + id).
/// * `active_id`  – the `id` of the currently selected button.
/// * `anim`       – optional animation state (for the sliding indicator).
/// * `on_select`  – closure that maps a button id to a message.
///
/// ## Design specs (approx):
/// - Container: Slate800 bg, radius 30, height 26
/// - Active indicator: Slate400 bg, white 20% border, radius 30
/// - Inactive tabs: transparent bg
/// - Active icon: white 100%, Inactive icon: Gray400
pub fn segmented_control<'a, Message: Clone + 'a>(
    buttons: &[SegmentedButton],
    active_id: &str,
    anim: &Option<SegmentedControlAnim>,
    on_select: impl Fn(&'static str) -> Message + 'a,
) -> iced::Element<'a, Message, Theme, iced::Renderer> {
    let tab_count = buttons.len();
    let total_width = tab_count as f32 * TAB_WIDTH + OUTER_PAD_H * 2.0;

    // Resolve active id → positional index for layout calculations.
    let active_idx = buttons.iter().position(|b| b.id == active_id).unwrap_or(0);

    let indicator_x = compute_indicator_x(active_idx, anim);

    let indicator = container(Space::new())
        .width(Length::Fixed(TAB_WIDTH))
        .height(Length::Fixed(TAB_HEIGHT))
        .style(|_theme: &Theme| container::Style {
            background: Some(Background::Color(ColorToken::Slate400.to_color())),
            border: Border {
                color: Color::from_rgba(1.0, 1.0, 1.0, 0.4),
                width: 1.0,
                radius: 30.0.into(),
            },
            shadow: ShadowToken::Xl.to_shadow(),
            ..Default::default()
        });

    let indicator_row = row![
        Space::new().width(Length::Fixed(OUTER_PAD_H + indicator_x)),
        indicator,
    ];

    let indicator_layer = container(indicator_row)
        .width(Length::Fixed(total_width))
        .height(Length::Fixed(TAB_HEIGHT));

    let is_animating = animation_running(anim);
    let mut tab_buttons: Vec<iced::Element<'a, Message, Theme, iced::Renderer>> = Vec::new();
    for (i, btn) in buttons.iter().enumerate() {
        let is_active = i == active_idx;
        let msg = on_select(btn.id);
        tab_buttons.push(tab_button(btn.icon, is_active, is_animating, msg));
    }

    let buttons_layer = container(row(tab_buttons))
        .width(Length::Fixed(total_width))
        .height(Length::Fixed(TAB_HEIGHT));

    let stacked = stack![indicator_layer, buttons_layer];

    container(stacked)
        .width(Length::Fixed(total_width))
        .height(Length::Fixed(TAB_HEIGHT))
        .style(|_theme: &Theme| container::Style {
            background: Some(Background::Color(ColorToken::Slate800.to_color())),
            border: Border {
                color: Color::TRANSPARENT,
                width: 0.0,
                radius: 30.0.into(),
            },
            ..Default::default()
        })
        .into()
}

/// Compute the indicator's current x-offset, accounting for animation.
fn compute_indicator_x(active_idx: usize, anim: &Option<SegmentedControlAnim>) -> f32 {
    let target_x = active_idx as f32 * TAB_WIDTH;

    let Some(a) = anim else {
        return target_x;
    };

    let elapsed = a.started_at.elapsed().as_millis();
    if elapsed >= ANIM_DURATION_MS {
        return target_x;
    }

    let t = (elapsed as f32) / (ANIM_DURATION_MS as f32); // 0.0 → 1.0
    let eased = ease_out_cubic(t);

    let from_x = a.from_idx as f32 * TAB_WIDTH;
    from_x + (target_x - from_x) * eased
}

/// Ease-out cubic: decelerating towards the end.
#[inline]
fn ease_out_cubic(t: f32) -> f32 {
    1.0 - (1.0 - t).powi(3)
}

/// Build a single transparent icon button inside the segmented control.
///
/// When `is_animating` is true, hover / press visual feedback is suppressed so
/// the sliding indicator animation stays clean.
fn tab_button<'a, Message: Clone + 'a>(
    icon_data: &'static [u8],
    is_active: bool,
    is_animating: bool,
    on_press: Message,
) -> iced::Element<'a, Message, Theme, iced::Renderer> {
    let icon_handle = svg::Handle::from_memory(icon_data);
    let icon_color = if is_active {
        Color::WHITE
    } else {
        ColorToken::Gray400.to_color()
    };
    let icon = svg(icon_handle)
        .width(Length::Fixed(20.0))
        .height(Length::Fixed(20.0))
        .style(move |_theme: &Theme, _status| svg::Style {
            color: Some(icon_color),
        });

    button(
        container(icon)
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill),
    )
    .width(Length::Fixed(TAB_WIDTH))
    .height(Length::Fixed(TAB_HEIGHT))
    .on_press(on_press)
    .padding(0)
    .style(move |_theme: &Theme, status| {
        // Suppress hover/press effects while the indicator is sliding so the
        // cursor hovering over the old position doesn't cause visual glitches.
        let bg = if is_animating || is_active {
            None
        } else {
            match status {
                button::Status::Hovered => {
                    Some(Background::Color(Color::from_rgba(1.0, 1.0, 1.0, 0.1)))
                }
                button::Status::Pressed => {
                    Some(Background::Color(Color::from_rgba(1.0, 1.0, 1.0, 0.05)))
                }
                _ => None,
            }
        };

        button::Style {
            background: bg,
            border: Border {
                color: Color::TRANSPARENT,
                width: 0.0,
                radius: 30.0.into(),
            },
            text_color: Color::WHITE,
            shadow: Shadow::default(),
            snap: false,
        }
    })
    .into()
}
