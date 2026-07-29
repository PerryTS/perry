#![cfg(target_os = "android")]

// Issue #552: force libperry_ui_android.a to bundle perry-ext-sharp's
// `js_sharp_*` symbols (resize / jpeg / toBuffer / etc). Without this `extern
// crate` reference, the rlib dep would be optimized out and the stubs in
// stdlib_stubs.rs would mask sharp at link time.
extern crate perry_ext_sharp;

pub mod app;
pub mod audio;
pub mod audio_playback;
pub mod background;
pub mod callback;
pub mod camera;
pub mod clipboard;
pub mod deeplinks;
pub mod dialog;
pub mod drag_drop;
pub mod fetch;
pub mod ffi;
pub mod file_dialog;
#[cfg(feature = "geisterhand")]
pub mod geisterhand_style;
pub mod geolocation;
pub mod image_picker;
pub mod jni_bridge;
pub mod json;
pub mod keyboard;
pub mod keychain;
pub mod location;
pub mod media_playback;
pub mod menu;
pub mod network;
pub mod pointer;
#[cfg(feature = "geisterhand")]
pub mod screenshot;
pub mod sheet;
pub mod state;
pub mod stdlib_stubs;
pub mod system;
pub mod toolbar;
pub mod widgets;
pub mod window;
pub mod ws;

// `pub use` re-exports keep every `#[no_mangle]` symbol on the same final
// linker name as before the topical split. The exports themselves live in
// `ffi/*.rs` — see each submodule for the original section grouping.
pub use ffi::*;

// =============================================================================
// JNI lifecycle
// =============================================================================

extern "C" {
    fn __android_log_print(prio: i32, tag: *const u8, fmt: *const u8, ...) -> i32;
    fn mallopt(param: i32, value: i32) -> i32;
}

/// Disable Android heap tagging (MTE) at library load — independent of
/// `JNI_OnLoad`.
///
/// Perry's NaN-boxing uses 48-bit pointers (`POINTER_MASK =
/// 0x0000_FFFF_FFFF_FFFF`); scudo's top-byte tag breaks pointer extraction.
/// `JNI_OnLoad` (below) also disables tagging, but a linked native library may
/// define its own `JNI_OnLoad` that wins the single ELF symbol slot, leaving
/// Perry's version unused. This `.init_array` constructor fires unconditionally
/// when the `.so` is `dlopen`'d — before `JNI_OnLoad` and before any Perry
/// allocation — so tagging is off no matter which `JNI_OnLoad` survives linking.
/// `M_BIONIC_SET_HEAP_TAGGING_LEVEL = -204`, level `0` = no tagging.
#[cfg(target_os = "android")]
#[used]
#[link_section = ".init_array"]
static PERRY_DISABLE_MTE_CTOR: extern "C" fn() = {
    extern "C" fn ctor() {
        unsafe {
            mallopt(-204, 0);
        }
    }
    ctor
};

pub fn log_debug(msg: &str) {
    let c_msg = std::ffi::CString::new(msg).unwrap_or_default();
    unsafe {
        __android_log_print(
            3,
            b"PerryDebug\0".as_ptr(),
            b"%s\0".as_ptr(),
            c_msg.as_ptr(),
        );
    }
}

/// Catch panics from widget functions, log them, and return 0 instead of aborting.
pub(crate) fn catch_panic(name: &str, f: impl FnOnce() -> i64 + std::panic::UnwindSafe) -> i64 {
    match std::panic::catch_unwind(f) {
        Ok(h) => h,
        Err(e) => {
            let detail = if let Some(s) = e.downcast_ref::<&str>() {
                s.to_string()
            } else if let Some(s) = e.downcast_ref::<String>() {
                s.clone()
            } else {
                "<unknown>".to_string()
            };
            let msg = format!("{} panicked: {}\0", name, detail);
            unsafe {
                __android_log_print(6, b"PerryJNI\0".as_ptr(), b"%s\0".as_ptr(), msg.as_ptr());
            }
            0
        }
    }
}

/// Catch panics from void widget functions, log them instead of aborting.
pub(crate) fn catch_panic_void(name: &str, f: impl FnOnce() + std::panic::UnwindSafe) {
    if let Err(e) = std::panic::catch_unwind(f) {
        let detail = if let Some(s) = e.downcast_ref::<&str>() {
            s.to_string()
        } else if let Some(s) = e.downcast_ref::<String>() {
            s.clone()
        } else {
            "<unknown>".to_string()
        };
        let msg = format!("{} panicked: {}\0", name, detail);
        unsafe {
            __android_log_print(6, b"PerryJNI\0".as_ptr(), b"%s\0".as_ptr(), msg.as_ptr());
        }
    }
}

/// Called by the JVM when the native library is loaded via System.loadLibrary().
#[no_mangle]
pub extern "C" fn JNI_OnLoad(vm: jni::JavaVM, _reserved: *mut std::ffi::c_void) -> jni::sys::jint {
    unsafe {
        __android_log_print(
            3,
            b"PerryJNI\0".as_ptr(),
            b"JNI_OnLoad: starting\0".as_ptr(),
        );
    }

    // Disable MTE (Memory Tagging Extension) tagged addresses.
    // Perry's NaN-boxing uses 48-bit pointers (POINTER_MASK = 0x0000_FFFF_FFFF_FFFF).
    // Android's MTE puts a tag in the top byte, making pointers 56 bits.
    // When NaN-boxed pointers are extracted, the MTE tag is lost, causing crashes.
    // Disabling tagged addresses makes the allocator use standard 48-bit pointers.
    // Disable heap tagging (MTE/TBI) for the allocator.
    // Perry's NaN-boxing uses 48-bit pointers (POINTER_MASK = 0x0000_FFFF_FFFF_FFFF).
    // Android's scudo allocator tags pointers with a top byte (e.g., 0xb4...),
    // which breaks NaN-boxing when the tag is stripped.
    // mallopt(M_BIONIC_SET_HEAP_TAGGING_LEVEL, 0) disables tagging for NEW allocations
    // without breaking the JVM (which keeps its own tagged pointers).
    #[cfg(target_os = "android")]
    unsafe {
        // M_BIONIC_SET_HEAP_TAGGING_LEVEL = -204, level 0 = no tagging
        let ret = mallopt(-204, 0);
        __android_log_print(
            3,
            b"PerryJNI\0".as_ptr(),
            b"JNI_OnLoad: mallopt(-204, 0) returned %d\0".as_ptr(),
            ret,
        );
    }

    jni_bridge::init_vm(vm);
    unsafe {
        __android_log_print(3, b"PerryJNI\0".as_ptr(), b"JNI_OnLoad: done\0".as_ptr());
    }
    jni::sys::JNI_VERSION_1_6
}

