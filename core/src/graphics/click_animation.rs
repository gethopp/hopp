use crate::utils::clock::Clock;
use crate::utils::geometry::Position;
use iced::widget::canvas::{Frame, Geometry, Path, Stroke};
use iced::{Color, Rectangle, Renderer};
use std::collections::VecDeque;
use std::sync::Arc;

const MAX_ANIMATIONS: usize = 30;
pub const ANIMATION_DURATION: u64 = 800;

/// #3b82f6 — Tailwind blue-500
const RIPPLE_COLOR: Color = Color {
    r: 0.231,
    g: 0.510,
    b: 0.965,
    a: 1.0,
};

const BASE_RADIUS: f32 = 12.0;
const MIN_SCALE: f32 = 0.8;
const MAX_SCALE: f32 = 1.6;
const STROKE_WIDTH: f32 = 2.0;

#[derive(Debug)]
struct ClickAnimation {
    position: Position,
    enabled_instant: Option<std::time::Instant>,
    clock: Arc<dyn Clock>,
}

impl ClickAnimation {
    fn enable(&mut self, position: Position) {
        self.position = position;
        self.enabled_instant = Some(self.clock.now());
    }

    fn disable(&mut self) {
        self.enabled_instant = None;
    }
}

#[derive(Debug)]
pub struct ClickAnimationRenderer {
    click_animation_position_sender: std::sync::mpsc::Sender<Position>,
    click_animation_position_receiver: std::sync::mpsc::Receiver<Position>,
    click_animations: Vec<ClickAnimation>,
    available_slots: VecDeque<usize>,
    used_slots: VecDeque<usize>,
    clock: Arc<dyn Clock>,
}

impl ClickAnimationRenderer {
    pub fn new(clock: Arc<dyn Clock>) -> Self {
        let mut click_animations = Vec::with_capacity(MAX_ANIMATIONS);
        let mut available_slots = VecDeque::with_capacity(MAX_ANIMATIONS);
        for _ in 0..MAX_ANIMATIONS {
            click_animations.push(ClickAnimation {
                position: Position { x: 0.0, y: 0.0 },
                enabled_instant: None,
                clock: clock.clone(),
            });
            available_slots.push_back(click_animations.len() - 1);
        }

        let (sender, receiver) = std::sync::mpsc::channel();
        Self {
            click_animation_position_sender: sender,
            click_animation_position_receiver: receiver,
            click_animations,
            available_slots,
            used_slots: VecDeque::new(),
            clock,
        }
    }

    /// Returns `true` when at least one click-pulse animation is still playing.
    pub fn is_animating(&self) -> bool {
        !self.used_slots.is_empty()
    }

    pub fn enable_click_animation(&mut self, position: Position) {
        if let Err(e) = self.click_animation_position_sender.send(position) {
            log::error!("enable_click_animation: error sending position: {e:?}");
        }
    }

    pub fn update(&mut self) {
        // Drain pending enable requests
        while let Ok(position) = self.click_animation_position_receiver.try_recv() {
            if self.available_slots.is_empty() {
                log::warn!("enable_click_animation: available_slots is empty");
                break;
            }
            let slot = self.available_slots.pop_front().unwrap();
            self.used_slots.push_back(slot);
            self.click_animations[slot].enable(position);
        }

        // Disable expired animations and reclaim slots
        let now = self.clock.now();
        while let Some(&front) = self.used_slots.front() {
            let anim = &self.click_animations[front];
            let expired = match anim.enabled_instant {
                Some(instant) => {
                    now.duration_since(instant).as_millis() > ANIMATION_DURATION.into()
                }
                None => true,
            };
            if expired {
                self.click_animations[front].disable();
                self.used_slots.pop_front();
                self.available_slots.push_back(front);
            } else {
                break;
            }
        }
    }

    pub fn draw(
        &self,
        renderer: &Renderer,
        bounds: Rectangle,
        translate: &dyn Fn(crate::utils::geometry::Position) -> crate::utils::geometry::Position,
    ) -> Geometry {
        let mut frame = Frame::new(renderer, bounds.size());
        let now = self.clock.now();
        for slot in &self.used_slots {
            let anim = &self.click_animations[*slot];
            let instant = match anim.enabled_instant {
                Some(i) => i,
                None => continue,
            };

            let elapsed = now.duration_since(instant).as_millis();
            if elapsed > ANIMATION_DURATION.into() {
                continue;
            }

            let pos = translate(anim.position);
            let x = pos.x as f32;
            let y = pos.y as f32;

            let t = elapsed as f32 / ANIMATION_DURATION as f32;
            let eased = 1.0 - (1.0 - t).powi(3);

            let scale = MIN_SCALE + (MAX_SCALE - MIN_SCALE) * eased;
            let radius = BASE_RADIUS * scale;

            let alpha = if eased <= 0.5 {
                1.0 - 0.4 * eased
            } else {
                0.8 * (1.0 - (eased - 0.5) / 0.5)
            };

            let color = Color {
                a: alpha,
                ..RIPPLE_COLOR
            };
            let circle = Path::circle(iced::Point::new(x, y), radius);
            frame.stroke(
                &circle,
                Stroke::default().with_color(color).with_width(STROKE_WIDTH),
            );
        }
        frame.into_geometry()
    }
}
