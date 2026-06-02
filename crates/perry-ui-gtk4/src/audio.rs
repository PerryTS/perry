//! Audio capture for Linux using PulseAudio simple API.
//!
//! Uses raw C FFI to libpulse-simple. Link with: -lpulse-simple -lpulse
//! Falls back gracefully if PulseAudio is not available.

use std::fs::File;
use std::io::Write;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;

// =============================================================================
// Shared atomic state
// =============================================================================

static CURRENT_DB: AtomicU64 = AtomicU64::new(0);
static CURRENT_PEAK: AtomicU64 = AtomicU64::new(0);

const WAVEFORM_SIZE: usize = 256;
static WAVEFORM_WRITE_INDEX: AtomicU64 = AtomicU64::new(0);
static mut WAVEFORM_BUFFER: [f64; WAVEFORM_SIZE] = [0.0; WAVEFORM_SIZE];

static RUNNING: AtomicBool = AtomicBool::new(false);

static AUDIO_CALLBACK: Mutex<Option<f64>> = Mutex::new(None);

// Recording state
static RECORDING: AtomicBool = AtomicBool::new(false);
static RECORDED_SAMPLES: Mutex<Vec<f32>> = Mutex::new(Vec::new());
static OUTPUT_FILENAME: Mutex<String> = Mutex::new(String::new());
const SAMPLE_RATE: u32 = 48000;

// =============================================================================
// A-weighting (48kHz coefficients)
// =============================================================================

struct AWeightState {
    sections: [[f64; 4]; 3],
}

impl AWeightState {
    fn new() -> Self {
        AWeightState {
            sections: [[0.0; 4]; 3],
        }
    }
}

const A_WEIGHT_SOS: [[f64; 6]; 3] = [
    [
        1.0,
        -2.0,
        1.0,
        1.0,
        -1.9746716508129498,
        0.97504628855498883,
    ],
    [
        1.0,
        -2.0,
        1.0,
        1.0,
        -1.1440825051498020,
        0.20482985688498268,
    ],
    [
        0.24649652853975498,
        -0.49299305707950996,
        0.24649652853975498,
        1.0,
        -0.48689808685150487,
        0.0,
    ],
];
const A_WEIGHT_GAIN: f64 = 0.11310782960598924;

fn a_weight_filter(sample: f64, state: &mut AWeightState) -> f64 {
    let mut x = sample * A_WEIGHT_GAIN;
    for (i, sos) in A_WEIGHT_SOS.iter().enumerate() {
        let b0 = sos[0];
        let b1 = sos[1];
        let b2 = sos[2];
        let a1 = sos[4];
        let a2 = sos[5];
        let s = &mut state.sections[i];
        let y = b0 * x + b1 * s[0] + b2 * s[1] - a1 * s[2] - a2 * s[3];
        s[1] = s[0];
        s[0] = x;
        s[3] = s[2];
        s[2] = y;
        x = y;
    }
    x
}

// =============================================================================
// PulseAudio simple API — raw C FFI declarations
// =============================================================================

// We use dlopen to load libpulse-simple at runtime so the app doesn't hard-fail
// on systems without PulseAudio installed.

#[repr(C)]
struct PaSampleSpec {
    format: i32, // pa_sample_format_t
    rate: u32,
    channels: u8,
}

// pa_sample_format_t values
const PA_SAMPLE_FLOAT32LE: i32 = 5;

// pa_stream_direction_t
const PA_STREAM_RECORD: i32 = 2;

type PaSimple = *mut std::ffi::c_void;

extern "C" {
    fn pa_simple_new(
        server: *const std::ffi::c_char, // NULL for default
        name: *const std::ffi::c_char,   // application name
        dir: i32,                        // PA_STREAM_RECORD
        dev: *const std::ffi::c_char,    // NULL for default device
        stream_name: *const std::ffi::c_char,
        ss: *const PaSampleSpec,
        map: *const std::ffi::c_void,  // NULL for default channel map
        attr: *const std::ffi::c_void, // NULL for default buffering
        error: *mut i32,
    ) -> PaSimple;

    fn pa_simple_read(
        s: PaSimple,
        data: *mut std::ffi::c_void,
        bytes: usize,
        error: *mut i32,
    ) -> i32;

    fn pa_simple_free(s: PaSimple);
}

// =============================================================================
// WAV file utilities
// =============================================================================

fn write_u32_le(writer: &mut File, value: u32) -> std::io::Result<()> {
    let bytes = [
        (value & 0xFF) as u8,
        ((value >> 8) & 0xFF) as u8,
        ((value >> 16) & 0xFF) as u8,
        ((value >> 24) & 0xFF) as u8,
    ];
    writer.write_all(&bytes)?;
    Ok(())
}

