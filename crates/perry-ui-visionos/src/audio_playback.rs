use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, Sel};
use objc2::msg_send;
use std::cell::RefCell;

extern "C" {
    fn js_nanbox_get_pointer(value: f64) -> i64;
    fn js_closure_call1(closure: *const u8, arg: f64) -> f64;
    fn js_stdlib_process_pending();
    fn js_promise_run_microtasks() -> i32;
}

// Raw ObjC runtime FFI for dynamic class registration
extern "C" {
    fn objc_allocateClassPair(
        superclass: *const std::ffi::c_void,
        name: *const i8,
        extra_bytes: usize,
    ) -> *mut std::ffi::c_void;
    fn objc_registerClassPair(cls: *mut std::ffi::c_void);
    fn class_addMethod(
        cls: *mut std::ffi::c_void,
        sel: *const std::ffi::c_void,
        imp: *const std::ffi::c_void,
        types: *const i8,
    ) -> bool;
    fn sel_registerName(name: *const i8) -> *const std::ffi::c_void;
    fn objc_getClass(name: *const i8) -> *const std::ffi::c_void;
}

// =============================================================================
// Data structures
// =============================================================================

struct PlayerEntry {
    player_node: Retained<AnyObject>,  // AVAudioPlayerNode
    buffer: Retained<AnyObject>,       // AVAudioPCMBuffer
    _format: Retained<AnyObject>,      // AVAudioFormat
    volume: f64,
    is_playing: bool,
    // Fade state
    fade_target: f64,
    fade_step: f64,
    fade_ticks_remaining: u32,
}

// =============================================================================
// Thread-local state
// =============================================================================

thread_local! {
    static PLAYBACK_ENGINE: RefCell<Option<Retained<AnyObject>>> = RefCell::new(None);
    static PLAYERS: RefCell<Vec<Option<PlayerEntry>>> = RefCell::new(Vec::new());
    static INTERRUPTION_CALLBACK: RefCell<Option<f64>> = RefCell::new(None);
    static FADE_TIMER: RefCell<Option<Retained<AnyObject>>> = RefCell::new(None);
    static FADE_TIMER_TARGET: RefCell<Option<Retained<AnyObject>>> = RefCell::new(None);
    static FADE_CLASS_REGISTERED: RefCell<bool> = RefCell::new(false);
    static INTERRUPTION_CLASS_REGISTERED: RefCell<bool> = RefCell::new(false);
}

// =============================================================================
// Helper: extract string from StringHeader pointer
// =============================================================================

fn str_from_header(ptr: *const u8) -> &'static str {
    if ptr.is_null() {
        return "";
    }
    unsafe {
        let header = ptr as *const perry_runtime::string::StringHeader;
        let len = (*header).byte_len as usize;
        let data = ptr.add(std::mem::size_of::<perry_runtime::string::StringHeader>());
        std::str::from_utf8_unchecked(std::slice::from_raw_parts(data, len))
    }
}

// =============================================================================
// Engine initialisation (lazy, on first player_create)
// =============================================================================

fn ensure_engine() -> bool {
    let already_running = PLAYBACK_ENGINE.with(|e| e.borrow().is_some());
    if already_running {
        return true;
    }

    unsafe {
        // Configure audio session for playback
        let session_cls = match AnyClass::get(c"AVAudioSession") {
            Some(cls) => cls,
            None => {
                crate::ws_log!("[audio_playback] AVAudioSession not found");
                return false;
            }
        };
        let session: *mut AnyObject = msg_send![session_cls, sharedInstance];
        if session.is_null() {
            crate::ws_log!("[audio_playback] sharedInstance is null");
            return false;
        }

        // Set category to Playback
        let category = objc2_foundation::NSString::from_str("AVAudioSessionCategoryPlayback");
        let mut error: *mut AnyObject = std::ptr::null_mut();
        let _: bool = msg_send![session, setCategory: &*category error: &mut error];
        if !error.is_null() {
            crate::ws_log!("[audio_playback] failed to set audio session category");
            return false;
        }

        // Activate the session
        error = std::ptr::null_mut();
        let _: bool = msg_send![session, setActive: true error: &mut error];
        if !error.is_null() {
            crate::ws_log!("[audio_playback] failed to activate audio session");
            return false;
        }

        // Create AVAudioEngine
        let engine_cls = match AnyClass::get(c"AVAudioEngine") {
            Some(cls) => cls,
            None => {
                crate::ws_log!("[audio_playback] AVAudioEngine not found");
                return false;
            }
        };
        let engine: Retained<AnyObject> = msg_send![engine_cls, new];

        // Start engine
        error = std::ptr::null_mut();
        let started: bool = msg_send![&*engine, startAndReturnError: &mut error];
        if !started {
            crate::ws_log!("[audio_playback] failed to start engine");
            return false;
        }

        crate::ws_log!("[audio_playback] engine started successfully");
        PLAYBACK_ENGINE.with(|e| {
            *e.borrow_mut() = Some(engine);
        });
        true
    }
}

