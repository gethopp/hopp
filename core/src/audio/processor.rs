use livekit::webrtc::native::apm::AudioProcessingModule;
use livekit::webrtc::native::audio_resampler::AudioResampler;

pub const APM_SAMPLE_RATE: u32 = 48000;
pub const APM_NUM_CHANNELS: u32 = 1;

pub struct AudioProcessor {
    apm: AudioProcessingModule,
    resampler: AudioResampler,
    speaker_sample_rate: u32,
    speaker_num_channels: u32,
    reverse_buffer: Vec<i16>,
}

impl AudioProcessor {
    pub fn new(speaker_sample_rate: u32, speaker_num_channels: u32) -> Self {
        Self {
            apm: AudioProcessingModule::new(true, true, false, true),
            resampler: AudioResampler::default(),
            speaker_sample_rate,
            speaker_num_channels,
            reverse_buffer: Vec::new(),
        }
    }

    /// Called from cpal callback with the mixed speaker output (f32).
    /// Resamples to mono 48kHz and feeds 10ms chunks to APM reverse stream.
    pub fn process_reverse(&mut self, data: &[f32]) {
        let i16_data: Vec<i16> = data.iter().map(|&s| (s * i16::MAX as f32) as i16).collect();
        let samples_per_channel = i16_data.len() / self.speaker_num_channels as usize;

        let resampled = self.resampler.remix_and_resample(
            &i16_data,
            samples_per_channel as u32,
            self.speaker_num_channels,
            self.speaker_sample_rate,
            APM_NUM_CHANNELS,
            APM_SAMPLE_RATE,
        );
        self.reverse_buffer.extend_from_slice(resampled);

        // Feed complete 10ms chunks (480 samples at 48kHz mono) to APM
        let chunk_size = (APM_SAMPLE_RATE / 100 * APM_NUM_CHANNELS) as usize;
        while self.reverse_buffer.len() >= chunk_size {
            let mut chunk: Vec<i16> = self.reverse_buffer.drain(..chunk_size).collect();
            if let Err(e) = self.apm.process_reverse_stream(
                &mut chunk,
                APM_SAMPLE_RATE as i32,
                APM_NUM_CHANNELS as i32,
            ) {
                log::warn!("APM process_reverse_stream failed: {e}");
            }
        }
    }

    /// Process mic audio through APM (echo cancellation + noise suppression).
    pub fn process(&mut self, chunk: &mut [i16]) {
        if let Err(e) =
            self.apm
                .process_stream(chunk, APM_SAMPLE_RATE as i32, APM_NUM_CHANNELS as i32)
        {
            log::warn!("APM process_stream failed: {e}");
        }
    }

    pub fn set_delay(&mut self, delay_ms: i32) {
        if let Err(e) = self.apm.set_stream_delay_ms(delay_ms) {
            log::warn!("APM set_stream_delay_ms failed: {e}");
        }
    }

    pub fn update_speaker_config(&mut self, sample_rate: u32, num_channels: u32) {
        self.speaker_sample_rate = sample_rate;
        self.speaker_num_channels = num_channels;
        self.reverse_buffer.clear();
    }
}
