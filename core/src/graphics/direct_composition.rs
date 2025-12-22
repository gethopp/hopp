use crate::graphics::graphics_context::{OverlayError, OverlayResult};
use raw_window_handle::RawWindowHandle;
use std::{os::raw::c_void, sync::Arc};
use windows::core::*;
use windows::Win32::{
    Foundation::{HMODULE, HWND},
    Graphics::{Direct2D::*, Direct3D::*, Direct3D11::*, DirectComposition::*, Dxgi::*},
};
use winit::{raw_window_handle::HasWindowHandle, window::Window};

/*
 * For windows in order to create a transparent window we need to use
 * DirectComposition. For now we are conditionally compiling this code
 * as a temporary solution. In the future we will need to design it better.
 */
#[derive(Debug)]
pub struct DirectComposition {
    pub target: IDCompositionTarget,
    pub desktop: IDCompositionDesktopDevice,
}

impl DirectComposition {
    /*
     * Add direct composition to the window and update the
     * window style to remove the redirection bitmap in order
     * to remove the default white background.
     */
    pub fn new(window: Arc<Window>) -> Option<Self> {
        let window_handle = window.window_handle();
        let raw_handle = match window_handle {
            Ok(handle) => match handle.as_raw() {
                RawWindowHandle::Win32(handle) => handle,
                _ => {
                    log::error!("Failed to get raw win32 window handle");
                    return None;
                }
            },
            _ => {
                log::error!("Failed to get raw window handle");
                return None;
            }
        };

        let (target, desktop) = unsafe {
            /* let hwnd = HWND(raw_handle.hwnd.get() as *mut c_void);
            let win_style = GetWindowLongW(hwnd.clone(), GWL_EXSTYLE);
            SetWindowLongW(
                hwnd,
                GWL_EXSTYLE,
                win_style | (WS_EX_NOREDIRECTIONBITMAP.0 as i32),
            ); */

            let mut device = None;
            let _ = D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_HARDWARE,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                None,
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                None,
            );
            if device.is_none() {
                log::error!("Failed to create D3D11 device");
                return None;
            }
            let device = device.unwrap();

            let dxgi: Result<IDXGIDevice3> = device.cast();
            if let Err(e) = dxgi {
                log::error!("Failed to cast D3D11 device to IDXGIDevice3: {e:?}");
                return None;
            }
            let dxgi = dxgi.unwrap();

            let device_2d = D2D1CreateDevice(&dxgi, None);
            if let Err(e) = device_2d {
                log::error!("Failed to create D2D1 device: {e:?}");
                return None;
            }
            let device_2d = device_2d.unwrap();

            let desktop: Result<IDCompositionDesktopDevice> = DCompositionCreateDevice2(&device_2d);
            if let Err(e) = desktop {
                log::error!("Failed to create DComposition device: {e:?}");
                return None;
            }
            let desktop = desktop.unwrap();

            let target =
                desktop.CreateTargetForHwnd(HWND(raw_handle.hwnd.get() as *mut c_void), true);
            if let Err(e) = target {
                log::error!("Failed to create target for hwnd: {e:?}");
                return None;
            }
            let target = target.unwrap();

            (target, desktop)
        };

        Some(Self { target, desktop })
    }

    pub fn create_surface<'a>(
        &self,
        instance: &wgpu::Instance,
    ) -> OverlayResult<wgpu::Surface<'a>> {
        let surface_visual = unsafe {
            let visual = self.desktop.CreateVisual();
            if let Err(e) = visual {
                log::error!("Failed to create visual: {e:?}");
                return Err(OverlayError::SurfaceCreationError);
            }
            let visual = visual.unwrap();

            if let Err(e) = self.target.SetRoot(&visual) {
                log::error!("Failed to set root visual: {e:?}");
                return Err(OverlayError::SurfaceCreationError);
            }

            let surface_visual = self.desktop.CreateVisual();
            if let Err(e) = surface_visual {
                log::error!("Failed to create surface visual: {e:?}");
                return Err(OverlayError::SurfaceCreationError);
            }
            let surface_visual = surface_visual.unwrap();

            if let Err(e) = visual.AddVisual(&surface_visual, true, None) {
                log::error!("Failed to add visual: {e:?}");
                return Err(OverlayError::SurfaceCreationError);
            }
            surface_visual
        };

        unsafe {
            match instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::CompositionVisual(
                surface_visual.as_raw(),
            )) {
                Ok(surface) => Ok(surface),
                Err(e) => {
                    log::error!("Failed to create surface: {e:?}");
                    Err(OverlayError::SurfaceCreationError)
                }
            }
        }
    }

    pub fn commit(&self) -> OverlayResult<()> {
        unsafe {
            self.desktop.Commit().map_err(|e| {
                log::error!("Failed to commit: {e:?}");
                OverlayError::SurfaceCreationError
            })?;
        }
        Ok(())
    }
}

impl Drop for DirectComposition {
    fn drop(&mut self) {
        unsafe {
            let _ = self.target.SetRoot(None);
            let _ = self.desktop.Commit();
        }
    }
}
