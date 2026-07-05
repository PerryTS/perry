use objc2::msg_send;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject};
use std::cell::RefCell;
use std::fs::File;
use std::io::Write;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;

static CURRENT_DB: AtomicU64 = AtomicU64::new(0);
static CURRENT_PEAK: AtomicU64 = AtomicU64::new(0);

// =============================================================================
// Recording state
// =============================================================================

static RECORDING: AtomicBool = AtomicBool::new(false);
static RECORDED_SAMPLES: Mutex<Vec<f32>> = Mutex::new(Vec::new());
static OUTPUT_FILENAME: Mutex<String> = Mutex::new(String::new());
/// Sample rate captured live from the input format. Bits of an f64.
static RECORDING_SAMPLE_RATE: AtomicU64 = AtomicU64::new(0);

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
    f64::from_bits(CURRENT_DB.load(Ordering::Relaxed))
}

pub fn get_peak() -> f64 {
    f64::from_bits(CURRENT_PEAK.load(Ordering::Relaxed))
}

/// Device identifier (e.g. "Watch7,5") as a JS string handle, from
/// sysctl hw.machine — same source as the iOS/tvOS implementations.
pub fn get_device_model() -> i64 {
    extern "C" {
        fn js_string_from_bytes(ptr: *const u8, len: i32) -> i64;
    }
    let model = get_sysctl_model();
    unsafe { js_string_from_bytes(model.as_ptr(), model.len() as i32) }
}

fn get_sysctl_model() -> String {
    use std::ffi::CStr;
    let mut size: libc::size_t = 0;
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

/// Set the output filename a subsequent `start_recording()` will write to.
pub fn set_output_filename(filename: &str) {
    let mut slot = OUTPUT_FILENAME.lock().unwrap();
    slot.clear();
    slot.push_str(filename);
}

/// Begin accumulating microphone samples into the recording buffer.
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
