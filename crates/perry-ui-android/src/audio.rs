//! Audio capture for Android using AudioRecord via JNI.
//!
//! Architecture: We create an AudioRecord via JNI, spawn a Rust thread that
//! reads PCM data via JNI calls, and process (A-weight + RMS + dB) in Rust.
//! Results are stored in atomics, read lock-free by the main/UI thread.

use jni::JValue;
use std::fs::File;
use std::io::Write;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;

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
// Recording state
// =============================================================================

static RECORDING: AtomicBool = AtomicBool::new(false);
static RECORDED_SAMPLES: Mutex<Vec<f32>> = Mutex::new(Vec::new());
static OUTPUT_FILENAME: Mutex<String> = Mutex::new(String::new());
/// AudioRecord is configured at 48 kHz, but we still capture this live so a
/// future SAMPLE_RATE refactor only needs to update one place.
const RECORD_SAMPLE_RATE: u32 = 48_000;

// =============================================================================
// Sound level metering — shared A-weighting + Fast time weighting
// (see perry_ui::sound_level). One meter per capture session; reset on
// start/stop so a restart re-primes instead of decaying from stale energy.
// =============================================================================

static METER: Mutex<Option<perry_ui::sound_level::SoundLevelMeter>> = Mutex::new(None);

fn reset_meter() {
    *METER.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

/// Feed one buffer through the shared meter. Returns `(level_db, peak)`.
fn meter_process(samples: &[f32], sample_rate: f64) -> (f64, f32) {
    let mut guard = METER.lock().unwrap_or_else(|e| e.into_inner());
    let meter =
        guard.get_or_insert_with(|| perry_ui::sound_level::SoundLevelMeter::new(sample_rate));
    meter.ensure_sample_rate(sample_rate);
    meter.process(samples)
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
    let granted = jni_bridge::with_env(|env| {
        let _ = jni_bridge::push_local_frame(env, 8);
        let activity = crate::widgets::get_activity(env);
        let perm_str = env.new_string("android.permission.RECORD_AUDIO").unwrap();
        let result = env.call_static_method(
            jni::jni_str!("androidx/core/content/ContextCompat"),
            jni::jni_str!("checkSelfPermission"),
            jni::jni_sig!("(Landroid/content/Context;Ljava/lang/String;)I"),
            &[JValue::Object(&activity), JValue::Object(&perm_str.into())],
        );
        unsafe {
            let _ = jni_bridge::pop_local_frame(env, &jni::objects::JObject::null());
        }

        result.map(|v| v.i().unwrap_or(-1)).unwrap_or(-1) == 0
    });
    if !granted {
        return 0;
    }

    // Spawn a background thread that creates AudioRecord via JNI and reads data.
    // JNI calls must happen on a thread attached to the JVM.
    RUNNING.store(true, Ordering::Relaxed);

    std::thread::spawn(move || {
        jni_bridge::with_env(|env| {
            let _ = jni_bridge::push_local_frame(env, 32);

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
            let audio_record_cls = match env.find_class(jni::jni_str!("android/media/AudioRecord"))
            {
                Ok(cls) => cls,
                Err(_) => {
                    RUNNING.store(false, Ordering::Relaxed);
                    return;
                }
            };

            let min_buf = env.call_static_method(
                &audio_record_cls,
                jni::jni_str!("getMinBufferSize"),
                jni::jni_sig!("(III)I"),
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
                jni::jni_str!("android/media/AudioRecord"),
                jni::jni_sig!("(IIIII)V"),
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
            let state = env.call_method(
                &record,
                jni::jni_str!("getState"),
                jni::jni_sig!("()I"),
                &[],
            );
            if state.map(|v| v.i().unwrap_or(0)).unwrap_or(0) != 1 {
                RUNNING.store(false, Ordering::Relaxed);
                return;
            }

            // Start recording
            let _ = env.call_method(
                &record,
                jni::jni_str!("startRecording"),
                jni::jni_sig!("()V"),
                &[],
            );

            // Create a float array for reading samples
            let float_array = env
                .new_float_array(buffer_size_frames as usize)
                .expect("Failed to create float array");

            reset_meter();

            // Read loop
            while RUNNING.load(Ordering::Relaxed) {
                // AudioRecord.read(float[], int, int, int) — READ_BLOCKING = 0
                let read_result = env.call_method(
                    &record,
                    jni::jni_str!("read"),
                    jni::jni_sig!("([FIII)I"),
                    &[
                        JValue::Object(float_array.as_ref()),
                        JValue::Int(0),
                        JValue::Int(buffer_size_frames),
                        JValue::Int(0), // READ_BLOCKING
                    ],
                );

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
                let _ = float_array.get_region(env, 0, &mut samples);

                if RECORDING.load(Ordering::Relaxed) {
                    RECORDED_SAMPLES.lock().unwrap().extend_from_slice(&samples);
                }

                // Process: A-weight + RMS + dB
                // A-weighted, Fast (125 ms) energy-domain level.
                let (ema_db, peak) = meter_process(&samples, sample_rate as f64);

                CURRENT_DB.store(ema_db.to_bits(), Ordering::Relaxed);
                CURRENT_PEAK.store((peak as f64).to_bits(), Ordering::Relaxed);

                let idx = WAVEFORM_WRITE_INDEX.load(Ordering::Relaxed) as usize % WAVEFORM_SIZE;
                unsafe {
                    WAVEFORM_BUFFER[idx] = ema_db;
                }
                WAVEFORM_WRITE_INDEX.store((idx + 1) as u64, Ordering::Relaxed);
            }

            // Stop and release
            let _ = env.call_method(&record, jni::jni_str!("stop"), jni::jni_sig!("()V"), &[]);
            let _ = env.call_method(&record, jni::jni_str!("release"), jni::jni_sig!("()V"), &[]);
        })
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
    jni_bridge::with_env(|env| {
        let _ = jni_bridge::push_local_frame(env, 8);

        // android.os.Build.MODEL
        let build_cls = env.find_class(jni::jni_str!("android/os/Build")).ok();
        let model = build_cls
            .and_then(|cls| {
                env.get_static_field(
                    &cls,
                    jni::jni_str!("MODEL"),
                    jni::jni_sig!("Ljava/lang/String;"),
                )
                .ok()
                .and_then(|v| v.l().ok())
                .and_then(|obj| {
                    if obj.is_null() {
                        return None;
                    }
                    let jstr: jni::objects::JString =
                        unsafe { jni::objects::JString::from_raw(env, obj.into_raw()) };
                    jstr.try_to_string(env).ok().map(|s| String::from(s))
                })
            })
            .unwrap_or_else(|| "Unknown".to_string());

        unsafe {
            let _ = jni_bridge::pop_local_frame(env, &jni::objects::JObject::null());
        }

        let bytes = model.as_bytes();
        unsafe { js_string_from_bytes(bytes.as_ptr(), bytes.len() as i32) }
    })
}

/// Set the output filename a subsequent `start_recording()` will write to.
pub fn set_output_filename(filename: &str) {
    let mut slot = OUTPUT_FILENAME.lock().unwrap();
    slot.clear();
    slot.push_str(filename);
}

/// Begin accumulating microphone samples into the recording buffer. The
/// AudioRecord capture thread spawned by `start()` must already be running.
pub fn start_recording() {
    RECORDED_SAMPLES.lock().unwrap().clear();
    RECORDING.store(true, Ordering::Relaxed);
}

/// Stop recording and flush samples as a 16-bit mono PCM WAV at 48kHz.
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
        let _ = write_wav_header(&mut file, samples.len() as u32, RECORD_SAMPLE_RATE);
        let _ = write_wav_samples(&mut file, &samples);
    }
}

// =============================================================================
// WAV file utilities — 16-bit mono PCM
// =============================================================================

fn write_u32_le(writer: &mut File, value: u32) -> std::io::Result<()> {
    writer.write_all(&value.to_le_bytes())
}

fn write_u16_le(writer: &mut File, value: u16) -> std::io::Result<()> {
    writer.write_all(&value.to_le_bytes())
}

fn write_wav_header(writer: &mut File, num_samples: u32, sample_rate: u32) -> std::io::Result<()> {
    let bits_per_sample: u16 = 16;
    let channels: u16 = 1;
    let byte_rate = sample_rate * u32::from(channels) * u32::from(bits_per_sample) / 8;
    let block_align = u32::from(channels) * u32::from(bits_per_sample) / 8;
    let data_size = num_samples * u32::from(channels) * u32::from(bits_per_sample) / 8;
    let chunk_size = 36 + data_size;

    writer.write_all(b"RIFF")?;
    write_u32_le(writer, chunk_size)?;
    writer.write_all(b"WAVE")?;
    writer.write_all(b"fmt ")?;
    write_u32_le(writer, 16)?;
    write_u16_le(writer, 1)?;
    write_u16_le(writer, channels)?;
    write_u32_le(writer, sample_rate)?;
    write_u32_le(writer, byte_rate)?;
    write_u16_le(writer, block_align as u16)?;
    write_u16_le(writer, bits_per_sample)?;
    writer.write_all(b"data")?;
    write_u32_le(writer, data_size)?;
    Ok(())
}

fn write_wav_samples(writer: &mut File, samples: &[f32]) -> std::io::Result<()> {
    for &sample in samples {
        let clamped = sample.clamp(-1.0, 1.0);
        let int_sample = (clamped * i16::MAX as f32) as i16;
        writer.write_all(&int_sample.to_le_bytes())?;
    }
    Ok(())
}
