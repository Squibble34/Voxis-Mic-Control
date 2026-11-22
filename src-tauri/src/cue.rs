use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::SampleFormat;

const CUE_RATE_HZ: f64 = 48_000.0;
const WAV_HEADER_BYTES: usize = 44;
// wasapi still has a few ms queued after the last callback, dropping the stream right away clips the tail
const STREAM_DRAIN_TAIL: Duration = Duration::from_millis(80);

static MUTE_WAV: &[u8] = include_bytes!("../sounds/mute.wav");
static UNMUTE_WAV: &[u8] = include_bytes!("../sounds/unmute.wav");

static MUTE_SAMPLES: OnceLock<Vec<f32>> = OnceLock::new();
static UNMUTE_SAMPLES: OnceLock<Vec<f32>> = OnceLock::new();
static LATEST_CUE_ID: AtomicU64 = AtomicU64::new(0);
static SOUND_ENABLED: AtomicBool = AtomicBool::new(true);
static SOUND_VOLUME_BITS: AtomicU32 = AtomicU32::new(70.0_f32.to_bits());

pub fn set_settings(enabled: bool, volume_percent: f32) {
    SOUND_ENABLED.store(enabled, Ordering::Relaxed);
    SOUND_VOLUME_BITS.store(volume_percent.clamp(0.0, 100.0).to_bits(), Ordering::Relaxed);
}

pub fn play_if_enabled(muted: bool) {
    if !SOUND_ENABLED.load(Ordering::Relaxed) {
        return;
    }
    let volume = f32::from_bits(SOUND_VOLUME_BITS.load(Ordering::Relaxed));
    play(muted, volume);
}

fn decode_cue(wav: &[u8]) -> Vec<f32> {
    wav[WAV_HEADER_BYTES..]
        .chunks_exact(2)
        .map(|pair| i16::from_le_bytes([pair[0], pair[1]]) as f32 / 32768.0)
        .collect()
}

struct CueVoice {
    samples: &'static [f32],
    position: f64,
    step: f64,
    gain: f32,
    cue_id: u64,
    finished: Arc<AtomicBool>,
}

impl CueVoice {
    fn fill<T: Copy>(&mut self, out: &mut [T], channels: usize, convert: impl Fn(f32) -> T) {
        let superseded = LATEST_CUE_ID.load(Ordering::Relaxed) != self.cue_id;
        for frame in out.chunks_exact_mut(channels) {
            let index = self.position as usize;
            let value = if superseded || index + 1 >= self.samples.len() {
                self.finished.store(true, Ordering::Relaxed);
                0.0
            } else {
                let frac = (self.position - index as f64) as f32;
                let current = self.samples[index];
                let next = self.samples[index + 1];
                self.position += self.step;
                (current + (next - current) * frac) * self.gain
            };
            frame.fill(convert(value));
        }
    }
}

// newer cues bump the active id to cut off whatever was still playing so mute and unmute never overlap
pub fn play(muted: bool, volume_percent: f32) {
    let gain = (volume_percent / 100.0).clamp(0.0, 1.0);
    if gain <= 0.0001 {
        return;
    }
    let samples: &'static [f32] = if muted {
        MUTE_SAMPLES.get_or_init(|| decode_cue(MUTE_WAV))
    } else {
        UNMUTE_SAMPLES.get_or_init(|| decode_cue(UNMUTE_WAV))
    };
    let cue_id = LATEST_CUE_ID.fetch_add(1, Ordering::Relaxed) + 1;

    thread::spawn(move || {
        if let Err(err) = play_blocking(samples, gain, cue_id) {
            log::warn!("mute cue playback failed: {err}");
        }
    });
}

fn play_blocking(samples: &'static [f32], gain: f32, cue_id: u64) -> Result<(), String> {
    let host = cpal::default_host();
    let device = host.default_output_device().ok_or("no default output device")?;
    let output_config = device.default_output_config().map_err(|e| e.to_string())?;
    let channels = output_config.channels() as usize;
    let output_rate_hz = output_config.sample_rate() as f64;
    let sample_format = output_config.sample_format();

    let finished = Arc::new(AtomicBool::new(false));
    let mut voice = CueVoice {
        samples,
        position: 0.0,
        step: CUE_RATE_HZ / output_rate_hz,
        gain,
        cue_id,
        finished: Arc::clone(&finished),
    };
    let on_error = |err: cpal::Error| log::warn!("mute cue stream error: {err}");

    let stream = match sample_format {
        SampleFormat::F32 => device.build_output_stream(
            output_config.into(),
            move |samples_out: &mut [f32], _: &_| voice.fill(samples_out, channels, |value| value),
            on_error,
            None,
        ),
        SampleFormat::I16 => device.build_output_stream(
            output_config.into(),
            move |samples_out: &mut [i16], _: &_| {
                voice.fill(samples_out, channels, |value| (value * 32767.0).clamp(-32768.0, 32767.0) as i16)
            },
            on_error,
            None,
        ),
        other => return Err(format!("unsupported output format {other:?}")),
    }
    .map_err(|e| e.to_string())?;

    stream.play().map_err(|e| e.to_string())?;

    let give_up_at = Instant::now() + Duration::from_secs(3);
    while !finished.load(Ordering::Relaxed) && Instant::now() < give_up_at {
        thread::sleep(Duration::from_millis(10));
    }
    thread::sleep(STREAM_DRAIN_TAIL);
    Ok(())
}
