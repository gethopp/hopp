//! Interruptible Media Foundation capture for the Windows fallback camera path.
//!
//! Nokhwa negotiates the format, but its synchronous source reader cannot cancel a
//! stalled `ReadSample`. This reader uses MF's asynchronous callback mode instead.

use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TryRecvError};
use std::time::{Duration, Instant};
use std::{marker::PhantomData, rc::Rc};

use nokhwa::utils::{CameraFormat, FrameFormat};
use windows::core::{implement, GUID, HRESULT, PCWSTR};
use windows::Win32::Media::MediaFoundation::*;
use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_MULTITHREADED};

use super::CameraStreamMessage;

const STOP_POLL_INTERVAL: Duration = Duration::from_millis(20);
const FRAME_TIMEOUT: Duration = Duration::from_secs(5);

struct ReadResult {
    status: HRESULT,
    flags: u32,
    sample: Option<IMFSample>,
}

#[implement(IMFSourceReaderCallback)]
struct ReadCallback {
    tx: SyncSender<ReadResult>,
}

impl IMFSourceReaderCallback_Impl for ReadCallback_Impl {
    /// Queue a completed request without blocking MF's callback thread.
    fn OnReadSample(
        &self,
        status: HRESULT,
        _stream: u32,
        flags: u32,
        _timestamp: i64,
        sample: Option<&IMFSample>,
    ) -> windows::core::Result<()> {
        let _ = self.tx.try_send(ReadResult {
            status,
            flags,
            sample: sample.cloned(),
        });
        Ok(())
    }

    /// MF owns pending callback lifetimes; no worker waits for flush callbacks.
    fn OnFlush(&self, _stream: u32) -> windows::core::Result<()> {
        Ok(())
    }

    /// Read failures arrive through OnReadSample rather than stream events.
    fn OnEvent(&self, _stream: u32, _event: Option<&IMFMediaEvent>) -> windows::core::Result<()> {
        Ok(())
    }
}

struct MediaFoundation(PhantomData<Rc<()>>);

impl MediaFoundation {
    /// Own a startup reference independently of Nokhwa's synchronous reader.
    fn new() -> Result<Self, String> {
        unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }
            .ok()
            .map_err(|error| error.to_string())?;
        if let Err(error) = unsafe { MFStartup(MF_VERSION, MFSTARTUP_FULL) } {
            unsafe { CoUninitialize() };
            return Err(error.to_string());
        }
        Ok(Self(PhantomData))
    }
}

impl Drop for MediaFoundation {
    /// Balance our startup reference only after the reader and source are gone.
    fn drop(&mut self) {
        let _ = unsafe { MFShutdown() };
        unsafe { CoUninitialize() };
    }
}

struct MediaSource(IMFMediaSource);

impl Drop for MediaSource {
    /// Release the device on both successful shutdown and partial startup errors.
    fn drop(&mut self) {
        let _ = unsafe { self.0.Shutdown() };
    }
}

/// Owned negotiation metadata that can cross threads without COM interfaces.
pub(super) struct CaptureRequest {
    symbolic_link: String,
    format: CameraFormat,
}

impl CaptureRequest {
    /// Release Nokhwa on its creating thread before opening the async reader.
    pub(super) fn from_camera(camera: nokhwa::Camera, requested_fps: u32) -> Result<Self, String> {
        let symbolic_link = camera.info().misc().to_string();
        let negotiated = camera.camera_format();
        // Nokhwa's pinned MF backend reads the low half of MF_MT_FRAME_RATE
        // (the denominator) as FPS. Preserve its dimensions/subtype, but use
        // the application's actual requested FPS for native type selection.
        let format = CameraFormat::new(negotiated.resolution(), negotiated.format(), requested_fps);
        drop(camera);
        if symbolic_link.is_empty() {
            return Err("Camera has no Media Foundation device path".to_string());
        }
        Ok(Self {
            symbolic_link,
            format,
        })
    }
}

/// A single pending asynchronous frame request with an interruptible wait.
pub(super) struct AsyncCamera {
    reader: Option<IMFSourceReader>,
    source: Option<MediaSource>,
    events: Receiver<ReadResult>,
    format: CameraFormat,
    // Declared last so MF stays initialized while all COM interfaces are dropped.
    _mf: MediaFoundation,
}

