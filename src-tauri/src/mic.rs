use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, Stream, SupportedStreamConfig};
use df::tract::{DfParams, DfTract, ReduceMask, RuntimeParams};
use ndarray::{ArrayView2, ArrayViewMut2};
use nnnoiseless::{DenoiseState, FRAME_SIZE};

const OUTPUT_RATE_HZ: f64 = 48_000.0;
const TARGET_FILL_MS: f64 = 50.0;
const MAX_FILL_MS: f64 = 120.0;
// 0.5 percent is about 9 cents of pitch, far more than any real crystal mismatch needs
const MAX_RATIO_ADJUST: f64 = 0.005;
// averages the fill level over roughly 200 ms so the 10 ms capture callbacks dont jitter the correction
const FILL_SMOOTHING: f64 = 0.0001;

#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum NoiseEngine {
    Off = 0,
    Rnnoise = 1,
    DeepFilterNet = 2,
}

impl NoiseEngine {
    pub fn from_u8(val: u8) -> Self {
        match val {
            0 => NoiseEngine::Off,
            2 => NoiseEngine::DeepFilterNet,
            _ => NoiseEngine::Rnnoise,
        }
    }

    pub fn from_str(name: &str) -> Self {
        match name {
            "off" => NoiseEngine::Off,
            "deepfilternet" => NoiseEngine::DeepFilterNet,
            _ => NoiseEngine::Rnnoise,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            NoiseEngine::Off => "off",
            NoiseEngine::Rnnoise => "rnnoise",
            NoiseEngine::DeepFilterNet => "deepfilternet",
        }
    }
}

#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum VoiceTone {
    Natural = 0,
    Warm = 1,
    Crisp = 2,
}

impl VoiceTone {
    pub fn from_u8(val: u8) -> Self {
        match val {
            1 => VoiceTone::Warm,
            2 => VoiceTone::Crisp,
            _ => VoiceTone::Natural,
        }
    }

    pub fn from_str(name: &str) -> Self {
        match name {
            "warm" => VoiceTone::Warm,
            "crisp" => VoiceTone::Crisp,
            _ => VoiceTone::Natural,
        }
    }
}

#[derive(Clone, Copy)]
struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    z1: f32,
    z2: f32,
}

impl Biquad {
    fn new_high_pass(cutoff_hz: f32, sample_rate: f32) -> Self {
        let omega = 2.0 * std::f32::consts::PI * cutoff_hz / sample_rate;
        let cos_w = omega.cos();
        let alpha = omega.sin() / std::f32::consts::SQRT_2;
        let a0 = 1.0 + alpha;
        Self {
            b0: ((1.0 + cos_w) * 0.5) / a0,
            b1: -(1.0 + cos_w) / a0,
            b2: ((1.0 + cos_w) * 0.5) / a0,
            a1: (-2.0 * cos_w) / a0,
            a2: (1.0 - alpha) / a0,
            z1: 0.0,
            z2: 0.0,
        }
    }

    fn new_low_shelf(freq_hz: f32, gain_db: f32, sample_rate: f32) -> Self {
        let a = 10.0f32.powf(gain_db / 40.0);
        let omega = 2.0 * std::f32::consts::PI * freq_hz / sample_rate;
        let cos_w = omega.cos();
        let two_sqrt_a_alpha = 2.0 * a.sqrt() * (omega.sin() / std::f32::consts::SQRT_2);
        let a0 = (a + 1.0) + (a - 1.0) * cos_w + two_sqrt_a_alpha;
        Self {
            b0: (a * ((a + 1.0) - (a - 1.0) * cos_w + two_sqrt_a_alpha)) / a0,
            b1: (2.0 * a * ((a - 1.0) - (a + 1.0) * cos_w)) / a0,
            b2: (a * ((a + 1.0) - (a - 1.0) * cos_w - two_sqrt_a_alpha)) / a0,
            a1: (-2.0 * ((a - 1.0) + (a + 1.0) * cos_w)) / a0,
            a2: ((a + 1.0) + (a - 1.0) * cos_w - two_sqrt_a_alpha) / a0,
            z1: 0.0,
            z2: 0.0,
        }
    }

