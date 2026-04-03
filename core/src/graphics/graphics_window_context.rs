use iced_wgpu::graphics::Shell;
use iced_wgpu::Engine;
use std::sync::Arc;
use winit::event_loop::ActiveEventLoop;
use winit::window::Window;

#[derive(Debug, thiserror::Error)]
pub enum GraphicsWindowContextError {
    #[error("Failed to create surface")]
    SurfaceCreation,
    #[error("Failed to request adapter")]
    AdapterRequest,
    #[error("Failed to request device")]
    DeviceRequest,
}

pub struct SurfaceInfo {
    pub surface: wgpu::Surface<'static>,
    pub format: wgpu::TextureFormat,
    pub alpha_mode: wgpu::CompositeAlphaMode,
}

pub enum SurfaceInitProfile {
    StandardWindow,
    Overlay,
}

pub struct GraphicsWindowContext {
    pub instance: wgpu::Instance,
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub present_mode: wgpu::PresentMode,
    pub engine: Engine,
    surface_init_profile: SurfaceInitProfile,
}

impl GraphicsWindowContext {
    pub fn new(
        window: &Arc<Window>,
        power_preference: wgpu::PowerPreference,
        present_mode: wgpu::PresentMode,
        label: &str,
        surface_init_profile: SurfaceInitProfile,
    ) -> Result<(Self, SurfaceInfo), GraphicsWindowContextError> {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::PRIMARY,
            ..Default::default()
        });

        let surface = instance.create_surface(window.clone()).map_err(|e| {
            log::error!("{label}: failed to create surface: {e:?}");
            GraphicsWindowContextError::SurfaceCreation
        })?;

        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
        }))
        .map_err(|e| {
            log::error!("{label}: failed to request adapter: {e:?}");
            GraphicsWindowContextError::AdapterRequest
        })?;

        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default(),
            label: Some(label),
            memory_hints: wgpu::MemoryHints::default(),
            trace: wgpu::Trace::default(),
            experimental_features: wgpu::ExperimentalFeatures::disabled(),
        }))
        .map_err(|e| {
            log::error!("{label}: failed to request device: {e:?}");
            GraphicsWindowContextError::DeviceRequest
        })?;

        let surface_info = Self::configure_surface(
            surface,
            &adapter,
            &device,
            present_mode,
            window,
            &surface_init_profile,
        );

        let engine = Engine::new(
            &adapter,
            device.clone(),
            queue.clone(),
            surface_info.format,
            Some(iced_wgpu::graphics::Antialiasing::MSAAx4),
            Shell::headless(),
        );

        let ctx = Self {
            instance,
            adapter,
            device,
            queue,
            present_mode,
            engine,
            surface_init_profile,
        };
        Ok((ctx, surface_info))
    }

    pub fn create_surface(
        &self,
        window: &Arc<Window>,
    ) -> Result<SurfaceInfo, GraphicsWindowContextError> {
        let surface = self.instance.create_surface(window.clone()).map_err(|e| {
            log::error!("Failed to create surface: {e:?}");
            GraphicsWindowContextError::SurfaceCreation
        })?;
        Ok(Self::configure_surface(
            surface,
            &self.adapter,
            &self.device,
            self.present_mode,
            window,
            &self.surface_init_profile,
        ))
    }

    fn configure_surface(
        surface: wgpu::Surface<'static>,
        adapter: &wgpu::Adapter,
        device: &wgpu::Device,
        present_mode: wgpu::PresentMode,
        window: &Arc<Window>,
        profile: &SurfaceInitProfile,
    ) -> SurfaceInfo {
        let caps = surface.get_capabilities(adapter);
        let physical_size = window.inner_size();

        let (format, alpha_mode) = match profile {
            SurfaceInitProfile::StandardWindow => {
                let format = caps
                    .formats
                    .iter()
                    .copied()
                    .find(|f| !f.is_srgb())
                    .unwrap_or(caps.formats[0]);
                let alpha_mode = crate::window::vibrancy::pick_transparent_alpha_mode(&caps);
                (format, alpha_mode)
            }
            SurfaceInitProfile::Overlay => {
                let format = caps.formats[0];
                let alpha_mode = caps
                    .alpha_modes
                    .iter()
                    .find(|mode| {
                        #[allow(unused_variables)]
                        let post_multiplied = mode == &&wgpu::CompositeAlphaMode::PostMultiplied;
                        #[cfg(target_os = "windows")]
                        let post_multiplied = false;
                        (mode != &&wgpu::CompositeAlphaMode::Opaque)
                            && ((mode == &&wgpu::CompositeAlphaMode::PreMultiplied)
                                || post_multiplied)
                    })
                    .copied()
                    .unwrap_or(caps.alpha_modes[0]);
                (format, alpha_mode)
            }
        };

        let surface_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: physical_size.width.max(1),
            height: physical_size.height.max(1),
            present_mode,
            alpha_mode,
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(device, &surface_config);
        SurfaceInfo {
            surface,
            format,
            alpha_mode,
        }
    }
}

