//! perry/audio runtime — game-engine-style audio backed by AVAudioEngine.
//!
//! See `crates/perry-dispatch/src/audio_table.rs` for the dispatch table and
//! `types/perry/audio/index.d.ts` for the TS surface. Three handle kinds, all
//! 1-based and carried across the FFI as `f64` (NaN-boxed I64AsF64). Disjoint
//! ranges let any method that accepts "handle" disambiguate:
//!
//!   Sound       0x00000001 ..= 0x0FFFFFFF  — preloaded AVAudioPCMBuffer
//!   PlaybackId  0x10000001 ..= 0x1FFFFFFF  — live AVAudioPlayerNode voice
//!   Bus         0x20000001 ..= 0x2FFFFFFF  — AVAudioMixerNode group (0 = master)

use crate::ffi::class_addMethod;
use crate::ffi::sel_registerName;
use objc2::msg_send;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, Sel};
use std::cell::RefCell;

extern "C" {
    fn js_nanbox_get_pointer(value: f64) -> i64;
    fn js_closure_call0(closure: *const u8) -> f64;
    #[allow(dead_code)]
    fn js_closure_call1(closure: *const u8, arg: f64) -> f64;
    fn js_run_stdlib_pump();
    fn js_promise_run_microtasks() -> i32;
}

// Raw ObjC runtime FFI for dynamic NSTimer-target class registration.
extern "C" {
    fn objc_allocateClassPair(
        superclass: *const std::ffi::c_void,
        name: *const i8,
        extra_bytes: usize,
    ) -> *mut std::ffi::c_void;
    fn objc_registerClassPair(cls: *mut std::ffi::c_void);
    fn objc_getClass(name: *const i8) -> *const std::ffi::c_void;
}

// =============================================================================
// Handle classification
// =============================================================================

const SOUND_BASE: i64 = 0x0000_0001;
const SOUND_MAX: i64 = 0x0FFF_FFFF;
const PLAYBACK_BASE: i64 = 0x1000_0001;
const PLAYBACK_MAX: i64 = 0x1FFF_FFFF;
const BUS_BASE: i64 = 0x2000_0001;
const BUS_MAX: i64 = 0x2FFF_FFFF;

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
enum HandleKind {
    Master,
    Sound(usize),
    Playback(usize),
    Bus(usize),
    Invalid,
}

fn classify(h: f64) -> HandleKind {
    if h == 0.0 {
        return HandleKind::Master;
    }
    let id = h as i64;
    if (SOUND_BASE..=SOUND_MAX).contains(&id) {
        HandleKind::Sound((id - SOUND_BASE) as usize)
    } else if (PLAYBACK_BASE..=PLAYBACK_MAX).contains(&id) {
        HandleKind::Playback((id - PLAYBACK_BASE) as usize)
    } else if (BUS_BASE..=BUS_MAX).contains(&id) {
        HandleKind::Bus((id - BUS_BASE) as usize)
    } else {
        HandleKind::Invalid
    }
}

fn sound_handle_id(idx: usize) -> i64 {
    SOUND_BASE + idx as i64
}
fn playback_handle_id(idx: usize) -> i64 {
    PLAYBACK_BASE + idx as i64
}
fn bus_handle_id(idx: usize) -> i64 {
    BUS_BASE + idx as i64
}

// =============================================================================
// State
// =============================================================================

struct SoundEntry {
    buffer: Option<Retained<AnyObject>>, // AVAudioPCMBuffer (None for streaming)
    file: Option<Retained<AnyObject>>, // AVAudioFile (Some for streaming, also held for non-streaming sample-rate)
    format: Retained<AnyObject>,       // AVAudioFormat
    sample_rate: f64,
    frame_length: u32,
    default_volume: f32,
    bus_handle: f64,
    is_streaming: bool,
    on_loaded: Option<f64>,
}

struct VoiceEntry {
    player_node: Retained<AnyObject>,       // AVAudioPlayerNode
    varispeed: Option<Retained<AnyObject>>, // AVAudioUnitVarispeed (None if fallback)
    sound_idx: usize,
    bus_handle: f64,
    is_playing: bool,
    is_paused: bool,
    looping: bool,
    volume: f32,
    on_ended: Option<f64>,
    /// True once user/runtime stops the voice explicitly — used by streaming
    /// loop to avoid re-scheduling after stop().
    manually_stopped: bool,
}

struct BusEntry {
    mixer: Retained<AnyObject>, // AVAudioMixerNode
    _name: String,
    parent_id: f64,
    volume: f32,
    muted: bool,
    pre_mute_volume: f32,
    soloed: bool,
}

struct Fade {
    handle: f64, // raw target handle
    start_vol: f32,
    target_vol: f32,
    ticks_total: u32,
    ticks_left: u32,
    then_stop: bool,
}

thread_local! {
    static ENGINE: RefCell<Option<Retained<AnyObject>>> = RefCell::new(None);
    static SOUNDS: RefCell<Vec<Option<SoundEntry>>> = RefCell::new(Vec::new());
    static VOICES: RefCell<Vec<Option<VoiceEntry>>> = RefCell::new(Vec::new());
    static BUSES: RefCell<Vec<Option<BusEntry>>> = RefCell::new(Vec::new());
    static FADES: RefCell<Vec<Fade>> = RefCell::new(Vec::new());
    static FADE_TIMER: RefCell<Option<Retained<AnyObject>>> = RefCell::new(None);
    static FADE_TIMER_TARGET: RefCell<Option<Retained<AnyObject>>> = RefCell::new(None);
    static FADE_CLASS_REGISTERED: RefCell<bool> = RefCell::new(false);
    static PENDING_ENDED: RefCell<Vec<usize>> = RefCell::new(Vec::new());
    static PENDING_LOADED: RefCell<Vec<usize>> = RefCell::new(Vec::new());
    static WARNED_SET_PAN: RefCell<bool> = RefCell::new(false);
    static WARNED_VARISPEED_FALLBACK: RefCell<bool> = RefCell::new(false);
}

