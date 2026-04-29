use objc2::msg_send;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject};
use std::cell::RefCell;
use std::sync::atomic::{AtomicU64, Ordering};

static CURRENT_DB: AtomicU64 = AtomicU64::new(0);
static CURRENT_PEAK: AtomicU64 = AtomicU64::new(0);

thread_local! {
    static ENGINE: RefCell<Option<Retained<AnyObject>>> = RefCell::new(None);
    static EMA_DB: RefCell<f64> = RefCell::new(0.0);
}

pub fn start() -> i64 {
    let already_running = ENGINE.with(|e| e.borrow().is_some());
    if already_running {
        return 1;
    }

    unsafe {
        let session_cls = match AnyClass::get(c"AVAudioSession") {
            Some(cls) => cls,
            None => return 0,
        };
        let session: *mut AnyObject = msg_send![session_cls, sharedInstance];
        if session.is_null() {
            return 0;
        }

        // Use setCategory:mode:options:error: for watchOS compatibility
        let category = objc2_foundation::NSString::from_str("AVAudioSessionCategoryPlayAndRecord");
        let mode = objc2_foundation::NSString::from_str("AVAudioSessionModeMeasurement");
        let options: usize = 0; // NSUInteger
        let mut error: *mut AnyObject = std::ptr::null_mut();
        let _: bool = msg_send![session, setCategory: &*category
                                         mode: &*mode
                                         options: options
                                         error: &mut error];

        let _: bool = msg_send![session, setActive: true error: &mut error];

        // recordPermission returns NSUInteger (usize on this platform)
        let record_permission: usize = msg_send![session, recordPermission];
        if record_permission == 1 {
            return 0;
        } // denied
        if record_permission == 0 {
            let permission_block = block2::RcBlock::new(|_granted: objc2::runtime::Bool| {});
            let _: () = msg_send![session, requestRecordPermission: &*permission_block];
            return 0; // retry after grant
        }

        let engine_cls = match AnyClass::get(c"AVAudioEngine") {
            Some(cls) => cls,
            None => return 0,
        };
        let engine: Retained<AnyObject> = msg_send![engine_cls, new];

        let input_node: *mut AnyObject = msg_send![&*engine, inputNode];
        if input_node.is_null() {
            return 0;
        }

        // outputFormatForBus: takes NSUInteger (usize)
        let bus: usize = 0;
        let format: *mut AnyObject = msg_send![input_node, outputFormatForBus: bus];
        if format.is_null() {
            return 0;
        }

        let sample_rate: f64 = msg_send![format, sampleRate];
        if sample_rate <= 0.0 {
            return 0;
        }

        let tap_block =
            block2::RcBlock::new(move |buffer: *mut AnyObject, _when: *mut AnyObject| {
                process_audio_buffer(buffer, sample_rate);
            });

        let buffer_size: u32 = 1024;
        let tap_bus: u32 = 0; // AVAudioNodeBus is UInt32
        let _: () = msg_send![
            input_node,
            installTapOnBus: tap_bus
            bufferSize: buffer_size
            format: format
            block: &*tap_block
        ];

        let mut start_error: *mut AnyObject = std::ptr::null_mut();
        let started: bool = msg_send![&*engine, startAndReturnError: &mut start_error];
        if !started {
            let _: () = msg_send![input_node, removeTapOnBus: tap_bus];
            return 0;
        }

        ENGINE.with(|e| {
            *e.borrow_mut() = Some(engine);
        });
        // DEBUG: write a test value so we can tell if engine started vs tap not firing
        CURRENT_DB.store(42.0_f64.to_bits(), Ordering::Relaxed);
        1
    }
}

pub fn stop() {
    ENGINE.with(|e| {
        if let Some(engine) = e.borrow_mut().take() {
            unsafe {
                let input_node: *mut AnyObject = msg_send![&*engine, inputNode];
                if !input_node.is_null() {
                    let bus: u32 = 0;
                    let _: () = msg_send![input_node, removeTapOnBus: bus];
                }
                let _: () = msg_send![&*engine, stop];
            }
        }
    });
}

pub fn get_level() -> f64 {
    // DEBUG: always return 55 to verify this function is called
    return 55.0;
    #[allow(unreachable_code)]
    f64::from_bits(CURRENT_DB.load(Ordering::Relaxed))
}

pub fn get_peak() -> f64 {
    f64::from_bits(CURRENT_PEAK.load(Ordering::Relaxed))
}

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
    let mut sum_sq = 0.0f64;
    let mut peak = 0.0f32;

    for i in 0..n {
        let sample = *samples.add(i);
        let abs_sample = sample.abs();
        if abs_sample > peak {
            peak = abs_sample;
        }
        sum_sq += (sample as f64) * (sample as f64);
    }

    let rms = (sum_sq / n as f64).sqrt();
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
}
