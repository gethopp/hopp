//! A separate, capturable window with known changing content for the media harness.
use std::sync::Arc;
use std::time::{Duration, Instant};

use winit::application::ApplicationHandler;
use winit::dpi::{LogicalPosition, LogicalSize};
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::window::{Window, WindowId, WindowLevel};

struct Fixture {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    started: Instant,
    reported: bool,
    last_draw: Instant,
    modifiers: ModifiersState,
    text: String,
}

impl Fixture {
    fn new(event_loop: &ActiveEventLoop) -> Self {
        let window = Arc::new(
            event_loop
                .create_window(
                    Window::default_attributes()
                        .with_title("Hopp media test fixture")
                        .with_window_level(WindowLevel::AlwaysOnTop)
                        .with_resizable(false)
                        .with_inner_size(LogicalSize::new(640.0, 400.0))
                        .with_position(LogicalPosition::new(100.0, 100.0)),
                )
                .expect("create fixture window"),
        );
        if let Some(monitor) = event_loop.primary_monitor() {
            let scale = monitor.scale_factor();
            let origin = monitor.position().to_logical::<f64>(scale);
            let size = monitor.size().to_logical::<f64>(scale);
            window.set_outer_position(LogicalPosition::new(
                origin.x + (size.width - 640.0) / 2.0,
                origin.y + (size.height - 400.0) / 2.0,
            ));
        }
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
        let surface = instance
            .create_surface(window.clone())
            .expect("fixture surface");
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            compatible_surface: Some(&surface),
            ..Default::default()
        }))
        .expect("fixture adapter");
        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
                .expect("fixture device");
        let caps = surface.get_capabilities(&adapter);
        let size = window.inner_size();
        surface.configure(
            &device,
            &wgpu::SurfaceConfiguration {
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                format: caps.formats[0],
                width: size.width.max(1),
                height: size.height.max(1),
                present_mode: wgpu::PresentMode::AutoVsync,
                alpha_mode: wgpu::CompositeAlphaMode::Opaque,
                view_formats: vec![],
                desired_maximum_frame_latency: 2,
            },
        );
        Self {
            window,
            surface,
            device,
            queue,
            started: Instant::now(),
            reported: false,
            last_draw: Instant::now() - Duration::from_millis(50),
            modifiers: ModifiersState::empty(),
            text: "hopp-fixture".to_string(),
        }
    }

    fn draw(&mut self) {
        let output = self.surface.get_current_texture().expect("fixture frame");
        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let color = if (self.started.elapsed().as_millis() / 200) % 2 == 0 {
            wgpu::Color {
                r: 1.0,
                g: 0.0,
                b: 0.0,
                a: 1.0,
            }
        } else {
            wgpu::Color {
                r: 0.0,
                g: 0.0,
                b: 1.0,
                a: 1.0,
            }
        };
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        {
            let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(color),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                ..Default::default()
            });
        }
        self.queue.submit(Some(encoder.finish()));
        self.window.pre_present_notify();
        output.present();
        self.last_draw = Instant::now();
        if !self.reported {
            self.report();
            self.reported = true;
        }
    }

    fn input(&mut self, event: WindowEvent) {
        match event {
            WindowEvent::ModifiersChanged(modifiers) => self.modifiers = modifiers.state(),
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => {
                println!("{}", serde_json::json!({"event":"click"}));
            }
            WindowEvent::KeyboardInput { event, .. }
                if event.state.is_pressed() && !event.repeat =>
            {
                let command = self.modifiers.super_key() || self.modifiers.control_key();
                match event.logical_key {
                    Key::Character(key) if !command => {
                        self.text.push_str(&key);
                        println!(
                            "{}",
                            serde_json::json!({"event":"key","text":key.as_str(),"value":self.text})
                        );
                    }
                    Key::Named(NamedKey::Backspace) if !command => {
                        self.text.pop();
                        println!(
                            "{}",
                            serde_json::json!({"event":"backspace","value":self.text})
                        );
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    fn report(&self) {
        #[cfg(target_os = "macos")]
        {
            use objc2::rc::Retained;
            use objc2_app_kit::NSView;
            use raw_window_handle::{HasWindowHandle, RawWindowHandle};
            let handle = self.window.window_handle().expect("fixture native handle");
            let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
                panic!("AppKit handle required")
            };
            let view: Retained<NSView> =
                unsafe { Retained::retain(handle.ns_view.as_ptr().cast()) }
                    .expect("fixture NSView");
            let window = view.window().expect("fixture NSWindow");
            let position = self
                .window
                .inner_position()
                .expect("fixture position")
                .to_logical::<f64>(self.window.scale_factor());
            let size = self
                .window
                .inner_size()
                .to_logical::<f64>(self.window.scale_factor());
            let display_id = core_graphics::display::CGDisplay::main().id;
            println!(
                "{}",
                serde_json::json!({
                    "id": window.windowNumber() as u32, "display_id": display_id,
                    "content_type": "Window", "x":position.x,"y":position.y,
                    "width":size.width,"height":size.height,
                })
            );
        }
        #[cfg(not(target_os = "macos"))]
        panic!("the capture fixture currently requires macOS");
    }
}

#[derive(Default)]
struct App(Option<Fixture>);

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.0.is_none() {
            self.0 = Some(Fixture::new(event_loop));
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::RedrawRequested => self.0.as_mut().expect("fixture initialized").draw(),
            event => self.0.as_mut().expect("fixture initialized").input(event),
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(fixture) = &self.0 {
            let next_frame = fixture.last_draw + Duration::from_millis(50);
            if Instant::now() >= next_frame {
                fixture.window.request_redraw();
            }
            event_loop.set_control_flow(ControlFlow::WaitUntil(next_frame));
        }
    }
}

fn main() -> Result<(), winit::error::EventLoopError> {
    let event_loop = EventLoop::new()?;
    event_loop.run_app(&mut App::default())
}
