use super::CameraStreamMessage;
use memmap2::{MmapOptions, MmapRaw};
use nokhwa::utils::{CameraFormat, CameraIndex, FrameFormat};
use nokhwa::Camera;
use std::io;
use std::os::fd::RawFd;
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};
use v4l::device::Handle;
use v4l::v4l2::{self, vidioc};
use v4l::v4l_sys::{v4l2_buffer, v4l2_requestbuffers};
use v4l::video::Capture;

const READ_TIMEOUT: Duration = Duration::from_millis(250);
const STALL_TIMEOUT: Duration = Duration::from_secs(5);

pub(super) struct BoundedCamera {
    queue: MappedQueue,
    format: CameraFormat,
}

impl BoundedCamera {
    pub(super) fn new(camera: Camera) -> Result<Self, String> {
        let index = camera.index().clone();
        let format = camera.camera_format();
        // Release Nokhwa's device before opening our own handle. Formats belong
        // to a V4L handle, so reapply and verify its negotiated mode below.
        drop(camera);
        let device = match index {
            CameraIndex::Index(index) => v4l::Device::new(index as usize),
            CameraIndex::String(path) => v4l::Device::with_path(path),
        }
        .map_err(|error| format!("Failed to open V4L camera: {error}"))?;
        let fourcc = match format.format() {
            FrameFormat::YUYV => b"YUYV",
            FrameFormat::MJPEG => b"MJPG",
            FrameFormat::NV12 => b"NV12",
            FrameFormat::GRAY => b"GRAY",
            FrameFormat::RAWRGB => b"RGB3",
            FrameFormat::RAWBGR => b"BGR3",
            FrameFormat::BGRA => return Err("Unsupported V4L frame format: BGRA".to_string()),
        };
        let requested = v4l::Format::new(format.width(), format.height(), v4l::FourCC::new(fourcc));
        let actual = device
            .set_format(&requested)
            .map_err(|error| format!("Failed to set V4L format: {error}"))?;
        if actual.width != format.width()
            || actual.height != format.height()
            || actual.fourcc != requested.fourcc
        {
            return Err(format!("V4L camera changed negotiated format: {actual}"));
        }
        device
            .set_params(&v4l::video::capture::Parameters::with_fps(
                format.frame_rate(),
            ))
            .map_err(|error| format!("Failed to set V4L frame rate: {error}"))?;
        let queue = MappedQueue::new(&device)
            .map_err(|error| format!("Failed to open V4L stream: {error}"))?;
        Ok(Self { queue, format })
    }

    pub(super) fn frame(
        &mut self,
        stop_rx: &mpsc::Receiver<CameraStreamMessage>,
    ) -> Result<Option<nokhwa::Buffer>, String> {
        read_frame(&mut self.queue, stop_rx, Instant::now()).map(|frame| {
            frame.map(|bytes| {
                nokhwa::Buffer::new(self.format.resolution(), &bytes, self.format.format())
            })
        })
    }

    pub(super) fn frame_format(&self) -> FrameFormat {
        self.format.format()
    }

    pub(super) fn stop_stream(&mut self) -> io::Result<()> {
        self.queue.stop()
    }
}

// Keep queue operations separate from the retry loop so tests can exercise real
// bounded poll waits and delayed frames without opening a camera.
trait FrameQueue {
    fn fd(&self) -> RawFd;
    fn dequeue_frame(&mut self) -> io::Result<Vec<u8>>;
}

struct MappedQueue {
    // Drop mappings before the last device handle. Drop::drop stops DMA first.
    buffers: Vec<MmapRaw>,
    handle: Arc<Handle>,
    streaming: bool,
}

