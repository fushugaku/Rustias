use cpal::{
    FromSample, SizedSample,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

const SOURCE_RATE: u32 = 48_000;
const MAX_LIVE_FRAMES: usize = SOURCE_RATE as usize * 2;
const LIVE_START_FRAMES: usize = 4_800;

struct Playback {
    live: VecDeque<[i32; 2]>,
    preview: VecDeque<[i32; 2]>,
    preview_active: bool,
    live_active: bool,
    live_enabled: bool,
    phase: u64,
    gain: f32,
    callbacks: u64,
    audible_frames: u64,
    peak: f32,
    error: Option<String>,
}
impl Default for Playback {
    fn default() -> Self {
        Self {
            live: VecDeque::new(),
            preview: VecDeque::new(),
            preview_active: false,
            live_active: false,
            live_enabled: true,
            phase: 0,
            gain: 0.3,
            callbacks: 0,
            audible_frames: 0,
            peak: 0.0,
            error: None,
        }
    }
}
impl Playback {
    fn next(&mut self, output_rate: u32) -> [f32; 2] {
        if !self.preview_active && !self.live_active {
            self.live_active = self.live_enabled && self.live.len() >= LIVE_START_FRAMES;
        }
        let queue = if self.preview_active {
            &mut self.preview
        } else if self.live_active {
            &mut self.live
        } else {
            return [0.0; 2];
        };
        let Some(&first) = queue.front() else {
            self.preview_active = false;
            self.live_active = false;
            self.phase = 0;
            return [0.0; 2];
        };
        // Conversion only at the audio-device boundary. Stored Q31/WAV data
        // stays at the native 48 kHz rate and is never rescaled per patch.
        let second = queue.get(1).copied().unwrap_or(first);
        let fraction = self.phase as f64 / output_rate as f64;
        let frame = std::array::from_fn(|channel| {
            let sample =
                first[channel] as f64 + (second[channel] as f64 - first[channel] as f64) * fraction;
            (sample / 2_147_483_648.0 * self.gain as f64) as f32
        });
        self.phase += SOURCE_RATE as u64;
        let consumed = self.phase / output_rate as u64;
        self.phase %= output_rate as u64;
        for _ in 0..consumed {
            queue.pop_front();
        }
        if queue.is_empty() {
            self.preview_active = false;
            self.live_active = false;
            self.phase = 0;
        }
        frame
    }
    fn stop(&mut self) {
        self.live.clear();
        self.preview.clear();
        self.live_active = false;
        self.preview_active = false;
        self.phase = 0;
    }
}

/// Bounded producer port used by the engine thread, independent of UI polling.
#[derive(Clone)]
pub struct LiveInput(Arc<Mutex<Playback>>);
impl LiveInput {
    pub fn push(&self, frames: impl IntoIterator<Item = [i32; 2]>) {
        let mut state = self.0.lock().unwrap();
        if state.preview_active || !state.live_enabled {
            return;
        }
        for frame in frames {
            if state.live.len() == MAX_LIVE_FRAMES {
                state.live.pop_front();
            }
            state.live.push_back(frame);
        }
    }
    pub fn clear(&self) {
        self.0.lock().unwrap().stop();
    }
    pub fn pause(&self) {
        let mut state = self.0.lock().unwrap();
        state.stop();
        state.live_enabled = false;
    }
    pub fn resume(&self) {
        self.0.lock().unwrap().live_enabled = true;
    }
}

#[derive(Clone, Debug)]
pub struct PlaybackStatus {
    pub device: String,
    pub sample_rate: u32,
    pub channels: u16,
    pub callbacks: u64,
    pub audible_frames: u64,
    pub buffered_frames: usize,
    pub preview_playing: bool,
    pub peak: f32,
    pub error: Option<String>,
}

pub struct Player {
    state: Arc<Mutex<Playback>>,
    device: String,
    config: cpal::StreamConfig,
    _stream: cpal::Stream,
}
impl Player {
    pub fn new() -> Result<Self, String> {
        let host = cpal::default_host();
        let device = host.default_output_device().ok_or("Нет аудиовыхода")?;
        Self::open(device)
    }
    fn open(device: cpal::Device) -> Result<Self, String> {
        // Use the device's supported default, including mono, multichannel,
        // integer and 44.1 kHz outputs, instead of requiring stereo F32/48k.
        let supported = device.default_output_config().map_err(|e| e.to_string())?;
        let config = supported.config();
        if config.channels == 0 || config.sample_rate == 0 {
            return Err("Некорректная конфигурация аудиовыхода".into());
        }
        let state = Arc::new(Mutex::new(Playback::default()));
        macro_rules! build {
            ($sample:ty) => {
                build_stream::<$sample>(&device, &config, state.clone())?
            };
        }
        let stream = match supported.sample_format() {
            cpal::SampleFormat::F32 => build!(f32),
            cpal::SampleFormat::F64 => build!(f64),
            cpal::SampleFormat::I8 => build!(i8),
            cpal::SampleFormat::I16 => build!(i16),
            cpal::SampleFormat::I24 => build!(cpal::I24),
            cpal::SampleFormat::I32 => build!(i32),
            cpal::SampleFormat::I64 => build!(i64),
            cpal::SampleFormat::U8 => build!(u8),
            cpal::SampleFormat::U16 => build!(u16),
            cpal::SampleFormat::U24 => build!(cpal::U24),
            cpal::SampleFormat::U32 => build!(u32),
            cpal::SampleFormat::U64 => build!(u64),
            other => return Err(format!("Неподдерживаемый формат аудиовыхода: {other:?}")),
        };
        stream.play().map_err(|e| e.to_string())?;
        Ok(Self {
            state,
            device: device.to_string(),
            config,
            _stream: stream,
        })
    }
    pub fn live_input(&self) -> LiveInput {
        LiveInput(self.state.clone())
    }
    pub fn play(&self, frames: Vec<[i32; 2]>) {
        let mut state = self.state.lock().unwrap();
        state.stop();
        state.preview = frames.into();
        state.preview_active = !state.preview.is_empty();
    }
    pub fn stop(&self) {
        self.live_input().pause();
    }
    pub fn gain(&self, value: f32) {
        self.state.lock().unwrap().gain = value.clamp(0.0, 1.0);
    }
    pub fn status(&self) -> PlaybackStatus {
        let state = self.state.lock().unwrap();
        PlaybackStatus {
            device: self.device.clone(),
            sample_rate: self.config.sample_rate,
            channels: self.config.channels,
            callbacks: state.callbacks,
            audible_frames: state.audible_frames,
            buffered_frames: state.live.len() + state.preview.len(),
            preview_playing: state.preview_active,
            peak: state.peak,
            error: state.error.clone(),
        }
    }
}

fn build_stream<T: SizedSample + FromSample<f32>>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    state: Arc<Mutex<Playback>>,
) -> Result<cpal::Stream, String> {
    let errors = state.clone();
    let channels = config.channels as usize;
    let rate = config.sample_rate;
    device
        .build_output_stream(
            config.clone(),
            move |data: &mut [T], _| {
                data.fill(T::EQUILIBRIUM);
                let Ok(mut state) = state.try_lock() else {
                    return;
                };
                state.callbacks += 1;
                state.peak = 0.0;
                for output in data.chunks_exact_mut(channels) {
                    let frame = state.next(rate);
                    let peak = frame[0].abs().max(frame[1].abs());
                    state.peak = state.peak.max(peak);
                    if peak > 0.0 {
                        state.audible_frames += 1;
                    }
                    if channels == 1 {
                        output[0] = T::from_sample((frame[0] + frame[1]) * 0.5);
                    } else {
                        output[0] = T::from_sample(frame[0]);
                        output[1] = T::from_sample(frame[1]);
                    }
                }
            },
            move |error| {
                let message = format!("Аудиовыход: {error}");
                eprintln!("{message}");
                errors.lock().unwrap().error = Some(message);
            },
            None,
        )
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_preserves_q31_stereo_and_stops() {
        let mut state = Playback {
            gain: 1.0,
            preview_active: true,
            preview: [[i32::MIN, i32::MAX], [1 << 30, -(1 << 30)]].into(),
            ..Default::default()
        };
        assert_eq!(state.next(48_000), [-1.0, 1.0]);
        assert_eq!(state.next(48_000), [0.5, -0.5]);
        assert_eq!(state.next(48_000), [0.0; 2]);
        assert!(!state.preview_active);
    }
    #[test]
    fn output_rate_changes_duration_without_changing_pitch_clock() {
        for rate in [44_100, 48_000, 96_000] {
            let mut state = Playback {
                preview: vec![[1 << 30; 2]; 48_000].into(),
                preview_active: true,
                gain: 1.0,
                ..Default::default()
            };
            let mut count = 0;
            while state.preview_active {
                assert_eq!(state.next(rate), [0.5; 2]);
                count += 1;
            }
            assert_eq!(count, rate);
        }
    }
    #[test]
    fn keyboard_stream_reaches_output_and_buffer_is_bounded() {
        let state = Arc::new(Mutex::new(Playback {
            gain: 1.0,
            ..Default::default()
        }));
        let input = LiveInput(state.clone());
        input.push(vec![[1 << 30, -(1 << 30)]; MAX_LIVE_FRAMES + 10]);
        assert_eq!(state.lock().unwrap().live.len(), MAX_LIVE_FRAMES);
        assert_eq!(state.lock().unwrap().next(48_000), [0.5, -0.5]);
        input.clear();
        assert_eq!(state.lock().unwrap().next(48_000), [0.0; 2]);
    }
    #[test]
    fn preview_is_not_contaminated_by_live_input() {
        let state = Arc::new(Mutex::new(Playback {
            gain: 1.0,
            preview_active: true,
            preview: [[1 << 30; 2]].into(),
            ..Default::default()
        }));
        LiveInput(state.clone()).push([[i32::MIN; 2]; LIVE_START_FRAMES]);
        let mut state = state.lock().unwrap();
        assert!(state.live.is_empty());
        assert_eq!(state.next(48_000), [0.5; 2]);
    }
    #[test]
    fn stop_stays_silent_until_the_next_performance() {
        let state = Arc::new(Mutex::new(Playback {
            gain: 1.0,
            ..Default::default()
        }));
        let input = LiveInput(state.clone());
        input.pause();
        input.push(vec![[1 << 30; 2]; LIVE_START_FRAMES]);
        assert!(state.lock().unwrap().live.is_empty());
        assert_eq!(state.lock().unwrap().next(48_000), [0.0; 2]);
        input.resume();
        input.push(vec![[1 << 30; 2]; LIVE_START_FRAMES]);
        assert_eq!(state.lock().unwrap().next(48_000), [0.5; 2]);
    }
}