// =============================================================================
// Public API
// =============================================================================

/// Create a player for the given audio file. Returns a 1-based handle as f64,
/// or 0.0 on failure.
pub fn player_create(filename_ptr: i64) -> f64 {
    if !ensure_engine() {
        return 0.0;
    }

    let filename = str_from_header(filename_ptr as *const u8);
    if filename.is_empty() {
        crate::ws_log!("[audio_playback] empty filename");
        return 0.0;
    }

    // Split filename into name and extension if it contains a dot;
    // otherwise treat as name with "m4a" extension.
    let (name, ext) = if let Some(dot_pos) = filename.rfind('.') {
        (&filename[..dot_pos], &filename[dot_pos + 1..])
    } else {
        (filename, "m4a")
    };

    unsafe {
        let bundle_cls = AnyClass::get(c"NSBundle").unwrap();
        let bundle: *mut AnyObject = msg_send![bundle_cls, mainBundle];
        if bundle.is_null() {
            crate::ws_log!("[audio_playback] mainBundle is null");
            return 0.0;
        }

        let ns_name = objc2_foundation::NSString::from_str(name);
        let ns_ext = objc2_foundation::NSString::from_str(ext);

        // Try root of bundle first
        let mut url: *mut AnyObject = msg_send![
            bundle,
            URLForResource: &*ns_name
            withExtension: &*ns_ext
        ];

        // Try the sounds/ subdirectory
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
            crate::ws_log!("[audio_playback] file not found: {}.{}", name, ext);
            return 0.0;
        }

        // Create AVAudioFile(forReading: url)
        let file_cls = match AnyClass::get(c"AVAudioFile") {
            Some(cls) => cls,
            None => {
                crate::ws_log!("[audio_playback] AVAudioFile not found");
                return 0.0;
            }
        };
        let file_alloc: *mut AnyObject = msg_send![file_cls, alloc];
        let mut error: *mut AnyObject = std::ptr::null_mut();
        let file: *mut AnyObject = msg_send![file_alloc, initForReading: url error: &mut error];
        if file.is_null() || !error.is_null() {
            crate::ws_log!("[audio_playback] failed to open audio file: {}.{}", name, ext);
            return 0.0;
        }

        // Get processing format and frame count
        let format: *mut AnyObject = msg_send![file, processingFormat];
        if format.is_null() {
            crate::ws_log!("[audio_playback] processingFormat is null");
            return 0.0;
        }
        let format: Retained<AnyObject> = Retained::retain(format).unwrap();
        let frame_count: i64 = msg_send![file, length];
        if frame_count <= 0 {
            crate::ws_log!("[audio_playback] audio file has no frames");
            return 0.0;
        }

        // Create AVAudioPCMBuffer
        let buffer_cls = match AnyClass::get(c"AVAudioPCMBuffer") {
            Some(cls) => cls,
            None => {
                crate::ws_log!("[audio_playback] AVAudioPCMBuffer not found");
                return 0.0;
            }
        };
        let buffer_alloc: *mut AnyObject = msg_send![buffer_cls, alloc];
        let capacity = frame_count as u32;
        let format_ptr: *mut AnyObject = &*format as *const AnyObject as *mut AnyObject;
        let buffer: *mut AnyObject = msg_send![
            buffer_alloc,
            initWithPCMFormat: format_ptr
            frameCapacity: capacity
        ];
        if buffer.is_null() {
            crate::ws_log!("[audio_playback] failed to create AVAudioPCMBuffer");
            return 0.0;
        }
        let buffer = Retained::retain(buffer).unwrap();

        // Read file into buffer
        error = std::ptr::null_mut();
        let read_ok: bool = msg_send![file, readIntoBuffer: &*buffer error: &mut error];
        if !read_ok || !error.is_null() {
            crate::ws_log!("[audio_playback] failed to read audio file into buffer");
            return 0.0;
        }

        // Create AVAudioPlayerNode
        let player_cls = match AnyClass::get(c"AVAudioPlayerNode") {
            Some(cls) => cls,
            None => {
                crate::ws_log!("[audio_playback] AVAudioPlayerNode not found");
                return 0.0;
            }
        };
        let player_node: Retained<AnyObject> = msg_send![player_cls, new];

        // Attach player node to engine and connect
        let handle = PLAYBACK_ENGINE.with(|e| {
            let borrow = e.borrow();
            let engine = borrow.as_ref().unwrap();

            // engine.attachNode(playerNode)
            let _: () = msg_send![&**engine, attachNode: &*player_node];

            // engine.mainMixerNode
            let mixer: *mut AnyObject = msg_send![&**engine, mainMixerNode];

            // Get the buffer's format for the connection
            let buf_format: *mut AnyObject = msg_send![&*buffer, format];

            // engine.connect(playerNode, to: mixer, format: format)
            let _: () = msg_send![
                &**engine,
                connect: &*player_node
                to: mixer
                format: buf_format
            ];

            // Store PlayerEntry
            let entry = PlayerEntry {
                player_node,
                buffer,
                _format: format,
                volume: 1.0,
                is_playing: false,
                fade_target: 1.0,
                fade_step: 0.0,
                fade_ticks_remaining: 0,
            };

            PLAYERS.with(|p| {
                let mut players = p.borrow_mut();
                // Find an empty slot or push a new one
                for (i, slot) in players.iter_mut().enumerate() {
                    if slot.is_none() {
                        *slot = Some(entry);
                        return (i + 1) as f64;
                    }
                }
                players.push(Some(entry));
                players.len() as f64
            })
        });

        crate::ws_log!("[audio_playback] player created, handle={}", handle);
        handle
    }
}