impl AsyncCamera {
    /// Open the negotiated device on the capture worker's own COM apartment.
    pub(super) fn new(request: CaptureRequest) -> Result<Self, String> {
        let CaptureRequest {
            symbolic_link,
            format,
        } = request;
        let mf = MediaFoundation::new()?;
        let source = open_source(&symbolic_link)?;
        let (tx, events) = mpsc::sync_channel(1);
        let callback: IMFSourceReaderCallback = ReadCallback { tx }.into();
        let attributes = attributes(2).map_err(|error| error.to_string())?;
        unsafe { attributes.SetUnknown(&MF_SOURCE_READER_ASYNC_CALLBACK, &callback) }
            .map_err(|error| error.to_string())?;
        unsafe { attributes.SetUINT32(&MF_READWRITE_DISABLE_CONVERTERS, 1) }
            .map_err(|error| error.to_string())?;
        let reader = unsafe { MFCreateSourceReaderFromMediaSource(&source.0, &attributes) }
            .map_err(|error| error.to_string())?;
        unsafe { select_format(&reader, format) }.map_err(|error| error.to_string())?;
        Ok(Self {
            reader: Some(reader),
            source: Some(source),
            events,
            format,
            _mf: mf,
        })
    }

    /// Return the input format used by the shared frame conversion path.
    pub(super) fn frame_format(&self) -> FrameFormat {
        self.format.format()
    }

    /// Wait for a sample, a stop request, or a stalled-device timeout.
    pub(super) fn frame(
        &mut self,
        stop: &Receiver<CameraStreamMessage>,
    ) -> Result<Option<nokhwa::Buffer>, String> {
        let reader = self.reader.as_ref().ok_or("Camera stream is stopped")?;
        let deadline = Instant::now() + FRAME_TIMEOUT;
        loop {
            // In asynchronous mode all four output arguments MUST be null.
            unsafe {
                reader.ReadSample(
                    MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32,
                    0,
                    None,
                    None,
                    None,
                    None,
                )
            }
            .map_err(|error| error.to_string())?;
            let Some(result) = wait_for_sample(&self.events, stop, deadline)? else {
                return Ok(None);
            };
            result.status.ok().map_err(|error| error.to_string())?;
            if result.flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32 != 0 {
                return Err("Camera stream ended".to_string());
            }
            if result.flags & MF_SOURCE_READERF_CURRENTMEDIATYPECHANGED.0 as u32 != 0 {
                return Err("Camera changed input format; restart capture".to_string());
            }
            if let Some(sample) = result.sample {
                let data = copy_sample(&sample)?;
                return Ok(Some(nokhwa::Buffer::new(
                    self.format.resolution(),
                    &data,
                    self.format.format(),
                )));
            }
            // A stream tick has no sample; request the next sample while keeping
            // the original deadline, so endless ticks cannot conceal a stall.
        }
    }

    /// Cancel outstanding reads before dropping the source and joining capture.
    pub(super) fn stop_stream(&mut self) -> Result<(), String> {
        let reader = self.reader.take();
        let result = if let Some(reader) = reader.as_ref() {
            // With ASYNC_CALLBACK set Flush returns immediately and cancels
            // pending requests; it never waits for a stalled ReadSample.
            unsafe { reader.Flush(MF_SOURCE_READER_ALL_STREAMS.0 as u32) }
                .map_err(|error| error.to_string())
        } else {
            Ok(())
        };
        // Keep the reader alive until source shutdown cancels driver activity;
        // releasing its last reference first can otherwise wait for that work.
        self.source.take();
        drop(reader);
        result
    }
}

impl Drop for AsyncCamera {
    /// Always cancel pending requests, including when the capture worker fails.
    fn drop(&mut self) {
        let _ = self.stop_stream();
    }
}

/// Poll only the application-owned channel, never a blocking device read.
fn wait_for_sample<T>(
    events: &Receiver<T>,
    stop: &Receiver<CameraStreamMessage>,
    deadline: Instant,
) -> Result<Option<T>, String> {
    wait_for_sample_with_observer(events, stop, deadline, || {})
}

/// Observe entry to each interruptible wait; tests synchronize with a pending read.
fn wait_for_sample_with_observer<T>(
    events: &Receiver<T>,
    stop: &Receiver<CameraStreamMessage>,
    deadline: Instant,
    mut before_wait: impl FnMut(),
) -> Result<Option<T>, String> {
    loop {
        match stop.try_recv() {
            Ok(CameraStreamMessage::StopCapture) | Err(TryRecvError::Disconnected) => {
                return Ok(None)
            }
            _ => {}
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err("Camera did not deliver a frame for 5 seconds".to_string());
        }
        before_wait();
        match events.recv_timeout(remaining.min(STOP_POLL_INTERVAL)) {
            Ok(sample) => return Ok(Some(sample)),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                return Err("Camera callback disconnected".to_string())
            }
        }
    }
}

/// Allocate MF attributes, treating an unexpected null success as an error.
fn attributes(count: u32) -> Result<IMFAttributes, windows::core::Error> {
    let mut attributes = None;
    unsafe { MFCreateAttributes(&mut attributes, count)? };
    attributes.ok_or_else(windows::core::Error::from_win32)
}

