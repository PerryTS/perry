//! Audio capture for Android using AudioRecord via JNI.
//!
//! Architecture: We create an AudioRecord via JNI, spawn a Rust thread that
//! reads PCM data via JNI calls, and process (A-weight + RMS + dB) in Rust.
//! Results are stored in atomics, read lock-free by the main/UI thread.

use jni::objects::{JObject, JValue};
use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use crate::jni_bridge;

// =============================================================================
// Shared atomic state (same layout as macOS/iOS)
// =============================================================================

static CURRENT_DB: AtomicU64 = AtomicU64::new(0);
static CURRENT_PEAK: AtomicU64 = AtomicU64::new(0);

const WAVEFORM_SIZE: usize = 256;
static WAVEFORM_WRITE_INDEX: AtomicU64 = AtomicU64::new(0);
static mut WAVEFORM_BUFFER: [f64; WAVEFORM_SIZE] = [0.0; WAVEFORM_SIZE];

static RUNNING: AtomicBool = AtomicBool::new(false);

// =============================================================================
// A-weighting (48kHz coefficients, shared with macOS/iOS)
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
// Public API
// =============================================================================

extern "C" {
    fn js_string_from_bytes(ptr: *const u8, len: i32) -> i64;
}

pub fn start() -> i64 {
    if RUNNING.load(Ordering::Relaxed) {
        return 1;
    }

    // Check RECORD_AUDIO permission. PerryActivity requests all dangerous
    // permissions declared in the manifest at startup (before native code runs),
    // so this should already be granted by the time we get here.
    {
        let mut env = jni_bridge::get_env();
        let _ = env.push_local_frame(8);
        let activity = crate::widgets::get_activity(&mut env);
        let perm_str = env.new_string("android.permission.RECORD_AUDIO").unwrap();
        let result = env.call_static_method(
            "androidx/core/content/ContextCompat",
            "checkSelfPermission",
            "(Landroid/content/Context;Ljava/lang/String;)I",
            &[JValue::Object(&activity), JValue::Object(&perm_str.into())],
        );
        unsafe {
            env.pop_local_frame(&jni::objects::JObject::null());
        }

        let granted = result.map(|v| v.i().unwrap_or(-1)).unwrap_or(-1) == 0;
        if !granted {
            return 0;
        }
    }

    // Spawn a background thread that creates AudioRecord via JNI and reads data.
    // JNI calls must happen on a thread attached to the JVM.
    let vm = jni_bridge::get_vm().clone();

    RUNNING.store(true, Ordering::Relaxed);

    std::thread::spawn(move || {
        // Attach this thread to the JVM
        let mut env = vm
            .attach_current_thread_permanently()
            .expect("Failed to attach audio thread to JVM");

        let _ = env.push_local_frame(32);

        // Constants for AudioRecord
        // MediaRecorder.AudioSource.MIC = 1
        // AudioFormat.CHANNEL_IN_MONO = 16
        // AudioFormat.ENCODING_PCM_FLOAT = 4
        let audio_source: i32 = 1;
        let sample_rate: i32 = 48000;
        let channel_config: i32 = 16; // CHANNEL_IN_MONO
        let audio_format: i32 = 4; // ENCODING_PCM_FLOAT
        let buffer_size_frames: i32 = 1024;

        // Get minimum buffer size
        let audio_record_cls = match env.find_class("android/media/AudioRecord") {
            Ok(cls) => cls,
            Err(_) => {
                RUNNING.store(false, Ordering::Relaxed);
                return;
            }
        };

        let min_buf = env.call_static_method(
            &audio_record_cls,
            "getMinBufferSize",
            "(III)I",
            &[
                JValue::Int(sample_rate),
                JValue::Int(channel_config),
                JValue::Int(audio_format),
            ],
        );
        let min_buffer_size = match min_buf {
            Ok(v) => v.i().unwrap_or(4096).max(buffer_size_frames * 4), // float = 4 bytes
            Err(_) => {
                RUNNING.store(false, Ordering::Relaxed);
                return;
            }
        };

        // Create AudioRecord
        let record = env.new_object(
            "android/media/AudioRecord",
            "(IIIII)V",
            &[
                JValue::Int(audio_source),
                JValue::Int(sample_rate),
                JValue::Int(channel_config),
                JValue::Int(audio_format),
                JValue::Int(min_buffer_size),
            ],
        );
        let record = match record {
            Ok(r) => r,
            Err(_) => {
                RUNNING.store(false, Ordering::Relaxed);
                return;
            }
        };

        // Check state (1 = STATE_INITIALIZED)
        let state = env.call_method(&record, "getState", "()I", &[]);
        if state.map(|v| v.i().unwrap_or(0)).unwrap_or(0) != 1 {
            RUNNING.store(false, Ordering::Relaxed);
            return;
        }

        // Start recording
        let _ = env.call_method(&record, "startRecording", "()V", &[]);

        // Create a float array for reading samples
        let float_array = env
            .new_float_array(buffer_size_frames)
            .expect("Failed to create float array");

        let mut filter_state = AWeightState::new();
        let mut ema_db: f64 = 0.0;

        // Read loop
        while RUNNING.load(Ordering::Relaxed) {
            // AudioRecord.read(float[], int, int, int) — READ_BLOCKING = 0
            let float_array_obj = unsafe { JObject::from_raw(float_array.as_raw()) };
            let read_result = env.call_method(
                &record,
                "read",
                "([FIII)I",
                &[
                    JValue::Object(&float_array_obj),
                    JValue::Int(0),
                    JValue::Int(buffer_size_frames),
                    JValue::Int(0), // READ_BLOCKING
                ],
            );
            std::mem::forget(float_array_obj); // Don't drop — we still own float_array

            let frames_read = match read_result {
                Ok(v) => v.i().unwrap_or(0),
                Err(_) => break,
            };

            if frames_read <= 0 {
                continue;
            }

            // Copy float data from Java array to Rust
            let n = frames_read as usize;
            let mut samples = vec![0.0f32; n];
            let _ = env.get_float_array_region(&float_array, 0, &mut samples);

            // Process: A-weight + RMS + dB
            let mut sum_sq = 0.0f64;
            let mut peak = 0.0f32;

            for i in 0..n {
                let s = samples[i];
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

        // Stop and release
        let _ = env.call_method(&record, "stop", "()V", &[]);
        let _ = env.call_method(&record, "release", "()V", &[]);
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

pub fn get_waveform(_count: f64) -> f64 {
    // Waveform array creation not available on Android yet (js_array_create missing).
    // Return 0.0 (callers handle this gracefully).
    0.0
}

pub fn get_device_model() -> i64 {
    let mut env = jni_bridge::get_env();
    let _ = env.push_local_frame(8);

    // android.os.Build.MODEL
    let build_cls = env.find_class("android/os/Build").ok();
    let model = build_cls
        .and_then(|cls| {
            env.get_static_field(&cls, "MODEL", "Ljava/lang/String;")
                .ok()
                .and_then(|v| v.l().ok())
                .and_then(|obj| {
                    if obj.is_null() {
                        return None;
                    }
                    let jstr: jni::objects::JString = obj.into();
                    env.get_string(&jstr).ok().map(|s| String::from(s))
                })
        })
        .unwrap_or_else(|| "Unknown".to_string());

    unsafe {
        env.pop_local_frame(&jni::objects::JObject::null());
    }

    let bytes = model.as_bytes();
    unsafe { js_string_from_bytes(bytes.as_ptr(), bytes.len() as i32) }
}
