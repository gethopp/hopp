use super::PermissionsTrait;

pub struct PlatformPermissions;

impl PermissionsTrait for PlatformPermissions {
    fn screenshare() -> bool {
        log::info!("Default screenshare permission check");
        true
    }

    fn accessibility() -> bool {
        log::info!("Default accessibility permission check");
        true
    }

    fn microphone() -> bool {
        log::info!("Default microphone permission check");
        true
    }

    fn camera() -> bool {
        log::info!("Default camera permission check");
        true
    }
}