impl MappedQueue {
    fn new(device: &v4l::Device) -> io::Result<Self> {
        let mut queue = Self {
            buffers: Vec::new(),
            handle: device.handle(),
            streaming: false,
        };
        let mut request = v4l2_requestbuffers {
            count: 4,
            type_: v4l::buffer::Type::VideoCapture as u32,
            memory: v4l::memory::Memory::Mmap as u32,
            // SAFETY: this C structure contains only integers/reserved bytes;
            // V4L requires all unused fields to be initialized to zero.
            ..unsafe { std::mem::zeroed() }
        };
        // SAFETY: request is a live, correctly typed VIDIOC_REQBUFS argument.
        unsafe {
            v4l2::ioctl(
                queue.fd(),
                vidioc::VIDIOC_REQBUFS,
                &mut request as *mut _ as *mut _,
            )?;
        }
        if request.count == 0 {
            return Err(io::Error::other("V4L allocated no camera buffers"));
        }
        for index in 0..request.count {
            let buffer = queue.buffer_ioctl(vidioc::VIDIOC_QUERYBUF, index)?;
            // Raw mappings permit the driver to modify queued buffers without
            // keeping Rust slice references alive across those modifications.
            // SAFETY: QUERYBUF returned the MMAP offset union member for this
            // single-planar MMAP buffer.
            let offset = unsafe { buffer.m.offset };
            let mapping = MmapOptions::new()
                .offset(offset as u64)
                .len(buffer.length as usize)
                // V4L requires shared read/write mappings even for capture.
                .map_raw(queue.fd())?;
            queue.buffers.push(mapping);
            queue.buffer_ioctl(vidioc::VIDIOC_QBUF, index)?;
        }
        queue.set_streaming(true)?;
        Ok(queue)
    }

    fn buffer_ioctl(&self, request: vidioc::_IOC_TYPE, index: u32) -> io::Result<v4l2_buffer> {
        let mut buffer = v4l2_buffer {
            type_: v4l::buffer::Type::VideoCapture as u32,
            memory: v4l::memory::Memory::Mmap as u32,
            index,
            // SAFETY: integer fields and the pointer/integer union permit a
            // zero value; V4L requires reserved fields to be zero.
            ..unsafe { std::mem::zeroed() }
        };
        // SAFETY: all callers use single-planar buffer ioctls accepting a live
        // v4l2_buffer. The nonblocking descriptor is retained by self.handle.
        unsafe {
            v4l2::ioctl(self.fd(), request, &mut buffer as *mut _ as *mut _)?;
        }
        Ok(buffer)
    }

    fn set_streaming(&mut self, streaming: bool) -> io::Result<()> {
        let mut buffer_type = v4l::buffer::Type::VideoCapture as u32;
        let request = if streaming {
            vidioc::VIDIOC_STREAMON
        } else {
            vidioc::VIDIOC_STREAMOFF
        };
        // SAFETY: both stream ioctls accept a pointer to the capture type.
        unsafe {
            v4l2::ioctl(self.fd(), request, &mut buffer_type as *mut _ as *mut _)?;
        }
        self.streaming = streaming;
        Ok(())
    }

    fn stop(&mut self) -> io::Result<()> {
        if self.streaming {
            self.set_streaming(false)?;
        }
        Ok(())
    }
}

impl FrameQueue for MappedQueue {
    fn fd(&self) -> RawFd {
        self.handle.fd()
    }

    fn dequeue_frame(&mut self) -> io::Result<Vec<u8>> {
        let buffer = self.buffer_ioctl(vidioc::VIDIOC_DQBUF, 0)?;
        let length = checked_frame_length(
            buffer.bytesused as usize,
            self.buffers.get(buffer.index as usize).map(MmapRaw::len),
        )?;
        let mapping = &self.buffers[buffer.index as usize];
        // SAFETY: successful DQBUF transfers this buffer from the driver to us.
        // The validated range lies within its live mapping. No reference escapes
        // this copy, and the buffer is only returned to the driver afterward.
        let bytes = unsafe { std::slice::from_raw_parts(mapping.as_ptr(), length) }.to_vec();
        self.buffer_ioctl(vidioc::VIDIOC_QBUF, buffer.index)?;
        Ok(bytes)
    }
}

impl Drop for MappedQueue {
    fn drop(&mut self) {
        if let Err(error) = self.stop() {
            log::warn!("Failed to stop V4L camera stream: {error}");
        }
    }
}

fn checked_frame_length(bytesused: usize, mapped_length: Option<usize>) -> io::Result<usize> {
    let mapped_length = mapped_length
        .ok_or_else(|| io::Error::other("V4L returned an invalid camera buffer index"))?;
    if bytesused == 0 || bytesused > mapped_length {
        return Err(io::Error::other(
            "V4L returned an invalid camera frame length",
        ));
    }
    Ok(bytesused)
}