/// Play the audio in a loop.
pub fn player_play(handle: f64) {
    let idx = handle as usize;
    if idx == 0 {
        return;
    }
    let idx = idx - 1;

    PLAYERS.with(|p| {
        let mut players = p.borrow_mut();
        if let Some(Some(entry)) = players.get_mut(idx) {
            unsafe {
                // Schedule buffer for looping:
                // AVAudioPlayerNodeBufferLoops = 2
                let _: () = msg_send![
                    &*entry.player_node,
                    scheduleBuffer: &*entry.buffer
                    atTime: std::ptr::null::<AnyObject>()
                    options: 2u64
                    completionHandler: std::ptr::null::<AnyObject>()
                ];

                let _: () = msg_send![&*entry.player_node, play];
            }
            entry.is_playing = true;
            crate::ws_log!("[audio_playback] player {} playing", handle);
        }
    });
}

/// Stop playback and cancel any active fade.
pub fn player_stop(handle: f64) {
    let idx = handle as usize;
    if idx == 0 {
        return;
    }
    let idx = idx - 1;

    PLAYERS.with(|p| {
        let mut players = p.borrow_mut();
        if let Some(Some(entry)) = players.get_mut(idx) {
            unsafe {
                let _: () = msg_send![&*entry.player_node, stop];
            }
            entry.is_playing = false;
            entry.fade_ticks_remaining = 0;
            crate::ws_log!("[audio_playback] player {} stopped", handle);
        }
    });
}

/// Set volume immediately (0.0 to 1.0).
pub fn player_set_volume(handle: f64, volume: f64) {
    let idx = handle as usize;
    if idx == 0 {
        return;
    }
    let idx = idx - 1;

    PLAYERS.with(|p| {
        let mut players = p.borrow_mut();
        if let Some(Some(entry)) = players.get_mut(idx) {
            let vol = volume.max(0.0).min(1.0);
            unsafe {
                let _: () = msg_send![&*entry.player_node, setVolume: vol as f32];
            }
            entry.volume = vol;
        }
    });
}