// =============================================================================
// Helpers
// =============================================================================

fn str_from_header(ptr: *const u8) -> &'static str {
    if ptr.is_null() {
        return "";
    }
    unsafe {
        let header = ptr as *const crate::string_header::StringHeader;
        let len = (*header).byte_len as usize;
        let data = ptr.add(std::mem::size_of::<crate::string_header::StringHeader>());
        std::str::from_utf8_unchecked(std::slice::from_raw_parts(data, len))
    }
}

/// Push into the first empty slot or extend; return the slot index.
fn slot_insert<T>(vec: &mut Vec<Option<T>>, entry: T) -> usize {
    for (i, slot) in vec.iter_mut().enumerate() {
        if slot.is_none() {
            *slot = Some(entry);
            return i;
        }
    }
    vec.push(Some(entry));
    vec.len() - 1
}

// =============================================================================
// Engine
// =============================================================================

fn ensure_engine() -> bool {
    if ENGINE.with(|e| e.borrow().is_some()) {
        return true;
    }
    unsafe {
        if let Some(session_cls) = AnyClass::get(c"AVAudioSession") {
            let session: *mut AnyObject = msg_send![session_cls, sharedInstance];
            if !session.is_null() {
                let cat = objc2_foundation::NSString::from_str("AVAudioSessionCategoryPlayback");
                let mut error: *mut AnyObject = std::ptr::null_mut();
                let _: bool = msg_send![session, setCategory: &*cat, error: &mut error];
                error = std::ptr::null_mut();
                let _: bool = msg_send![session, setActive: true, error: &mut error];
            }
        }

        let engine_cls = match AnyClass::get(c"AVAudioEngine") {
            Some(c) => c,
            None => {
                eprintln!("[perry/audio] AVAudioEngine not found");
                return false;
            }
        };
        let engine: Retained<AnyObject> = msg_send![engine_cls, new];
        // Materialize the main mixer before start.
        let _: *mut AnyObject = msg_send![&*engine, mainMixerNode];
        let mut error: *mut AnyObject = std::ptr::null_mut();
        let started: bool = msg_send![&*engine, startAndReturnError: &mut error];
        if !started {
            eprintln!("[perry/audio] failed to start engine");
            return false;
        }
        ENGINE.with(|e| *e.borrow_mut() = Some(engine));
        true
    }
}

/// Borrowed pointer to the AVAudioNode for a bus handle. `0` ⇒ mainMixerNode.
unsafe fn resolve_bus_node(bus_h: f64) -> *mut AnyObject {
    match classify(bus_h) {
        HandleKind::Master => ENGINE.with(|e| {
            let b = e.borrow();
            match b.as_ref() {
                Some(eng) => msg_send![&**eng, mainMixerNode],
                None => std::ptr::null_mut(),
            }
        }),
        HandleKind::Bus(idx) => BUSES.with(|b| {
            let buses = b.borrow();
            match buses.get(idx) {
                Some(Some(entry)) => &*entry.mixer as *const AnyObject as *mut AnyObject,
                _ => std::ptr::null_mut(),
            }
        }),
        _ => std::ptr::null_mut(),
    }
}

// =============================================================================
// loadSound / unload / onLoaded
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_audio_load_sound(path_ptr: i64, bus: f64, stream: f64) -> i64 {
    if !ensure_engine() {
        return 0;
    }
    let is_streaming = stream != 0.0;

    let filename = str_from_header(path_ptr as *const u8);
    if filename.is_empty() {
        return 0;
    }
    let (name, ext) = if let Some(dot) = filename.rfind('.') {
        (&filename[..dot], &filename[dot + 1..])
    } else {
        (filename, "m4a")
    };

    unsafe {
        let bundle_cls = match AnyClass::get(c"NSBundle") {
            Some(c) => c,
            None => return 0,
        };
        let bundle: *mut AnyObject = msg_send![bundle_cls, mainBundle];
        if bundle.is_null() {
            return 0;
        }
        let ns_name = objc2_foundation::NSString::from_str(name);
        let ns_ext = objc2_foundation::NSString::from_str(ext);
        let mut url: *mut AnyObject =
            msg_send![bundle, URLForResource: &*ns_name withExtension: &*ns_ext];
        if url.is_null() {
            let ns_subdir = objc2_foundation::NSString::from_str("sounds");
            url = msg_send![
                bundle,
                URLForResource: &*ns_name
                withExtension: &*ns_ext
                subdirectory: &*ns_subdir
            ];
        }
        if url.is_null() {
            let url_cls = match AnyClass::get(c"NSURL") {
                Some(c) => c,
                None => return 0,
            };
            let ns_path = objc2_foundation::NSString::from_str(filename);
            url = msg_send![url_cls, fileURLWithPath: &*ns_path];
        }
        if url.is_null() {
            eprintln!("[perry/audio] loadSound: file not found: {}", filename);
            return 0;
        }

        let file_cls = match AnyClass::get(c"AVAudioFile") {
            Some(c) => c,
            None => return 0,
        };
        let file_alloc: *mut AnyObject = msg_send![file_cls, alloc];
        let mut error: *mut AnyObject = std::ptr::null_mut();
        let file_raw: *mut AnyObject =
            msg_send![file_alloc, initForReading: url, error: &mut error];
        if file_raw.is_null() || !error.is_null() {
            eprintln!("[perry/audio] loadSound: open failed: {}", filename);
            return 0;
        }
        let file: Retained<AnyObject> = Retained::retain(file_raw).unwrap();

        let format_raw: *mut AnyObject = msg_send![&*file, processingFormat];
        if format_raw.is_null() {
            return 0;
        }
        let format: Retained<AnyObject> = Retained::retain(format_raw).unwrap();
        let frame_count: i64 = msg_send![&*file, length];
        if frame_count <= 0 {
            return 0;
        }
        let sample_rate: f64 = msg_send![&*format, sampleRate];
        let capacity = frame_count as u32;

        let buffer_opt = if is_streaming {
            None
        } else {
            let buffer_cls = match AnyClass::get(c"AVAudioPCMBuffer") {
                Some(c) => c,
                None => return 0,
            };
            let buf_alloc: *mut AnyObject = msg_send![buffer_cls, alloc];
            let format_ptr: *mut AnyObject = &*format as *const AnyObject as *mut AnyObject;
            let buffer_raw: *mut AnyObject = msg_send![
                buf_alloc,
                initWithPCMFormat: format_ptr
                frameCapacity: capacity
            ];
            if buffer_raw.is_null() {
                return 0;
            }
            let buffer = Retained::retain(buffer_raw).unwrap();

            error = std::ptr::null_mut();
            let read_ok: bool = msg_send![&*file, readIntoBuffer: &*buffer error: &mut error];
            if !read_ok || !error.is_null() {
                eprintln!("[perry/audio] loadSound: read failed: {}", filename);
                return 0;
            }
            Some(buffer)
        };

        let entry = SoundEntry {
            buffer: buffer_opt,
            file: Some(file),
            format,
            sample_rate,
            frame_length: capacity,
            default_volume: 1.0,
            bus_handle: bus,
            is_streaming,
            on_loaded: None,
        };
        let idx = SOUNDS.with(|s| slot_insert(&mut s.borrow_mut(), entry));
        sound_handle_id(idx)
    }
}

