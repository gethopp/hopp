use objc2_av_foundation::*;
use objc2_core_media::*;
use objc2_core_video::*;
use objc2_foundation::*;

pub fn test(sample_buffer: &CMSampleBuffer) {
    unsafe {
        let image_buffer = CMSampleBufferGetImageBuffer(sample_buffer);
        // let pixel_buffer = image_buffer as *mut CVPixelBuffer;
    }
}
