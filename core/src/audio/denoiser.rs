use realfft::RealFftPlanner;
use rubato::audioadapter_buffers::direct::InterleavedSlice;
use rubato::{
    calculate_cutoff, Async, FixedAsync, Resampler, SincInterpolationParameters,
    SincInterpolationType, WindowFunction,
};
use rustfft::num_complex::Complex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tract_core::internal::tract_ndarray::{Array, IxDyn};
use tract_onnx::prelude::*;

const BLOCK_LEN: usize = 512;
const BLOCK_SHIFT: usize = 128;
const FFT_OUT_SIZE: usize = BLOCK_LEN / 2 + 1; // 257 frequency bins
const MEMORY_ELEMENTS: usize = 1 * 2 * BLOCK_SHIFT * 2;
const DENOISE_RATE: u32 = 16_000;

const SPECTRAL_MODEL_BYTES: &[u8] = include_bytes!("../../resources/models/dtln_model_1.onnx");
const SIGNAL_MODEL_BYTES: &[u8] = include_bytes!("../../resources/models/dtln_model_2.onnx");

type TractModel = SimplePlan<TypedFact, Box<dyn TypedOp>, Graph<TypedFact, Box<dyn TypedOp>>>;

struct DtlnEngine {
    spectral_model: TractModel,
    signal_model: TractModel,

    fft_planner: RealFftPlanner<f32>,
    fft_scratch: Vec<Complex<f32>>,

    spectrum: Vec<Complex<f32>>,
    masked_spectrum: Vec<Complex<f32>>,
    signal_buf: Vec<f32>,

    in_magnitude: [f32; FFT_OUT_SIZE],
    in_phase: [f32; FFT_OUT_SIZE],

    spectral_memory: Vec<f32>,
    signal_memory: Vec<f32>,

    in_buffer: [f32; BLOCK_LEN],
    out_buffer: [f32; BLOCK_LEN],
}

impl DtlnEngine {
    fn new() -> Result<Self, String> {
        let spectral_model = tract_onnx::onnx()
            .model_for_read(&mut &SPECTRAL_MODEL_BYTES[..])
            .map_err(|e| format!("Failed to load spectral model: {e}"))?
            .with_input_fact(0, f32::fact(&[1, 1, FFT_OUT_SIZE]).into())
            .map_err(|e| format!("Failed to set spectral input 0: {e}"))?
            .with_input_fact(1, f32::fact(&[1, 2, BLOCK_SHIFT, 2]).into())
            .map_err(|e| format!("Failed to set spectral input 1: {e}"))?
            .into_optimized()
            .map_err(|e| format!("Failed to optimize spectral model: {e}"))?
            .into_runnable()
            .map_err(|e| format!("Failed to build spectral plan: {e}"))?;

        let signal_model = tract_onnx::onnx()
            .model_for_read(&mut &SIGNAL_MODEL_BYTES[..])
            .map_err(|e| format!("Failed to load signal model: {e}"))?
            .with_input_fact(0, f32::fact(&[1, 1, BLOCK_LEN]).into())
            .map_err(|e| format!("Failed to set signal input 0: {e}"))?
            .with_input_fact(1, f32::fact(&[1, 2, BLOCK_SHIFT, 2]).into())
            .map_err(|e| format!("Failed to set signal input 1: {e}"))?
            .into_optimized()
            .map_err(|e| format!("Failed to optimize signal model: {e}"))?
            .into_runnable()
            .map_err(|e| format!("Failed to build signal plan: {e}"))?;

        let mut fft_planner = RealFftPlanner::new();
        let fwd = fft_planner.plan_fft_forward(BLOCK_LEN);
        let inv = fft_planner.plan_fft_inverse(BLOCK_LEN);
        let scratch_len = fwd.get_scratch_len().max(inv.get_scratch_len());

        Ok(Self {
            spectral_model,
            signal_model,
            fft_planner,
            fft_scratch: vec![Complex::ZERO; scratch_len],
            spectrum: vec![Complex::ZERO; FFT_OUT_SIZE],
            masked_spectrum: vec![Complex::ZERO; FFT_OUT_SIZE],
            signal_buf: vec![0f32; BLOCK_LEN],
            in_magnitude: [0f32; FFT_OUT_SIZE],
            in_phase: [0f32; FFT_OUT_SIZE],
            spectral_memory: vec![0f32; MEMORY_ELEMENTS],
            signal_memory: vec![0f32; MEMORY_ELEMENTS],
            in_buffer: [0f32; BLOCK_LEN],
            out_buffer: [0f32; BLOCK_LEN],
        })
    }

