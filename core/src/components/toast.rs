//! Reusable toast notification component for iced.
//!
//! Renders a pill-shaped notification overlay with fade-in/out animation.
//! Follows the same animation pattern as `segmented_control.rs`: store an
//! `Instant` at show-time, compute opacity each frame from elapsed time.
//!
//! Single-toast semantics: assigning a new `Some(ToastState)` discards any
//! previous toast automatically.

use std::time::Instant;

use iced::widget::{container, text};
use iced::{gradient, Alignment, Background, Border, Color, Length, Padding, Radians, Theme};

use crate::windows::colors::ColorToken;

const FADE_IN_MS: u128 = 200;
const FADE_OUT_MS: u128 = 200;

/// CSS-like absolute positioning within a `stack![]` overlay.
///
/// Each field acts like its CSS counterpart: the toast is pushed away from
/// that edge by the given number of logical pixels. Unset edges → centered
/// along that axis (or default to start for vertical).
pub struct ToastPosition {
    pub top: Option<f32>,
    pub right: Option<f32>,
    pub bottom: Option<f32>,
    pub left: Option<f32>,
}

pub struct ToastState {
    text: String,
    shown_at: Instant,
    display_duration_ms: u128,
    position: ToastPosition,
}

pub fn show_toast(text: String, display_duration_ms: u128, position: ToastPosition) -> ToastState {
    ToastState {
        text,
        shown_at: Instant::now(),
        display_duration_ms,
        position,
    }
}

/// Returns `true` while the toast (including its fade-out tail) is still visible.
pub fn toast_active(state: &Option<ToastState>) -> bool {
    match state {
        Some(s) => {
            s.shown_at.elapsed().as_millis() < FADE_IN_MS + s.display_duration_ms + FADE_OUT_MS
        }
        None => false,
    }
}

/// Auto-clear the toast once the full lifecycle (fade-in + display + fade-out) has elapsed.
pub fn tick_toast(state: &mut Option<ToastState>) {
    if let Some(s) = state {
        if s.shown_at.elapsed().as_millis() >= FADE_IN_MS + s.display_duration_ms + FADE_OUT_MS {
            *state = None;
        }
    }
}

fn compute_opacity(state: &ToastState) -> f32 {
    let elapsed = state.shown_at.elapsed().as_millis();

    if elapsed < FADE_IN_MS {
        let t = elapsed as f32 / FADE_IN_MS as f32;
        simple_easing::cubic_out(t)
    } else if elapsed < FADE_IN_MS + state.display_duration_ms {
        1.0
    } else {
        let fade_elapsed = elapsed - FADE_IN_MS - state.display_duration_ms;
        let t = (fade_elapsed as f32 / FADE_OUT_MS as f32).min(1.0);
        1.0 - simple_easing::cubic_out(t)
    }
}

/// Build the toast overlay element, or `None` if no toast is visible.
///
/// The returned element is a full-size container meant to be layered on top
/// of existing content via `stack![]`.
pub fn toast_view<'a, Message: 'a>(
    state: &Option<ToastState>,
    position_override: Option<&ToastPosition>,
) -> Option<iced::Element<'a, Message, Theme, iced::Renderer>> {
    let state = state.as_ref()?;
    let opacity = compute_opacity(state);
    if opacity <= 0.0 {
        return None;
    }

    let pos = position_override.unwrap_or(&state.position);
    let label_text = state.text.clone();
    let pad_top = pos.top.unwrap_or(0.0);
    let pad_right = pos.right.unwrap_or(0.0);
    let pad_bottom = pos.bottom.unwrap_or(0.0);
    let pad_left = pos.left.unwrap_or(0.0);
    let align_x = match (pos.left.is_some(), pos.right.is_some()) {
        (true, false) => Alignment::Start,
        (false, true) => Alignment::End,
        _ => Alignment::Center,
    };
    let align_y = match (pos.top.is_some(), pos.bottom.is_some()) {
        (false, true) => Alignment::End,
        _ => Alignment::Start,
    };

    let label = text(label_text)
        .size(10.0)
        .color(Color::from_rgba(1.0, 1.0, 1.0, opacity));

    let pill = container(label)
        .padding(Padding::from([4.0, 12.0]))
        .style(move |_theme: &Theme| {
            let top = ColorToken::Zinc800.to_color();
            let bottom = ColorToken::Zinc700.to_color();
            let grad = gradient::Linear::new(Radians(std::f32::consts::PI))
                .add_stop(0.0, Color::from_rgba(top.r, top.g, top.b, 0.4 * opacity))
                .add_stop(
                    1.0,
                    Color::from_rgba(bottom.r, bottom.g, bottom.b, 0.4 * opacity),
                );

            container::Style {
                background: Some(Background::Gradient(grad.into())),
                border: Border {
                    color: Color::from_rgba(1.0, 1.0, 1.0, 0.2 * opacity),
                    width: 1.0,
                    radius: 15.0.into(),
                },
                ..Default::default()
            }
        });

    let positioned = container(pill)
        .width(Length::Fill)
        .height(Length::Fill)
        .padding(Padding {
            top: pad_top,
            right: pad_right,
            bottom: pad_bottom,
            left: pad_left,
        })
        .align_x(align_x)
        .align_y(align_y);

    Some(positioned.into())
}