    fn new_high_shelf(freq_hz: f32, gain_db: f32, sample_rate: f32) -> Self {
        let a = 10.0f32.powf(gain_db / 40.0);
        let omega = 2.0 * std::f32::consts::PI * freq_hz / sample_rate;
        let cos_w = omega.cos();
        let two_sqrt_a_alpha = 2.0 * a.sqrt() * (omega.sin() / std::f32::consts::SQRT_2);
        let a0 = (a + 1.0) - (a - 1.0) * cos_w + two_sqrt_a_alpha;
        Self {
            b0: (a * ((a + 1.0) + (a - 1.0) * cos_w + two_sqrt_a_alpha)) / a0,
            b1: (-2.0 * a * ((a - 1.0) + (a + 1.0) * cos_w)) / a0,
            b2: (a * ((a + 1.0) + (a - 1.0) * cos_w - two_sqrt_a_alpha)) / a0,
            a1: (2.0 * ((a - 1.0) - (a + 1.0) * cos_w)) / a0,
            a2: ((a + 1.0) - (a - 1.0) * cos_w - two_sqrt_a_alpha) / a0,
            z1: 0.0,
            z2: 0.0,
        }
    }

    fn process(&mut self, x: f32) -> f32 {
        let input = if x.is_finite() { x } else { 0.0 };
        let y = self.b0 * input + self.z1;
        let z1 = self.b1 * input - self.a1 * y + self.z2;
        let z2 = self.b2 * input - self.a2 * y;
        if !y.is_finite() || !z1.is_finite() || !z2.is_finite() {
            self.reset();
            return 0.0;
        }
        // flush denormals to zero so quiet silences over hours of uptime never trigger cpu subnormal penalties
        self.z1 = if z1.abs() < 1e-20 { 0.0 } else { z1 };
        self.z2 = if z2.abs() < 1e-20 { 0.0 } else { z2 };
        y
    }

    fn reset(&mut self) {
        self.z1 = 0.0;
        self.z2 = 0.0;
    }
}

// avoids hard clipping when gain goes above 100 percent
fn soft_limit(sample: f32) -> f32 {
    let abs = sample.abs();
    if abs <= 0.85 {
        sample
    } else {
        let excess = abs - 0.85;
        let limited = 0.85 + 0.15 * (excess / 0.15).tanh();
        limited.copysign(sample)
    }
}

struct CaptureFilter {
    input_rate_hz: f64,
    resample_frac: f64,
    last_mono: f32,
    has_last_mono: bool,
    active_engine: NoiseEngine,
    applied_atten_lim: f32,
    applied_pf_beta: f32,
    active_high_pass: bool,
    active_tone: VoiceTone,
    hp_filter: Biquad,
    warm_low: Biquad,
    warm_high: Biquad,
    crisp_low: Biquad,
    crisp_high: Biquad,
    gate_envelope: f32,
    gate_gain: f32,
    gate_hold_samples: usize,
    comp_env: f32,
    funny_hp: Biquad,
    funny_hold_sample: f32,
    funny_decimate_counter: u8,
    thread_promoted: bool,
    df_smoothed_ms: f32,
    df_fallback_until: Option<Instant>,
    rnnoise: Box<DenoiseState<'static>>,
    deep_filter: Option<DfTract>,
    frame_pending: Vec<f32>,
    staged_samples: Vec<f32>,
}

// dftract uses rc internally for its tensor buffers, moving the whole struct once into the cpal callback thread is safe
unsafe impl Send for CaptureFilter {}

impl CaptureFilter {
    fn new(input_rate_hz: f64, initial_engine: NoiseEngine, atten_lim: f32, pf_beta: f32) -> Self {
        let runtime_params = RuntimeParams::default_with_ch(1)
            .with_atten_lim(atten_lim)
            .with_thresholds(-15.0, 35.0, 35.0)
            .with_post_filter(pf_beta)
            .with_mask_reduce(ReduceMask::MAX);
        let deep_filter = match DfTract::new(DfParams::default(), &runtime_params) {
            Ok(model) => Some(model),
            Err(err) => {
                log::error!("failed to initialize deepfilternet: {err}");
                None
            }
        };

        let rate = OUTPUT_RATE_HZ as f32;
        CaptureFilter {
            input_rate_hz,
            resample_frac: 0.0,
            last_mono: 0.0,
            has_last_mono: false,
            active_engine: initial_engine,
            applied_atten_lim: atten_lim,
            applied_pf_beta: pf_beta,
            active_high_pass: false,
            active_tone: VoiceTone::Natural,
            hp_filter: Biquad::new_high_pass(80.0, rate),
            warm_low: Biquad::new_low_shelf(160.0, 3.5, rate),
            warm_high: Biquad::new_high_shelf(3500.0, 2.0, rate),
            crisp_low: Biquad::new_low_shelf(180.0, -2.5, rate),
            crisp_high: Biquad::new_high_shelf(4000.0, 3.5, rate),
            gate_envelope: 0.0,
            gate_gain: 1.0,
            gate_hold_samples: 0,
            comp_env: 0.0,
            funny_hp: Biquad::new_high_pass(520.0, rate),
            funny_hold_sample: 0.0,
            funny_decimate_counter: 0,
            thread_promoted: false,
            df_smoothed_ms: 1.0,
            df_fallback_until: None,
            rnnoise: DenoiseState::new(),
            deep_filter,
            frame_pending: Vec::with_capacity(FRAME_SIZE),
            staged_samples: Vec::with_capacity(FRAME_SIZE * 4),
        }
    }

