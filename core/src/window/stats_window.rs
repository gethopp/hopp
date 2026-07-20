use std::sync::Arc;
use std::time::{Duration, Instant as StdInstant};

use iced::widget::{column, container, text, Space};
use iced::{Color, Element, Length, Pixels, Size, Theme};
use iced_wgpu::core::mouse;
use iced_wgpu::graphics::{Shell, Viewport};
use iced_wgpu::Engine;
use iced_winit::core::renderer::Style;
use iced_winit::core::time::Instant;
use iced_winit::core::{window as iced_window, Event};
use iced_winit::runtime::user_interface::Cache;
use iced_winit::runtime::UserInterface;
use iced_winit::Clipboard;
use livekit::participant::ConnectionQuality;
use winit::dpi::LogicalSize;
use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::ModifiersState;
use winit::window::{Window, WindowAttributes, WindowLevel};

use crate::components::fonts::{self as fonts_mod, GEIST_MEDIUM, GEIST_REGULAR};
use crate::livekit::stats::RoomStats;
use crate::room_service::RoomService;

const STATS_WINDOW_WIDTH: f64 = 260.0;
const STATS_WINDOW_HEIGHT: f64 = 200.0;
const REDRAW_INTERVAL: Duration = Duration::from_secs(1);
const PADDING: f32 = 10.0;

#[derive(Debug, Clone)]
enum StatsMessage {}

pub struct StatsWindow {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    _queue: wgpu::Queue,
    format: wgpu::TextureFormat,
    _engine: Engine,
    renderer: iced::Renderer,
    viewport: Viewport,
    cache: Option<Cache>,
    clipboard: Clipboard,
    cursor: mouse::Cursor,
    modifiers: ModifiersState,
    resized: bool,
    last_redraw: StdInstant,
    stats: RoomStats,
    connection_quality: Option<ConnectionQuality>,
}