#[no_mangle]
pub extern "C" fn perry_audio_unload(sound: f64) {
    let idx = match classify(sound) {
        HandleKind::Sound(i) => i,
        _ => return,
    };
    // Stop and remove any live voices using this sound first.
    stop_voices_of_sound(idx, 0.0);
    SOUNDS.with(|s| {
        let mut sounds = s.borrow_mut();
        if let Some(slot) = sounds.get_mut(idx) {
            slot.take();
        }
    });
}

#[no_mangle]
pub extern "C" fn perry_audio_on_loaded(sound: f64, callback: f64) {
    let idx = match classify(sound) {
        HandleKind::Sound(i) => i,
        _ => return,
    };
    // We preload synchronously, so just queue a fire on the next pump.
    SOUNDS.with(|s| {
        let mut sounds = s.borrow_mut();
        if let Some(Some(entry)) = sounds.get_mut(idx) {
            entry.on_loaded = Some(callback);
        } else {
            return;
        }
    });
    PENDING_LOADED.with(|p| p.borrow_mut().push(idx));
    drain_pending_callbacks();
}

// =============================================================================
// play / stop / pause / resume
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_audio_play(
    sound: f64,
    volume: f64,
    loop_: f64,
    rate: f64,
    pan: f64,
    fade_in_ms: f64,
) -> i64 {
    let _ = pan;

    let sound_idx = match classify(sound) {
        HandleKind::Sound(i) => i,
        _ => return 0,
    };
    if !ensure_engine() {
        return 0;
    }

    // Snapshot what we need from the sound entry (need to release the
    // SOUNDS borrow before allocating a voice that may itself touch SOUNDS).
    let (buffer_opt, file_opt, format_ret, default_volume, bus_h, is_streaming) =
        match SOUNDS.with(|s| {
            let sounds = s.borrow();
            sounds.get(sound_idx).and_then(|o| o.as_ref()).map(|e| {
                (
                    e.buffer.clone(),
                    e.file.clone(),
                    e.format.clone(),
                    e.default_volume,
                    e.bus_handle,
                    e.is_streaming,
                )
            })
        }) {
            Some(t) => t,
            None => return 0,
        };

    let final_vol = if volume >= 0.0 {
        (volume as f32).clamp(0.0, 1.0)
    } else {
        default_volume
    };
    let looping = loop_ != 0.0;
    let fade_in = fade_in_ms > 0.0;
    let initial_vol = if fade_in { 0.0_f32 } else { final_vol };

    unsafe {
        let player_cls = match AnyClass::get(c"AVAudioPlayerNode") {
            Some(c) => c,
            None => return 0,
        };
        let player_node: Retained<AnyObject> = msg_send![player_cls, new];

        // Attach to engine, connect player -> [varispeed] -> bus mixer.
        let bus_node = resolve_bus_node(bus_h);
        if bus_node.is_null() {
            return 0;
        }
        // For streaming we don't have a PCM buffer; AVAudioPlayerNode
        // accepts the file's processingFormat directly for connection.
        let buf_format: *mut AnyObject = if let Some(buf) = buffer_opt.as_ref() {
            msg_send![&**buf, format]
        } else {
            &*format_ret as *const AnyObject as *mut AnyObject
        };

        // Try to insert AVAudioUnitVarispeed between player and bus. If the
        // class is unavailable on this platform/version, fall back to direct
        // player→bus connection (current rate=1.0 only behavior).
        let rate_clamped = (rate as f32).clamp(0.25, 4.0);
        let varispeed_opt: Option<Retained<AnyObject>> =
            match AnyClass::get(c"AVAudioUnitVarispeed") {
                Some(vs_cls) => {
                    let vs: Retained<AnyObject> = msg_send![vs_cls, new];
                    let _: () = msg_send![&*vs, setRate: rate_clamped];
                    Some(vs)
                }
                None => None,
            };

        ENGINE.with(|e| {
            let b = e.borrow();
            if let Some(engine) = b.as_ref() {
                let _: () = msg_send![&**engine, attachNode: &*player_node];
                if let Some(vs) = varispeed_opt.as_ref() {
                    let _: () = msg_send![&**engine, attachNode: &**vs];
                    let _: () = msg_send![
                        &**engine,
                        connect: &*player_node
                        to: &**vs
                        format: buf_format
                    ];
                    let _: () = msg_send![
                        &**engine,
                        connect: &**vs
                        to: bus_node
                        format: buf_format
                    ];
                } else {
                    WARNED_VARISPEED_FALLBACK.with(|w| {
                        if !*w.borrow() {
                            eprintln!(
                                "[perry/audio] AVAudioUnitVarispeed unavailable; setRate will be a no-op"
                            );
                            *w.borrow_mut() = true;
                        }
                    });
                    let _: () = msg_send![
                        &**engine,
                        connect: &*player_node
                        to: bus_node
                        format: buf_format
                    ];
                }
            }
        });

        let _: () = msg_send![&*player_node, setVolume: initial_vol];

        // Insert the voice entry first so the completion handler can
        // reference our slot index.
        let entry = VoiceEntry {
            player_node: player_node.clone(),
            varispeed: varispeed_opt,
            sound_idx,
            bus_handle: bus_h,
            is_playing: true,
            is_paused: false,
            looping,
            volume: initial_vol,
            on_ended: None,
            manually_stopped: false,
        };
        let voice_idx = VOICES.with(|v| slot_insert(&mut v.borrow_mut(), entry));

        if is_streaming {
            // Streaming uses scheduleFile (no native looping flag — we
            // re-schedule from the ended pump when looping is set).
            if let Some(file) = file_opt.as_ref() {
                let cb = make_ended_block(voice_idx);
                let _: () = msg_send![
                    &*player_node,
                    scheduleFile: &**file
                    atTime: std::ptr::null::<AnyObject>()
                    completionHandler: &*cb
                ];
                std::mem::forget(cb);
            }
        } else if let Some(buffer_ret) = buffer_opt.as_ref() {
            // Preloaded PCM buffer. AVAudioPlayerNodeBufferLoops (= 1<<0)
            // does the loop natively for non-streaming sounds.
            let cb = make_ended_block(voice_idx);
            let options: u64 = if looping {
                1 /* AVAudioPlayerNodeBufferLoops */
            } else {
                0
            };
            let _: () = msg_send![
                &*player_node,
                scheduleBuffer: &**buffer_ret
                atTime: std::ptr::null::<AnyObject>()
                options: options
                completionHandler: &*cb
            ];
            std::mem::forget(cb);
        }

        let _: () = msg_send![&*player_node, play];

        if fade_in {
            schedule_fade(
                playback_handle_id(voice_idx) as f64,
                initial_vol,
                final_vol,
                fade_in_ms,
                false,
            );
        }

        playback_handle_id(voice_idx)
    }
}