    fn process_frames<T: Copy>(
        &mut self,
        interleaved: &[T],
        channels: usize,
        requested_engine: NoiseEngine,
        atten_lim: f32,
        pf_beta: f32,
        high_pass: bool,
        tone: VoiceTone,
        gate_enabled: bool,
        gate_threshold: f32,
        compressor_enabled: bool,
        gain: f32,
        funny_mic: bool,
        to_f32: impl Fn(T) -> f32,
    ) -> (&[f32], f32) {
        if !self.thread_promoted {
            crate::server::promote_audio_thread();
            self.thread_promoted = true;
        }
        let engine = if requested_engine == NoiseEngine::DeepFilterNet {
            if let Some(until) = self.df_fallback_until {
                if Instant::now() < until {
                    NoiseEngine::Rnnoise
                } else {
                    self.df_fallback_until = None;
                    self.df_smoothed_ms = 2.0;
                    NoiseEngine::DeepFilterNet
                }
            } else {
                NoiseEngine::DeepFilterNet
            }
        } else {
            requested_engine
        };
        if engine != self.active_engine {
            self.frame_pending.clear();
            self.active_engine = engine;
        }
        if high_pass != self.active_high_pass {
            self.hp_filter.reset();
            self.active_high_pass = high_pass;
        }
        if tone != self.active_tone {
            self.warm_low.reset();
            self.warm_high.reset();
            self.crisp_low.reset();
            self.crisp_high.reset();
            self.active_tone = tone;
        }
        if let Some(df) = self.deep_filter.as_mut() {
            if (atten_lim - self.applied_atten_lim).abs() > 0.05 {
                df.set_atten_lim(atten_lim);
                self.applied_atten_lim = atten_lim;
            }
            if (pf_beta - self.applied_pf_beta).abs() > 0.0005 {
                df.set_pf_beta(pf_beta);
                self.applied_pf_beta = pf_beta;
            }
        }

        self.staged_samples.clear();
        let step = self.input_rate_hz / OUTPUT_RATE_HZ;
        let same_rate = (self.input_rate_hz - OUTPUT_RATE_HZ).abs() < 0.5;
        let mut batch_peak = 0.0f32;

        for frame in interleaved.chunks_exact(channels) {
            let mono = frame.iter().map(|&sample| to_f32(sample)).sum::<f32>() / channels as f32;
            batch_peak = batch_peak.max(mono.abs());
            if same_rate {
                self.push_48k_sample(mono);
                continue;
            }
            if !self.has_last_mono {
                self.last_mono = mono;
                self.has_last_mono = true;
                continue;
            }
            while self.resample_frac <= 1.0 {
                let interpolated = self.last_mono + (mono - self.last_mono) * self.resample_frac as f32;
                self.push_48k_sample(interpolated);
                self.resample_frac += step;
            }
            self.resample_frac -= 1.0;
            self.last_mono = mono;
        }

        if self.active_tone != VoiceTone::Natural {
            for sample in &mut self.staged_samples {
                *sample = match self.active_tone {
                    VoiceTone::Natural => *sample,
                    VoiceTone::Warm => {
                        let s = self.warm_low.process(*sample);
                        self.warm_high.process(s)
                    }
                    VoiceTone::Crisp => {
                        let s = self.crisp_low.process(*sample);
                        self.crisp_high.process(s)
                    }
                };
            }
        }

        if gate_enabled {
            let hold_max = (0.14 * OUTPUT_RATE_HZ) as usize;
            let eff_gain = gain.max(0.1);
            for sample in &mut self.staged_samples {
                let level = sample.abs() * eff_gain;
                let coeff = if level > self.gate_envelope { 0.02 } else { 0.0005 };
                self.gate_envelope += (level - self.gate_envelope) * coeff;

                if self.gate_envelope >= gate_threshold {
                    self.gate_hold_samples = hold_max;
                    self.gate_gain += (1.0 - self.gate_gain) * 0.015;
                } else if self.gate_hold_samples > 0 {
                    self.gate_hold_samples -= 1;
                } else {
                    self.gate_gain *= 0.9993;
                    if self.gate_gain < 0.0001 {
                        self.gate_gain = 0.0;
                    }
                }
                *sample *= self.gate_gain;
            }
        } else {
            self.gate_gain = 1.0;
            self.gate_hold_samples = 0;
        }

        if compressor_enabled {
            for sample in &mut self.staged_samples {
                let abs = sample.abs();
                let coeff = if abs > self.comp_env { 0.004 } else { 0.00018 };
                self.comp_env += (abs - self.comp_env) * coeff;

                if self.comp_env > 0.015 {
                    let threshold = 0.16f32;
                    let gain_mult = if self.comp_env > threshold {
                        let target = threshold + (self.comp_env - threshold) * 0.33;
                        (target / self.comp_env) * 1.65
                    } else {
                        1.65
                    };
                    *sample = soft_limit(*sample * gain_mult);
                }
            }
        } else {
            self.comp_env = 0.0;
        }

        if funny_mic {
            for sample in &mut self.staged_samples {
                let thin = self.funny_hp.process(*sample);
                if self.funny_decimate_counter == 0 {
                    let overdriven = (thin * 22.0).clamp(-1.0, 1.0);
                    let crushed = (overdriven * 4.0).round() / 4.0;
                    self.funny_hold_sample = crushed.clamp(-0.9, 0.9);
                }
                self.funny_decimate_counter = (self.funny_decimate_counter + 1) & 7;
                *sample = self.funny_hold_sample;
            }
        }

        (&self.staged_samples, batch_peak)
    }

