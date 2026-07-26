use crate::utils::geometry::{aspect_fit, Extent, Frame};
use livekit::webrtc::{
    prelude::{NV12Buffer, VideoBuffer, VideoFrame, VideoRotation},
    video_source::native::NativeVideoSource,
};
use screencapturekit::{
    cg::CGRect,
    cm::{CMSampleBufferExt, CMSampleBufferSCExt, IOSurfaceLockOptions},
    error::SCStreamErrorCode,
    prelude::*,
    stream::delegate_trait::StreamCallbacks,
};
use socket_lib::{Content, ContentType};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc, Mutex,
};

use super::CapturerError;

#[allow(dead_code)]
pub enum StreamRuntimeMessage {
    Failed,
    FrameChanged,
    Stop,
    StopCapture,
    UserStoppedCapture,
}

struct StreamBuffer {
    video_frame: VideoFrame<NV12Buffer>,
}

impl StreamBuffer {
    pub fn new(width: u32, height: u32) -> Self {
        let video_frame = VideoFrame {
            rotation: VideoRotation::VideoRotation0,
            buffer: NV12Buffer::new(width, height),
            timestamp_us: 0,
            frame_metadata: None,
        };
        StreamBuffer { video_frame }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CropRect {
    x: usize,
    y: usize,
    width: usize,
    height: usize,
}

impl CropRect {
    fn full_frame(width: usize, height: usize) -> Option<Self> {
        let width = width & !1;
        let height = height & !1;
        (width > 0 && height > 0).then_some(Self {
            x: 0,
            y: 0,
            width,
            height,
        })
    }
}

fn nv12_crop_rect(
    rect: CGRect,
    point_pixel_scale: f64,
    surface_width: usize,
    surface_height: usize,
) -> Option<CropRect> {
    let values = [
        rect.origin.x,
        rect.origin.y,
        rect.size.width,
        rect.size.height,
        point_pixel_scale,
    ];
    if values.iter().any(|value| !value.is_finite())
        || rect.size.width <= 0.0
        || rect.size.height <= 0.0
        || point_pixel_scale <= 0.0
    {
        return None;
    }

    let left = (rect.origin.x * point_pixel_scale).max(0.0).ceil() as usize;
    let top = (rect.origin.y * point_pixel_scale).max(0.0).ceil() as usize;
    let right = ((rect.origin.x + rect.size.width) * point_pixel_scale)
        .min(surface_width as f64)
        .floor() as usize;
    let bottom = ((rect.origin.y + rect.size.height) * point_pixel_scale)
        .min(surface_height as f64)
        .floor() as usize;

    // NV12 chroma samples cover 2x2 luma pixels, so crop inward to even bounds.
    let x = (left + 1) & !1;
    let y = (top + 1) & !1;
    let right = right & !1;
    let bottom = bottom & !1;

    (right > x && bottom > y).then_some(CropRect {
        x,
        y,
        width: right - x,
        height: bottom - y,
    })
}

pub struct Stream {
    sc_stream: Option<SCStream>,
    permanent_error_tx: mpsc::Sender<StreamRuntimeMessage>,
    stream_buffer: Arc<Mutex<StreamBuffer>>,
    buffer_source: NativeVideoSource,
    frame: Arc<Mutex<Frame>>,
    stream_resolution: Extent,
    source: Content,
    failures_count: Arc<Mutex<u64>>,
    output_extent: Arc<Mutex<Extent>>,
    scale: f64,
}

impl Stream {
    pub fn new(
        source: Content,
        stream_resolution: Extent,
        scale: f64,
        tx: mpsc::Sender<StreamRuntimeMessage>,
        buffer_source: NativeVideoSource,
    ) -> Result<Self, CapturerError> {
        Ok(Stream {
            sc_stream: None,
            permanent_error_tx: tx,
            stream_buffer: Arc::new(Mutex::new(StreamBuffer::new(1, 1))),
            buffer_source,
            frame: Arc::new(Mutex::new(Frame::default())),
            stream_resolution,
            source,
            failures_count: Arc::new(Mutex::new(0)),
            output_extent: Arc::new(Mutex::new(Extent {
                width: 0.,
                height: 0.,
            })),
            scale,
        })
    }

    pub fn start_capture(&mut self) -> Result<(), CapturerError> {
        let content = self.source;
        log::info!("macos_stream::start_capture: Starting capture for {content}");

        let shareable_content = SCShareableContent::get().map_err(|e| {
            log::error!("start_capture: failed to get shareable content: {e}");
            CapturerError::DesktopCapturerCreationError
        })?;

        let (stream_width, stream_height, filter, crop_window) = match content.content_type {
            ContentType::Display => {
                let displays = shareable_content.displays();
                if displays.is_empty() {
                    return Err(CapturerError::CaptureSourceListEmpty);
                }
                let display = displays
                    .into_iter()
                    .find(|display| display.display_id() == content.id)
                    .ok_or(CapturerError::SelectedSourceNotFound)?;
                let native_width = (display.width() as f64 * self.scale) as u32;
                let native_height = (display.height() as f64 * self.scale) as u32;
                let (width, height) = aspect_fit(
                    native_width,
                    native_height,
                    self.stream_resolution.width as u32,
                    self.stream_resolution.height as u32,
                );
                let filter = SCContentFilter::create()
                    .with_display(&display)
                    .with_excluding_windows(&[])
                    .build();
                (width, height, filter, false)
            }
            ContentType::Window => {
                let window = shareable_content
                    .windows()
                    .into_iter()
                    .find(|window| window.window_id() == content.id)
                    .ok_or(CapturerError::SelectedSourceNotFound)?;
                let frame = window.frame();
                *self.frame.lock().unwrap() = Frame {
                    origin_x: frame.origin.x,
                    origin_y: frame.origin.y,
                    extent: Extent {
                        width: frame.size.width,
                        height: frame.size.height,
                    },
                };
                let filter = SCContentFilter::create().with_window(&window).build();
                let backing_scale = f64::from(filter.point_pixel_scale());
                let native_width = (frame.size.width * backing_scale) as u32;
                let native_height = (frame.size.height * backing_scale) as u32;
                let (width, height) = aspect_fit(
                    native_width,
                    native_height,
                    self.stream_resolution.width as u32,
                    self.stream_resolution.height as u32,
                );
                (width, height, filter, true)
            }
        };
        if stream_width < 2 || stream_height < 2 {
            return Err(CapturerError::InvalidStreamDimensions);
        }
        log::info!(
            "start_capture: configured output {stream_width}x{stream_height}, crop_window: {crop_window}"
        );

        {
            let mut extent = self.output_extent.lock().unwrap();
            extent.width = stream_width as f64;
            extent.height = stream_height as f64;
        }
        {
            let mut sb = self.stream_buffer.lock().unwrap();
            *sb = StreamBuffer::new(stream_width, stream_height);
        }

        let config = SCStreamConfiguration::new()
            .with_width(stream_width)
            .with_height(stream_height)
            .with_pixel_format(PixelFormat::YCbCr_420v)
            .with_shows_cursor(false)
            .with_fps(60);

        let error_tx = self.permanent_error_tx.clone();
        let stop_tx = self.permanent_error_tx.clone();
        let error_failures_count = self.failures_count.clone();
        let stream_failed = Arc::new(AtomicBool::new(false));
        let stop_stream_failed = stream_failed.clone();
        let delegate = StreamCallbacks::new()
            .on_error(move |error| {
                if error.stream_error_code() == Some(SCStreamErrorCode::UserStopped) {
                    return;
                }

                stream_failed.store(true, Ordering::Release);
                log::error!("SCStream error: {error}");
                *error_failures_count.lock().unwrap() += 1;
                let _ = error_tx.send(StreamRuntimeMessage::Failed);
            })
            .on_stop(move |error| {
                if !stop_stream_failed.load(Ordering::Acquire) {
                    if let Some(msg) = error {
                        log::info!("SCStream stopped by user: {msg}");
                        let _ = stop_tx.send(StreamRuntimeMessage::UserStoppedCapture);
                    }
                }
            });

        let stream_buffer = self.stream_buffer.clone();
        let buffer_source = self.buffer_source.clone();
        let failures_count = self.failures_count.clone();
        let frame_arc = self.frame.clone();
        let frame_changed_tx = self.permanent_error_tx.clone();
        let output_extent = self.output_extent.clone();
        let capture_start = std::time::Instant::now();

        let handler = move |sample: CMSampleBuffer, of_type: SCStreamOutputType| {
            if !matches!(of_type, SCStreamOutputType::Screen) {
                return;
            }
            let frame_status = sample.frame_status();
            if frame_status.is_some_and(|status| !status.has_content()) {
                return;
            }

            {
                *failures_count.lock().unwrap() = 0;
            }

            let pixel_buffer = match sample.image_buffer() {
                Some(pb) => pb,
                None => return,
            };

            let io_surface = match pixel_buffer.io_surface() {
                Some(s) => s,
                None => {
                    log::warn!("start_capture handler: frame not IOSurface-backed");
                    return;
                }
            };

            let guard = match io_surface.lock(IOSurfaceLockOptions::READ_ONLY) {
                Ok(g) => g,
                Err(e) => {
                    log::warn!("start_capture handler: IOSurface lock failed: {e}");
                    return;
                }
            };

            let src_y = match guard.plane_data(0) {
                Some(d) => d,
                None => return,
            };
            let src_uv = match guard.plane_data(1) {
                Some(d) => d,
                None => return,
            };

            let src_stride_y = io_surface.bytes_per_row_of_plane(0);
            let src_stride_uv = io_surface.bytes_per_row_of_plane(1);
            let frame_width = io_surface.width_of_plane(0);
            let frame_height = io_surface.height_of_plane(0);

            if frame_width == 0 || frame_height == 0 {
                return;
            }

            let (content_rect, scale_factor) = if let Some(info) = sample.frame_info() {
                let scale_factor = info.scale_factor.unwrap_or(1.0);
                if crop_window {
                    if let Some(screen_rect) = info.screen_rect {
                        let mut frame = frame_arc.lock().unwrap();
                        let next_frame = Frame {
                            origin_x: screen_rect.origin.x,
                            origin_y: screen_rect.origin.y,
                            extent: Extent {
                                width: screen_rect.size.width,
                                height: screen_rect.size.height,
                            },
                        };
                        if frame.origin_x != next_frame.origin_x
                            || frame.origin_y != next_frame.origin_y
                            || frame.extent.width != next_frame.extent.width
                            || frame.extent.height != next_frame.extent.height
                        {
                            *frame = next_frame;
                            let _ = frame_changed_tx.send(StreamRuntimeMessage::FrameChanged);
                        }
                    }
                }
                (info.content_rect, scale_factor)
            } else {
                (None, 1.0)
            };

            let crop = if crop_window {
                match content_rect {
                    Some(rect) => {
                        match nv12_crop_rect(rect, scale_factor, frame_width, frame_height) {
                            Some(crop) => crop,
                            None => return,
                        }
                    }
                    None => match CropRect::full_frame(frame_width, frame_height) {
                        Some(crop) => crop,
                        None => return,
                    },
                }
            } else {
                match CropRect::full_frame(frame_width, frame_height) {
                    Some(crop) => crop,
                    None => return,
                }
            };

            let mut sb = stream_buffer.lock().unwrap();
            let output_resized = sb.video_frame.buffer.width() != crop.width as u32
                || sb.video_frame.buffer.height() != crop.height as u32;
            if output_resized {
                *sb = StreamBuffer::new(crop.width as u32, crop.height as u32);
                let mut extent = output_extent.lock().unwrap();
                extent.width = crop.width as f64;
                extent.height = crop.height as f64;
            }

            let (dst_stride_y, dst_stride_uv) = sb.video_frame.buffer.strides();
            let (dst_y, dst_uv) = sb.video_frame.buffer.data_mut();

            for row in 0..crop.height {
                let src_off = (crop.y + row) * src_stride_y + crop.x;
                let dst_off = row * dst_stride_y as usize;
                dst_y[dst_off..dst_off + crop.width]
                    .copy_from_slice(&src_y[src_off..src_off + crop.width]);
            }

            for row in 0..crop.height / 2 {
                let src_off = (crop.y / 2 + row) * src_stride_uv + crop.x;
                let dst_off = row * dst_stride_uv as usize;
                dst_uv[dst_off..dst_off + crop.width]
                    .copy_from_slice(&src_uv[src_off..src_off + crop.width]);
            }

            sb.video_frame.timestamp_us = capture_start.elapsed().as_micros() as i64;
            buffer_source.capture_frame(&sb.video_frame);
        };

        let mut sc_stream = SCStream::new_with_delegate(&filter, &config, delegate);
        sc_stream.add_output_handler(handler, SCStreamOutputType::Screen);

        sc_stream.start_capture().map_err(|e| {
            log::error!("start_capture: SCK start_capture failed: {e}");
            CapturerError::DesktopCapturerCreationError
        })?;

        self.sc_stream = Some(sc_stream);
        Ok(())
    }

    pub fn stop_capture(&mut self) {
        if let Some(ref stream) = self.sc_stream {
            if let Err(e) = stream.stop_capture() {
                log::warn!("stop_capture: SCK stop error: {e}");
            }
        }
        self.sc_stream = None;
    }

    pub fn copy(mut self) -> Result<Self, ()> {
        if self.sc_stream.is_some() {
            log::warn!("Stream::copy: Stream is running, stopping it");
            self.stop_capture();
        }

        Ok(Stream {
            sc_stream: None,
            permanent_error_tx: self.permanent_error_tx.clone(),
            stream_buffer: self.stream_buffer.clone(),
            buffer_source: self.buffer_source.clone(),
            frame: self.frame.clone(),
            stream_resolution: self.stream_resolution,
            source: self.source,
            failures_count: self.failures_count.clone(),
            output_extent: self.output_extent.clone(),
            scale: self.scale,
        })
    }

    pub fn get_failures_count(&self) -> u64 {
        *self.failures_count.lock().unwrap()
    }

    pub fn get_stream_extent(&self) -> Extent {
        *self.output_extent.lock().unwrap()
    }

    pub fn frame(&self) -> Option<Arc<Mutex<Frame>>> {
        matches!(self.source.content_type, ContentType::Window).then(|| self.frame.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crops_odd_source_dimensions_to_even_nv12_output() {
        let crop = nv12_crop_rect(CGRect::new(1.0, 1.0, 6.0, 4.0), 1.0, 8, 6).unwrap();
        assert_eq!(
            crop,
            CropRect {
                x: 2,
                y: 2,
                width: 4,
                height: 2,
            }
        );
        assert_eq!(
            CropRect::full_frame(5, 3),
            Some(CropRect {
                x: 0,
                y: 0,
                width: 4,
                height: 2,
            })
        );

        let buffer = NV12Buffer::new(crop.width as u32, crop.height as u32);
        assert_eq!(buffer.strides(), (4, 4));
        assert_eq!(buffer.data().1.len(), 4);
    }
}