/// Fade volume to target over duration seconds.
pub fn player_fade_to(handle: f64, target: f64, duration: f64) {
    let idx = handle as usize;
    if idx == 0 {
        return;
    }
    let idx = idx - 1;

    let target = target.max(0.0).min(1.0);

    PLAYERS.with(|p| {
        let mut players = p.borrow_mut();
        if let Some(Some(entry)) = players.get_mut(idx) {
            if duration <= 0.0 {
                // Instant change
                entry.volume = target;
                entry.fade_ticks_remaining = 0;
                unsafe {
                    let _: () = msg_send![&*entry.player_node, setVolume: target as f32];
                }
                if target == 0.0 {
                    unsafe {
                        let _: () = msg_send![&*entry.player_node, stop];
                    }
                    entry.is_playing = false;
                }
                return;
            }

            let ticks = (duration * 30.0).max(1.0) as u32;
            entry.fade_target = target;
            entry.fade_step = (target - entry.volume) / ticks as f64;
            entry.fade_ticks_remaining = ticks;
        }
    });

    // Ensure the global fade timer is running
    ensure_fade_timer();
}

/// Returns 1.0 if playing, 0.0 otherwise. -1.0 for invalid handle.
pub fn player_is_playing(handle: f64) -> f64 {
    let idx = handle as usize;
    if idx == 0 {
        return -1.0;
    }
    let idx = idx - 1;

    PLAYERS.with(|p| {
        let players = p.borrow();
        match players.get(idx) {
            Some(Some(entry)) => {
                if entry.is_playing { 1.0 } else { 0.0 }
            }
            _ => -1.0,
        }
    })
}

/// Destroy a player, detaching it from the engine and freeing resources.
pub fn player_destroy(handle: f64) {
    let idx = handle as usize;
    if idx == 0 {
        return;
    }
    let idx = idx - 1;

    PLAYERS.with(|p| {
        let mut players = p.borrow_mut();
        if let Some(slot) = players.get_mut(idx) {
            if let Some(entry) = slot.take() {
                unsafe {
                    if entry.is_playing {
                        let _: () = msg_send![&*entry.player_node, stop];
                    }
                    // Detach from engine
                    PLAYBACK_ENGINE.with(|e| {
                        if let Some(engine) = e.borrow().as_ref() {
                            let _: () = msg_send![&**engine, detachNode: &*entry.player_node];
                        }
                    });
                }
                crate::ws_log!("[audio_playback] player {} destroyed", handle);
            }
        }
    });
}

/// Set Now Playing info (lock screen / Control Center metadata).
pub fn player_set_now_playing(title_ptr: i64) {
    let title = str_from_header(title_ptr as *const u8);

    unsafe {
        // MPNowPlayingInfoCenter.defaultCenter
        let center_cls = match AnyClass::get(c"MPNowPlayingInfoCenter") {
            Some(cls) => cls,
            None => {
                crate::ws_log!("[audio_playback] MPNowPlayingInfoCenter not found");
                return;
            }
        };
        let center: *mut AnyObject = msg_send![center_cls, defaultCenter];
        if center.is_null() {
            crate::ws_log!("[audio_playback] defaultCenter is null");
            return;
        }

        // Create NSMutableDictionary
        let dict_cls = AnyClass::get(c"NSMutableDictionary").unwrap();
        let dict: Retained<AnyObject> = msg_send![dict_cls, new];

        // Set title: MPMediaItemPropertyTitle = "title"
        let key = objc2_foundation::NSString::from_str("title");
        let value = objc2_foundation::NSString::from_str(title);
        let _: () = msg_send![&*dict, setObject: &*value forKey: &*key];

        // Set nowPlayingInfo
        let _: () = msg_send![center, setNowPlayingInfo: &*dict];

        // Set up MPRemoteCommandCenter play/pause commands
        let cmd_center_cls = match AnyClass::get(c"MPRemoteCommandCenter") {
            Some(cls) => cls,
            None => {
                crate::ws_log!("[audio_playback] MPRemoteCommandCenter not found");
                return;
            }
        };
        let cmd_center: *mut AnyObject = msg_send![cmd_center_cls, sharedCommandCenter];
        if cmd_center.is_null() {
            return;
        }

        // Enable play command
        let play_cmd: *mut AnyObject = msg_send![cmd_center, playCommand];
        if !play_cmd.is_null() {
            let _: () = msg_send![play_cmd, setEnabled: true];
        }

        // Enable pause command
        let pause_cmd: *mut AnyObject = msg_send![cmd_center, pauseCommand];
        if !pause_cmd.is_null() {
            let _: () = msg_send![pause_cmd, setEnabled: true];
        }

        crate::ws_log!("[audio_playback] now playing set: {}", title);
    }
}