/// Build a `void(^)()` completion block that, when invoked, queues the voice
/// for end-of-life cleanup. AVAudioPlayerNode fires this on its render thread
/// — we push to PENDING_ENDED and let the next pump invoke the JS callback.
fn make_ended_block(voice_idx: usize) -> block2::RcBlock<dyn Fn()> {
    block2::RcBlock::new(move || {
        // Note: this fires on an audio thread. We can't touch thread_local!
        // safely from another thread. Use a Mutex-backed crossthread queue
        // instead.
        cross_thread_push_ended(voice_idx);
    })
}

// =============================================================================
// Cross-thread ended queue (audio thread → main thread).
// =============================================================================

use std::sync::Mutex;
static CROSS_ENDED: Mutex<Vec<usize>> = Mutex::new(Vec::new());

fn cross_thread_push_ended(idx: usize) {
    if let Ok(mut q) = CROSS_ENDED.lock() {
        q.push(idx);
    }
}

fn drain_cross_ended() {
    let drained: Vec<usize> = match CROSS_ENDED.lock() {
        Ok(mut q) => std::mem::take(&mut *q),
        Err(_) => Vec::new(),
    };
    if drained.is_empty() {
        return;
    }
    PENDING_ENDED.with(|p| p.borrow_mut().extend(drained));
}

fn drain_pending_callbacks() {
    drain_cross_ended();

    // onLoaded
    let loaded: Vec<usize> = PENDING_LOADED.with(|p| std::mem::take(&mut *p.borrow_mut()));
    for idx in loaded {
        let cb = SOUNDS.with(|s| {
            let mut sounds = s.borrow_mut();
            sounds
                .get_mut(idx)
                .and_then(|o| o.as_mut())
                .and_then(|e| e.on_loaded.take())
        });
        if let Some(closure_f64) = cb {
            unsafe {
                js_run_stdlib_pump();
                let _ = js_promise_run_microtasks();
                let ptr = js_nanbox_get_pointer(closure_f64);
                let _ = js_closure_call0(ptr as *const u8);
            }
        }
    }

    // onEnded — fire and clean up the voice slot (unless looping).
    let ended: Vec<usize> = PENDING_ENDED.with(|p| std::mem::take(&mut *p.borrow_mut()));
    for idx in ended {
        // First check whether this is a streaming loop voice that needs to
        // be re-scheduled (AVAudioPlayerNode doesn't natively loop
        // scheduleFile:). If so, re-schedule and DON'T fire onEnded.
        let restream = VOICES.with(|v| {
            let voices = v.borrow();
            match voices.get(idx).and_then(|o| o.as_ref()) {
                Some(e) if e.looping && !e.manually_stopped => SOUNDS.with(|s| {
                    let sounds = s.borrow();
                    sounds
                        .get(e.sound_idx)
                        .and_then(|o| o.as_ref())
                        .filter(|se| se.is_streaming)
                        .and_then(|se| se.file.clone())
                        .map(|file| (e.player_node.clone(), file))
                }),
                _ => None,
            }
        });
        if let Some((player_node, file)) = restream {
            unsafe {
                let cb = make_ended_block(idx);
                let _: () = msg_send![
                    &*player_node,
                    scheduleFile: &*file
                    atTime: std::ptr::null::<AnyObject>()
                    completionHandler: &*cb
                ];
                std::mem::forget(cb);
            }
            continue;
        }

        let (cb, should_clean) = VOICES.with(|v| {
            let mut voices = v.borrow_mut();
            match voices.get_mut(idx) {
                Some(Some(entry)) => {
                    let cb = entry.on_ended.take();
                    let clean = !entry.looping;
                    if clean {
                        entry.is_playing = false;
                    }
                    (cb, clean)
                }
                _ => (None, false),
            }
        });
        if let Some(closure_f64) = cb {
            unsafe {
                js_run_stdlib_pump();
                let _ = js_promise_run_microtasks();
                let ptr = js_nanbox_get_pointer(closure_f64);
                let _ = js_closure_call0(ptr as *const u8);
            }
        }
        if should_clean {
            // Detach the player node (and varispeed) from the engine and free the slot.
            let taken = VOICES.with(|v| {
                let mut voices = v.borrow_mut();
                voices.get_mut(idx).and_then(|slot| slot.take())
            });
            if let Some(entry) = taken {
                unsafe {
                    ENGINE.with(|e| {
                        if let Some(engine) = e.borrow().as_ref() {
                            let _: () = msg_send![&**engine, detachNode: &*entry.player_node];
                            if let Some(vs) = entry.varispeed.as_ref() {
                                let _: () = msg_send![&**engine, detachNode: &**vs];
                            }
                        }
                    });
                }
            }
        }
    }
}

