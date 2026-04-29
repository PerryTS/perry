//! watchOS game loop support.
//!
//! When a Perry app targets watchOS with `--features watchos-game-loop`, the
//! compiler generates `_perry_user_main` instead of `main`. This module
//! provides the actual `main` entry point which:
//!
//! 1. Spawns `_perry_user_main` on a background "game thread"
//! 2. Calls `WKApplicationMain` on the main thread (required by WatchKit)
//!
//! This mirrors `ios_game_loop.rs` but uses WatchKit's `WKApplicationMain`
//! + `WKApplicationDelegate` instead of UIKit's `UIApplicationMain`
//! + `UIApplicationDelegate`. A native UI library (e.g. Bloom Engine's
//! `native/watchos/` crate) is expected to register its own
//! `WKApplicationDelegate` subclass via `perry_register_native_classes()`
//! so it can attach a `CAMetalLayer`-backed view on
//! `applicationDidFinishLaunching`. The fallback registered here just
//! calls `perry_scene_will_connect(NULL)` so the native lib can still
//! grab a window without owning the delegate class itself.

use std::ffi::c_void;

extern "C" {
    fn _perry_user_main() -> i32;

    fn WKApplicationMain(
        argc: i32,
        argv: *const *const u8,
        delegateClassName: *const c_void,
    ) -> i32;

    /// Provided by native libraries (e.g., Bloom Engine) to register ObjC classes
    /// (like a custom WKApplicationDelegate) before WKApplicationMain starts.
    /// Has a weak no-op default below — native lib's strong definition wins.
    fn perry_register_native_classes();

    /// Called when the window/root view becomes available. Native libraries
    /// implement this to create their Metal view and wgpu surface. Passed
    /// NULL on watchOS because there's no UIWindowScene equivalent — the
    /// native lib is expected to resolve the active WKApplication and
    /// root controller itself. Has a weak no-op default below — native
    /// lib's strong definition wins.
    fn perry_scene_will_connect(scene: *const c_void);
}

// Weak no-op fallbacks. The native lib's strong definitions take precedence
// at link time on Mach-O — so users who plumb in a Bloom-style native lib
// override these with their real CAMetalLayer setup. With no native lib the
// build still succeeds (and runs to a black screen) instead of failing with
// "Undefined symbols: _perry_register_native_classes / _perry_scene_will_connect".
// arm64 `ret` is a single instruction; the params on the C side are c_void
// pointers we never read.
core::arch::global_asm!(
    ".globl _perry_register_native_classes",
    ".weak_definition _perry_register_native_classes",
    ".p2align 2",
    "_perry_register_native_classes:",
    "    ret",
    "",
    ".globl _perry_scene_will_connect",
    ".weak_definition _perry_scene_will_connect",
    ".p2align 2",
    "_perry_scene_will_connect:",
    "    ret",
);

/// App delegate — calls `perry_scene_will_connect(NULL)` on launch so
/// native libs can set up their Metal view without owning the delegate.
unsafe extern "C" fn app_did_finish_launching(_this: *mut c_void, _sel: *const c_void) {
    perry_scene_will_connect(std::ptr::null());
}

fn register_app_delegate() {
    extern "C" {
        fn objc_getClass(name: *const u8) -> *const c_void;
        fn objc_allocateClassPair(
            superclass: *const c_void,
            name: *const u8,
            extra: usize,
        ) -> *mut c_void;
        fn objc_registerClassPair(cls: *mut c_void);
        fn class_addMethod(
            cls: *mut c_void,
            sel: *const c_void,
            imp: *const c_void,
            types: *const u8,
        ) -> bool;
        fn sel_registerName(name: *const u8) -> *const c_void;
        fn objc_getProtocol(name: *const u8) -> *const c_void;
        fn class_addProtocol(cls: *mut c_void, protocol: *const c_void) -> bool;
    }

    unsafe {
        let existing = objc_getClass(b"PerryWatchGameLoopAppDelegate\0".as_ptr());
        if !existing.is_null() {
            return;
        }

        let superclass = objc_getClass(b"NSObject\0".as_ptr());
        if superclass.is_null() {
            return;
        }

        let cls =
            objc_allocateClassPair(superclass, b"PerryWatchGameLoopAppDelegate\0".as_ptr(), 0);
        if cls.is_null() {
            return;
        }

        // applicationDidFinishLaunching: `-(void)applicationDidFinishLaunching`
        // Type encoding: `v16@0:8` (void return, self+_cmd).
        let sel = sel_registerName(b"applicationDidFinishLaunching\0".as_ptr());
        class_addMethod(
            cls,
            sel,
            app_did_finish_launching as *const c_void,
            b"v16@0:8\0".as_ptr(),
        );

        let protocol = objc_getProtocol(b"WKApplicationDelegate\0".as_ptr());
        if !protocol.is_null() {
            class_addProtocol(cls, protocol);
        }

        objc_registerClassPair(cls);
    }
}

static NATIVE_CLASSES_REGISTERED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Called by native libraries after registering their ObjC classes.
/// Mirrors `perry_ios_classes_registered` on iOS.
#[no_mangle]
pub extern "C" fn perry_watchos_classes_registered() {
    NATIVE_CLASSES_REGISTERED.store(true, std::sync::atomic::Ordering::Release);
}

/// The actual `main` entry point for watchOS game-loop apps.
/// Spawns the user's code on a game thread, then enters WKApplicationMain.
#[no_mangle]
pub extern "C" fn main() -> i32 {
    register_app_delegate();

    // Spawn the game thread — runs the user's TypeScript code.
    std::thread::Builder::new()
        .name("perry-game".to_string())
        .spawn(|| unsafe {
            _perry_user_main();
        })
        .expect("Failed to spawn game thread");

    // Register native library ObjC classes (e.g., a bloom-provided
    // PerryWatchGameLoopAppDelegate that owns the Metal view setup).
    unsafe {
        perry_register_native_classes();
    }

    // Hint to suppress the unused-static warning when no native lib has
    // signalled back — the flag is observable from native libs that want
    // to check registration state before returning.
    let _ = NATIVE_CLASSES_REGISTERED.load(std::sync::atomic::Ordering::Acquire);

    unsafe {
        // Build an NSString for the delegate class name via
        // CFStringCreateWithCString (toll-free bridged to NSString).
        extern "C" {
            fn CFStringCreateWithCString(
                alloc: *const c_void,
                cstr: *const u8,
                encoding: u32,
            ) -> *const c_void;
        }
        const K_CF_STRING_ENCODING_UTF8: u32 = 0x08000100;

        let delegate_name = CFStringCreateWithCString(
            std::ptr::null(),
            b"PerryWatchGameLoopAppDelegate\0".as_ptr(),
            K_CF_STRING_ENCODING_UTF8,
        );

        // Native lib may have registered its own class under the same
        // name via `perry_register_native_classes` — WKApplicationMain
        // will pick up whichever one is registered by Objective-C class
        // name at this point.
        WKApplicationMain(0, std::ptr::null(), delegate_name);
    }

    0
}
