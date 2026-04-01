#[derive(Clone, Copy)]
pub enum DeviceKind {
    Input,
    Output,
}

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::DeviceMonitor;

#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
pub use windows::DeviceMonitor;
