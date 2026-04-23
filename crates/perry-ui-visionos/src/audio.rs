use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject};
use objc2::msg_send;
use std::cell::RefCell;
use std::sync::atomic::{AtomicU64, Ordering};

// =============================================================================
// Shared atomic state — written by audio thread, read by main thread
// =============================================================================

static CURRENT_DB: AtomicU64 = AtomicU64::new(0);
static CURRENT_PEAK: AtomicU64 = AtomicU64::new(0);

const WAVEFORM_SIZE: usize = 256;
static WAVEFORM_WRITE_INDEX: AtomicU64 = AtomicU64::new(0);
static mut WAVEFORM_BUFFER: [f64; WAVEFORM_SIZE] = [0.0; WAVEFORM_SIZE];

// =============================================================================
// A-weighting IIR filter
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

// A-weighting coefficients for 48kHz (default on modern Apple devices).
const A_WEIGHT_SOS: [[f64; 6]; 3] = [
    [1.0, -2.0, 1.0, 1.0, -1.9746716508129498, 0.97504628855498883],
    [1.0, -2.0, 1.0, 1.0, -1.1440825051498020, 0.20482985688498268],
    [0.24649652853975498, -0.49299305707950996, 0.24649652853975498, 1.0, -0.48689808685150487, 0.0],
];

const A_WEIGHT_GAIN: f64 = 0.11310782960598924;

fn a_weight_filter(sample: f64, state: &mut AWeightState) -> f64 {
    let mut x = sample * A_WEIGHT_GAIN;
    for (i, sos) in A_WEIGHT_SOS.iter().enumerate() {
        let b0 = sos[0]; let b1 = sos[1]; let b2 = sos[2];
        let a1 = sos[4]; let a2 = sos[5];
        let s = &mut state.sections[i];
        let y = b0 * x + b1 * s[0] + b2 * s[1] - a1 * s[2] - a2 * s[3];
        s[1] = s[0]; s[0] = x; s[3] = s[2]; s[2] = y;
        x = y;
    }
    x
}

// =============================================================================
// Thread-local state
// =============================================================================

thread_local! {
    static ENGINE: RefCell<Option<Retained<AnyObject>>> = RefCell::new(None);
    static FILTER_STATE: RefCell<AWeightState> = RefCell::new(AWeightState::new());
    static EMA_DB: RefCell<f64> = RefCell::new(0.0);
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
        let tap_block = block2::RcBlock::new(move |buffer: *mut AnyObject, _when: *mut AnyObject| {
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
        ENGINE.with(|e| { *e.borrow_mut() = Some(engine); });
        1
    }
}

pub fn stop() {
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
    unsafe {
        js_string_from_bytes(model.as_ptr(), model.len() as i32)
    }
}

// =============================================================================
// Internal: Audio processing
// =============================================================================

unsafe fn process_audio_buffer(buffer: *mut AnyObject, sample_rate: f64) {
    if buffer.is_null() { return; }

    let float_channel_data: *const *const f32 = msg_send![buffer, floatChannelData];
    if float_channel_data.is_null() { return; }

    let frame_length: u32 = msg_send![buffer, frameLength];
    if frame_length == 0 { return; }

    let samples: *const f32 = *float_channel_data;
    if samples.is_null() { return; }

    let n = frame_length as usize;
    let mut sum_sq = 0.0f64;
    let mut peak = 0.0f32;

    FILTER_STATE.with(|fs| {
        let mut state = fs.borrow_mut();
        for i in 0..n {
            let sample = *samples.add(i);
            let abs_sample = sample.abs();
            if abs_sample > peak { peak = abs_sample; }
            let weighted = a_weight_filter(sample as f64, &mut state);
            sum_sq += weighted * weighted;
        }
    });

    let rms = (sum_sq / n as f64).sqrt();
    // Map digital RMS to dB SPL. 0 dBFS ≈ 110 dB SPL (conservative;
    // per-device calibration in calibration.ts refines this).
    let db_raw = if rms > 1.0e-10 {
        20.0 * rms.log10() + 110.0
    } else {
        0.0
    };
    let db_clamped = db_raw.max(0.0).min(140.0);

    let dt = n as f64 / sample_rate;
    let tau = 0.125;
    let alpha = 1.0 - (-dt / tau).exp();

    let smoothed = EMA_DB.with(|ema| {
        let mut current = ema.borrow_mut();
        *current += alpha * (db_clamped - *current);
        *current
    });

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
        libc::sysctlbyname(name.as_ptr(), std::ptr::null_mut(), &mut size, std::ptr::null_mut(), 0);
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