fn stop_voices_of_sound(sound_idx: usize, fade_out_ms: f64) {
    // Snapshot voice indices first (so we don't hold the borrow across stop()).
    let indices: Vec<usize> = VOICES.with(|v| {
        v.borrow()
            .iter()
            .enumerate()
            .filter_map(|(i, slot)| match slot {
                Some(e) if e.sound_idx == sound_idx => Some(i),
                _ => None,
            })
            .collect()
    });
    for i in indices {
        stop_voice(i, fade_out_ms);
    }
}

fn stop_voice(voice_idx: usize, fade_out_ms: f64) {
    if fade_out_ms > 0.0 {
        let cur = VOICES.with(|v| {
            v.borrow()
                .get(voice_idx)
                .and_then(|o| o.as_ref())
                .map(|e| e.volume)
        });
        if let Some(cur_vol) = cur {
            schedule_fade(
                playback_handle_id(voice_idx) as f64,
                cur_vol,
                0.0,
                fade_out_ms,
                true,
            );
            return;
        }
    }
    // Instant stop. Mark the voice as manually_stopped before taking it so
    // any in-flight completion handler that fires post-stop knows not to
    // re-schedule a streaming loop.
    VOICES.with(|v| {
        let mut voices = v.borrow_mut();
        if let Some(Some(entry)) = voices.get_mut(voice_idx) {
            entry.manually_stopped = true;
        }
    });
    let taken = VOICES.with(|v| {
        let mut voices = v.borrow_mut();
        voices.get_mut(voice_idx).and_then(|slot| slot.take())
    });
    if let Some(entry) = taken {
        unsafe {
            let _: () = msg_send![&*entry.player_node, stop];
            ENGINE.with(|e| {
                if let Some(engine) = e.borrow().as_ref() {
                    let _: () = msg_send![&**engine, detachNode: &*entry.player_node];
                    if let Some(vs) = entry.varispeed.as_ref() {
                        let _: () = msg_send![&**engine, detachNode: &**vs];
                    }
                }
            });
        }
    }
}

#[no_mangle]
pub extern "C" fn perry_audio_stop(handle: f64, fade_out_ms: f64) {
    match classify(handle) {
        HandleKind::Playback(idx) => stop_voice(idx, fade_out_ms),
        HandleKind::Sound(idx) => stop_voices_of_sound(idx, 0.0),
        _ => {}
    }
    drain_pending_callbacks();
}

#[no_mangle]
pub extern "C" fn perry_audio_pause(playback: f64) {
    let idx = match classify(playback) {
        HandleKind::Playback(i) => i,
        _ => return,
    };
    VOICES.with(|v| {
        let mut voices = v.borrow_mut();
        if let Some(Some(entry)) = voices.get_mut(idx) {
            if entry.is_playing && !entry.is_paused {
                unsafe {
                    let _: () = msg_send![&*entry.player_node, pause];
                }
                entry.is_paused = true;
            }
        }
    });
}

#[no_mangle]
pub extern "C" fn perry_audio_resume(playback: f64) {
    let idx = match classify(playback) {
        HandleKind::Playback(i) => i,
        _ => return,
    };
    VOICES.with(|v| {
        let mut voices = v.borrow_mut();
        if let Some(Some(entry)) = voices.get_mut(idx) {
            if entry.is_paused {
                unsafe {
                    let _: () = msg_send![&*entry.player_node, play];
                }
                entry.is_paused = false;
            }
        }
    });
}

// =============================================================================
// setVolume / setRate / setPan
// =============================================================================

fn apply_volume_now(handle: f64, vol: f32) {
    let vol = vol.clamp(0.0, 1.0);
    match classify(handle) {
        HandleKind::Master => unsafe {
            ENGINE.with(|e| {
                if let Some(engine) = e.borrow().as_ref() {
                    let mixer: *mut AnyObject = msg_send![&**engine, mainMixerNode];
                    let _: () = msg_send![mixer, setOutputVolume: vol];
                }
            });
        },
        HandleKind::Bus(idx) => {
            BUSES.with(|b| {
                let mut buses = b.borrow_mut();
                if let Some(Some(entry)) = buses.get_mut(idx) {
                    entry.volume = vol;
                }
            });
            // Let solo/mute state decide the effective output volume.
            reapply_solo_state();
        }
        HandleKind::Sound(idx) => {
            SOUNDS.with(|s| {
                let mut sounds = s.borrow_mut();
                if let Some(Some(entry)) = sounds.get_mut(idx) {
                    entry.default_volume = vol;
                }
            });
        }
        HandleKind::Playback(idx) => {
            VOICES.with(|v| {
                let mut voices = v.borrow_mut();
                if let Some(Some(entry)) = voices.get_mut(idx) {
                    entry.volume = vol;
                    unsafe {
                        let _: () = msg_send![&*entry.player_node, setVolume: vol];
                    }
                }
            });
        }
        HandleKind::Invalid => {}
    }
}

