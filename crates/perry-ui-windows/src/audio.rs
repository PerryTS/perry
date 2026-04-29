//! Audio capture for Windows using WASAPI (Windows Audio Session API).
//!
//! Uses the default capture device in shared mode with event-driven buffering.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

// =============================================================================
// Shared atomic state
// =============================================================================

static CURRENT_DB: AtomicU64 = AtomicU64::new(0);
static CURRENT_PEAK: AtomicU64 = AtomicU64::new(0);

const WAVEFORM_SIZE: usize = 256;
static WAVEFORM_WRITE_INDEX: AtomicU64 = AtomicU64::new(0);
static mut WAVEFORM_BUFFER: [f64; WAVEFORM_SIZE] = [0.0; WAVEFORM_SIZE];

static RUNNING: AtomicBool = AtomicBool::new(false);

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
// Public API
// =============================================================================

extern "C" {
    fn js_string_from_bytes(ptr: *const u8, len: i32) -> i64;
    fn js_array_create() -> i64;
    fn js_array_push_f64(array_ptr: i64, value: f64);
}

#[cfg(target_os = "windows")]
pub fn start() -> i64 {
    use windows::core::*;
    use windows::Win32::Media::Audio::*;
    use windows::Win32::System::Com::*;

    if RUNNING.load(Ordering::Relaxed) {
        return 1;
    }

    RUNNING.store(true, Ordering::Relaxed);

    std::thread::spawn(|| {
        unsafe {
            // Initialize COM on this thread
            let hr = CoInitializeEx(None, COINIT_MULTITHREADED);
            if hr.is_err() {
                eprintln!("[audio] COM init failed");
                RUNNING.store(false, Ordering::Relaxed);
                return;
            }

            // Get default capture device
            let enumerator: IMMDeviceEnumerator =
                match CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) {
                    Ok(e) => e,
                    Err(_) => {
                        eprintln!("[audio] Failed to create device enumerator");
                        RUNNING.store(false, Ordering::Relaxed);
                        CoUninitialize();
                        return;
                    }
                };

            let device = match enumerator.GetDefaultAudioEndpoint(eCapture, eConsole) {
                Ok(d) => d,
                Err(_) => {
                    eprintln!("[audio] No capture device found");
                    RUNNING.store(false, Ordering::Relaxed);
                    CoUninitialize();
                    return;
                }
            };

            // Activate audio client
            let audio_client: IAudioClient = match device.Activate(CLSCTX_ALL, None) {
                Ok(c) => c,
                Err(_) => {
                    eprintln!("[audio] Failed to activate audio client");
                    RUNNING.store(false, Ordering::Relaxed);
                    CoUninitialize();
                    return;
                }
            };

            // Get mix format
            let mix_format_ptr = match audio_client.GetMixFormat() {
                Ok(f) => f,
                Err(_) => {
                    eprintln!("[audio] Failed to get mix format");
                    RUNNING.store(false, Ordering::Relaxed);
                    CoUninitialize();
                    return;
                }
            };
            let mix_format = &*mix_format_ptr;
            let sample_rate = mix_format.nSamplesPerSec;
            let channels = mix_format.nChannels as usize;
            let bits_per_sample = mix_format.wBitsPerSample;

            eprintln!(
                "[audio] WASAPI format: {}Hz, {} ch, {} bit",
                sample_rate, channels, bits_per_sample
            );

            // Initialize in shared mode
            // Buffer duration: 100ms in 100-nanosecond units
            let buffer_duration: i64 = 1_000_000; // 100ms
            if audio_client
                .Initialize(
                    AUDCLNT_SHAREMODE_SHARED,
                    0,
                    buffer_duration,
                    0,
                    mix_format_ptr,
                    None,
                )
                .is_err()
            {
                eprintln!("[audio] Failed to initialize audio client");
                RUNNING.store(false, Ordering::Relaxed);
                CoUninitialize();
                return;
            }

            // Get capture client
            let capture_client: IAudioCaptureClient = match audio_client.GetService() {
                Ok(c) => c,
                Err(_) => {
                    eprintln!("[audio] Failed to get capture client");
                    RUNNING.store(false, Ordering::Relaxed);
                    CoUninitialize();
                    return;
                }
            };

            // Start capture
            if audio_client.Start().is_err() {
                eprintln!("[audio] Failed to start capture");
                RUNNING.store(false, Ordering::Relaxed);
                CoUninitialize();
                return;
            }

            eprintln!("[audio] WASAPI capture started");

            let mut filter_state = AWeightState::new();
            let mut ema_db: f64 = 0.0;
            let is_float = bits_per_sample == 32;

            // Capture loop
            while RUNNING.load(Ordering::Relaxed) {
                // Sleep briefly to let buffer accumulate
                std::thread::sleep(std::time::Duration::from_millis(20));

                // Read all available packets
                loop {
                    let packet_length = match capture_client.GetNextPacketSize() {
                        Ok(len) => len,
                        Err(_) => break,
                    };
                    if packet_length == 0 {
                        break;
                    }

                    let mut data_ptr: *mut u8 = std::ptr::null_mut();
                    let mut frames_available: u32 = 0;
                    let mut flags: u32 = 0;

                    if capture_client
                        .GetBuffer(&mut data_ptr, &mut frames_available, &mut flags, None, None)
                        .is_err()
                    {
                        break;
                    }

                    let n = frames_available as usize;
                    if n > 0 && !data_ptr.is_null() {
                        let mut sum_sq = 0.0f64;
                        let mut peak = 0.0f32;

                        // Process samples (take first channel only)
                        for i in 0..n {
                            let sample = if is_float {
                                let ptr = data_ptr as *const f32;
                                *ptr.add(i * channels)
                            } else {
                                // 16-bit PCM
                                let ptr = data_ptr as *const i16;
                                (*ptr.add(i * channels)) as f32 / 32768.0
                            };

                            let abs_s = sample.abs();
                            if abs_s > peak {
                                peak = abs_s;
                            }
                            let weighted = a_weight_filter(sample as f64, &mut filter_state);
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

                        let idx =
                            WAVEFORM_WRITE_INDEX.load(Ordering::Relaxed) as usize % WAVEFORM_SIZE;
                        WAVEFORM_BUFFER[idx] = ema_db;
                        WAVEFORM_WRITE_INDEX.store((idx + 1) as u64, Ordering::Relaxed);
                    }

                    let _ = capture_client.ReleaseBuffer(frames_available);
                }
            }

            let _ = audio_client.Stop();
            CoUninitialize();
            eprintln!("[audio] WASAPI capture stopped");
        }
    });

    1
}

#[cfg(not(target_os = "windows"))]
pub fn start() -> i64 {
    0
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
    #[cfg(target_os = "windows")]
    {
        // Get computer name via environment variable (avoids Windows API versioning issues)
        let name = std::env::var("COMPUTERNAME").unwrap_or_else(|_| "Windows".to_string());
        unsafe { js_string_from_bytes(name.as_ptr(), name.len() as i32) }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let model = b"Unknown";
        unsafe { js_string_from_bytes(model.as_ptr(), model.len() as i32) }
    }
}