/// Register a JS callback to be invoked on audio session interruption.
/// The callback receives 1.0 when interrupted (began) and 0.0 when resumed (ended).
pub fn player_set_on_interruption(callback: f64) {
    INTERRUPTION_CALLBACK.with(|c| {
        *c.borrow_mut() = Some(callback);
    });

    register_interruption_observer();
}

// =============================================================================
// Fade timer
// =============================================================================

/// The fadeTick: callback — iterates all players with active fades.
unsafe extern "C" fn fade_tick(
    _this: *mut AnyObject,
    _sel: *const std::ffi::c_void,
    _timer: *mut AnyObject,
) {
    let mut any_active = false;

    PLAYERS.with(|p| {
        let mut players = p.borrow_mut();
        for slot in players.iter_mut() {
            if let Some(entry) = slot.as_mut() {
                if entry.fade_ticks_remaining == 0 {
                    continue;
                }

                entry.fade_ticks_remaining -= 1;
                if entry.fade_ticks_remaining == 0 {
                    // Fade complete — snap to exact target
                    entry.volume = entry.fade_target;
                    let _: () = msg_send![&*entry.player_node, setVolume: entry.fade_target as f32];

                    // If faded to silence, stop playback
                    if entry.fade_target == 0.0 {
                        let _: () = msg_send![&*entry.player_node, stop];
                        entry.is_playing = false;
                    }
                } else {
                    entry.volume += entry.fade_step;
                    let vol = entry.volume.max(0.0).min(1.0);
                    entry.volume = vol;
                    let _: () = msg_send![&*entry.player_node, setVolume: vol as f32];
                    any_active = true;
                }
            }
        }
    });

    if !any_active {
        // No fades active — invalidate the timer
        FADE_TIMER.with(|ft| {
            if let Some(timer) = ft.borrow_mut().take() {
                let _: () = msg_send![&*timer, invalidate];
            }
        });
        // Release the target
        FADE_TIMER_TARGET.with(|t| {
            t.borrow_mut().take();
        });
    }
}

