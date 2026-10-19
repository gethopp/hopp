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

/// Captures Linux camera frames with bounded, cancellable waits.
pub(super) struct BoundedCamera {
    queue: MappedQueue,
    format: CameraFormat,
}

impl BoundedCamera {
    /// Reopens the negotiated Nokhwa mode as a nonblocking V4L stream.
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

    /// Waits for a valid frame, cancellation, or the camera stall deadline.
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

    /// Returns the pixel format negotiated when the camera was opened.
    pub(super) fn frame_format(&self) -> FrameFormat {
        self.format.format()
    }

    /// Stops driver capture before the camera's mappings are released.
    pub(super) fn stop_stream(&mut self) -> io::Result<()> {
        self.queue.stop()
    }
}

/// Separates queue operations from cancellable waits for hardware-free tests.
trait FrameQueue {
    /// Returns the nonblocking capture descriptor used for readiness polling.
    fn fd(&self) -> RawFd;
    /// Copies and requeues a frame, returning `WouldBlock` for discardable frames.
    fn dequeue_frame(&mut self) -> io::Result<Vec<u8>>;
}

/// Owns driver buffers and the handle that keeps their mappings alive.
struct MappedQueue {
    // Drop mappings before the last device handle. Drop::drop stops DMA first.
    buffers: Vec<MmapRaw>,
    handle: Arc<Handle>,
    streaming: bool,
}

impl MappedQueue {
    /// Allocates, maps, and queues buffers before starting the camera stream.
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

    /// Issues a single-planar buffer ioctl against the retained camera handle.
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

    /// Changes the driver streaming state, recording it only on success.
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

    /// Stops DMA before buffers are unmapped; repeated calls are harmless.
    fn stop(&mut self) -> io::Result<()> {
        if self.streaming {
            self.set_streaming(false)?;
        }
        Ok(())
    }
}

impl FrameQueue for MappedQueue {
    /// Returns the device descriptor retained with the buffer mappings.
    fn fd(&self) -> RawFd {
        self.handle.fd()
    }

    /// Copies a valid frame and returns every known dequeued buffer to V4L.
    fn dequeue_frame(&mut self) -> io::Result<Vec<u8>> {
        let buffer = self.buffer_ioctl(vidioc::VIDIOC_DQBUF, 0)?;
        let mapping = self.buffers.get(buffer.index as usize);
        consume_dequeued_buffer(
            &buffer,
            mapping.map(MmapRaw::len),
            |length| {
                let mapping = mapping.expect("validated camera buffer mapping");
                // SAFETY: successful DQBUF transfers this buffer from the driver
                // to us. The validated range lies within its live mapping. No
                // reference escapes this copy; QBUF follows the copy.
                unsafe { std::slice::from_raw_parts(mapping.as_ptr(), length) }.to_vec()
            },
            |index| self.buffer_ioctl(vidioc::VIDIOC_QBUF, index).map(|_| ()),
        )
    }
}

impl Drop for MappedQueue {
    /// Stops driver writes before Rust drops the mapped buffers.
    fn drop(&mut self) {
        if let Err(error) = self.stop() {
            log::warn!("Failed to stop V4L camera stream: {error}");
        }
    }
}

/// Copies usable payloads and requeues every buffer with a known mapping.
///
/// Corrupt frames are retryable, but invalid indices and failed requeues are
/// fatal: neither case allows safe continued ownership of the driver queue.
fn consume_dequeued_buffer(
    buffer: &v4l2_buffer,
    mapped_length: Option<usize>,
    copy_frame: impl FnOnce(usize) -> Vec<u8>,
    requeue: impl FnOnce(u32) -> io::Result<()>,
) -> io::Result<Vec<u8>> {
    let mapped_length = mapped_length
        .ok_or_else(|| io::Error::other("V4L returned an invalid camera buffer index"))?;
    let frame = if buffer.flags & v4l::v4l_sys::V4L2_BUF_FLAG_ERROR != 0 {
        Err(io::Error::new(
            io::ErrorKind::WouldBlock,
            "V4L returned a camera frame with a driver error",
        ))
    } else {
        checked_frame_length(buffer.bytesused as usize, mapped_length).map(copy_frame)
    };
    // A failed QBUF loses a buffer from the driver queue. Even EAGAIN/EINTR
    // must terminate capture rather than masquerade as a discardable frame.
    requeue(buffer.index).map_err(|error| {
        io::Error::other(format!("Failed to requeue V4L camera buffer: {error}"))
    })?;
    frame
}

/// Rejects empty or out-of-bounds payloads without ending the capture stream.
fn checked_frame_length(bytesused: usize, mapped_length: usize) -> io::Result<usize> {
    if bytesused == 0 || bytesused > mapped_length {
        return Err(io::Error::new(
            io::ErrorKind::WouldBlock,
            "V4L returned an invalid camera frame length",
        ));
    }
    Ok(bytesused)
}

/// Waits for camera readiness for at most one cancellation polling interval.
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

