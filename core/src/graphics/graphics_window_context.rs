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

pub struct GraphicsWindowContext {
    pub instance: wgpu::Instance,
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub present_mode: wgpu::PresentMode,
}

impl GraphicsWindowContext {
    pub fn new(
        window: &Arc<Window>,
        power_preference: wgpu::PowerPreference,
        present_mode: wgpu::PresentMode,
        label: &str,
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

        let surface_info =
            Self::configure_surface(surface, &adapter, &device, present_mode, window);

        let ctx = Self {
            instance,
            adapter,
            device,
            queue,
            present_mode,
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
        ))
    }

    fn configure_surface(
        surface: wgpu::Surface<'static>,
        adapter: &wgpu::Adapter,
        device: &wgpu::Device,
        present_mode: wgpu::PresentMode,
        window: &Arc<Window>,
    ) -> SurfaceInfo {
        let caps = surface.get_capabilities(adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| !f.is_srgb())
            .unwrap_or(caps.formats[0]);
        let alpha_mode = crate::window::vibrancy::pick_transparent_alpha_mode(&caps);
        let physical_size = window.inner_size();
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
        )?;

        Ok(Self {
            camera_context,
            screensharing_context,
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
}
