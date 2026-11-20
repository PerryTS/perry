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

static CURRENT_DB: AtomicU64 = AtomicU64::new(0);
static CURRENT_PEAK: AtomicU64 = AtomicU64::new(0);

const WAVEFORM_SIZE: usize = 256;
static WAVEFORM_WRITE_INDEX: AtomicU64 = AtomicU64::new(0);
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
// Thread-local state
// =============================================================================

thread_local! {
    static ENGINE: RefCell<Option<Retained<AnyObject>>> = RefCell::new(None);
}

extern "C" {
    fn js_string_from_bytes(ptr: *const u8, len: i32) -> i64;
    fn js_array_create() -> i64;
    fn js_array_push_f64(array_ptr: i64, value: f64);
}

// =============================================================================
// Public API
// =============================================================================

pub fn start() -> i64 {
    let already_running = ENGINE.with(|e| e.borrow().is_some());
    if already_running {
        return 1;
    }
    reset_meter();

    unsafe {
        // Configure audio session for recording
        let session_cls = match AnyClass::get(c"AVAudioSession") {
            Some(cls) => cls,
            None => {
                crate::ws_log!("[audio] AVAudioSession not found");
                return 0;
            }
        };
        let session: *mut AnyObject = msg_send![session_cls, sharedInstance];
        if session.is_null() {
            crate::ws_log!("[audio] sharedInstance is null");
            return 0;
        }

        // Set category to Record
        let category = objc2_foundation::NSString::from_str("AVAudioSessionCategoryRecord");
        let mut error: *mut AnyObject = std::ptr::null_mut();
        let _: bool = msg_send![session, setCategory: &*category error: &mut error];
        if !error.is_null() {
            crate::ws_log!("[audio] failed to set audio session category");
            return 0;
        }

        // Activate the session
        let _: bool = msg_send![session, setActive: true error: &mut error];

        // Request microphone permission
        // On iOS, this shows the system permission dialog if not yet determined.
        // The block is called asynchronously with the result.
        // For simplicity, we proceed optimistically — if denied, the engine will fail to start.
        let record_permission: i64 = msg_send![session, recordPermission];
        // 0 = undetermined, 1 = denied, 2 = granted
        if record_permission == 1 {
            crate::ws_log!("[audio] microphone permission denied");
            return 0;
        }
        if record_permission == 0 {
            // Request permission - this will show the system dialog.
            // Use a no-op block; the user calls audioStart() again after granting.
            let permission_block = block2::RcBlock::new(|_granted: objc2::runtime::Bool| {
                // Permission result handled on next audioStart() call
            });
            let _: () = msg_send![session, requestRecordPermission: &*permission_block];
            crate::ws_log!("[audio] requesting microphone permission");
            return 0; // Caller should retry after permission is granted
        }

        // Create AVAudioEngine
        let engine_cls = match AnyClass::get(c"AVAudioEngine") {
            Some(cls) => cls,
            None => {
                crate::ws_log!("[audio] AVAudioEngine not found");
                return 0;
            }
        };
        let engine: Retained<AnyObject> = msg_send![engine_cls, new];

        // Get input node
        let input_node: *mut AnyObject = msg_send![&*engine, inputNode];
        if input_node.is_null() {
            crate::ws_log!("[audio] inputNode is null");
            return 0;
        }

        // Get format
        let format: *mut AnyObject = msg_send![input_node, outputFormatForBus: 0u64];
        if format.is_null() {
            crate::ws_log!("[audio] could not get input format");
            return 0;
        }

        let sample_rate: f64 = msg_send![format, sampleRate];
        crate::ws_log!("[audio] input format: {}Hz", sample_rate);

        // Create tap block
        let tap_block =
            block2::RcBlock::new(move |buffer: *mut AnyObject, _when: *mut AnyObject| {
                process_audio_buffer(buffer, sample_rate);
            });

        // Install tap
        let buffer_size: u32 = 1024;
        let _: () = msg_send![
            input_node,
            installTapOnBus: 0u32
            bufferSize: buffer_size
            format: format
            block: &*tap_block
        ];

        // Start engine
        let mut error: *mut AnyObject = std::ptr::null_mut();
        let started: bool = msg_send![&*engine, startAndReturnError: &mut error];
        if !started {
            crate::ws_log!("[audio] failed to start engine");
            let _: () = msg_send![input_node, removeTapOnBus: 0u32];
            return 0;
        }

        crate::ws_log!("[audio] engine started successfully");
        ENGINE.with(|e| {
            *e.borrow_mut() = Some(engine);
        });
        1
    }
}

pub fn stop() {
    reset_meter();
    ENGINE.with(|e| {
        if let Some(engine) = e.borrow_mut().take() {
            unsafe {
                let input_node: *mut AnyObject = msg_send![&*engine, inputNode];
                if !input_node.is_null() {
                    let _: () = msg_send![input_node, removeTapOnBus: 0u32];
                }
                let _: () = msg_send![&*engine, stop];
            }
            crate::ws_log!("[audio] engine stopped");
        }
    });
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
    let model = get_sysctl_model();
    unsafe { js_string_from_bytes(model.as_ptr(), model.len() as i32) }
}

/// Set the output filename a subsequent `start_recording()` will write to.
pub fn set_output_filename(filename: &str) {
    let mut slot = OUTPUT_FILENAME.lock().unwrap();
    slot.clear();
    slot.push_str(filename);
}

/// Begin accumulating microphone samples into the recording buffer. The
/// AVAudioEngine tap installed by `start()` must already be running.
pub fn start_recording() {
    RECORDED_SAMPLES.lock().unwrap().clear();
    RECORDING.store(true, Ordering::Relaxed);
}

/// Stop recording and flush samples as a 16-bit mono PCM WAV.
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

unsafe fn process_audio_buffer(buffer: *mut AnyObject, sample_rate: f64) {
    if buffer.is_null() {
        return;
    }

    let float_channel_data: *const *const f32 = msg_send![buffer, floatChannelData];
    if float_channel_data.is_null() {
        return;
    }

    let frame_length: u32 = msg_send![buffer, frameLength];
    if frame_length == 0 {
        return;
    }

    let samples: *const f32 = *float_channel_data;
    if samples.is_null() {
        return;
    }

    let n = frame_length as usize;

    if RECORDING.load(Ordering::Relaxed) {
        RECORDING_SAMPLE_RATE.store(sample_rate.to_bits(), Ordering::Relaxed);
        let slice = std::slice::from_raw_parts(samples, n);
        RECORDED_SAMPLES.lock().unwrap().extend_from_slice(slice);
    }

    // A-weighted, Fast (125 ms) energy-domain level — see perry_ui::sound_level.
    let (smoothed, peak) = meter_process(std::slice::from_raw_parts(samples, n), sample_rate);

    CURRENT_DB.store(smoothed.to_bits(), Ordering::Relaxed);
    CURRENT_PEAK.store((peak as f64).to_bits(), Ordering::Relaxed);

    let idx = WAVEFORM_WRITE_INDEX.load(Ordering::Relaxed) as usize % WAVEFORM_SIZE;
    WAVEFORM_BUFFER[idx] = smoothed;
    WAVEFORM_WRITE_INDEX.store((idx + 1) as u64, Ordering::Relaxed);
}

// =============================================================================
// Internal: Device model detection
// =============================================================================

fn get_sysctl_model() -> String {
    use std::ffi::CStr;
    let mut size: libc::size_t = 0;
    // On iOS, hw.machine returns the device identifier (e.g., "iPhone15,2")
    let name = c"hw.machine";
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
