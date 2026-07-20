use crate::ffi::js_array_push_f64;
use crate::ffi::js_string_from_bytes;
use objc2::msg_send;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject};
use std::cell::RefCell;
use std::fs::File;
use std::io::Write;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;

// =============================================================================
// Shared atomic state — written by audio thread, read by main thread
// =============================================================================

/// Current smoothed dB(A) level, stored as f64 bit pattern.
static CURRENT_DB: AtomicU64 = AtomicU64::new(0);

/// Current peak sample amplitude (0.0-1.0), stored as f64 bit pattern.
static CURRENT_PEAK: AtomicU64 = AtomicU64::new(0);

/// Ring buffer of recent dB samples for waveform display.
/// Protected by a simple spin-lock via AtomicU64.
const WAVEFORM_SIZE: usize = 256;
static WAVEFORM_WRITE_INDEX: AtomicU64 = AtomicU64::new(0);

/// We use a static mut array because the audio callback needs to write to it
/// and it's a fixed-size ring buffer. Access is safe because:
/// - Only one writer (audio thread via tap callback)
/// - Reader (main thread) tolerates stale reads
static mut WAVEFORM_BUFFER: [f64; WAVEFORM_SIZE] = [0.0; WAVEFORM_SIZE];

// =============================================================================
// Recording state
// =============================================================================

static RECORDING: AtomicBool = AtomicBool::new(false);
static RECORDED_SAMPLES: Mutex<Vec<f32>> = Mutex::new(Vec::new());
static OUTPUT_FILENAME: Mutex<String> = Mutex::new(String::new());
/// Sample rate captured live from the input format. Bits of an f64.
/// 0 means "not yet observed"; stop_recording falls back to 48000 in that case.
static RECORDING_SAMPLE_RATE: AtomicU64 = AtomicU64::new(0);

// =============================================================================
// A-weighting IIR filter state (per audio session)
// =============================================================================

/// Biquad filter state for A-weighting.
/// A-weighting is implemented as 3 cascaded second-order IIR sections.
struct AWeightState {
    /// Filter state for each of the 3 biquad sections: [x[n-1], x[n-2], y[n-1], y[n-2]]
    sections: [[f64; 4]; 3],
}

impl AWeightState {
    fn new() -> Self {
        AWeightState {
            sections: [[0.0; 4]; 3],
        }
    }

    fn reset(&mut self) {
        self.sections = [[0.0; 4]; 3];
    }
}

// A-weighting filter coefficients for 48000 Hz sample rate.
// Derived from the analog A-weighting transfer function, bilinear-transformed.
// 3 cascaded second-order sections (SOS).
//
// Each section: y[n] = (b0*x[n] + b1*x[n-1] + b2*x[n-2] - a1*y[n-1] - a2*y[n-2]) / a0
// Stored as [b0, b1, b2, a0, a1, a2]
//
// These coefficients are computed for 48kHz (the default sample rate on modern
// Apple devices) using the standard analog A-weighting poles/zeros bilinear-
// transformed to digital. The filter has ~0 dB gain at 1 kHz and rolls off
// low and high frequencies per IEC 61672.
const A_WEIGHT_SOS: [[f64; 6]; 3] = [
    // Section 1: High-pass pair (20.6 Hz poles)
    [
        1.0,
        -2.0,
        1.0,
        1.0,
        -1.9746716508129498,
        0.975_046_288_554_988_9,
    ],
    // Section 2: High-pass pair (107.7 Hz, 737.9 Hz poles)
    [
        1.0,
        -2.0,
        1.0,
        1.0,
        -1.144_082_505_149_802,
        0.20482985688498268,
    ],
    // Section 3: Low-pass pair (12194 Hz poles)
    [
        0.24649652853975498,
        -0.49299305707950996,
        0.24649652853975498,
        1.0,
        -0.486_898_086_851_504_9,
        0.0,
    ],
];

// Overall gain to normalize the A-weighting filter to 0 dB at 1 kHz reference.
const A_WEIGHT_GAIN: f64 = 0.11310782960598924;