    fn feed(&mut self, samples: &[f32]) -> Result<[f32; BLOCK_SHIFT], String> {
        debug_assert_eq!(samples.len(), BLOCK_SHIFT);

        self.in_buffer.copy_within(BLOCK_SHIFT.., 0);
        self.in_buffer[(BLOCK_LEN - BLOCK_SHIFT)..].copy_from_slice(samples);

        // Forward FFT
        let fft = self.fft_planner.plan_fft_forward(BLOCK_LEN);
        let mut fft_in = self.in_buffer;
        fft.process_with_scratch(&mut fft_in, &mut self.spectrum, &mut self.fft_scratch)
            .map_err(|e| format!("FFT forward failed: {e}"))?;

        for i in 0..FFT_OUT_SIZE {
            self.in_magnitude[i] = self.spectrum[i].norm();
            self.in_phase[i] = self.spectrum[i].arg();
        }

        // Spectral model (model 1)
        {
            let mag =
                Array::from_shape_vec(IxDyn(&[1, 1, FFT_OUT_SIZE]), self.in_magnitude.to_vec())
                    .map_err(|e| format!("Failed to create magnitude array: {e}"))?;
            let mem =
                Array::from_shape_vec(IxDyn(&[1, 2, BLOCK_SHIFT, 2]), self.spectral_memory.clone())
                    .map_err(|e| format!("Failed to create spectral memory array: {e}"))?;

            let outputs = self
                .spectral_model
                .run(tvec![mag.into_tensor().into(), mem.into_tensor().into()])
                .map_err(|e| format!("Spectral model inference failed: {e}"))?;

            let mask = outputs[0]
                .to_array_view::<f32>()
                .map_err(|e| format!("Failed to extract mask: {e}"))?;
            let mask_flat = mask.as_slice().expect("contiguous mask");
            for i in 0..FFT_OUT_SIZE {
                let mag = self.in_magnitude[i] * mask_flat[i];
                let phase = self.in_phase[i];
                self.masked_spectrum[i] = Complex::new(mag * phase.cos(), mag * phase.sin());
            }
            self.masked_spectrum[0] = Complex::new(self.in_magnitude[0] * mask_flat[0], 0.0);
            let last = FFT_OUT_SIZE - 1;
            self.masked_spectrum[last] =
                Complex::new(self.in_magnitude[last] * mask_flat[last], 0.0);

            let new_mem = outputs[1]
                .to_array_view::<f32>()
                .map_err(|e| format!("Failed to extract spectral memory: {e}"))?;
            self.spectral_memory
                .copy_from_slice(new_mem.as_slice().expect("contiguous memory"));
        }

        // Inverse FFT
        let ifft = self.fft_planner.plan_fft_inverse(BLOCK_LEN);
        ifft.process_with_scratch(
            &mut self.masked_spectrum,
            &mut self.signal_buf,
            &mut self.fft_scratch,
        )
        .map_err(|e| format!("FFT inverse failed: {e}"))?;
        for s in &mut self.signal_buf {
            *s /= BLOCK_LEN as f32;
        }

        // Signal model (model 2)
        {
            let sig = Array::from_shape_vec(IxDyn(&[1, 1, BLOCK_LEN]), self.signal_buf.clone())
                .map_err(|e| format!("Failed to create signal array: {e}"))?;
            let mem =
                Array::from_shape_vec(IxDyn(&[1, 2, BLOCK_SHIFT, 2]), self.signal_memory.clone())
                    .map_err(|e| format!("Failed to create signal memory array: {e}"))?;

            let outputs = self
                .signal_model
                .run(tvec![sig.into_tensor().into(), mem.into_tensor().into()])
                .map_err(|e| format!("Signal model inference failed: {e}"))?;

            let out_view = outputs[0]
                .to_array_view::<f32>()
                .map_err(|e| format!("Failed to extract signal output: {e}"))?;
            let out_slice = out_view.as_slice().expect("contiguous output");

            self.out_buffer.copy_within(BLOCK_SHIFT.., 0);
            self.out_buffer[BLOCK_LEN - BLOCK_SHIFT..].fill(0f32);
            for (a, b) in self.out_buffer.iter_mut().zip(out_slice.iter()) {
                *a += b;
            }

            let new_mem = outputs[1]
                .to_array_view::<f32>()
                .map_err(|e| format!("Failed to extract signal memory: {e}"))?;
            self.signal_memory
                .copy_from_slice(new_mem.as_slice().expect("contiguous memory"));
        }

        Ok(self.out_buffer[..BLOCK_SHIFT]
            .try_into()
            .expect("slice is BLOCK_SHIFT long"))
    }
}

