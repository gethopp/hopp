use accessibility_sys::AXIsProcessTrusted;
use av_foundation::{
    capture_device::{AVAuthorizationStatusAuthorized, AVCaptureDevice},
    media_format::{AVMediaTypeAudio, AVMediaTypeVideo},
};
use core_graphics::access::ScreenCaptureAccess;

use super::PermissionsTrait;

pub struct PlatformPermissions;

impl PermissionsTrait for PlatformPermissions {
    fn screenshare() -> bool {
        log::info!("macOS screenshare permission check");
        let access = ScreenCaptureAccess;
        access.preflight()
    }

    fn accessibility() -> bool {
        log::info!("macOS accessibility permission check");

        unsafe { AXIsProcessTrusted() }
    }

    fn microphone() -> bool {
        log::info!("macOS microphone permission check");
        unsafe {
            let media_type = AVMediaTypeAudio;
            AVCaptureDevice::authorization_status_for_media_type(media_type)
                == AVAuthorizationStatusAuthorized
        }
    }

    fn camera() -> bool {
        log::info!("macOS camera permission check");
        unsafe {
            let media_type = AVMediaTypeVideo;
            AVCaptureDevice::authorization_status_for_media_type(media_type)
                == AVAuthorizationStatusAuthorized
        }
    }

    fn request_camera() {
        log::info!("macOS camera permission request");
        unsafe {
            AVCaptureDevice::request_access_for_media_type(AVMediaTypeVideo, |granted| {
                log::info!("macOS camera permission request granted: {granted:?}");
            })
        }
    }

    fn request_microphone() {
        log::info!("macOS microphone permission request");
        unsafe {
            AVCaptureDevice::request_access_for_media_type(AVMediaTypeAudio, |granted| {
                log::info!("macOS microphone permission request granted: {granted:?}");
            })
        }
    }
}