/// Apply A-weighting filter to a single sample using cascaded biquad sections.
fn a_weight_filter(sample: f64, state: &mut AWeightState) -> f64 {
    let mut x = sample * A_WEIGHT_GAIN;
    for (i, sos) in A_WEIGHT_SOS.iter().enumerate() {
        let b0 = sos[0];
        let b1 = sos[1];
        let b2 = sos[2];
        // a0 is sos[3], always 1.0 for these coefficients
        let a1 = sos[4];
        let a2 = sos[5];

        let s = &mut state.sections[i];
        let y = b0 * x + b1 * s[0] + b2 * s[1] - a1 * s[2] - a2 * s[3];

        // Shift state
        s[1] = s[0]; // x[n-2] = x[n-1]
        s[0] = x; // x[n-1] = x[n]
        s[3] = s[2]; // y[n-2] = y[n-1]
        s[2] = y; // y[n-1] = y[n]

        x = y;
    }
    x
}

// =============================================================================
// Thread-local state for AVAudioEngine lifecycle
// =============================================================================

thread_local! {
    static ENGINE: RefCell<Option<Retained<AnyObject>>> = const { RefCell::new(None) };
}

// =============================================================================
// ObjC FFI helpers
// =============================================================================

extern "C" {
    fn js_array_create() -> i64;
}

/// ObjC type encoding for AVAudioPCMBuffer block:
/// void (^)(AVAudioPCMBuffer *, AVAudioTime *)
/// We use raw pointers since we access through msg_send! anyway.
#[repr(C)]
struct AudioBufferList {
    _opaque: [u8; 0],
}

// =============================================================================
// Public API
// =============================================================================

/// Start audio capture. Returns 1 on success, 0 on failure.
pub fn start() -> i64 {
    // Check if already running
    let already_running = ENGINE.with(|e| e.borrow().is_some());
    if already_running {
        return 1;
    }

    unsafe {
        // Create AVAudioEngine
        let engine_cls = match AnyClass::get(c"AVAudioEngine") {
            Some(cls) => cls,
            None => {
                eprintln!("[audio] AVAudioEngine class not found — link AVFoundation.framework");
                return 0;
            }
        };
        let engine: Retained<AnyObject> = msg_send![engine_cls, new];

        // Get input node (microphone)
        let input_node: *mut AnyObject = msg_send![&*engine, inputNode];
        if input_node.is_null() {
            eprintln!("[audio] inputNode is null — no microphone available");
            return 0;
        }

        // Get the output format of the input node at bus 0
        let format: *mut AnyObject = msg_send![input_node, outputFormatForBus: 0u64];
        if format.is_null() {
            eprintln!("[audio] could not get input format");
            return 0;
        }

        // Read sample rate from format
        let sample_rate: f64 = msg_send![format, sampleRate];
        let channel_count: u32 = msg_send![format, channelCount];
        eprintln!(
            "[audio] input format: {}Hz, {} channels",
            sample_rate, channel_count
        );

        // Create the tap block
        // The block signature is: void (^)(AVAudioPCMBuffer *buffer, AVAudioTime *when)
        let tap_block =
            block2::RcBlock::new(move |buffer: *mut AnyObject, _when: *mut AnyObject| {
                process_audio_buffer(buffer, sample_rate);
            });

        // Install tap on bus 0
        let buffer_size: u32 = 1024;
        let _: () = msg_send![
            input_node,
            installTapOnBus: 0u32
            bufferSize: buffer_size
            format: format
            block: &*tap_block
        ];

        // Start the engine
        let mut error: *mut AnyObject = std::ptr::null_mut();
        let started: bool = msg_send![&*engine, startAndReturnError: &mut error];
        if !started {
            eprintln!("[audio] failed to start AVAudioEngine");
            if !error.is_null() {
                let desc: *mut AnyObject = msg_send![error, localizedDescription];
                if !desc.is_null() {
                    let utf8: *const u8 = msg_send![desc, UTF8String];
                    if !utf8.is_null() {
                        let s = std::ffi::CStr::from_ptr(utf8 as *const i8);
                        eprintln!("[audio] error: {}", s.to_string_lossy());
                    }
                }
            }
            // Remove the tap we just installed
            let _: () = msg_send![input_node, removeTapOnBus: 0u32];
            return 0;
        }

        eprintln!("[audio] engine started successfully");

        // Store engine to keep it alive and for later stop()
        ENGINE.with(|e| {
            *e.borrow_mut() = Some(engine);
        });

        1
    }
}