fn write_u16_le(writer: &mut File, value: u16) -> std::io::Result<()> {
    writer.write_all(&[(value & 0xFF) as u8, ((value >> 8) & 0xFF) as u8])?;
    Ok(())
}

fn write_wav_header(writer: &mut File, num_samples: u32) -> std::io::Result<()> {
    let bits_per_sample = 16;
    let channels = 1u16;
    let byte_rate = (SAMPLE_RATE * u32::from(channels) * bits_per_sample) / 8;
    let block_align = (u32::from(channels) * bits_per_sample) / 8;
    let data_size = num_samples * u32::from(channels) * (bits_per_sample / 8);
    let chunk_size = 36 + data_size;

    writer.write_all(b"RIFF")?;
    write_u32_le(writer, chunk_size)?;
    writer.write_all(b"WAVE")?;
    writer.write_all(b"fmt ")?;
    write_u32_le(writer, 16)?;
    write_u16_le(writer, 1)?;
    write_u16_le(writer, channels)?;
    write_u32_le(writer, SAMPLE_RATE)?;
    write_u32_le(writer, byte_rate)?;
    write_u16_le(writer, block_align as u16)?;
    write_u16_le(writer, bits_per_sample as u16)?;
    writer.write_all(b"data")?;
    write_u32_le(writer, data_size)?;
    Ok(())
}

fn write_wav_samples(writer: &mut File, samples: &[f32]) -> std::io::Result<()> {
    for &sample in samples {
        let int_sample = (sample * i16::MAX as f32)
            .max(i16::MIN as f32)
            .min(i16::MAX as f32) as i16;
        writer.write_all(&int_sample.to_le_bytes())?;
    }
    Ok(())
}

// =============================================================================
// Public API
// =============================================================================

extern "C" {
    fn js_string_from_bytes(ptr: *const u8, len: i32) -> i64;
    fn js_array_create() -> i64;
    fn js_array_push_f64(array_ptr: i64, value: f64);
    fn js_closure_call2(closure: *const u8, arg1: f64, arg2: f64);
    fn js_nanbox_get_pointer(value: f64) -> i64;
    fn js_nanbox_pointer(ptr: i64) -> f64;
}

pub fn set_output_filename(filename: &str) {
    let mut slot = OUTPUT_FILENAME.lock().unwrap();
    slot.clear();
    slot.push_str(filename);
}

pub fn start_recording() {
    RECORDED_SAMPLES.lock().unwrap().clear();
    RECORDING.store(true, Ordering::Relaxed);
}

pub fn stop_recording() {
    RECORDING.store(false, Ordering::Relaxed);

    let filename = {
        let mut slot = OUTPUT_FILENAME.lock().unwrap();
        if slot.is_empty() {
            return;
        }
        std::mem::take(&mut *slot)
    };

    let samples = std::mem::take(&mut *RECORDED_SAMPLES.lock().unwrap());
    if samples.is_empty() {
        return;
    }

    if let Ok(mut file) = File::create(&filename) {
        let _ = write_wav_header(&mut file, samples.len() as u32);
        let _ = write_wav_samples(&mut file, &samples);
    }
}