impl StatsWindow {
    pub fn new(event_loop: &ActiveEventLoop) -> Result<Self, Box<dyn std::error::Error>> {
        let attrs = WindowAttributes::default()
            .with_title("Stats")
            .with_inner_size(LogicalSize::new(STATS_WINDOW_WIDTH, STATS_WINDOW_HEIGHT))
            .with_resizable(false)
            .with_window_level(WindowLevel::AlwaysOnTop);

        #[cfg(target_os = "macos")]
        let attrs = {
            use winit::platform::macos::WindowAttributesExtMacOS;
            attrs
                .with_title_hidden(true)
                .with_titlebar_transparent(true)
                .with_fullsize_content_view(true)
        };

        let window = Arc::new(event_loop.create_window(attrs)?);

        #[cfg(target_os = "windows")]
        let backends = wgpu::Backends::DX12;
        #[cfg(not(target_os = "windows"))]
        let backends = wgpu::Backends::PRIMARY;

        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends,
            ..Default::default()
        });
        let surface = instance.create_surface(window.clone())?;
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
        }))
        .unwrap();
        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                label: Some("stats_window"),
                memory_hints: wgpu::MemoryHints::default(),
                trace: wgpu::Trace::default(),
                experimental_features: wgpu::ExperimentalFeatures::disabled(),
            }))?;

        let caps = surface.get_capabilities(&adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| !f.is_srgb())
            .unwrap_or(caps.formats[0]);
        let physical = window.inner_size();
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: physical.width.max(1),
            height: physical.height.max(1),
            present_mode: wgpu::PresentMode::AutoVsync,
            alpha_mode: wgpu::CompositeAlphaMode::Opaque,
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        let engine = Engine::new(
            &adapter,
            device.clone(),
            queue.clone(),
            format,
            None,
            Shell::headless(),
        );
        let wgpu_renderer =
            iced_wgpu::Renderer::new(engine.clone(), GEIST_REGULAR, Pixels::from(13));
        fonts_mod::load_fonts();
        let renderer = iced::Renderer::Primary(wgpu_renderer);

        let viewport = Viewport::with_physical_size(
            Size::new(physical.width.max(1), physical.height.max(1)),
            window.scale_factor() as f32,
        );
        let clipboard = Clipboard::connect(window.clone());

        Ok(Self {
            window,
            surface,
            device,
            _queue: queue,
            format,
            _engine: engine,
            renderer,
            viewport,
            cache: Some(Cache::default()),
            clipboard,
            cursor: mouse::Cursor::Unavailable,
            modifiers: ModifiersState::default(),
            resized: false,
            last_redraw: StdInstant::now() - REDRAW_INTERVAL,
            stats: RoomStats::default(),
            connection_quality: None,
        })
    }

    pub fn window_id(&self) -> winit::window::WindowId {
        self.window.id()
    }

    pub fn request_redraw(&self) {
        self.window.request_redraw();
    }

    pub fn next_redraw_at(&self) -> StdInstant {
        self.last_redraw + REDRAW_INTERVAL
    }

    pub fn update_stats(&mut self, room_service: &RoomService) {
        self.stats = room_service.stats();
        self.connection_quality = room_service.connection_quality();
    }

    pub fn handle_window_event(&mut self, event: WindowEvent) {
        match event {
            WindowEvent::ModifiersChanged(m) => self.modifiers = m.state(),
            WindowEvent::Resized(new_size) => {
                if new_size.width > 0 && new_size.height > 0 {
                    self.viewport = Viewport::with_physical_size(
                        Size::new(new_size.width, new_size.height),
                        self.window.scale_factor() as f32,
                    );
                    self.resized = true;
                    self.window.request_redraw();
                }
            }
            WindowEvent::RedrawRequested => {
                if self.last_redraw.elapsed() >= REDRAW_INTERVAL {
                    self.redraw();
                    self.last_redraw = StdInstant::now();
                }
            }
            WindowEvent::CloseRequested => {
                self.window.set_visible(false);
            }
            _ => {}
        }
    }

    fn view(
        stats: &RoomStats,
        connection_quality: Option<ConnectionQuality>,
    ) -> Element<'_, StatsMessage, Theme, iced::Renderer> {
        let cq = match connection_quality {
            Some(ConnectionQuality::Excellent) => "Excellent",
            Some(ConnectionQuality::Good) => "Good",
            Some(ConnectionQuality::Poor) => "Poor",
            Some(ConnectionQuality::Lost) => "Lost",
            None => "N/A",
        };

        let label_color = Color::from_rgb(0.6, 0.6, 0.6);
        let value_color = Color::WHITE;

        let line =
            |label: &str, value: String| -> Element<'_, StatsMessage, Theme, iced::Renderer> {
                iced::widget::row![
                    text(label.to_string())
                        .size(11)
                        .color(label_color)
                        .width(Length::FillPortion(1)),
                    text(value)
                        .size(11)
                        .color(value_color)
                        .font(GEIST_MEDIUM)
                        .width(Length::FillPortion(1)),
                ]
                .spacing(4)
                .into()
            };

        let content = column![
            text("Stats")
                .size(13)
                .color(Color::WHITE)
                .font(GEIST_MEDIUM),
            Space::new().height(4),
            line("Connection", cq.to_string()),
            line(
                "Screen",
                format!(
                    "{}x{}@{:.0}fps",
                    stats.screenshare_width, stats.screenshare_height, stats.screenshare_fps
                )
            ),
            line("Codec", stats.screenshare_codec_id.clone()),
            line(
                "Jitter buf",
                format!("{:.1}ms", stats.screenshare_jitter_buffer_delay)
            ),
            line(
                "SS inbound",
                format!("{:.2} Mbps", stats.screenshare_input_bps / 1_000_000.0)
            ),
            line(
                "Total in",
                format!("{:.2} Mbps", stats.total_input_bps / 1_000_000.0)
            ),
            line(
                "Total out",
                format!("{:.2} Mbps", stats.total_output_bps / 1_000_000.0)
            ),
        ]
        .spacing(2)
        .padding(PADDING);

        container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(|_theme: &Theme| container::Style {
                background: Some(iced::Background::Color(Color::from_rgba(
                    0.1, 0.1, 0.1, 0.95,
                ))),
                border: iced::Border {
                    color: Color::from_rgb(0.3, 0.3, 0.3),
                    width: 1.0,
                    radius: 8.0.into(),
                },
                ..Default::default()
            })
            .into()
    }

    fn redraw(&mut self) {
        if self.resized {
            let size = self.window.inner_size();
            if size.width > 0 && size.height > 0 {
                self.surface.configure(
                    &self.device,
                    &wgpu::SurfaceConfiguration {
                        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                        format: self.format,
                        width: size.width,
                        height: size.height,
                        present_mode: wgpu::PresentMode::AutoVsync,
                        alpha_mode: wgpu::CompositeAlphaMode::Opaque,
                        view_formats: vec![],
                        desired_maximum_frame_latency: 2,
                    },
                );
            }
            self.resized = false;
        }

        let output = match self.surface.get_current_texture() {
            Ok(o) => o,
            Err(e) => {
                log::error!("StatsWindow::redraw: failed to get texture: {e:?}");
                return;
            }
        };
        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let cache = self.cache.take().unwrap_or_default();
        let mut interface = UserInterface::build(
            Self::view(&self.stats, self.connection_quality),
            self.viewport.logical_size(),
            cache,
            &mut self.renderer,
        );

        let _ = interface.update(
            &[Event::Window(iced_window::Event::RedrawRequested(
                Instant::now(),
            ))],
            self.cursor,
            &mut self.renderer,
            &mut self.clipboard,
            &mut Vec::new(),
        );

        interface.draw(
            &mut self.renderer,
            &Theme::Dark,
            &Style {
                text_color: Color::WHITE,
            },
            self.cursor,
        );
        self.cache = Some(interface.into_cache());

        let wgpu_renderer = match &mut self.renderer {
            iced::Renderer::Primary(r) => r,
            _ => unreachable!(),
        };
        wgpu_renderer.present(None, output.texture.format(), &view, &self.viewport);
        self.window.pre_present_notify();
        output.present();
    }
}
