//! perry/system hapticPlay — WKInterfaceDevice playHaptic:.
//!
//! watchOS is the platform where `hapticPlay` matters most (Taptic
//! Engine on the wrist), so every semantic HapticType maps to a real
//! WatchKit haptic. The raw values below are verified against the
//! watchOS 26.5 SDK's `WatchKit/WKInterfaceDevice.h`:
//!
//! ```c
//! typedef NS_ENUM(NSInteger, WKHapticType) {
//!     WKHapticTypeNotification,   // 0
//!     WKHapticTypeDirectionUp,    // 1
//!     WKHapticTypeDirectionDown,  // 2
//!     WKHapticTypeSuccess,        // 3
//!     WKHapticTypeFailure,        // 4  (double buzz)
//!     WKHapticTypeRetry,          // 5
//!     WKHapticTypeStart,          // 6
//!     WKHapticTypeStop,           // 7
//!     WKHapticTypeClick,          // 8
//!     ...
//! };
//! ```

use objc2::msg_send;
use objc2::runtime::{AnyClass, AnyObject};

extern "C" {
    // Returns non-zero when called on the process's main thread.
    fn pthread_main_np() -> std::ffi::c_int;
    // dispatch_get_main_queue() is a macro; the actual symbol is
    // _dispatch_main_q (same idiom as the iOS crate's widgets).
    static _dispatch_main_q: std::ffi::c_void;
    fn dispatch_async_f(
        queue: *const std::ffi::c_void,
        context: *mut std::ffi::c_void,
        work: unsafe extern "C" fn(*mut std::ffi::c_void),
    );
}

/// Map a Perry `HapticType` name to the WKHapticType raw value.
fn wk_haptic_type(name: &str) -> i64 {
    match name {
        "success" => 3,       // WKHapticTypeSuccess
        "error" => 4,         // WKHapticTypeFailure — double buzz
        "warning" => 5,       // WKHapticTypeRetry
        "directionUp" => 1,   // WKHapticTypeDirectionUp
        "directionDown" => 2, // WKHapticTypeDirectionDown
        "start" => 6,         // WKHapticTypeStart
        "stop" => 7,          // WKHapticTypeStop
        // light / medium / heavy / click / selection — WatchKit has no
        // intensity-graded impact haptics; Click is the closest neutral
        // tap. Unknown names also land here.
        _ => 8, // WKHapticTypeClick
    }
}

unsafe fn play(kind: i64) {
    if let Some(device_cls) = AnyClass::get(c"WKInterfaceDevice") {
        let device: *mut AnyObject = msg_send![device_cls, currentDevice];
        if !device.is_null() {
            let _: () = msg_send![device, playHaptic: kind];
        }
    }
}

unsafe extern "C" fn play_trampoline(ctx: *mut std::ffi::c_void) {
    // The WKHapticType raw value (0..=8) rides in the context pointer
    // itself — no allocation, and it fits a 32-bit pointer on the
    // ILP32 arm64_32 watch target.
    play(ctx as usize as i64);
}

/// Play a haptic effect via the Taptic Engine.
///
/// `WKInterfaceDevice` is main-thread-only. Perry's watch FFI already
/// runs on the main thread (main-thread Timer pump), so the direct
/// path is the norm; the `dispatch_async` hop is a safety net for
/// calls arriving from a background thread (fire-and-forget is fine
/// for a haptic).
#[no_mangle]
pub extern "C" fn perry_system_haptic_play(type_ptr: i64) {
    let kind = wk_haptic_type(crate::str_from_header(type_ptr as *const u8));
    unsafe {
        if pthread_main_np() != 0 {
            play(kind);
        } else {
            dispatch_async_f(
                &_dispatch_main_q as *const _ as *const std::ffi::c_void,
                kind as usize as *mut std::ffi::c_void,
                play_trampoline,
            );
        }
    }
}