fn current_volume(handle: f64) -> Option<f32> {
    match classify(handle) {
        HandleKind::Master => unsafe {
            ENGINE.with(|e| {
                e.borrow().as_ref().map(|engine| {
                    let mixer: *mut AnyObject = msg_send![&**engine, mainMixerNode];
                    let v: f32 = msg_send![mixer, outputVolume];
                    v
                })
            })
        },
        HandleKind::Bus(idx) => BUSES.with(|b| {
            b.borrow()
                .get(idx)
                .and_then(|o| o.as_ref())
                .map(|e| e.volume)
        }),
        HandleKind::Sound(idx) => SOUNDS.with(|s| {
            s.borrow()
                .get(idx)
                .and_then(|o| o.as_ref())
                .map(|e| e.default_volume)
        }),
        HandleKind::Playback(idx) => VOICES.with(|v| {
            v.borrow()
                .get(idx)
                .and_then(|o| o.as_ref())
                .map(|e| e.volume)
        }),
        HandleKind::Invalid => None,
    }
}

#[no_mangle]
pub extern "C" fn perry_audio_set_volume(handle: f64, volume: f64, fade_ms: f64) {
    let target = (volume as f32).clamp(0.0, 1.0);
    if fade_ms <= 0.0 {
        apply_volume_now(handle, target);
        return;
    }
    let start = current_volume(handle).unwrap_or(target);
    schedule_fade(handle, start, target, fade_ms, false);
}

#[no_mangle]
pub extern "C" fn perry_audio_set_rate(playback: f64, rate: f64) {
    let idx = match classify(playback) {
        HandleKind::Playback(i) => i,
        _ => return,
    };
    let rate_f = (rate as f32).clamp(0.25, 4.0);
    VOICES.with(|v| {
        let voices = v.borrow();
        if let Some(Some(entry)) = voices.get(idx) {
            if let Some(vs) = entry.varispeed.as_ref() {
                unsafe {
                    let _: () = msg_send![&**vs, setRate: rate_f];
                }
            }
        }
    });
}

#[no_mangle]
pub extern "C" fn perry_audio_set_pan(playback: f64, pan: f64) {
    let _ = pan;
    let idx = match classify(playback) {
        HandleKind::Playback(i) => i,
        _ => {
            WARNED_SET_PAN.with(|w| {
                if !*w.borrow() {
                    eprintln!("[perry/audio] TODO: setPan only supported on voices");
                    *w.borrow_mut() = true;
                }
            });
            return;
        }
    };
    // AVAudioPlayerNode supports setPan: directly.
    let pan_f = (pan as f32).clamp(-1.0, 1.0);
    VOICES.with(|v| {
        let voices = v.borrow();
        if let Some(Some(entry)) = voices.get(idx) {
            unsafe {
                let _: () = msg_send![&*entry.player_node, setPan: pan_f];
            }
        }
    });
}

// =============================================================================
// fadeIn / fadeOut / crossfade
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_audio_fade_in(playback: f64, duration_ms: f64, to_vol: f64) {
    let target = (to_vol as f32).clamp(0.0, 1.0);
    let start = current_volume(playback).unwrap_or(0.0);
    if duration_ms <= 0.0 {
        apply_volume_now(playback, target);
        return;
    }
    schedule_fade(playback, start, target, duration_ms, false);
}

#[no_mangle]
pub extern "C" fn perry_audio_fade_out(playback: f64, duration_ms: f64) {
    let start = current_volume(playback).unwrap_or(1.0);
    if duration_ms <= 0.0 {
        // Stop instantly.
        match classify(playback) {
            HandleKind::Playback(idx) => stop_voice(idx, 0.0),
            _ => apply_volume_now(playback, 0.0),
        }
        return;
    }
    let then_stop = matches!(classify(playback), HandleKind::Playback(_));
    schedule_fade(playback, start, 0.0, duration_ms, then_stop);
}

#[no_mangle]
pub extern "C" fn perry_audio_crossfade(from: f64, to: f64, duration_ms: f64) {
    // Compose fade_out(from) + fade_in(to, 1.0).
    perry_audio_fade_out(from, duration_ms);
    perry_audio_fade_in(to, duration_ms, 1.0);
}

// =============================================================================
// Buses
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_audio_create_bus(name_ptr: i64, parent: f64) -> i64 {
    if !ensure_engine() {
        return 0;
    }
    let name = str_from_header(name_ptr as *const u8).to_string();
    // Validate parent
    match classify(parent) {
        HandleKind::Master | HandleKind::Bus(_) => {}
        _ => {
            eprintln!("[perry/audio] createBus: invalid parent handle");
            return 0;
        }
    }
    unsafe {
        let mixer_cls = match AnyClass::get(c"AVAudioMixerNode") {
            Some(c) => c,
            None => return 0,
        };
        let mixer: Retained<AnyObject> = msg_send![mixer_cls, new];

        let parent_node = resolve_bus_node(parent);
        if parent_node.is_null() {
            return 0;
        }

        let attached: bool = ENGINE.with(|e| {
            let b = e.borrow();
            if let Some(engine) = b.as_ref() {
                let _: () = msg_send![&**engine, attachNode: &*mixer];
                let null_fmt: *const AnyObject = std::ptr::null();
                let _: () = msg_send![
                    &**engine,
                    connect: &*mixer
                    to: parent_node
                    format: null_fmt
                ];
                true
            } else {
                false
            }
        });
        if !attached {
            return 0;
        }

        let entry = BusEntry {
            mixer,
            _name: name,
            parent_id: parent,
            volume: 1.0,
            muted: false,
            pre_mute_volume: 1.0,
            soloed: false,
        };
        let idx = BUSES.with(|b| slot_insert(&mut b.borrow_mut(), entry));
        bus_handle_id(idx)
    }
}

