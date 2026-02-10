use crate::utils::clock::Clock;
use crate::utils::geometry::Position;
use iced::widget::canvas::{Frame, Path, Stroke};
use iced::Color;
use std::collections::VecDeque;
use std::sync::Arc;

const MAX_ANIMATIONS: usize = 30;
pub const ANIMATION_DURATION: u64 = 1000;
const RING_DELAY_MS: u128 = 300;

/// Color #B91801
const CLICK_COLOR: Color = Color {
    r: 0.725,
    g: 0.094,
    b: 0.004,
    a: 1.,
};

const INITIAL_RADIUS: f32 = 5.0;
const MAX_RADIUS_GROWTH: f32 = 30.0;

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

    pub fn draw(&self, frame: &mut Frame) {
        for slot in &self.used_slots {
            let anim = &self.click_animations[*slot];
            let instant = match anim.enabled_instant {
                Some(i) => i,
                None => continue,
            };

            let elapsed = self.clock.now().duration_since(instant).as_millis();
            if elapsed > ANIMATION_DURATION.into() {
                continue;
            }

            let x = anim.position.x as f32;
            let y = anim.position.y as f32;

            if elapsed <= RING_DELAY_MS {
                // Filled circle phase
                let circle = Path::circle(iced::Point::new(x, y), INITIAL_RADIUS);
                frame.fill(&circle, CLICK_COLOR);
            } else {
                // Expanding ring phase
                let t = (elapsed - RING_DELAY_MS) as f32
                    / (ANIMATION_DURATION as f32 - RING_DELAY_MS as f32);
                let radius = INITIAL_RADIUS + MAX_RADIUS_GROWTH * t;
                let circle = Path::circle(iced::Point::new(x, y), radius);
                frame.stroke(
                    &circle,
                    Stroke::default().with_color(CLICK_COLOR).with_width(2.0),
                );
            }
        }
    }
}