    fn push_48k_sample(&mut self, raw_sample: f32) {
        let sample = if self.active_high_pass {
            self.hp_filter.process(raw_sample)
        } else {
            raw_sample
        };
        match self.active_engine {
            NoiseEngine::Off => {
                self.staged_samples.push(sample);
            }
            NoiseEngine::Rnnoise => {
                // nnnoiseless expects i16 sized values in f32, not the usual -1 to 1 range
                self.frame_pending.push(sample * 32768.0);
                if self.frame_pending.len() == FRAME_SIZE {
                    let mut cleaned = [0.0f32; FRAME_SIZE];
                    self.rnnoise.process_frame(&mut cleaned, &self.frame_pending);
                    self.frame_pending.clear();
                    self.staged_samples.extend(cleaned.iter().map(|&out| out / 32768.0));
                }
            }
            NoiseEngine::DeepFilterNet => {
                let Some(df) = self.deep_filter.as_mut() else {
                    self.staged_samples.push(sample);
                    return;
                };
                self.frame_pending.push(sample);
                if self.frame_pending.len() == FRAME_SIZE {
                    let start = Instant::now();
                    let mut cleaned = [0.0f32; FRAME_SIZE];
                    let input_view = ArrayView2::from_shape((1, FRAME_SIZE), &self.frame_pending).unwrap();
                    let output_view = ArrayViewMut2::from_shape((1, FRAME_SIZE), &mut cleaned).unwrap();
                    let _ = df.process(input_view, output_view);
                    self.frame_pending.clear();
                    for s in &mut cleaned {
                        if !s.is_finite() {
                            *s = 0.0;
                        }
                    }
                    self.staged_samples.extend_from_slice(&cleaned);

                    let elapsed_ms = start.elapsed().as_secs_f32() * 1000.0;
                    self.df_smoothed_ms += (elapsed_ms - self.df_smoothed_ms) * 0.25;
                    // 480 samples is 10 ms of real time, leave headroom before frames miss their deadline
                    if self.df_smoothed_ms > 7.5 {
                        log::warn!(
                            "deepfilternet frame took {:.1} ms under heavy cpu load, falling back to rnnoise for 10s",
                            self.df_smoothed_ms
                        );
                        self.df_fallback_until = Some(Instant::now() + Duration::from_secs(10));
                    }
                }
            }
        }
    }
}

struct FeedState {
    samples: VecDeque<f32>,
    frac: f64,
    primed: bool,
    smoothed_fill: f64,
    ratio: f64,
    underruns: u64,
    overflow_drops: u64,
}

pub struct MicFeed {
    state: Mutex<FeedState>,
    sidetone_samples: Mutex<VecDeque<f32>>,
    target_device_id: Mutex<String>,
    device_changed: AtomicBool,
    muted: AtomicBool,
    gain_bits: AtomicU32,
    raw_peak_bits: AtomicU32,
    engine: AtomicU8,
    atten_lim_bits: AtomicU32,
    pf_beta_bits: AtomicU32,
    auto_default_mic: AtomicBool,
    high_pass_enabled: AtomicBool,
    voice_tone: AtomicU8,
    noise_gate_enabled: AtomicBool,
    noise_gate_threshold_bits: AtomicU32,
    compressor_enabled: AtomicBool,
    funny_mic_enabled: AtomicBool,
    sidetone_enabled: AtomicBool,
}