/// Stop audio capture and release resources.
pub fn stop() {
    ENGINE.with(|e| {
        if let Some(engine) = e.borrow_mut().take() {
            unsafe {
                // Get input node and remove tap
                let input_node: *mut AnyObject = msg_send![&*engine, inputNode];
                if !input_node.is_null() {
                    let _: () = msg_send![input_node, removeTapOnBus: 0u32];
                }

                // Stop the engine
                let _: () = msg_send![&*engine, stop];
            }
            eprintln!("[audio] engine stopped");
        }
    });
}

/// Get the current smoothed dB(A) level. Lock-free atomic read.
pub fn get_level() -> f64 {
    f64::from_bits(CURRENT_DB.load(Ordering::Relaxed))
}

/// Get the current peak sample amplitude. Lock-free atomic read.
pub fn get_peak() -> f64 {
    f64::from_bits(CURRENT_PEAK.load(Ordering::Relaxed))
}

/// Get recent dB samples as an array for waveform rendering.
/// Returns a NaN-boxed array handle.
pub fn get_waveform(count: f64) -> f64 {
    let n = (count as usize).min(WAVEFORM_SIZE);
    let write_idx = WAVEFORM_WRITE_INDEX.load(Ordering::Relaxed) as usize;

    unsafe {
        let array = js_array_create();
        for i in 0..n {
            // Read from ring buffer, starting from oldest
            let idx = (write_idx + WAVEFORM_SIZE - n + i) % WAVEFORM_SIZE;
            let sample = WAVEFORM_BUFFER[idx];
            js_array_push_f64(array as *mut std::ffi::c_void, sample);
        }
        // Return as f64 (NaN-boxed pointer)
        f64::from_bits(array as u64)
    }
}

/// Get the device model identifier string (e.g., "MacBookPro18,3"),
/// read from sysctl `hw.model`. Returns a raw `*mut StringHeader` (i64)
/// via `js_string_from_bytes`; the `ReturnKind::Str` dispatch row
/// NaN-boxes it with STRING_TAG (#5972 — declaring it `F64` passed the
/// pointer bits through as a double and callers read `NaN`).
pub fn get_device_model() -> i64 {
    let model = get_sysctl_model();
    unsafe { js_string_from_bytes(model.as_ptr(), model.len() as u32) as i64 }
}

/// Set the output filename a subsequent `start_recording()` will write to.
/// Stored separately from the recording flag so callers can configure the
/// destination once and toggle recording on/off without re-passing it.
pub fn set_output_filename(filename: &str) {
    let mut slot = OUTPUT_FILENAME.lock().unwrap();
    slot.clear();
    slot.push_str(filename);
}

/// Begin accumulating microphone samples into the recording buffer. The
/// AVAudioEngine tap installed by `start()` must already be running — calling
/// this without `audioStart()` produces an empty WAV.
pub fn start_recording() {
    RECORDED_SAMPLES.lock().unwrap().clear();
    RECORDING.store(true, Ordering::Relaxed);
}