/// DTLN noise cancellation with automatic 48kHz <-> 16kHz resampling.
/// TODO: Check again if we can capture at 16kHz to avoid the resampling.
pub struct Denoiser {
    engine: DtlnEngine,
    enabled: Arc<AtomicBool>,
    downsampler: Async<f32>,
    upsampler: Async<f32>,
    down_out: Vec<f32>,
    up_out: Vec<f32>,
    accumulator: Vec<f32>,
    output_accumulator: Vec<f32>,
    samples_per_block: usize,
}

impl Denoiser {
    pub fn new(input_sample_rate: u32, enabled: Arc<AtomicBool>) -> Result<Self, String> {
        let engine = DtlnEngine::new()?;

        let samples_per_block =
            (BLOCK_SHIFT as f64 * input_sample_rate as f64 / DENOISE_RATE as f64).round() as usize;

        let sinc_len = 256;
        // TODO(@konsalex):
        // Used BlackmanHarris2 with good results,
        // but Google's implementation uses Hann. Revisit it needed.
        // https://chromium.googlesource.com/external/webrtc/+/23868b64bc1a0a3226011327bc079c1c67f6ea4b/webrtc/modules/audio_processing/aec/aec_core_neon.cc#646.
        // let window = WindowFunction::BlackmanHarris2;
        let window = WindowFunction::Hann;
        let sinc_params = SincInterpolationParameters {
            sinc_len,
            f_cutoff: calculate_cutoff(sinc_len, window),
            interpolation: SincInterpolationType::Cubic,
            oversampling_factor: 256,
            window,
        };

        let downsampler = Async::<f32>::new_sinc(
            DENOISE_RATE as f64 / input_sample_rate as f64,
            1.1,
            &sinc_params,
            samples_per_block,
            1,
            FixedAsync::Input,
        )
        .map_err(|e| format!("Failed to create downsampler: {e}"))?;

        let upsampler = Async::<f32>::new_sinc(
            input_sample_rate as f64 / DENOISE_RATE as f64,
            1.1,
            &sinc_params,
            BLOCK_SHIFT,
            1,
            FixedAsync::Input,
        )
        .map_err(|e| format!("Failed to create upsampler: {e}"))?;

        let down_out = vec![0f32; downsampler.output_frames_max()];
        let up_out = vec![0f32; upsampler.output_frames_max()];

        Ok(Self {
            engine,
            enabled,
            downsampler,
            upsampler,
            down_out,
            up_out,
            accumulator: Vec::with_capacity(samples_per_block * 4),
            output_accumulator: Vec::new(),
            samples_per_block,
        })
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    /// Denoise i16 samples in-place. No-op when disabled.
    pub fn process(&mut self, samples: &mut [i16]) {
        if !self.is_enabled() {
            return;
        }

        // Convert i16 -> f32 and accumulate
        for &s in samples.iter() {
            self.accumulator.push(s as f32 / i16::MAX as f32);
        }

        while self.accumulator.len() >= self.samples_per_block {
            // Downsample to 16kHz
            let mut block = [0f32; BLOCK_SHIFT];
            let downsample_output_count = {
                let input = InterleavedSlice::new(
                    &self.accumulator[..self.samples_per_block],
                    1,
                    self.samples_per_block,
                )
                .expect("input adapter");
                let out_cap = self.down_out.len();
                let mut output = InterleavedSlice::new_mut(&mut self.down_out, 1, out_cap)
                    .expect("output adapter");
                self.downsampler
                    .process_into_buffer(&input, &mut output, None)
                    .expect("downsample failed")
                    .1
            };
            let copy_count = downsample_output_count.min(BLOCK_SHIFT);
            block[..copy_count].copy_from_slice(&self.down_out[..copy_count]);
            self.accumulator.drain(..self.samples_per_block);

            // Run DTLN engine
            let denoised = match self.engine.feed(&block) {
                Ok(d) => d,
                Err(e) => {
                    log::error!("Denoise error: {e}");
                    block
                }
            };

            // Upsample back to input rate
            let upsample_output_count = {
                let input =
                    InterleavedSlice::new(&denoised[..], 1, BLOCK_SHIFT).expect("input adapter");
                let out_cap = self.up_out.len();
                let mut output = InterleavedSlice::new_mut(&mut self.up_out, 1, out_cap)
                    .expect("output adapter");
                self.upsampler
                    .process_into_buffer(&input, &mut output, None)
                    .expect("upsample failed")
                    .1
            };
            self.output_accumulator
                .extend_from_slice(&self.up_out[..upsample_output_count]);
        }

        // Write denoised samples back to the input slice
        let write_count = samples.len().min(self.output_accumulator.len());
        for i in 0..write_count {
            samples[i] = (self.output_accumulator[i].clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
        }
        for sample in samples.iter_mut().skip(write_count) {
            *sample = 0;
        }
        self.output_accumulator.drain(..write_count);
    }
}