impl MicFeed {
    pub fn new() -> Self {
        MicFeed {
            state: Mutex::new(FeedState {
                samples: VecDeque::with_capacity(16384),
                frac: 0.0,
                primed: false,
                smoothed_fill: 0.0,
                ratio: 1.0,
                underruns: 0,
                overflow_drops: 0,
            }),
            sidetone_samples: Mutex::new(VecDeque::with_capacity(4096)),
            target_device_id: Mutex::new(String::new()),
            device_changed: AtomicBool::new(false),
            muted: AtomicBool::new(false),
            gain_bits: AtomicU32::new(1.0f32.to_bits()),
            raw_peak_bits: AtomicU32::new(0.0f32.to_bits()),
            engine: AtomicU8::new(NoiseEngine::Rnnoise as u8),
            atten_lim_bits: AtomicU32::new(100.0f32.to_bits()),
            pf_beta_bits: AtomicU32::new(0.0f32.to_bits()),
            auto_default_mic: AtomicBool::new(true),
            high_pass_enabled: AtomicBool::new(false),
            voice_tone: AtomicU8::new(VoiceTone::Natural as u8),
            noise_gate_enabled: AtomicBool::new(false),
            noise_gate_threshold_bits: AtomicU32::new(0.06f32.to_bits()),
            compressor_enabled: AtomicBool::new(false),
            funny_mic_enabled: AtomicBool::new(false),
            sidetone_enabled: AtomicBool::new(false),
        }
    }

    pub fn set_target_device(&self, device_id: String) {
        if let Ok(mut guard) = self.target_device_id.lock() {
            if *guard != device_id {
                *guard = device_id;
                self.device_changed.store(true, Ordering::Relaxed);
            }
        }
    }

    pub fn target_device(&self) -> String {
        self.target_device_id.lock().map(|g| g.clone()).unwrap_or_default()
    }

    pub fn set_muted(&self, muted: bool) {
        self.muted.store(muted, Ordering::Relaxed);
    }

    pub fn is_muted(&self) -> bool {
        self.muted.load(Ordering::Relaxed)
    }

    pub fn set_gain(&self, gain: f32) {
        self.gain_bits.store(gain.clamp(0.0, 2.0).to_bits(), Ordering::Relaxed);
    }

    pub fn gain(&self) -> f32 {
        f32::from_bits(self.gain_bits.load(Ordering::Relaxed))
    }

    pub fn set_engine(&self, engine: NoiseEngine) {
        self.engine.store(engine as u8, Ordering::Relaxed);
    }

    pub fn engine(&self) -> NoiseEngine {
        NoiseEngine::from_u8(self.engine.load(Ordering::Relaxed))
    }

    pub fn set_atten_lim_db(&self, lim_db: f32) {
        self.atten_lim_bits.store(lim_db.clamp(5.0, 100.0).to_bits(), Ordering::Relaxed);
    }

    pub fn atten_lim_db(&self) -> f32 {
        f32::from_bits(self.atten_lim_bits.load(Ordering::Relaxed))
    }

    pub fn set_post_filter_beta(&self, beta: f32) {
        self.pf_beta_bits.store(beta.clamp(0.0, 0.05).to_bits(), Ordering::Relaxed);
    }

    pub fn post_filter_beta(&self) -> f32 {
        f32::from_bits(self.pf_beta_bits.load(Ordering::Relaxed))
    }

    pub fn set_auto_default_mic(&self, enabled: bool) {
        self.auto_default_mic.store(enabled, Ordering::Relaxed);
    }

    pub fn auto_default_mic(&self) -> bool {
        self.auto_default_mic.load(Ordering::Relaxed)
    }

    pub fn set_high_pass_enabled(&self, enabled: bool) {
        self.high_pass_enabled.store(enabled, Ordering::Relaxed);
    }

    pub fn high_pass_enabled(&self) -> bool {
        self.high_pass_enabled.load(Ordering::Relaxed)
    }

    pub fn set_voice_tone(&self, tone: VoiceTone) {
        self.voice_tone.store(tone as u8, Ordering::Relaxed);
    }

    pub fn voice_tone(&self) -> VoiceTone {
        VoiceTone::from_u8(self.voice_tone.load(Ordering::Relaxed))
    }

    pub fn set_noise_gate_enabled(&self, enabled: bool) {
        self.noise_gate_enabled.store(enabled, Ordering::Relaxed);
    }

    pub fn noise_gate_enabled(&self) -> bool {
        self.noise_gate_enabled.load(Ordering::Relaxed)
    }

    pub fn set_noise_gate_threshold(&self, threshold: f32) {
        self.noise_gate_threshold_bits
            .store(threshold.clamp(0.005, 0.50).to_bits(), Ordering::Relaxed);
    }

