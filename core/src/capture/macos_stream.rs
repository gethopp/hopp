use crate::utils::geometry::{aspect_fit, Extent, Frame};
use crate::UserEvent;
use livekit::webrtc::{
    prelude::{NV12Buffer, VideoFrame, VideoRotation},
    video_source::native::NativeVideoSource,
};
use screencapturekit::{
    cm::{CMSampleBufferExt, CMSampleBufferSCExt, IOSurfaceLockOptions},
    prelude::*,
    stream::delegate_trait::StreamCallbacks,
};
use socket_lib::{Content, ContentType};
use std::sync::{mpsc, Arc, Mutex};
use winit::dpi::PhysicalPosition;
use winit::event_loop::EventLoopProxy;

use super::CapturerError;
use crate::capture::window_bounds_macos;

#[allow(dead_code)]
pub enum StreamRuntimeMessage {
    Failed,
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

pub struct Stream {
    sc_stream: Option<SCStream>,
    permanent_error_tx: mpsc::Sender<StreamRuntimeMessage>,
    stream_buffer: Arc<Mutex<StreamBuffer>>,
    buffer_source: NativeVideoSource,
    frame: Arc<Mutex<Frame>>,
    stream_resolution: Extent,
    content: Content,
    failures_count: Arc<Mutex<u64>>,
    output_extent: Arc<Mutex<Extent>>,
    scale: f64,
    display_position: PhysicalPosition<i32>,
    event_loop_proxy: EventLoopProxy<UserEvent>,
}

impl Stream {
    pub fn new(
        stream_resolution: Extent,
        scale: f64,
        display_position: PhysicalPosition<i32>,
        tx: mpsc::Sender<StreamRuntimeMessage>,
        buffer_source: NativeVideoSource,
        content_type: ContentType,
        event_loop_proxy: EventLoopProxy<UserEvent>,
    ) -> Result<Self, CapturerError> {
        Ok(Stream {
            sc_stream: None,
            permanent_error_tx: tx,
            stream_buffer: Arc::new(Mutex::new(StreamBuffer::new(1, 1))),
            buffer_source,
            frame: Arc::new(Mutex::new(Frame::default())),
            stream_resolution,
            content: Content {
                content_type,
                id: 0,
            },
            failures_count: Arc::new(Mutex::new(0)),
            output_extent: Arc::new(Mutex::new(Extent {
                width: 0.,
                height: 0.,
            })),
            scale,
            display_position,
            event_loop_proxy,
        })
    }

