//! Cross-platform permissions checking module.
//!
//! This module provides a unified interface for checking system permissions across different
//! operating systems. It uses conditional compilation to load platform-specific implementations
//! while exposing a consistent API.
//!
//! # Platform Support
//!
//! - **macOS**: Uses native system APIs (Core Graphics, Accessibility, AVFoundation)
//! - **Other platforms**: Default implementation that returns `true` for all permissions
// Platform-specific modules
#[cfg(target_os = "macos")]
#[path = "permissions/macos.rs"]
mod platform;

#[cfg(not(target_os = "macos"))]
#[path = "permissions/default.rs"]
mod platform;

pub use platform::PlatformPermissions;

/// Trait defining the permission checking interface for all platforms.
///
/// Platform-specific modules must implement this trait to provide actual
/// permission checking logic for their respective operating systems.
trait PermissionsTrait {
    /// Checks if screen capture/recording permission is granted.
    fn screenshare() -> bool;

    /// Checks if accessibility permission is granted.
    fn accessibility() -> bool;

    /// Checks if microphone access permission is granted.
    fn microphone() -> bool;

    /// Checks if camera access permission is granted.
    fn camera() -> bool;

    /// Requests camera access permission.
    fn request_camera();

    /// Requests microphone access permission.
    fn request_microphone();
}

/// Checks if any of the required permissions are not granted.
///
/// This convenience function checks all three permission types and returns
/// `true` if any are missing. Useful for determining if the app needs to
/// show permission setup UI.
///
/// # Returns
///
/// `true` if one or more permissions are missing, `false` if all are granted.
pub fn has_ungranted_permissions() -> bool {
    !PlatformPermissions::screenshare()
        || !PlatformPermissions::accessibility()
        || !PlatformPermissions::microphone()
        || !PlatformPermissions::camera()
}

/// Checks if screen sharing/recording permission is granted.
///
/// Required for capturing and sharing screen content during calls.
///
/// # Platform Implementation
///
/// - **macOS**: Uses Core Graphics `ScreenCaptureAccess::preflight()`
/// - **Others**: Returns `true` (no restriction)
pub fn screenshare() -> bool {
    PlatformPermissions::screenshare()
}

/// Checks if accessibility permission is granted.
///
/// Required for controlling other applications and receiving input events,
/// which enables remote control functionality during screen sharing.
///
/// # Platform Implementation
///
/// - **macOS**: Uses Accessibility API `AXIsProcessTrusted()`
/// - **Others**: Returns `true` (no restriction)
pub fn accessibility() -> bool {
    PlatformPermissions::accessibility()
}

/// Checks if microphone access permission is granted.
///
/// Required for audio communication during calls.
///
/// # Platform Implementation
///
/// - **macOS**: Uses AVFoundation `AVCaptureDevice::authorization_status_for_media_type()`
/// - **Others**: Returns `true` (no restriction)
pub fn microphone() -> bool {
    PlatformPermissions::microphone()
}

/// Checks if camera access permission is granted.
///
/// Required for video communication during calls.
///
/// # Platform Implementation
///
/// - **macOS**: Uses AVFoundation `AVCaptureDevice::authorization_status_for_media_type()`
/// - **Others**: Returns `true` (no restriction)
pub fn camera() -> bool {
    PlatformPermissions::camera()
}

/// Requests camera access permission.
///
/// # Platform Implementation
///
/// - **macOS**: Uses AVFoundation `AVCaptureDevice::request_access_for_media_type()`
/// - **Others**: Returns `true` (no restriction)
pub fn request_camera() {
    PlatformPermissions::request_camera()
}

/// Requests microphone access permission.
///
/// # Platform Implementation
///
/// - **macOS**: Uses AVFoundation `AVCaptureDevice::request_access_for_media_type()`
/// - **Others**: Returns `true` (no restriction)
pub fn request_microphone() {
    PlatformPermissions::request_microphone()
}