    pub fn noise_gate_threshold(&self) -> f32 {
        f32::from_bits(self.noise_gate_threshold_bits.load(Ordering::Relaxed))
    }

    pub fn set_compressor_enabled(&self, enabled: bool) {
        self.compressor_enabled.store(enabled, Ordering::Relaxed);
    }

    pub fn compressor_enabled(&self) -> bool {
        self.compressor_enabled.load(Ordering::Relaxed)
    }

    pub fn set_funny_mic_enabled(&self, enabled: bool) {
        self.funny_mic_enabled.store(enabled, Ordering::Relaxed);
    }

    pub fn funny_mic_enabled(&self) -> bool {
        self.funny_mic_enabled.load(Ordering::Relaxed)
    }

    pub fn set_sidetone_enabled(&self, enabled: bool) {
        self.sidetone_enabled.store(enabled, Ordering::Relaxed);
        if !enabled {
            if let Ok(mut buf) = self.sidetone_samples.lock() {
                buf.clear();
            }
        }
    }

    pub fn sidetone_enabled(&self) -> bool {
        self.sidetone_enabled.load(Ordering::Relaxed)
    }

    // decay on read instead of resetting to zero so short transients show up and fall smoothly
    pub fn take_peak_level(&self) -> f32 {
        let raw = f32::from_bits(self.raw_peak_bits.load(Ordering::Relaxed));
        let decayed = raw * 0.72;
        self.raw_peak_bits.store(decayed.to_bits(), Ordering::Relaxed);
        (raw * self.gain()).clamp(0.0, 1.0)
    }

    fn record_raw_peak(&self, batch_peak: f32) {
        let current = f32::from_bits(self.raw_peak_bits.load(Ordering::Relaxed));
        if batch_peak > current {
            self.raw_peak_bits.store(batch_peak.to_bits(), Ordering::Relaxed);
        }
    }

    fn push_processed(&self, processed: &[f32]) {
        if processed.is_empty() {
            return;
        }
        let max_fill = (MAX_FILL_MS / 1000.0 * OUTPUT_RATE_HZ) as usize;
        let target_fill = (TARGET_FILL_MS / 1000.0 * OUTPUT_RATE_HZ) as usize;
        let mut state = self.state.lock().unwrap();
        state.samples.extend(processed.iter().copied());
        if state.samples.len() > max_fill {
            // snap back to target fill on a stall spike so we dont carry 120 ms of latency for the next minute
            let drop_to = if state.primed { target_fill } else { max_fill };
            let dropped = state.samples.len() - drop_to;
            state.samples.drain(..dropped);
            state.overflow_drops += dropped as u64;
            if state.primed {
                state.smoothed_fill = target_fill as f64;
            }
        }
        drop(state);

        if self.sidetone_enabled() {
            let gain = self.gain();
            if let Ok(mut sidetone) = self.sidetone_samples.lock() {
                sidetone.extend(processed.iter().map(|&s| soft_limit(s * gain)));
                let max_sidetone = (0.06 * OUTPUT_RATE_HZ) as usize;
                if sidetone.len() > max_sidetone {
                    let excess = sidetone.len() - max_sidetone;
                    sidetone.drain(..excess);
                }
            }
        }
    }

    fn reset_for_reconnect(&self) {
        self.raw_peak_bits.store(0.0f32.to_bits(), Ordering::Relaxed);
        let mut state = self.state.lock().unwrap();
        state.samples.clear();
        state.frac = 0.0;
        state.primed = false;
        state.ratio = 1.0;
    }

    pub fn pull(&self, out_bytes: &mut [u8]) {
        let target_fill = TARGET_FILL_MS / 1000.0 * OUTPUT_RATE_HZ;
        let muted = self.is_muted();
        let gain = self.gain();
        let mut state = self.state.lock().unwrap();

        for out_sample in out_bytes.chunks_exact_mut(2) {
            if !state.primed {
                if (state.samples.len() as f64) < target_fill {
                    out_sample.copy_from_slice(&0i16.to_le_bytes());
                    continue;
                }
                // capture runs before anything is attached so the backlog would otherwise play as stale audio
                let keep = target_fill as usize;
                if state.samples.len() > keep {
                    let excess = state.samples.len() - keep;
                    state.samples.drain(..excess);
                }
                state.primed = true;
                state.smoothed_fill = target_fill;
            }
            if state.samples.len() < 2 {
                state.primed = false;
                state.frac = 0.0;
                state.underruns += 1;
                out_sample.copy_from_slice(&0i16.to_le_bytes());
                continue;
            }

            let current = state.samples[0];
            let next = state.samples[1];
            let value = current + (next - current) * state.frac as f32;
            let pcm = if muted {
                0i16
            } else {
                let shaped = soft_limit(value * gain);
                (shaped * 32767.0).clamp(-32768.0, 32767.0) as i16
            };
            out_sample.copy_from_slice(&pcm.to_le_bytes());

            let fill = state.samples.len() as f64;
            state.smoothed_fill += (fill - state.smoothed_fill) * FILL_SMOOTHING;
            let fill_error = ((state.smoothed_fill - target_fill) / target_fill).clamp(-1.0, 1.0);
            // a fuller buffer means the mic is running ahead so we consume a little faster
            state.ratio = 1.0 + fill_error * MAX_RATIO_ADJUST;

            state.frac += state.ratio;
            while state.frac >= 1.0 && !state.samples.is_empty() {
                state.samples.pop_front();
                state.frac -= 1.0;
            }
        }
    }
}

