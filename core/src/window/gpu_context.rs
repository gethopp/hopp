//! Shared wgpu GPU resources (Instance, Adapter, Device, Queue).
//!
//! On Windows the DX12 backend takes a significant amount of time to create an
//! `Instance`, request an `Adapter`, and compile the initial pipeline state.
//! Creating these for *every* window causes noticeable delays.
//!
//! This module provides a single [`GpuContext`] that is lazily initialized once
//! and then cloned into each new window, eliminating repeated DX12 startup
//! overhead.

use std::sync::Arc;

/// Shared wgpu resources that can be cloned across windows.
///
/// `wgpu::Device` and `wgpu::Queue` are internally reference-counted, so
/// cloning them is cheap and all windows share the same GPU connection.
#[derive(Clone)]
pub struct GpuContext {
    pub instance: Arc<wgpu::Instance>,
    pub adapter: Arc<wgpu::Adapter>,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
}

/// Error returned when GPU context creation fails.
#[derive(Debug, thiserror::Error)]
pub enum GpuContextError {
    #[error("No suitable GPU adapter found")]
    AdapterRequest,
    #[error("Failed to request GPU device: {0}")]
    DeviceRequest(wgpu::RequestDeviceError),
}

impl GpuContext {
    /// Create a new shared GPU context.
    ///
    /// `compatible_surface` may be `None` if no surface exists yet; the adapter
    /// will be chosen based on `power_preference` alone.  In practice the first
    /// window should pass its surface for best compatibility.
    pub fn new(
        compatible_surface: Option<&wgpu::Surface<'_>>,
    ) -> Result<Self, GpuContextError> {
        let instance = Self::create_instance();

        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::None,
            compatible_surface,
            force_fallback_adapter: false,
        }))
        .map_err(|e| {
            log::error!("GpuContext: failed to request adapter: {e:?}");
            GpuContextError::AdapterRequest
        })?;

        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                label: Some("SharedGpuContext device"),
                memory_hints: wgpu::MemoryHints::default(),
                trace: wgpu::Trace::default(),
                experimental_features: wgpu::ExperimentalFeatures::disabled(),
            }))
            .map_err(|e| {
                log::error!("GpuContext: failed to request device: {e:?}");
                GpuContextError::DeviceRequest(e)
            })?;

        log::info!(
            "GpuContext: created shared GPU context (adapter: {:?})",
            adapter.get_info().name
        );

        Ok(Self {
            instance: Arc::new(instance),
            adapter: Arc::new(adapter),
            device,
            queue,
        })
    }

    /// Create the wgpu Instance with platform-appropriate backend selection.
    pub fn create_instance() -> wgpu::Instance {
        // Force DX12 on Windows — Intel UHD 630 Vulkan drivers crash in
        // surface.configure() with STATUS_ACCESS_VIOLATION.
        #[cfg(target_os = "windows")]
        let backends = wgpu::Backends::DX12;
        #[cfg(not(target_os = "windows"))]
        let backends = wgpu::Backends::PRIMARY;

        wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends,
            ..Default::default()
        })
    }
}
