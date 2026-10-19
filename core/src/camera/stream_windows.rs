use livekit::webrtc::video_source::native::NativeVideoSource;
use socket_lib::CameraDevice;
use std::sync::{mpsc, Arc};

use super::{CameraStreamConfig, CameraStreamMessage};
use crate::livekit::video::VideoBufferManager;

#[path = "stream_directshow.rs"]
mod directshow;
#[path = "stream_nokhwa.rs"]
mod media_foundation;

enum CameraBackend {
    MediaFoundation(media_foundation::CameraStream),
    DirectShow(directshow::DirectShowStream),
}

pub struct CameraStream {
    backend: CameraBackend,
}

impl CameraStream {
    /// Opens the selected device, preferring DirectShow with same-device MF fallback.
    pub fn new(
        device_name: &str,
        error_tx: mpsc::Sender<CameraStreamMessage>,
        video_buffer_manager: Arc<VideoBufferManager>,
        buffer_source: NativeVideoSource,
        config: Arc<CameraStreamConfig>,
    ) -> Result<Self, String> {
        let devices = list_devices();
        let device = select_device(&devices, device_name)
            .ok_or_else(|| "No cameras available".to_string())?;

        let backend = if device.id.starts_with("directshow:") {
            match directshow::DirectShowStream::new(
                &device.name,
                &device.id,
                error_tx.clone(),
                video_buffer_manager.clone(),
                buffer_source.clone(),
                config.clone(),
            ) {
                Ok(stream) => CameraBackend::DirectShow(stream),
                Err(error) => {
                    if !media_foundation::list_devices()
                        .iter()
                        .any(|native| native.name == device.name)
                    {
                        return Err(error);
                    }
                    log::warn!(
                        "DirectShow camera '{}' failed to start: {error}; trying Media Foundation",
                        device.name
                    );
                    CameraBackend::MediaFoundation(
                        media_foundation::CameraStream::new(
                            &device.name,
                            error_tx,
                            video_buffer_manager,
                            buffer_source,
                            config,
                        )
                        .map_err(|fallback| {
                            format!("DirectShow: {error}; Media Foundation: {fallback}")
                        })?,
                    )
                }
            }
        } else {
            CameraBackend::MediaFoundation(media_foundation::CameraStream::new(
                &device.name,
                error_tx,
                video_buffer_manager,
                buffer_source,
                config,
            )?)
        };
        Ok(Self { backend })
    }

    /// Releases the device through the backend that successfully opened it.
    pub fn stop_capture(&mut self) {
        match &mut self.backend {
            CameraBackend::MediaFoundation(stream) => stream.stop_capture(),
            CameraBackend::DirectShow(stream) => stream.stop_capture(),
        }
    }

    /// Returns the active backend's failure count for capture retry decisions.
    pub fn get_failures_count(&self) -> u32 {
        match &self.backend {
            CameraBackend::MediaFoundation(stream) => stream.get_failures_count(),
            CameraBackend::DirectShow(stream) => stream.get_failures_count(),
        }
    }

    /// Returns the opened device's name rather than a stale saved preference.
    pub fn device_name(&self) -> &str {
        match &self.backend {
            CameraBackend::MediaFoundation(stream) => stream.device_name(),
            CameraBackend::DirectShow(stream) => stream.device_name(),
        }
    }

    /// Reopens the same backend and device using the latest shared configuration.
    pub fn copy(&self) -> Result<Self, String> {
        let backend = match &self.backend {
            CameraBackend::MediaFoundation(stream) => {
                CameraBackend::MediaFoundation(stream.copy()?)
            }
            CameraBackend::DirectShow(stream) => CameraBackend::DirectShow(stream.copy()?),
        };
        Ok(Self { backend })
    }
}

/// Uses the saved name when available, otherwise the first enumerated camera.
fn select_device<'a>(devices: &'a [CameraDevice], device_name: &str) -> Option<&'a CameraDevice> {
    if device_name.is_empty() {
        return devices.first();
    }
    devices
        .iter()
        .find(|device| device.name == device_name)
        .or_else(|| {
            log::warn!("Camera '{device_name}' not found, falling back to default");
            devices.first()
        })
}

/// Prefers DirectShow for duplicate names while retaining Media Foundation-only
/// devices, sorting the result, and identifying exactly one default camera.
fn merge_devices(
    mut native: Vec<CameraDevice>,
    directshow: Vec<CameraDevice>,
) -> Vec<CameraDevice> {
    for device in directshow {
        if let Some(existing) = native
            .iter_mut()
            .find(|existing| existing.name == device.name)
        {
            *existing = device;
        } else {
            native.push(device);
        }
    }
    native.sort_by(|left, right| left.name.cmp(&right.name));
    for (index, device) in native.iter_mut().enumerate() {
        device.default = index == 0;
    }
    native
}

/// Combines physical and virtual cameras exposed by either Windows backend.
pub fn list_devices() -> Vec<CameraDevice> {
    let directshow = directshow::list_devices().unwrap_or_else(|error| {
        log::warn!("Failed to query DirectShow cameras: {error}");
        Vec::new()
    });
    let devices = merge_devices(media_foundation::list_devices(), directshow);
    log::info!(
        "Windows cameras: {}",
        devices
            .iter()
            .map(|device| device.name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    );
    devices
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds an enumerated-device fixture with a backend-specific identifier.
    fn device(name: &str, id: &str) -> CameraDevice {
        CameraDevice {
            name: name.to_string(),
            id: id.to_string(),
            default: true,
        }
    }

    /// Unplugging a saved camera must preserve startup through another device.
    #[test]
    fn missing_saved_camera_falls_back_to_first_available_device() {
        let cameras = vec![
            device("Integrated Camera", "directshow:integrated"),
            device("OBS Virtual Camera", "directshow:obs"),
        ];
        let fallback = select_device(&cameras, "Unplugged USB Camera").unwrap();
        assert_eq!(fallback.id, "directshow:integrated");
        assert_eq!(select_device(&cameras, "").unwrap().id, fallback.id);
        assert_eq!(
            select_device(&cameras, "OBS Virtual Camera").unwrap().id,
            "directshow:obs"
        );
        assert!(select_device(&[], "Unplugged USB Camera").is_none());
    }

    /// Duplicate physical-camera names prefer DirectShow without hiding virtuals.
    #[test]
    fn physical_and_virtual_cameras_use_native_directshow_capture() {
        let cameras = merge_devices(
            vec![device("Insta360 Link 2", "0")],
            vec![
                device("OBS Virtual Camera", "directshow:obs"),
                device("Insta360 Link 2", "directshow:insta360"),
                device("Camera (NVIDIA Broadcast)", "directshow:nvidia"),
            ],
        );
        assert_eq!(cameras.len(), 3);
        assert_eq!(cameras[0].name, "Camera (NVIDIA Broadcast)");
        assert_eq!(cameras[1].id, "directshow:insta360");
        assert_eq!(cameras[2].id, "directshow:obs");
        assert_eq!(cameras.iter().filter(|camera| camera.default).count(), 1);
    }

    /// Devices exposed only by Media Foundation remain selectable after merging.
    #[test]
    fn media_foundation_only_devices_are_retained() {
        let cameras = merge_devices(
            vec![device("DroidCam Source 3", "1")],
            vec![device("OBS Virtual Camera", "directshow:obs")],
        );
        assert_eq!(cameras.len(), 2);
        assert_eq!(cameras[0].id, "1");
        assert!(cameras[0].default);
        assert!(!cameras[1].default);
    }
}