/// Open Nokhwa's symbolic device path so enumeration changes cannot select a different camera.
fn open_source(symbolic_link: &str) -> Result<MediaSource, String> {
    unsafe {
        let attributes = attributes(2).map_err(|error| error.to_string())?;
        attributes
            .SetGUID(
                &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE,
                &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_GUID,
            )
            .map_err(|error| error.to_string())?;
        let symbolic_link: Vec<u16> = symbolic_link.encode_utf16().chain(Some(0)).collect();
        attributes
            .SetString(
                &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_SYMBOLIC_LINK,
                PCWSTR(symbolic_link.as_ptr()),
            )
            .map_err(|error| error.to_string())?;
        MFCreateDeviceSource(&attributes)
            .map(MediaSource)
            .map_err(|error| error.to_string())
    }
}

/// Preserve Nokhwa's size/subtype and choose the nearest native frame rate.
unsafe fn select_format(
    reader: &IMFSourceReader,
    format: CameraFormat,
) -> windows::core::Result<()> {
    let subtype = match format.format() {
        FrameFormat::YUYV => MFVideoFormat_YUY2,
        FrameFormat::NV12 => MFVideoFormat_NV12,
        FrameFormat::MJPEG => MFVideoFormat_MJPG,
        FrameFormat::RAWBGR => MFVideoFormat_RGB24,
        // Nokhwa's MF GRAY mapping is Y800, which differs from MF's L8 GUID.
        FrameFormat::GRAY => GUID::from_u128(0x30303859_0000_0010_8000_00aa00389b71),
        _ => {
            return Err(windows::core::Error::from_hresult(
                windows::Win32::Foundation::E_INVALIDARG,
            ))
        }
    };
    let stream = MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32;
    let mut index = 0;
    let mut best: Option<(f64, IMFMediaType)> = None;
    loop {
        let media_type = match reader.GetNativeMediaType(stream, index) {
            Ok(media_type) => media_type,
            Err(error) if error.code() == MF_E_NO_MORE_TYPES => break,
            Err(error) => return Err(error),
        };
        if let Some(distance) = native_rate_distance(&media_type, format, subtype)? {
            if best.as_ref().is_none_or(|(current, _)| distance < *current) {
                best = Some((distance, media_type));
            }
        }
        index += 1;
    }
    let (_, media_type) =
        best.ok_or_else(|| windows::core::Error::from_hresult(MF_E_INVALIDMEDIATYPE))?;
    reader.SetCurrentMediaType(stream, None, &media_type)
}

/// Reject mismatched sizes/subtypes and score the native rational frame rate.
unsafe fn native_rate_distance(
    media_type: &IMFMediaType,
    format: CameraFormat,
    subtype: GUID,
) -> windows::core::Result<Option<f64>> {
    let size = media_type.GetUINT64(&MF_MT_FRAME_SIZE)?;
    if media_type.GetGUID(&MF_MT_SUBTYPE)? != subtype
        || size >> 32 != format.resolution().width() as u64
        || size as u32 != format.resolution().height()
    {
        return Ok(None);
    }
    let rate = media_type.GetUINT64(&MF_MT_FRAME_RATE)?;
    let denominator = rate as u32;
    if denominator == 0 || rate >> 32 == 0 {
        return Ok(None);
    }
    Ok(Some(
        ((rate >> 32) as f64 / denominator as f64 - format.frame_rate() as f64).abs(),
    ))
}