    pub fn start_capture(&mut self, content: Content) -> Result<(), CapturerError> {
        log::info!("macos_stream::start_capture: Starting capture for {content}");

        let shareable = SCShareableContent::get().map_err(|e| {
            log::error!("start_capture: Failed to get shareable content: {e}");
            CapturerError::DesktopCapturerCreationError
        })?;

        let (native_width, native_height, filter) = match content.content_type {
            ContentType::Display => {
                let displays = shareable.displays();
                if displays.is_empty() {
                    return Err(CapturerError::CaptureSourceListEmpty);
                }
                let display = displays
                    .into_iter()
                    .find(|d| d.display_id() as u64 == content.id)
                    .ok_or(CapturerError::SelectedSourceNotFound)?;
                let native_width = (display.width() as f64 * self.scale) as u32;
                let native_height = (display.height() as f64 * self.scale) as u32;
                let filter = SCContentFilter::create()
                    .with_display(&display)
                    .with_excluding_windows(&[])
                    .build();
                (native_width, native_height, filter)
            }
            ContentType::Window => {
                let window = shareable
                    .windows()
                    .into_iter()
                    .find(|w| w.window_id() as u64 == content.id)
                    .ok_or(CapturerError::SelectedSourceNotFound)?;
                let frame = window.frame();
                let native_width = (frame.size.width * self.scale).round().max(1.0) as u32;
                let native_height = (frame.size.height * self.scale).round().max(1.0) as u32;
                // Seed overlay bounds from CGWindowList (top-left Quartz space).
                if let Some(seed) = window_bounds_macos::display_local_frame_on_monitor(
                    window.window_id(),
                    self.scale,
                    self.display_position,
                ) {
                    *self.frame.lock().unwrap() = seed;
                }
                let filter = SCContentFilter::create().with_window(&window).build();
                (native_width, native_height, filter)
            }
        };

        let (stream_width, stream_height) = aspect_fit(
            native_width,
            native_height,
            self.stream_resolution.width as u32,
            self.stream_resolution.height as u32,
        );
        log::info!(
            "start_capture: output {stream_width}x{stream_height} from source {native_width}x{native_height} (scale: {})",
            self.scale
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
        let delegate = StreamCallbacks::new()
            .on_error(move |error| {
                log::error!("SCStream error: {error}");
                *error_failures_count.lock().unwrap() += 1;
                let _ = error_tx.send(StreamRuntimeMessage::Failed);
            })
            .on_stop(move |error| {
                if let Some(msg) = error {
                    log::info!("SCStream stopped with error: {msg}");
                    let _ = stop_tx.send(StreamRuntimeMessage::UserStoppedCapture);
                }
            });

        let stream_buffer = self.stream_buffer.clone();
        let buffer_source = self.buffer_source.clone();
        let failures_count = self.failures_count.clone();
        let frame_arc = self.frame.clone();
        let scale = self.scale;
        let event_loop_proxy = self.event_loop_proxy.clone();
        let track_window_id = matches!(content.content_type, ContentType::Window).then_some(content.id);
        let last_window_query = Mutex::new(
            std::time::Instant::now()
                .checked_sub(std::time::Duration::from_secs(1))
                .unwrap_or_else(std::time::Instant::now),
        );
        let display_position = self.display_position;
        let capture_start = std::time::Instant::now();

        let handler = move |sample: CMSampleBuffer, of_type: SCStreamOutputType| {
            if !matches!(of_type, SCStreamOutputType::Screen) {
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

            // Keep overlay markers glued to the shared window as it moves/resizes.
            // Window shares: CGWindowList bounds (top-left Quartz → display-local physical).
            // Display shares: SCK content_rect (top-left display points).
            let next_frame = if let Some(window_id) = track_window_id {
                let should_query = {
                    let mut last = last_window_query.lock().unwrap();
                    if last.elapsed() >= std::time::Duration::from_millis(50) {
                        *last = std::time::Instant::now();
                        true
                    } else {
                        false
                    }
                };
                if should_query {
                    window_bounds_macos::display_local_frame_on_monitor(
                        window_id as u32,
                        scale,
                        display_position,
                    )
                } else {
                    None
                }
            } else {
                sample.content_rect().map(|content_rect| Frame {
                    origin_x: content_rect.origin.x * scale,
                    origin_y: content_rect.origin.y * scale,
                    extent: Extent {
                        width: content_rect.size.width * scale,
                        height: content_rect.size.height * scale,
                    },
                })
            };

            if let Some(next) = next_frame {
                let changed = {
                    let mut frame = frame_arc.lock().unwrap();
                    if *frame != next {
                        *frame = next;
                        true
                    } else {
                        false
                    }
                };
                if changed {
                    let _ = event_loop_proxy.send_event(UserEvent::RequestRedraw);
                }
            }

            let mut sb = stream_buffer.lock().unwrap();
            let (dst_stride_y, dst_stride_uv) = sb.video_frame.buffer.strides();
            let (dst_y, dst_uv) = sb.video_frame.buffer.data_mut();

            // Copy Y plane row-by-row to handle stride mismatch
            let copy_width_y = frame_width.min(dst_stride_y as usize);
            for row in 0..frame_height {
                let src_off = row * src_stride_y;
                let dst_off = row * dst_stride_y as usize;
                dst_y[dst_off..dst_off + copy_width_y]
                    .copy_from_slice(&src_y[src_off..src_off + copy_width_y]);
            }

            // Copy UV plane (half height, interleaved CbCr)
            let uv_height = frame_height / 2;
            let copy_width_uv = (frame_width).min(dst_stride_uv as usize);
            for row in 0..uv_height {
                let src_off = row * src_stride_uv;
                let dst_off = row * dst_stride_uv as usize;
                dst_uv[dst_off..dst_off + copy_width_uv]
                    .copy_from_slice(&src_uv[src_off..src_off + copy_width_uv]);
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
        self.content = content;
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
            content: self.content,
            failures_count: self.failures_count.clone(),
            output_extent: self.output_extent.clone(),
            scale: self.scale,
            display_position: self.display_position,
            event_loop_proxy: self.event_loop_proxy.clone(),
        })
    }

    pub fn get_failures_count(&self) -> u64 {
        *self.failures_count.lock().unwrap()
    }

    pub fn capture_content(&self) -> Content {
        self.content
    }

    pub fn get_stream_extent(&self) -> Extent {
        *self.output_extent.lock().unwrap()
    }

    /// Live capture bounds in physical pixels (updated as the shared window moves).
    pub fn get_capture_frame(&self) -> Frame {
        *self.frame.lock().unwrap()
    }
}