/// Register the PerryFadeTimerTarget class dynamically.
fn register_fade_timer_class() {
    FADE_CLASS_REGISTERED.with(|reg| {
        if *reg.borrow() {
            return;
        }
        *reg.borrow_mut() = true;

        unsafe {
            let superclass = objc_getClass(c"NSObject".as_ptr());
            let cls = objc_allocateClassPair(superclass, c"PerryFadeTimerTarget".as_ptr(), 0);
            if cls.is_null() {
                return; // Already registered
            }

            // fadeTick: — type encoding: v@:@ (void, self, _cmd, timer)
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

/// Ensure the global 30Hz fade timer is running.
fn ensure_fade_timer() {
    let already_running = FADE_TIMER.with(|ft| ft.borrow().is_some());
    if already_running {
        return;
    }

    register_fade_timer_class();

    unsafe {
        let target_cls = match AnyClass::get(c"PerryFadeTimerTarget") {
            Some(cls) => cls,
            None => {
                crate::ws_log!("[audio_playback] PerryFadeTimerTarget class not found");
                return;
            }
        };
        let target: Retained<AnyObject> = msg_send![target_cls, new];

        let sel = Sel::register(c"fadeTick:");
        let timer: Retained<AnyObject> = msg_send![
            objc2::class!(NSTimer),
            scheduledTimerWithTimeInterval: (1.0 / 30.0f64),
            target: &*target,
            selector: sel,
            userInfo: std::ptr::null::<AnyObject>(),
            repeats: true
        ];

        FADE_TIMER.with(|ft| {
            *ft.borrow_mut() = Some(timer);
        });
        FADE_TIMER_TARGET.with(|t| {
            *t.borrow_mut() = Some(target);
        });
    }
}

// =============================================================================
// Audio session interruption observer
// =============================================================================

/// The handleInterruption: callback — invoked by NSNotificationCenter.
unsafe extern "C" fn handle_interruption(
    _this: *mut AnyObject,
    _sel: *const std::ffi::c_void,
    notification: *mut AnyObject,
) {
    if notification.is_null() {
        return;
    }

    // Get userInfo dictionary
    let user_info: *mut AnyObject = msg_send![notification, userInfo];
    if user_info.is_null() {
        return;
    }

    // Get the interruption type from the userInfo.
    // Key: AVAudioSessionInterruptionTypeKey
    let type_key = objc2_foundation::NSString::from_str("AVAudioSessionInterruptionType");
    let type_val: *mut AnyObject = msg_send![user_info, objectForKey: &*type_key];
    if type_val.is_null() {
        return;
    }

    let interruption_type: u64 = msg_send![type_val, unsignedIntegerValue];
    // 1 = began, 0 = ended
    let callback_arg = if interruption_type == 1 { 1.0 } else { 0.0 };

    crate::ws_log!("[audio_playback] interruption type={}", interruption_type);

    // If interruption ended, reactivate the audio session and restart the engine
    if interruption_type == 0 {
        let session_cls = AnyClass::get(c"AVAudioSession").unwrap();
        let session: *mut AnyObject = msg_send![session_cls, sharedInstance];
        if !session.is_null() {
            let mut error: *mut AnyObject = std::ptr::null_mut();
            let _: bool = msg_send![session, setActive: true error: &mut error];
        }

        // Restart the engine if it was stopped
        PLAYBACK_ENGINE.with(|e| {
            if let Some(engine) = e.borrow().as_ref() {
                let running: bool = msg_send![&**engine, isRunning];
                if !running {
                    let mut error: *mut AnyObject = std::ptr::null_mut();
                    let _: bool = msg_send![&**engine, startAndReturnError: &mut error];
                    crate::ws_log!("[audio_playback] engine restarted after interruption");
                }
            }
        });
    }

    // Invoke JS callback
    let cb = INTERRUPTION_CALLBACK.with(|c| *c.borrow());
    if let Some(closure_f64) = cb {
        js_stdlib_process_pending();
        js_promise_run_microtasks();
        let closure_ptr = js_nanbox_get_pointer(closure_f64);
        js_closure_call1(closure_ptr as *const u8, callback_arg);
    }
}

/// Register the PerryInterruptionObserver class dynamically.
fn register_interruption_class() {
    INTERRUPTION_CLASS_REGISTERED.with(|reg| {
        if *reg.borrow() {
            return;
        }
        *reg.borrow_mut() = true;

        unsafe {
            let superclass = objc_getClass(c"NSObject".as_ptr());
            let cls = objc_allocateClassPair(
                superclass,
                c"PerryInterruptionObserver".as_ptr(),
                0,
            );
            if cls.is_null() {
                return; // Already registered
            }

            // handleInterruption: — type encoding: v@:@ (void, self, _cmd, notification)
            let sel = sel_registerName(c"handleInterruption:".as_ptr());
            class_addMethod(
                cls,
                sel,
                handle_interruption as *const std::ffi::c_void,
                c"v@:@".as_ptr(),
            );

            objc_registerClassPair(cls);
        }
    });
}

/// Register for AVAudioSessionInterruptionNotification.
fn register_interruption_observer() {
    register_interruption_class();

    unsafe {
        let observer_cls = match AnyClass::get(c"PerryInterruptionObserver") {
            Some(cls) => cls,
            None => {
                crate::ws_log!("[audio_playback] PerryInterruptionObserver class not found");
                return;
            }
        };
        let observer: Retained<AnyObject> = msg_send![observer_cls, new];

        let nc: *const AnyObject = msg_send![
            AnyClass::get(c"NSNotificationCenter").unwrap(),
            defaultCenter
        ];

        let notif_name = objc2_foundation::NSString::from_str("AVAudioSessionInterruptionNotification");
        let sel = Sel::register(c"handleInterruption:");

        // Get the shared audio session as the notification sender
        let session_cls = AnyClass::get(c"AVAudioSession").unwrap();
        let session: *mut AnyObject = msg_send![session_cls, sharedInstance];

        let _: () = msg_send![
            nc,
            addObserver: &*observer,
            selector: sel,
            name: &*notif_name,
            object: session
        ];

        // Keep the observer alive
        std::mem::forget(observer);

        crate::ws_log!("[audio_playback] interruption observer registered");
    }
}