/// Stop recording and flush the accumulated samples to the configured filename
/// as a 16-bit mono PCM WAV. No-op if no filename was set or no samples were
/// captured. The recording flag is cleared first so the audio thread stops
/// appending before we drain the buffer.
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

    let observed = f64::from_bits(RECORDING_SAMPLE_RATE.load(Ordering::Relaxed));
    let sample_rate = if observed > 0.0 {
        observed as u32
    } else {
        48_000
    };

    if let Ok(mut file) = File::create(&filename) {
        let _ = write_wav_header(&mut file, samples.len() as u32, sample_rate);
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

// =============================================================================
// Internal: Audio processing
// =============================================================================

/// EMA (exponential moving average) state. Uses thread_local because the
/// tap block always runs on the same audio thread.
thread_local! {
    static FILTER_STATE: RefCell<AWeightState> = RefCell::new(AWeightState::new());
    static EMA_DB: RefCell<f64> = const { RefCell::new(0.0) };
}

/// Process a single audio buffer from the AVAudioEngine tap.
/// Called on the audio thread.
unsafe fn process_audio_buffer(buffer: *mut AnyObject, sample_rate: f64) {
    if buffer.is_null() {
        return;
    }

    // Get float channel data from AVAudioPCMBuffer
    // floatChannelData returns a float** (array of channel pointers)
    let float_channel_data: *const *const f32 = msg_send![buffer, floatChannelData];
    if float_channel_data.is_null() {
        return;
    }

    // Get frame length
    let frame_length: u32 = msg_send![buffer, frameLength];
    if frame_length == 0 {
        return;
    }

    // Get channel 0 samples
    let samples: *const f32 = *float_channel_data;
    if samples.is_null() {
        return;
    }

    let n = frame_length as usize;

    // Capture samples into the recording buffer if recording is active.
    // The atomic flag is checked first to avoid acquiring the mutex on
    // every buffer in the common (non-recording) case. Sample rate is
    // also stored so stop_recording can write the correct WAV header.
    if RECORDING.load(Ordering::Relaxed) {
        RECORDING_SAMPLE_RATE.store(sample_rate.to_bits(), Ordering::Relaxed);
        let slice = std::slice::from_raw_parts(samples, n);
        RECORDED_SAMPLES.lock().unwrap().extend_from_slice(slice);
    }

    // Apply A-weighting and compute RMS
    let mut sum_sq = 0.0f64;
    let mut peak = 0.0f32;

    FILTER_STATE.with(|fs| {
        let mut state = fs.borrow_mut();
        for i in 0..n {
            let sample = *samples.add(i);

            // Track peak
            let abs_sample = sample.abs();
            if abs_sample > peak {
                peak = abs_sample;
            }

            // Apply A-weighting filter
            let weighted = a_weight_filter(sample as f64, &mut state);

            // Accumulate for RMS
            sum_sq += weighted * weighted;
        }
    });

    // Compute RMS of A-weighted samples
    let rms = (sum_sq / n as f64).sqrt();

    // Convert digital RMS to approximate dB SPL.
    //
    // The offset maps 0 dBFS (digital full scale) to a physical SPL level.
    // On most Apple devices, the mic AGC and analog front-end place 0 dBFS
    // at roughly 110-130 dB SPL. We use 110 as a conservative starting
    // point; the per-device calibration table in calibration.ts refines this.
    //
    // Formula: dB_SPL ≈ 20 * log10(rms) + DIGITAL_TO_SPL_OFFSET
    const DIGITAL_TO_SPL_OFFSET: f64 = 110.0;
    let db_raw = if rms > 1.0e-10 {
        20.0 * rms.log10() + DIGITAL_TO_SPL_OFFSET
    } else {
        0.0
    };

    // Clamp to reasonable range
    let db_clamped = db_raw.max(0.0).min(140.0);

    // EMA smoothing (time constant ~125ms for "fast" response)
    let dt = n as f64 / sample_rate;
    let tau = 0.125; // 125ms time constant
    let alpha = 1.0 - (-dt / tau).exp();

    let smoothed = EMA_DB.with(|ema| {
        let mut current = ema.borrow_mut();
        *current += alpha * (db_clamped - *current);
        *current
    });

    // Store results atomically
    CURRENT_DB.store(smoothed.to_bits(), Ordering::Relaxed);
    CURRENT_PEAK.store((peak as f64).to_bits(), Ordering::Relaxed);

    // Update waveform ring buffer
    let idx = WAVEFORM_WRITE_INDEX.load(Ordering::Relaxed) as usize % WAVEFORM_SIZE;
    WAVEFORM_BUFFER[idx] = smoothed;
    WAVEFORM_WRITE_INDEX.store((idx + 1) as u64, Ordering::Relaxed);
}

// =============================================================================
// Internal: Device model detection
// =============================================================================

/// Get the hardware model identifier via sysctl (e.g., "MacBookPro18,3").
fn get_sysctl_model() -> String {
    use std::ffi::CStr;
    let mut size: libc::size_t = 0;
    let name = c"hw.model";
    unsafe {
        libc::sysctlbyname(
            name.as_ptr(),
            std::ptr::null_mut(),
            &mut size,
            std::ptr::null_mut(),
            0,
        );
        if size == 0 {
            return "Unknown".to_string();
        }
        let mut buf = vec![0u8; size];
        libc::sysctlbyname(
            name.as_ptr(),
            buf.as_mut_ptr() as *mut _,
            &mut size,
            std::ptr::null_mut(),
            0,
        );
        CStr::from_ptr(buf.as_ptr() as *const i8)
            .to_string_lossy()
            .into_owned()
    }
}