/// Called from PerryActivity after the native library is loaded.
/// Initializes the JNI cache on the calling thread.
#[no_mangle]
pub extern "C" fn Java_com_perry_app_PerryBridge_nativeInit(
    mut env: jni::JNIEnv,
    _class: jni::objects::JClass,
) {
    // Recover state that `JNI_OnLoad` would normally set up, in case a linked
    // native library's own `JNI_OnLoad` shadowed Perry's (see
    // `jni_bridge::ensure_vm` and `PERRY_DISABLE_MTE_CTOR`). The constructor
    // above already disabled MTE at load; re-asserting it here is idempotent and
    // cheap, and keeps the guarantee even on platforms without `.init_array`.
    #[cfg(target_os = "android")]
    unsafe {
        mallopt(-204, 0);
    }
    jni_bridge::ensure_vm(&env);
    jni_bridge::init_cache(&mut env);
}

/// Called from PerryActivity when the Activity is being destroyed.
#[no_mangle]
pub extern "C" fn Java_com_perry_app_PerryBridge_nativeShutdown(
    _env: jni::JNIEnv,
    _class: jni::objects::JClass,
) {
    app::signal_shutdown();
}

/// OS memory-pressure bridge (#6184). PerryActivity.onLowMemory() /
/// onTrimMemory(level) forward here (see PerryBridge.nativeMemoryPressure):
/// `onLowMemory` and the severe `onTrimMemory` levels map to `2` (full
/// collect + trigger clamp), lighter trims to `1` (minor collect). Runs on
/// the Activity's main thread, which owns the JS arena.
#[no_mangle]
pub extern "C" fn Java_com_perry_app_PerryBridge_nativeMemoryPressure(
    _env: jni::JNIEnv,
    _class: jni::objects::JClass,
    level: jni::sys::jint,
) {
    extern "C" {
        // perry-runtime's OS-memory-pressure entry point: level 1 =
        // collect-if-safe (minor), level 2+ = full collect + clamp trigger.
        fn js_gc_memory_pressure(level: u32) -> u32;
    }
    unsafe {
        js_gc_memory_pressure(level as u32);
    }
}

#[cfg(not(test))]
extern "C" {
    fn main() -> i32;
}

// js_stdlib_init_dispatch and js_stdlib_process_pending — now provided by perry-runtime

/// Called from the native thread to run the compiled TypeScript entry point.
/// This wraps the compiler-generated `main()` function as a JNI method on PerryBridge,
/// so the Activity doesn't need its own native method (which would require package-specific JNI names).
#[cfg(not(test))]
#[no_mangle]
pub extern "C" fn Java_com_perry_app_PerryBridge_nativeMain(
    _env: jni::JNIEnv,
    _class: jni::objects::JClass,
) {
    // Set CWD to the app's internal files directory so that relative paths
    // (e.g. SQLite databases like "mango.db") resolve to a writable location.
    {
        let mut env = jni_bridge::get_env();
        let _ = env.push_local_frame(16);
        if let Ok(activity) = env.call_static_method(
            "com/perry/app/PerryBridge",
            "getActivity",
            "()Landroid/app/Activity;",
            &[],
        ) {
            if let Ok(act_obj) = activity.l() {
                if let Ok(files_dir) =
                    env.call_method(&act_obj, "getFilesDir", "()Ljava/io/File;", &[])
                {
                    if let Ok(fd_obj) = files_dir.l() {
                        if let Ok(abs_val) =
                            env.call_method(&fd_obj, "getAbsolutePath", "()Ljava/lang/String;", &[])
                        {
                            if let Ok(abs_obj) = abs_val.l() {
                                if let Ok(path_str) = env.get_string((&abs_obj).into()) {
                                    let path: String = path_str.into();
                                    let _ = std::fs::create_dir_all(&path);
                                    let _ = std::env::set_current_dir(&path);
                                }
                            }
                        }
                    }
                }
            }
        }
        unsafe {
            env.pop_local_frame(&jni::objects::JObject::null());
        }
    }

    unsafe {
        __android_log_print(
            3,
            b"PerryJNI\0".as_ptr(),
            b"nativeMain: calling main()\0".as_ptr(),
        );
        main();
        __android_log_print(
            3,
            b"PerryJNI\0".as_ptr(),
            b"nativeMain: main() returned, parking thread\0".as_ptr(),
        );
    }

    // Park this thread forever — do NOT let it exit.
    // Module-level arrays/objects are allocated on this thread's arena.
    // If the thread exits, the arena's Drop frees all blocks, turning
    // every module-level pointer into a dangling reference. The UI thread's
    // pump ticks call into compiled functions (getLevelInfo etc.) that read
    // these pointers — segfault if the arena was freed.
    loop {
        std::thread::park();
    }
}