pub fn spawn_sidetone_worker(feed: Arc<MicFeed>) {
    thread::spawn(move || {
        loop {
            if !feed.sidetone_enabled() {
                thread::sleep(Duration::from_millis(80));
                continue;
            }
            let stream_dead = Arc::new(AtomicBool::new(false));
            match open_sidetone_stream(Arc::clone(&feed), Arc::clone(&stream_dead)) {
                Ok(_stream) => {
                    while feed.sidetone_enabled() && !stream_dead.load(Ordering::Relaxed) {
                        thread::sleep(Duration::from_millis(80));
                    }
                }
                Err(err) => {
                    log::warn!("sidetone output error: {err}");
                    thread::sleep(Duration::from_millis(500));
                }
            }
        }
    });
}

fn open_sidetone_stream(feed: Arc<MicFeed>, stream_dead: Arc<AtomicBool>) -> Result<Stream, String> {
    let host = cpal::default_host();
    let device = host.default_output_device().ok_or("no default output device")?;
    let config = device.default_output_config().map_err(|e| e.to_string())?;
    let channels = config.channels() as usize;
    let out_rate = config.sample_rate() as f64;
    let step = OUTPUT_RATE_HZ / out_rate;
    let mut frac = 0.0f64;

    let on_error = move |err: cpal::Error| {
        log::warn!("sidetone playback error: {err}");
        stream_dead.store(true, Ordering::Relaxed);
    };

    let stream = match config.sample_format() {
        SampleFormat::F32 => device.build_output_stream(
            config.into(),
            move |data: &mut [f32], _: &_| {
                let Ok(mut buf) = feed.sidetone_samples.lock() else {
                    data.fill(0.0);
                    return;
                };
                for frame in data.chunks_exact_mut(channels) {
                    let sample = if buf.len() >= 2 {
                        let s0 = buf[0];
                        let s1 = buf[1];
                        let val = s0 + (s1 - s0) * frac as f32;
                        frac += step;
                        while frac >= 1.0 && !buf.is_empty() {
                            buf.pop_front();
                            frac -= 1.0;
                        }
                        val
                    } else {
                        buf.pop_front().unwrap_or(0.0)
                    };
                    frame.fill(sample);
                }
            },
            on_error,
            None,
        ),
        SampleFormat::I16 => device.build_output_stream(
            config.into(),
            move |data: &mut [i16], _: &_| {
                let Ok(mut buf) = feed.sidetone_samples.lock() else {
                    data.fill(0);
                    return;
                };
                for frame in data.chunks_exact_mut(channels) {
                    let sample = if buf.len() >= 2 {
                        let s0 = buf[0];
                        let s1 = buf[1];
                        let val = s0 + (s1 - s0) * frac as f32;
                        frac += step;
                        while frac >= 1.0 && !buf.is_empty() {
                            buf.pop_front();
                            frac -= 1.0;
                        }
                        val
                    } else {
                        buf.pop_front().unwrap_or(0.0)
                    };
                    let pcm = (sample * 32767.0).clamp(-32768.0, 32767.0) as i16;
                    frame.fill(pcm);
                }
            },
            on_error,
            None,
        ),
        other => return Err(format!("unsupported sidetone format {other:?}")),
    }
    .map_err(|e| e.to_string())?;

    stream.play().map_err(|e| e.to_string())?;
    Ok(stream)
}

pub fn spawn_capture_worker(feed: Arc<MicFeed>) {
    thread::spawn(move || {
        loop {
            let target_id = feed.target_device();
            feed.device_changed.store(false, Ordering::Relaxed);
            let stream_dead = Arc::new(AtomicBool::new(false));
            match open_stream(&target_id, Arc::clone(&feed), Arc::clone(&stream_dead)) {
                Ok(_stream) => {
                    while !stream_dead.load(Ordering::Relaxed)
                        && !feed.device_changed.load(Ordering::Relaxed)
                    {
                        thread::sleep(Duration::from_millis(150));
                    }
                    feed.reset_for_reconnect();
                }
                Err(err) => {
                    log::debug!("waiting for physical mic ({target_id}): {err}");
                    thread::sleep(Duration::from_secs(1));
                }
            }
        }
    });
}