pub struct ContextManager {
    pub camera_context: GraphicsWindowContext,
    pub screensharing_context: GraphicsWindowContext,
    pub overlay_context: GraphicsWindowContext,
}

impl ContextManager {
    pub fn new(event_loop: &ActiveEventLoop) -> Result<Self, GraphicsWindowContextError> {
        let cam_attrs =
            crate::window::camera_window::camera_window_attributes().with_visible(false);
        let cam_window = Arc::new(event_loop.create_window(cam_attrs).map_err(|e| {
            log::error!("ContextManager: failed to create dummy camera window: {e:?}");
            GraphicsWindowContextError::SurfaceCreation
        })?);
        let (camera_context, _) = GraphicsWindowContext::new(
            &cam_window,
            wgpu::PowerPreference::LowPower,
            wgpu::PresentMode::AutoVsync,
            "CameraWindow",
            SurfaceInitProfile::StandardWindow,
        )?;

        let screen_sharing_attrs =
            crate::window::screensharing_window::screensharing_window_attributes()
                .with_visible(false);
        let screen_sharing_window = Arc::new(
            event_loop
                .create_window(screen_sharing_attrs)
                .map_err(|e| {
                    log::error!(
                        "ContextManager: failed to create dummy screensharing window: {e:?}"
                    );
                    GraphicsWindowContextError::SurfaceCreation
                })?,
        );
        let (screensharing_context, _) = GraphicsWindowContext::new(
            &screen_sharing_window,
            wgpu::PowerPreference::None,
            wgpu::PresentMode::Immediate,
            "ScreensharingWindow",
            SurfaceInitProfile::StandardWindow,
        )?;

        let overlay_attrs = crate::window_manager::get_window_attributes().with_visible(false);
        let overlay_window = Arc::new(event_loop.create_window(overlay_attrs).map_err(|e| {
            log::error!("ContextManager: failed to create dummy overlay window: {e:?}");
            GraphicsWindowContextError::SurfaceCreation
        })?);
        let (overlay_context, _) = GraphicsWindowContext::new(
            &overlay_window,
            wgpu::PowerPreference::HighPerformance,
            wgpu::PresentMode::AutoVsync,
            "OverlayWindow",
            SurfaceInitProfile::Overlay,
        )?;

        Ok(Self {
            camera_context,
            screensharing_context,
            overlay_context,
        })
    }

    pub fn create_camera_surface(
        &self,
        window: &Arc<Window>,
    ) -> Result<SurfaceInfo, GraphicsWindowContextError> {
        self.camera_context.create_surface(window)
    }

    pub fn create_screensharing_surface(
        &self,
        window: &Arc<Window>,
    ) -> Result<SurfaceInfo, GraphicsWindowContextError> {
        self.screensharing_context.create_surface(window)
    }

    pub fn create_overlay_surface(
        &self,
        window: &Arc<Window>,
    ) -> Result<SurfaceInfo, GraphicsWindowContextError> {
        self.overlay_context.create_surface(window)
    }
}