/// Copy a delivered sample while balancing every successful media-buffer lock.
fn copy_sample(sample: &IMFSample) -> Result<Vec<u8>, String> {
    unsafe {
        let buffer = sample
            .ConvertToContiguousBuffer()
            .map_err(|error| error.to_string())?;
        let mut data = std::ptr::null_mut();
        let mut length = 0;
        buffer
            .Lock(&mut data, None, Some(&mut length))
            .map_err(|error| error.to_string())?;
        let copied = if length == 0 {
            Vec::new()
        } else if data.is_null() {
            let _ = buffer.Unlock();
            return Err("Camera returned a null media buffer".to_string());
        } else {
            std::slice::from_raw_parts(data, length as usize).to_vec()
        };
        buffer.Unlock().map_err(|error| error.to_string())?;
        Ok(copied)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    /// Stop interrupts an already pending read, and completion is checked before joining.
    fn stalled_callback_is_interruptible_before_join() {
        let (_callback, events) = mpsc::channel::<()>();
        let (stop, commands) = mpsc::channel();
        let (waiting, entered_wait) = mpsc::sync_channel(1);
        let (finished, completion) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            let result = wait_for_sample_with_observer(
                &events,
                &commands,
                Instant::now() + FRAME_TIMEOUT,
                || {
                    let _ = waiting.try_send(());
                },
            );
            finished.send(result).unwrap();
        });
        entered_wait.recv_timeout(Duration::from_secs(1)).unwrap();
        stop.send(CameraStreamMessage::StopCapture).unwrap();
        assert!(completion
            .recv_timeout(Duration::from_secs(1))
            .unwrap()
            .unwrap()
            .is_none());
        worker.join().unwrap();
    }

    #[test]
    /// Dropping the stream owner cancels a callback wait even without an explicit stop.
    fn disconnected_owner_cancels_pending_read() {
        let (_callback, events) = mpsc::channel::<()>();
        let (stop, commands) = mpsc::channel();
        drop(stop);
        assert!(
            wait_for_sample(&events, &commands, Instant::now() + FRAME_TIMEOUT)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    /// Completed samples are delivered and a destroyed callback is reported.
    fn callback_delivers_data_and_disconnection_is_reported() {
        let (callback, events) = mpsc::channel();
        let (_stop, commands) = mpsc::channel();
        callback.send(42).unwrap();
        assert_eq!(
            wait_for_sample(&events, &commands, Instant::now() + FRAME_TIMEOUT).unwrap(),
            Some(42)
        );
        drop(callback);
        assert!(wait_for_sample(&events, &commands, Instant::now() + FRAME_TIMEOUT).is_err());
    }

    #[test]
    /// A device that never invokes its callback fails within its read deadline.
    fn stalled_camera_has_a_deadline() {
        let (_callback, events) = mpsc::channel::<()>();
        let (_stop, commands) = mpsc::channel();
        assert!(wait_for_sample(&events, &commands, Instant::now()).is_err());
    }

    #[test]
    /// Late and duplicate callbacks cannot block cancellation or worker teardown.
    fn callbacks_after_worker_shutdown_do_not_block() {
        let (tx, events) = mpsc::sync_channel(1);
        let callback: IMFSourceReaderCallback = ReadCallback { tx }.into();
        // A driver can complete its read after cancellation. Both a full queue
        // and a destroyed receiver must be harmless on the MF callback thread.
        unsafe {
            callback.OnReadSample(HRESULT(0), 0, 0, 0, None).unwrap();
            callback.OnReadSample(HRESULT(0), 0, 0, 0, None).unwrap();
            drop(events);
            callback.OnReadSample(HRESULT(0), 0, 0, 0, None).unwrap();
            callback.OnFlush(0).unwrap();
        }
    }

    #[test]
    /// Native sample conversion copies owned bytes and releases the media buffer lock.
    fn mf_sample_copy_balances_the_buffer_lock() {
        let _mf = MediaFoundation::new().unwrap();
        unsafe {
            let sample = MFCreateSample().unwrap();
            let buffer = MFCreateMemoryBuffer(4).unwrap();
            let mut data = std::ptr::null_mut();
            buffer.Lock(&mut data, None, None).unwrap();
            std::ptr::copy_nonoverlapping([1, 2, 3, 4].as_ptr(), data, 4);
            buffer.Unlock().unwrap();
            buffer.SetCurrentLength(4).unwrap();
            sample.AddBuffer(&buffer).unwrap();
            assert_eq!(copy_sample(&sample).unwrap(), [1, 2, 3, 4]);
            // Copying again verifies that the previous successful lock was
            // released rather than leaving the delivered sample locked.
            assert_eq!(copy_sample(&sample).unwrap(), [1, 2, 3, 4]);
        }
    }

    #[test]
    /// Native rates use the full MF ratio and reject mismatched input dimensions.
    fn native_format_selection_uses_rational_frame_rate() {
        let _mf = MediaFoundation::new().unwrap();
        let format = CameraFormat::new(
            nokhwa::utils::Resolution::new(1920, 1080),
            FrameFormat::YUYV,
            30,
        );
        unsafe {
            let media_type = MFCreateMediaType().unwrap();
            media_type
                .SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_YUY2)
                .unwrap();
            media_type
                .SetUINT64(&MF_MT_FRAME_SIZE, (1920 << 32) | 1080)
                .unwrap();
            media_type
                .SetUINT64(&MF_MT_FRAME_RATE, (60 << 32) | 1)
                .unwrap();
            assert_eq!(
                native_rate_distance(&media_type, format, MFVideoFormat_YUY2).unwrap(),
                Some(30.0)
            );
            media_type
                .SetUINT64(&MF_MT_FRAME_RATE, (30_000 << 32) | 1001)
                .unwrap();
            assert!(
                native_rate_distance(&media_type, format, MFVideoFormat_YUY2)
                    .unwrap()
                    .unwrap()
                    < 0.04
            );
            media_type
                .SetUINT64(&MF_MT_FRAME_SIZE, (1280 << 32) | 720)
                .unwrap();
            assert_eq!(
                native_rate_distance(&media_type, format, MFVideoFormat_YUY2).unwrap(),
                None
            );
        }
    }
}