pub fn start() -> i64 {
    if RUNNING.load(Ordering::Relaxed) {
        return 1;
    }

    RUNNING.store(true, Ordering::Relaxed);

    std::thread::spawn(|| {
        let sample_rate: u32 = 48000;
        let buffer_frames: usize = 1024;

        let ss = PaSampleSpec {
            format: PA_SAMPLE_FLOAT32LE,
            rate: sample_rate,
            channels: 1,
        };

        let app_name = c"perry-dbmeter";
        let stream_name = c"capture";

        let mut error: i32 = 0;
        let pa = unsafe {
            pa_simple_new(
                std::ptr::null(),
                app_name.as_ptr(),
                PA_STREAM_RECORD,
                std::ptr::null(),
                stream_name.as_ptr(),
                &ss,
                std::ptr::null(),
                std::ptr::null(),
                &mut error,
            )
        };

        if pa.is_null() {
            eprintln!("[audio] Failed to open PulseAudio stream (error {})", error);
            RUNNING.store(false, Ordering::Relaxed);
            return;
        }

        eprintln!(
            "[audio] PulseAudio capture started ({}Hz mono)",
            sample_rate
        );

        let mut filter_state = AWeightState::new();
        let mut ema_db: f64 = 0.0;
        let mut buf = vec![0.0f32; buffer_frames];

        while RUNNING.load(Ordering::Relaxed) {
            let mut err: i32 = 0;
            let ret = unsafe {
                pa_simple_read(
                    pa,
                    buf.as_mut_ptr() as *mut std::ffi::c_void,
                    buffer_frames * std::mem::size_of::<f32>(),
                    &mut err,
                )
            };

            if ret < 0 {
                eprintln!("[audio] PulseAudio read error: {}", err);
                break;
            }

            // Process: A-weight + RMS + dB
            let n = buffer_frames;

            // Record samples if recording is enabled
            if RECORDING.load(Ordering::Relaxed) {
                RECORDED_SAMPLES.lock().unwrap().extend_from_slice(&buf);
            }

            invoke_audio_callback(buf.as_ptr(), buffer_frames);

            let mut sum_sq = 0.0f64;
            let mut peak = 0.0f32;

            for i in 0..n {
                let s = buf[i];
                let abs_s = s.abs();
                if abs_s > peak {
                    peak = abs_s;
                }
                let weighted = a_weight_filter(s as f64, &mut filter_state);
                sum_sq += weighted * weighted;
            }

            let rms = (sum_sq / n as f64).sqrt();
            let db_raw = if rms > 1.0e-10 {
                20.0 * rms.log10() + 110.0
            } else {
                0.0
            };
            let db_clamped = db_raw.max(0.0).min(140.0);

            let dt = n as f64 / sample_rate as f64;
            let tau = 0.125;
            let alpha = 1.0 - (-dt / tau).exp();
            ema_db += alpha * (db_clamped - ema_db);

            CURRENT_DB.store(ema_db.to_bits(), Ordering::Relaxed);
            CURRENT_PEAK.store((peak as f64).to_bits(), Ordering::Relaxed);

            let idx = WAVEFORM_WRITE_INDEX.load(Ordering::Relaxed) as usize % WAVEFORM_SIZE;
            unsafe {
                WAVEFORM_BUFFER[idx] = ema_db;
            }
            WAVEFORM_WRITE_INDEX.store((idx + 1) as u64, Ordering::Relaxed);
        }

        unsafe {
            pa_simple_free(pa);
        }
        eprintln!("[audio] PulseAudio capture stopped");
    });

    1
}

pub fn stop() {
    RUNNING.store(false, Ordering::Relaxed);
}

pub fn get_level() -> f64 {
    f64::from_bits(CURRENT_DB.load(Ordering::Relaxed))
}

pub fn get_peak() -> f64 {
    f64::from_bits(CURRENT_PEAK.load(Ordering::Relaxed))
}

pub fn get_waveform(count: f64) -> f64 {
    let n = (count as usize).min(WAVEFORM_SIZE);
    let write_idx = WAVEFORM_WRITE_INDEX.load(Ordering::Relaxed) as usize;
    unsafe {
        let array = js_array_create();
        for i in 0..n {
            let idx = (write_idx + WAVEFORM_SIZE - n + i) % WAVEFORM_SIZE;
            js_array_push_f64(array, WAVEFORM_BUFFER[idx]);
        }
        f64::from_bits(array as u64)
    }
}

pub fn get_device_model() -> i64 {
    // On Linux, use hostname as device identifier
    let mut hostname = [0u8; 256];
    let model = unsafe {
        if libc::gethostname(
            hostname.as_mut_ptr() as *mut std::ffi::c_char,
            hostname.len(),
        ) == 0
        {
            let len = hostname
                .iter()
                .position(|&b| b == 0)
                .unwrap_or(hostname.len());
            std::str::from_utf8_unchecked(&hostname[..len]).to_string()
        } else {
            "Linux".to_string()
        }
    };
    unsafe { js_string_from_bytes(model.as_ptr(), model.len() as i32) }
}

pub fn register_audio_callback(callback: f64) {
    *AUDIO_CALLBACK.lock().unwrap() = Some(callback);
}

pub fn unregister_audio_callback() {
    *AUDIO_CALLBACK.lock().unwrap() = None;
}

pub fn invoke_audio_callback(samples_ptr: *const f32, num_samples: usize) {
    let callback_opt = *AUDIO_CALLBACK.lock().unwrap();
    if callback_opt.is_none() {
        return;
    }
    let callback = callback_opt.unwrap();
    let callback_ptr = unsafe { js_nanbox_get_pointer(callback) } as *const u8;

    let samples_val = unsafe { js_nanbox_pointer(samples_ptr as i64) };
    let num_samples_val = num_samples as f64;

    unsafe {
        js_closure_call2(callback_ptr, samples_val, num_samples_val);
    }
}