fn poll_frame(fd: RawFd) -> io::Result<bool> {
    let mut descriptor = libc::pollfd {
        fd,
        events: libc::POLLIN,
        revents: 0,
    };
    // SAFETY: descriptor is a valid one-element pollfd array for this call.
    match unsafe { libc::poll(&mut descriptor, 1, READ_TIMEOUT.as_millis() as i32) } {
        -1 => Err(io::Error::last_os_error()),
        0 => Ok(false),
        _ => Ok(true),
    }
}

fn read_frame(
    queue: &mut impl FrameQueue,
    stop_rx: &mpsc::Receiver<CameraStreamMessage>,
    started: Instant,
) -> Result<Option<Vec<u8>>, String> {
    loop {
        if matches!(
            stop_rx.try_recv(),
            Ok(CameraStreamMessage::StopCapture) | Err(mpsc::TryRecvError::Disconnected)
        ) {
            return Ok(None);
        }
        if started.elapsed() >= STALL_TIMEOUT {
            return Err("Camera produced no frames for 5 seconds".to_string());
        }
        let result = poll_frame(queue.fd()).and_then(|ready| {
            if ready {
                queue.dequeue_frame().map(Some)
            } else {
                Ok(None)
            }
        });
        match result {
            Ok(Some(frame)) => return Ok(Some(frame)),
            Ok(None) => {}
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                ) => {}
            Err(error) => return Err(error.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::os::fd::AsRawFd;
    use std::os::unix::net::UnixStream;

    struct PipeQueue {
        reader: UnixStream,
        polling: mpsc::Sender<()>,
    }

    impl FrameQueue for PipeQueue {
        fn fd(&self) -> RawFd {
            let _ = self.polling.send(());
            self.reader.as_raw_fd()
        }

        fn dequeue_frame(&mut self) -> io::Result<Vec<u8>> {
            let mut bytes = [0; 4];
            self.reader.read_exact(&mut bytes)?;
            Ok(bytes.to_vec())
        }
    }

    #[test]
    fn stalled_read_can_stop_and_release_device_before_join() {
        let (reader, mut silent_camera) = UnixStream::pair().unwrap();
        silent_camera
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let (stop_tx, stop_rx) = mpsc::channel();
        let (done_tx, done_rx) = mpsc::channel();
        let (polling_tx, polling_rx) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            let mut queue = PipeQueue {
                reader,
                polling: polling_tx,
            };
            let result = read_frame(&mut queue, &stop_rx, Instant::now());
            drop(queue);
            done_tx.send(result).unwrap();
        });
        polling_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        stop_tx.send(CameraStreamMessage::StopCapture).unwrap();
        assert!(done_rx
            .recv_timeout(Duration::from_secs(2))
            .unwrap()
            .unwrap()
            .is_none());
        worker.join().unwrap();
        assert_eq!(silent_camera.read(&mut [0]).unwrap(), 0);
    }

    #[test]
    fn slow_frame_after_multiple_poll_timeouts_is_returned() {
        let (reader, mut camera) = UnixStream::pair().unwrap();
        let (_stop_tx, stop_rx) = mpsc::channel();
        let (done_tx, done_rx) = mpsc::channel();
        let (polling_tx, polling_rx) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            let mut queue = PipeQueue {
                reader,
                polling: polling_tx,
            };
            let result = read_frame(&mut queue, &stop_rx, Instant::now());
            done_tx.send(result).unwrap();
        });
        // Wait until two real poll calls time out before delivering the frame.
        for _ in 0..3 {
            polling_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        }
        camera.write_all(&[1, 2, 3, 4]).unwrap();
        assert_eq!(
            done_rx
                .recv_timeout(Duration::from_secs(2))
                .unwrap()
                .unwrap(),
            Some(vec![1, 2, 3, 4])
        );
        worker.join().unwrap();
    }

    #[test]
    fn stalled_frame_fails_at_deadline() {
        let (reader, _camera) = UnixStream::pair().unwrap();
        let (_stop_tx, stop_rx) = mpsc::channel();
        let (polling, _polling_rx) = mpsc::channel();
        assert!(read_frame(
            &mut PipeQueue { reader, polling },
            &stop_rx,
            Instant::now() - STALL_TIMEOUT
        )
        .is_err());
    }

    #[test]
    fn driver_frame_length_must_fit_mapping() {
        assert_eq!(checked_frame_length(4, Some(8)).unwrap(), 4);
        assert!(checked_frame_length(0, Some(8)).is_err());
        assert!(checked_frame_length(9, Some(8)).is_err());
        assert!(checked_frame_length(4, None).is_err());
    }
}