/// Retries transient or corrupt frames while preserving one stall deadline.
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
    use std::collections::VecDeque;
    use std::io::{Read, Write};
    use std::os::fd::AsRawFd;
    use std::os::unix::net::UnixStream;

    /// Supplies real readiness waits without needing a camera driver.
    struct PipeQueue {
        reader: UnixStream,
        polling: mpsc::Sender<()>,
    }

    impl FrameQueue for PipeQueue {
        /// Signals each polling attempt so tests can synchronize cancellation.
        fn fd(&self) -> RawFd {
            let _ = self.polling.send(());
            self.reader.as_raw_fd()
        }

        /// Copies a fixed mock frame delivered through the pipe.
        fn dequeue_frame(&mut self) -> io::Result<Vec<u8>> {
            let mut bytes = [0; 4];
            self.reader.read_exact(&mut bytes)?;
            Ok(bytes.to_vec())
        }
    }

    /// Simulates driver metadata and records ownership transfers and copies.
    struct MockBufferQueue {
        reader: UnixStream,
        frames: VecDeque<v4l2_buffer>,
        requeued: Vec<u32>,
        copied: Vec<u32>,
        requeue_error: Option<io::ErrorKind>,
    }

    impl MockBufferQueue {
        /// Keeps the poll descriptor readable while queued mock frames exist.
        fn new(frames: impl IntoIterator<Item = v4l2_buffer>) -> (Self, UnixStream) {
            let (reader, mut writer) = UnixStream::pair().unwrap();
            writer.write_all(&[1]).unwrap();
            (
                Self {
                    reader,
                    frames: frames.into_iter().collect(),
                    requeued: Vec::new(),
                    copied: Vec::new(),
                    requeue_error: None,
                },
                writer,
            )
        }
    }

    impl FrameQueue for MockBufferQueue {
        /// Exposes a ready descriptor so the production retry loop can run.
        fn fd(&self) -> RawFd {
            self.reader.as_raw_fd()
        }

        /// Exercises production buffer validation with recorded queue operations.
        fn dequeue_frame(&mut self) -> io::Result<Vec<u8>> {
            let buffer = self.frames.pop_front().expect("mock frames exhausted");
            let mapped_length = (buffer.index < 2).then_some(4);
            consume_dequeued_buffer(
                &buffer,
                mapped_length,
                |length| {
                    self.copied.push(buffer.index);
                    vec![buffer.index as u8; length]
                },
                |index| {
                    self.requeued.push(index);
                    if let Some(kind) = self.requeue_error {
                        Err(io::Error::new(kind, "mock QBUF failed"))
                    } else {
                        Ok(())
                    }
                },
            )
        }
    }

    /// Builds driver metadata without touching a camera device.
    fn mock_buffer(index: u32, bytesused: u32, flags: u32) -> v4l2_buffer {
        v4l2_buffer {
            index,
            bytesused,
            flags,
            // SAFETY: unused integer fields and the buffer union accept zero.
            ..unsafe { std::mem::zeroed() }
        }
    }

    /// Empty, oversized, and driver-flagged frames retain every queue buffer.
    #[test]
    fn bad_frames_are_requeued_without_reading_their_payloads() {
        for buffer in [
            mock_buffer(0, 0, 0),
            mock_buffer(1, 5, 0),
            mock_buffer(0, 4, v4l::v4l_sys::V4L2_BUF_FLAG_ERROR),
        ] {
            let index = buffer.index;
            let (mut queue, _writer) = MockBufferQueue::new([buffer]);
            assert_eq!(
                queue.dequeue_frame().unwrap_err().kind(),
                io::ErrorKind::WouldBlock
            );
            assert_eq!(queue.requeued, [index]);
            assert!(queue.copied.is_empty());
        }
    }

    /// Unknown indices terminate capture without returning an unknown buffer.
    #[test]
    fn invalid_buffer_index_is_fatal_and_never_requeued() {
        let (mut queue, _writer) = MockBufferQueue::new([mock_buffer(2, 4, 0)]);
        assert_eq!(
            queue.dequeue_frame().unwrap_err().kind(),
            io::ErrorKind::Other
        );
        assert!(queue.requeued.is_empty());
        assert!(queue.copied.is_empty());
    }

    /// Corrupt frames are skipped until the next valid frame can be published.
    #[test]
    fn read_frame_skips_bad_frames_and_returns_the_next_good_payload() {
        let (mut queue, _writer) = MockBufferQueue::new([
            mock_buffer(0, 0, 0),
            mock_buffer(1, 5, 0),
            mock_buffer(0, 4, v4l::v4l_sys::V4L2_BUF_FLAG_ERROR),
            mock_buffer(1, 4, 0),
        ]);
        let (_stop_tx, stop_rx) = mpsc::channel();
        assert_eq!(
            read_frame(&mut queue, &stop_rx, Instant::now()).unwrap(),
            Some(vec![1; 4])
        );
        assert_eq!(queue.requeued, [0, 1, 0, 1]);
        assert_eq!(queue.copied, [1]);
    }

    /// Failed ownership transfers propagate even when the payload was corrupt.
    #[test]
    fn requeue_failure_is_fatal_for_good_and_bad_frames() {
        for length in [0, 4] {
            for kind in [
                io::ErrorKind::Other,
                io::ErrorKind::WouldBlock,
                io::ErrorKind::Interrupted,
            ] {
                let (mut queue, _writer) = MockBufferQueue::new([mock_buffer(0, length, 0)]);
                queue.requeue_error = Some(kind);
                let (_stop_tx, stop_rx) = mpsc::channel();
                assert_eq!(
                    read_frame(&mut queue, &stop_rx, Instant::now()).unwrap_err(),
                    "Failed to requeue V4L camera buffer: mock QBUF failed"
                );
                assert_eq!(queue.requeued, [0]);
            }
        }
    }

    /// A stop request unblocks the capture worker within the poll interval.
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

    /// Slow but healthy cameras may span several readiness poll intervals.
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

    /// Poll retries cannot extend the deadline for an entirely stalled camera.
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

    /// Only nonempty frame lengths within their mapped buffer are usable.
    #[test]
    fn driver_frame_length_must_fit_mapping() {
        assert_eq!(checked_frame_length(4, 8).unwrap(), 4);
        assert_eq!(
            checked_frame_length(0, 8).unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );
        assert_eq!(
            checked_frame_length(9, 8).unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );
    }
}
