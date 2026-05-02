use realfft::RealFftPlanner;
use rustfft::num_complex::Complex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tract_core::internal::tract_ndarray::{Array, IxDyn};
use tract_onnx::prelude::*;

const BLOCK_LEN: usize = 512;
const BLOCK_SHIFT: usize = 128;
const FFT_OUT_SIZE: usize = BLOCK_LEN / 2 + 1;

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

    in_buffer: [f32; BLOCK_LEN],
    out_buffer: [f32; BLOCK_LEN],

    // Memory state tensors recycled from previous inference output to avoid
    // per-frame allocations.  Stored as owned Tensor (Send-safe) instead of
    // TValue (contains Rc, not Send).
    spectral_mem: Option<Tensor>,
    signal_mem: Option<Tensor>,
}

impl DtlnEngine {
    fn new() -> Result<Self, String> {
        let spectral_model = tract_onnx::onnx()
            .model_for_read(&mut &SPECTRAL_MODEL_BYTES[..])
            .map_err(|e| format!("Failed to load spectral model: {e}"))?
            .with_input_fact(0, f32::fact([1, 1, FFT_OUT_SIZE]).into())
            .map_err(|e| format!("Failed to set spectral input 0: {e}"))?
            .with_input_fact(1, f32::fact([1, 2, BLOCK_SHIFT, 2]).into())
            .map_err(|e| format!("Failed to set spectral input 1: {e}"))?
            .into_optimized()
            .map_err(|e| format!("Failed to optimize spectral model: {e}"))?
            .into_runnable()
            .map_err(|e| format!("Failed to build spectral plan: {e}"))?;

        let signal_model = tract_onnx::onnx()
            .model_for_read(&mut &SIGNAL_MODEL_BYTES[..])
            .map_err(|e| format!("Failed to load signal model: {e}"))?
            .with_input_fact(0, f32::fact([1, 1, BLOCK_LEN]).into())
            .map_err(|e| format!("Failed to set signal input 0: {e}"))?
            .with_input_fact(1, f32::fact([1, 2, BLOCK_SHIFT, 2]).into())
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
            in_buffer: [0f32; BLOCK_LEN],
            out_buffer: [0f32; BLOCK_LEN],
            spectral_mem: Some(
                Array::<f32, _>::zeros(IxDyn(&[1, 2, BLOCK_SHIFT, 2])).into_tensor(),
            ),
            signal_mem: Some(Array::<f32, _>::zeros(IxDyn(&[1, 2, BLOCK_SHIFT, 2])).into_tensor()),
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

            let spec_mem = self.spectral_mem.take().unwrap_or_else(|| {
                Array::<f32, _>::zeros(IxDyn(&[1, 2, BLOCK_SHIFT, 2])).into_tensor()
            });

            let mut outputs = self
                .spectral_model
                .run(tvec![mag.into_tensor().into(), spec_mem.into()])
                .map_err(|e| format!("Spectral model inference failed: {e}"))?;

            // Recycle memory tensor for next frame (O(1) Rc::try_unwrap, no allocation).
            self.spectral_mem = Some(outputs.remove(1).into_tensor());

            let mask = outputs[0]
                .to_array_view::<f32>()
                .map_err(|e| format!("Failed to extract mask: {e}"))?;
            let Some(mask_flat) = mask.as_slice() else {
                log::error!("Failed to get contiguous mask for spectral model");
                return Err("Non-contiguous mask array".into());
            };
            for (i, &mask_val) in mask_flat.iter().enumerate().take(FFT_OUT_SIZE) {
                let mag = self.in_magnitude[i] * mask_val;
                let phase = self.in_phase[i];
                self.masked_spectrum[i] = Complex::new(mag * phase.cos(), mag * phase.sin());
            }
            self.masked_spectrum[0] = Complex::new(self.in_magnitude[0] * mask_flat[0], 0.0);
            let last = FFT_OUT_SIZE - 1;
            self.masked_spectrum[last] =
                Complex::new(self.in_magnitude[last] * mask_flat[last], 0.0);
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

            let sig_mem = self.signal_mem.take().unwrap_or_else(|| {
                Array::<f32, _>::zeros(IxDyn(&[1, 2, BLOCK_SHIFT, 2])).into_tensor()
            });

            let mut outputs = self
                .signal_model
                .run(tvec![sig.into_tensor().into(), sig_mem.into()])
                .map_err(|e| format!("Signal model inference failed: {e}"))?;

            // Recycle memory tensor for next frame (O(1) Rc::try_unwrap, no allocation).
            self.signal_mem = Some(outputs.remove(1).into_tensor());

            let out_view = outputs[0]
                .to_array_view::<f32>()
                .map_err(|e| format!("Failed to extract signal output: {e}"))?;
            let Some(out_slice) = out_view.as_slice() else {
                log::error!("Failed to get contiguous output for signal model");
                return Err("Non-contiguous output array".into());
            };

            self.out_buffer.copy_within(BLOCK_SHIFT.., 0);
            self.out_buffer[BLOCK_LEN - BLOCK_SHIFT..].fill(0f32);
            for (a, b) in self.out_buffer.iter_mut().zip(out_slice.iter()) {
                *a += b;
            }
        }

        self.out_buffer[..BLOCK_SHIFT]
            .try_into()
            .map_err(|_| "Output buffer slice has unexpected length".to_string())
    }
}

pub struct Denoiser {
    engine: DtlnEngine,
    enabled: Arc<AtomicBool>,
    input_queue: Vec<f32>,
    output_queue: Vec<f32>,
}

impl Denoiser {
    pub fn new(enabled: Arc<AtomicBool>) -> Result<Self, String> {
        let engine = DtlnEngine::new()?;
        Ok(Self {
            engine,
            enabled,
            input_queue: Vec::with_capacity(BLOCK_SHIFT * 4),
            output_queue: Vec::with_capacity(BLOCK_SHIFT * 4),
        })
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    pub fn process(&mut self, samples: &mut [i16]) {
        if !self.is_enabled() {
            return;
        }

        // Queue new input samples as f32.
        for &s in samples.iter() {
            self.input_queue.push(s as f32 / i16::MAX as f32);
        }

        // Drain all full blocks from the input queue into the output queue.
        while self.input_queue.len() >= BLOCK_SHIFT {
            let block: [f32; BLOCK_SHIFT] = match self.input_queue[..BLOCK_SHIFT].try_into() {
                Ok(b) => b,
                Err(_) => {
                    log::error!("Failed to convert input block");
                    break;
                }
            };
            self.input_queue.drain(..BLOCK_SHIFT);
            let denoised = match self.engine.feed(&block) {
                Ok(d) => d,
                Err(e) => {
                    log::error!("Denoise error: {e}");
                    block
                }
            };
            self.output_queue.extend_from_slice(&denoised);
        }

        // Emit exactly samples.len() denoised samples aligned with the input
        // frame. When the output queue is short (e.g. first frame before the
        // pipeline has warmed up), emit silence for the missing tail rather
        // than leaking through the raw noisy input.
        let available = self.output_queue.len().min(samples.len());
        for (slot, &s) in samples[..available]
            .iter_mut()
            .zip(self.output_queue.iter())
        {
            *slot = (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
        }
        self.output_queue.drain(..available);
        for slot in samples[available..].iter_mut() {
            *slot = 0;
        }
    }
}