fn select_input_config(device: &cpal::Device) -> Result<SupportedStreamConfig, String> {
    if let Ok(default) = device.default_input_config() {
        if default.sample_rate() == 48_000
            && matches!(default.sample_format(), SampleFormat::F32 | SampleFormat::I16)
        {
            return Ok(default);
        }
    }
    if let Ok(configs) = device.supported_input_configs() {
        for range in configs {
            if range.min_sample_rate() <= 48_000
                && range.max_sample_rate() >= 48_000
                && matches!(range.sample_format(), SampleFormat::F32 | SampleFormat::I16)
            {
                return Ok(range.with_sample_rate(48_000));
            }
        }
    }
    device.default_input_config().map_err(|err| err.to_string())
}

fn find_cpal_device(target_id: &str) -> Result<(cpal::Device, String), String> {
    let host = cpal::default_host();
    let mut fallback = None;
    for device in host.input_devices().map_err(|err| err.to_string())? {
        let label = device.description().map(|desc| desc.to_string()).unwrap_or_default();
        if crate::audio::is_voxis_virtual_mic(&label) {
            continue;
        }
        let dev_id = device.id().map(|id| id.id().to_string()).unwrap_or_default();
        if !target_id.is_empty() && (dev_id == target_id || label.contains(target_id)) {
            return Ok((device, label));
        }
        if fallback.is_none() {
            fallback = Some((device, label));
        }
    }
    if target_id.is_empty() {
        if let Some(first) = fallback {
            return Ok(first);
        }
    }
    Err(format!("input device {target_id} not available"))
}

fn open_stream(
    target_id: &str,
    feed: Arc<MicFeed>,
    stream_dead: Arc<AtomicBool>,
) -> Result<Stream, String> {
    let (device, label) = find_cpal_device(target_id)?;
    let config = select_input_config(&device)?;
    let channels = config.channels() as usize;
    let input_rate_hz = config.sample_rate() as f64;
    let initial_engine = feed.engine();
    log::info!(
        "capturing from {label}: {input_rate_hz} Hz, {channels} ch, {:?}, engine={}",
        config.sample_format(),
        initial_engine.as_str()
    );

    let mut filter = CaptureFilter::new(
        input_rate_hz,
        initial_engine,
        feed.atten_lim_db(),
        feed.post_filter_beta(),
    );
    let on_error = move |err: cpal::Error| {
        // -2004287484 is audclnt_e_device_invalidated when a usb mic is unplugged
        log::warn!("capture stream error: {err}");
        if err.to_string().contains("-2004287484") || matches!(err.kind(), cpal::ErrorKind::DeviceChanged) {
            stream_dead.store(true, Ordering::Relaxed);
        }
    };
    let stream = match config.sample_format() {
        SampleFormat::F32 => device.build_input_stream(
            config.into(),
            move |samples: &[f32], _: &_| {
                let (processed, peak) = filter.process_frames(
                    samples,
                    channels,
                    feed.engine(),
                    feed.atten_lim_db(),
                    feed.post_filter_beta(),
                    feed.high_pass_enabled(),
                    feed.voice_tone(),
                    feed.noise_gate_enabled(),
                    feed.noise_gate_threshold(),
                    feed.compressor_enabled(),
                    feed.gain(),
                    feed.funny_mic_enabled(),
                    |sample| sample,
                );
                feed.record_raw_peak(peak);
                feed.push_processed(processed);
            },
            on_error,
            None,
        ),
        SampleFormat::I16 => device.build_input_stream(
            config.into(),
            move |samples: &[i16], _: &_| {
                let (processed, peak) = filter.process_frames(
                    samples,
                    channels,
                    feed.engine(),
                    feed.atten_lim_db(),
                    feed.post_filter_beta(),
                    feed.high_pass_enabled(),
                    feed.voice_tone(),
                    feed.noise_gate_enabled(),
                    feed.noise_gate_threshold(),
                    feed.compressor_enabled(),
                    feed.gain(),
                    feed.funny_mic_enabled(),
                    |sample| sample as f32 / 32768.0,
                );
                feed.record_raw_peak(peak);
                feed.push_processed(processed);
            },
            on_error,
            None,
        ),
        other => return Err(format!("unsupported capture sample format {other:?}")),
    }
    .map_err(|err| err.to_string())?;

    stream.play().map_err(|err| err.to_string())?;
    Ok(stream)
}