#[no_mangle]
pub extern "C" fn perry_audio_destroy_bus(bus: f64) {
    let idx = match classify(bus) {
        HandleKind::Bus(i) => i,
        _ => return,
    };
    let node_opt = BUSES.with(|b| {
        let mut buses = b.borrow_mut();
        buses
            .get_mut(idx)
            .and_then(|slot| slot.take())
            .map(|e| e.mixer)
    });
    if let Some(mixer) = node_opt {
        unsafe {
            ENGINE.with(|e| {
                if let Some(engine) = e.borrow().as_ref() {
                    let _: () = msg_send![&**engine, detachNode: &*mixer];
                }
            });
        }
    }
}

#[no_mangle]
pub extern "C" fn perry_audio_mute_bus(bus: f64, muted: f64) {
    let idx = match classify(bus) {
        HandleKind::Bus(i) => i,
        _ => return,
    };
    let should_mute = muted != 0.0;
    BUSES.with(|b| {
        let mut buses = b.borrow_mut();
        if let Some(Some(entry)) = buses.get_mut(idx) {
            if should_mute && !entry.muted {
                entry.pre_mute_volume = entry.volume;
                entry.muted = true;
            } else if !should_mute && entry.muted {
                entry.muted = false;
                entry.volume = entry.pre_mute_volume;
            }
        }
    });
    reapply_solo_state();
}

#[no_mangle]
pub extern "C" fn perry_audio_solo_bus(bus: f64, soloed: f64) {
    let idx = match classify(bus) {
        HandleKind::Bus(i) => i,
        _ => return,
    };
    let want = soloed != 0.0;
    BUSES.with(|b| {
        let mut buses = b.borrow_mut();
        if let Some(Some(entry)) = buses.get_mut(idx) {
            entry.soloed = want;
        }
    });
    reapply_solo_state();
}

/// Walk every bus and decide its effective output volume given current
/// solo flags. If ANY bus is soloed, every bus that is neither itself
/// soloed nor an ancestor of a soloed bus is forced to volume 0. The
/// master mixer is always considered an "ancestor of every bus" and is
/// left alone. When no bus is soloed, restore each bus to its stored
/// volume (modulo mute).
fn reapply_solo_state() {
    // Collect the soloed set first.
    let (any_solo, soloed_indices) = BUSES.with(|b| {
        let buses = b.borrow();
        let solo: Vec<usize> = buses
            .iter()
            .enumerate()
            .filter_map(|(i, slot)| match slot {
                Some(e) if e.soloed => Some(i),
                _ => None,
            })
            .collect();
        (!solo.is_empty(), solo)
    });

    // For each soloed bus, walk parents up to master and mark them
    // "audible". Anything not in the audible set is silenced.
    let audible: std::collections::HashSet<usize> = if any_solo {
        let mut set = std::collections::HashSet::new();
        BUSES.with(|b| {
            let buses = b.borrow();
            for start in &soloed_indices {
                let mut cur = Some(*start);
                while let Some(i) = cur {
                    if !set.insert(i) {
                        break;
                    }
                    let parent = buses.get(i).and_then(|o| o.as_ref()).map(|e| e.parent_id);
                    cur = match parent {
                        Some(p) => match classify(p) {
                            HandleKind::Bus(pi) => Some(pi),
                            _ => None,
                        },
                        None => None,
                    };
                }
            }
        });
        set
    } else {
        std::collections::HashSet::new()
    };

    // Apply per-bus effective volume.
    BUSES.with(|b| {
        let buses = b.borrow();
        for (i, slot) in buses.iter().enumerate() {
            if let Some(entry) = slot.as_ref() {
                let effective: f32 = if entry.muted {
                    0.0
                } else if any_solo && !audible.contains(&i) {
                    0.0
                } else {
                    entry.volume
                };
                unsafe {
                    let _: () = msg_send![&*entry.mixer, setOutputVolume: effective];
                }
            }
        }
    });
}

// =============================================================================
// Master / engine
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_audio_set_master_volume(volume: f64, fade_ms: f64) {
    if fade_ms <= 0.0 {
        apply_volume_now(0.0, (volume as f32).clamp(0.0, 1.0));
        return;
    }
    let start = current_volume(0.0).unwrap_or(1.0);
    schedule_fade(0.0, start, (volume as f32).clamp(0.0, 1.0), fade_ms, false);
}

#[no_mangle]
pub extern "C" fn perry_audio_suspend() {
    unsafe {
        ENGINE.with(|e| {
            if let Some(engine) = e.borrow().as_ref() {
                let _: () = msg_send![&**engine, pause];
            }
        });
    }
}

#[no_mangle]
pub extern "C" fn perry_audio_resume_all() {
    unsafe {
        ENGINE.with(|e| {
            if let Some(engine) = e.borrow().as_ref() {
                let mut error: *mut AnyObject = std::ptr::null_mut();
                let _: bool = msg_send![&**engine, startAndReturnError: &mut error];
            }
        });
    }
}

// =============================================================================
// Introspection
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_audio_is_playing(handle: f64) -> f64 {
    match classify(handle) {
        HandleKind::Playback(idx) => {
            VOICES.with(|v| match v.borrow().get(idx).and_then(|o| o.as_ref()) {
                Some(e) if e.is_playing && !e.is_paused => 1.0,
                _ => 0.0,
            })
        }
        HandleKind::Sound(idx) => VOICES.with(|v| {
            let any = v.borrow().iter().any(|slot| match slot {
                Some(e) => e.sound_idx == idx && e.is_playing && !e.is_paused,
                None => false,
            });
            if any {
                1.0
            } else {
                0.0
            }
        }),
        _ => 0.0,
    }
}

#[no_mangle]
pub extern "C" fn perry_audio_get_duration(sound: f64) -> f64 {
    match classify(sound) {
        HandleKind::Sound(idx) => {
            SOUNDS.with(|s| match s.borrow().get(idx).and_then(|o| o.as_ref()) {
                Some(e) if e.sample_rate > 0.0 => e.frame_length as f64 / e.sample_rate,
                _ => 0.0,
            })
        }
        _ => 0.0,
    }
}

#[no_mangle]
pub extern "C" fn perry_audio_get_position(playback: f64) -> f64 {
    let idx = match classify(playback) {
        HandleKind::Playback(i) => i,
        _ => return 0.0,
    };
    VOICES.with(|v| {
        let voices = v.borrow();
        let node = match voices.get(idx).and_then(|o| o.as_ref()) {
            Some(e) => e.player_node.clone(),
            None => return 0.0,
        };
        unsafe {
            let node_time: *mut AnyObject = msg_send![&*node, lastRenderTime];
            if node_time.is_null() {
                return 0.0;
            }
            let player_time: *mut AnyObject = msg_send![&*node, playerTimeForNodeTime: node_time];
            if player_time.is_null() {
                return 0.0;
            }
            let sample_time: i64 = msg_send![player_time, sampleTime];
            let sample_rate: f64 = msg_send![player_time, sampleRate];
            if sample_rate <= 0.0 {
                return 0.0;
            }
            (sample_time as f64) / sample_rate
        }
    })
}

// =============================================================================
// Callbacks: onEnded
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_audio_on_ended(playback: f64, callback: f64) {
    let idx = match classify(playback) {
        HandleKind::Playback(i) => i,
        _ => return,
    };
    VOICES.with(|v| {
        let mut voices = v.borrow_mut();
        if let Some(Some(entry)) = voices.get_mut(idx) {
            entry.on_ended = Some(callback);
        }
    });
}

// =============================================================================
// Fade timer
// =============================================================================

const FADE_HZ: f64 = 60.0;

fn schedule_fade(handle: f64, start: f32, target: f32, duration_ms: f64, then_stop: bool) {
    let ticks = ((duration_ms / 1000.0) * FADE_HZ).max(1.0) as u32;
    // Remove any existing fade for this handle first.
    FADES.with(|f| {
        let mut fades = f.borrow_mut();
        fades.retain(|fade| fade.handle != handle);
        fades.push(Fade {
            handle,
            start_vol: start,
            target_vol: target,
            ticks_total: ticks,
            ticks_left: ticks,
            then_stop,
        });
    });
    ensure_fade_timer();
}

unsafe extern "C" fn fade_tick(
    _this: *mut AnyObject,
    _sel: *const std::ffi::c_void,
    _timer: *mut AnyObject,
) {
    // Drain finished/active fades.
    let to_apply: Vec<(f64, f32, bool)> = FADES.with(|f| {
        let mut fades = f.borrow_mut();
        let mut out: Vec<(f64, f32, bool)> = Vec::new();
        fades.retain_mut(|fade| {
            if fade.ticks_left == 0 {
                return false;
            }
            fade.ticks_left -= 1;
            let done = fade.ticks_left == 0;
            let t = if fade.ticks_total == 0 {
                1.0
            } else {
                1.0 - (fade.ticks_left as f32 / fade.ticks_total as f32)
            };
            let v = fade.start_vol + (fade.target_vol - fade.start_vol) * t;
            out.push((fade.handle, v, done && fade.then_stop));
            !done
        });
        out
    });
    for (handle, vol, then_stop) in to_apply {
        apply_volume_now(handle, vol);
        if then_stop {
            if let HandleKind::Playback(idx) = classify(handle) {
                stop_voice(idx, 0.0);
            }
        }
    }
    // Drain JS callbacks while we're here on the main thread.
    drain_pending_callbacks();

    // Tear down the timer if no fades remain.
    let empty = FADES.with(|f| f.borrow().is_empty());
    if empty {
        FADE_TIMER.with(|ft| {
            if let Some(timer) = ft.borrow_mut().take() {
                let _: () = msg_send![&*timer, invalidate];
            }
        });
        FADE_TIMER_TARGET.with(|t| {
            t.borrow_mut().take();
        });
    }
}

fn register_fade_timer_class() {
    FADE_CLASS_REGISTERED.with(|reg| {
        if *reg.borrow() {
            return;
        }
        *reg.borrow_mut() = true;
        unsafe {
            let superclass = objc_getClass(c"NSObject".as_ptr());
            let cls = objc_allocateClassPair(superclass, c"PerryAudioFadeTarget".as_ptr(), 0);
            if cls.is_null() {
                return;
            }
            let sel = sel_registerName(c"fadeTick:".as_ptr());
            class_addMethod(
                cls,
                sel,
                fade_tick as *const std::ffi::c_void,
                c"v@:@".as_ptr(),
            );
            objc_registerClassPair(cls);
        }
    });
}

fn ensure_fade_timer() {
    if FADE_TIMER.with(|ft| ft.borrow().is_some()) {
        return;
    }
    register_fade_timer_class();
    unsafe {
        let target_cls = match AnyClass::get(c"PerryAudioFadeTarget") {
            Some(c) => c,
            None => return,
        };
        let target: Retained<AnyObject> = msg_send![target_cls, new];
        let sel = Sel::register(c"fadeTick:");
        let timer: Retained<AnyObject> = msg_send![
            objc2::class!(NSTimer),
            scheduledTimerWithTimeInterval: (1.0 / FADE_HZ),
            target: &*target,
            selector: sel,
            userInfo: std::ptr::null::<AnyObject>(),
            repeats: true
        ];
        FADE_TIMER.with(|ft| *ft.borrow_mut() = Some(timer));
        FADE_TIMER_TARGET.with(|t| *t.borrow_mut() = Some(target));
    }
}
