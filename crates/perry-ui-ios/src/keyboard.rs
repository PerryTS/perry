//! iOS keyboard event hook for `onKeyDown` / `onKeyUp` (issue #1864).
//!
//! Hooks into `PerryViewController` via `pressesBegan:withEvent:` and
//! `pressesEnded:withEvent:` (UIResponder methods available since iOS 13.4).
//! Each `UIPress` carries a `UIKey` whose `keyCode` is a HID usage code
//! (USB HID Keyboard/Keypad page) — we translate via the static [`HID_LUT`]
//! into the canonical [`KeyCode`] declared in `perry_ui::keys` and hand off
//! to [`perry_ui::key_dispatch::on_key_event`].
//!
//! All dispatch state (held-key bitset, focused-widget cache, modifier
//! snapshot) is shared with every other platform backend.

use objc2::msg_send;
use objc2::runtime::{AnyClass, AnyObject};
use objc2::sel;
use objc2_foundation::NSEnumerator;
use perry_ui::key_dispatch;
use perry_ui::keys::KeyCode;
use std::ffi::c_void;
use std::os::raw::c_char;

extern "C" {
    fn class_addMethod(
        cls: *mut c_void,
        name: *const c_void,
        imp: *const c_void,
        types: *const c_char,
    ) -> bool;
    fn objc_getClass(name: *const c_char) -> *mut c_void;
    fn sel_registerName(name: *const c_char) -> *const c_void;
}

// --- Public API (called from the FFI shims in ffi/widgets_basic.rs) ---------

pub fn set_on_key_down(handle: i64, callback: f64) {
    key_dispatch::set_on_key_down(handle, callback);
}
pub fn set_on_key_up(handle: i64, callback: f64) {
    key_dispatch::set_on_key_up(handle, callback);
}
pub fn focus_widget(handle: i64) {
    key_dispatch::focus_widget(handle);
}
pub fn blur_widget(handle: i64) {
    key_dispatch::blur_widget(handle);
}
pub fn is_key_down(code: u16) -> bool {
    key_dispatch::is_key_down(code)
}
pub fn current_modifiers() -> u32 {
    key_dispatch::current_modifiers()
}

// --- ObjC method overrides on PerryViewController ---------------------------

/// `canBecomeFirstResponder` — return YES so the VC receives hardware key
/// events when nothing else is the first responder.
unsafe extern "C" fn vc_can_become_first_responder(
    _this: *mut AnyObject,
    _sel: *const c_void,
) -> bool {
    true
}

/// `pressesBegan:withEvent:` — iterate `UIPress`es, extract `UIKey`,
/// dispatch each as a key-down.
unsafe extern "C" fn vc_presses_began(
    _this: *mut AnyObject,
    _sel: *const c_void,
    presses: *mut AnyObject,
    _event: *mut AnyObject,
) {
    dispatch_presses(presses, true);
}

unsafe extern "C" fn vc_presses_ended(
    _this: *mut AnyObject,
    _sel: *const c_void,
    presses: *mut AnyObject,
    _event: *mut AnyObject,
) {
    dispatch_presses(presses, false);
}

unsafe fn dispatch_presses(presses: *mut AnyObject, is_down: bool) {
    if presses.is_null() {
        return;
    }
    // Iterate the NSSet of UIPress objects.
    let enumerator: *mut NSEnumerator<AnyObject> = msg_send![presses, objectEnumerator];
    if enumerator.is_null() {
        return;
    }
    loop {
        let press: *mut AnyObject = msg_send![enumerator, nextObject];
        if press.is_null() {
            break;
        }
        // UIPress.key — `nil` on pre-13.4 / non-keyboard presses (Apple TV
        // remote buttons, game controllers). Skip those.
        let key: *mut AnyObject = msg_send![press, key];
        if key.is_null() {
            continue;
        }
        let hid: i64 = msg_send![key, keyCode];
        let ns_mods: u64 = msg_send![key, modifierFlags];
        let code = hid_to_keycode(hid);
        let mods = perry_mods_from_ui(ns_mods);
        // UIKey lacks an `isARepeat` getter on iOS; treat each event as a
        // fresh press. Apps that need edge-only detection can compare against
        // their own held set.
        key_dispatch::on_key_event(code, mods, is_down, false);
    }
}

#[inline]
fn perry_mods_from_ui(ns_mods: u64) -> u32 {
    // UIKeyModifierFlags share the bit layout with NSEventModifierFlags.
    let mut m = 0u32;
    if ns_mods & (1 << 20) != 0 {
        m |= 1;
    } // Command
    if ns_mods & (1 << 17) != 0 {
        m |= 2;
    } // Shift
    if ns_mods & (1 << 19) != 0 {
        m |= 4;
    } // Alternate (Option)
    if ns_mods & (1 << 18) != 0 {
        m |= 8;
    } // Control
    m
}

/// Install `canBecomeFirstResponder` / `pressesBegan:` / `pressesEnded:` on
/// the existing `PerryViewController` class. Called from `app_run` once the
/// class is registered. Idempotent — runtime ignores re-adds of the same SEL.
pub fn install_view_controller_overrides() {
    unsafe {
        let cls = objc_getClass(c"PerryViewController".as_ptr()) as *mut c_void;
        if cls.is_null() {
            return;
        }

        let sel_cbfr = sel_registerName(c"canBecomeFirstResponder".as_ptr());
        class_addMethod(
            cls,
            sel_cbfr,
            vc_can_become_first_responder as *const c_void,
            c"B@:".as_ptr(),
        );

        let sel_pb = sel_registerName(c"pressesBegan:withEvent:".as_ptr());
        class_addMethod(
            cls,
            sel_pb,
            vc_presses_began as *const c_void,
            c"v@:@@".as_ptr(),
        );

        let sel_pe = sel_registerName(c"pressesEnded:withEvent:".as_ptr());
        class_addMethod(
            cls,
            sel_pe,
            vc_presses_ended as *const c_void,
            c"v@:@@".as_ptr(),
        );
    }
}

/// Make the current root view controller's view become first responder so
/// hardware key events route to our overrides. Safe to call multiple times.
pub fn make_first_responder() {
    unsafe {
        let app_cls = AnyClass::get(c"UIApplication");
        let Some(app_cls) = app_cls else {
            return;
        };
        let app: *mut AnyObject = msg_send![app_cls, sharedApplication];
        if app.is_null() {
            return;
        }
        let windows: *mut AnyObject = msg_send![app, windows];
        if windows.is_null() {
            return;
        }
        let count: usize = msg_send![windows, count];
        for i in 0..count {
            let w: *mut AnyObject = msg_send![windows, objectAtIndex: i];
            if w.is_null() {
                continue;
            }
            let _: bool = msg_send![w, becomeFirstResponder];
        }
        // Suppress unused-var warning when sel macros are unused on this path.
        let _ = sel!(becomeFirstResponder);
    }
}

// --- HID usage → KeyCode LUT ------------------------------------------------

const KC_UNK: u16 = 0;

#[inline]
fn hid_to_keycode(hid: i64) -> KeyCode {
    if (0..HID_LUT.len() as i64).contains(&hid) {
        KeyCode(HID_LUT[hid as usize])
    } else {
        KeyCode::UNKNOWN
    }
}

/// Indexed by `UIKeyboardHIDUsage` (USB HID Keyboard/Keypad page). Values are
/// `KeyCode.0` ids from `perry_ui::keys`. Highest used code is F20 = 111, so
/// 128 slots suffice for the canonical set.
#[rustfmt::skip]
static HID_LUT: [u16; 128] = {
    let mut t = [KC_UNK; 128];

    // Letters (KeyboardA=4 .. KeyboardZ=29).
    t[4]  = 1;  t[5]  = 2;  t[6]  = 3;  t[7]  = 4;  t[8]  = 5;  t[9]  = 6;
    t[10] = 7;  t[11] = 8;  t[12] = 9;  t[13] = 10; t[14] = 11; t[15] = 12;
    t[16] = 13; t[17] = 14; t[18] = 15; t[19] = 16; t[20] = 17; t[21] = 18;
    t[22] = 19; t[23] = 20; t[24] = 21; t[25] = 22; t[26] = 23; t[27] = 24;
    t[28] = 25; t[29] = 26;

    // Digits — HID order is 1-9 then 0.
    t[30] = 28; t[31] = 29; t[32] = 30; t[33] = 31; t[34] = 32;
    t[35] = 33; t[36] = 34; t[37] = 35; t[38] = 36;
    t[39] = 27; // Keyboard0

    // Whitespace / edit / escape.
    t[40] = 54; // Return
    t[41] = 56; // Escape
    t[42] = 57; // DeleteOrBackspace
    t[43] = 55; // Tab
    t[44] = 53; // Spacebar

    // Punctuation.
    t[45] = 64; // Hyphen
    t[46] = 65; // Equal
    t[47] = 66; // [
    t[48] = 67; // ]
    t[49] = 68; // \
    t[51] = 69; // ;
    t[52] = 70; // '
    t[53] = 74; // `
    t[54] = 71; // ,
    t[55] = 72; // .
    t[56] = 73; // /

    // F1–F12 (HID 58..69).
    t[58] = 37; t[59] = 38; t[60] = 39; t[61] = 40; t[62] = 41; t[63] = 42;
    t[64] = 43; t[65] = 44; t[66] = 45; t[67] = 46; t[68] = 47; t[69] = 48;

    // Insert / nav.
    t[73] = 63; // Insert
    t[74] = 59; // Home
    t[75] = 61; // PageUp
    t[76] = 58; // DeleteForward
    t[77] = 60; // End
    t[78] = 62; // PageDown
    t[79] = 52; // RightArrow
    t[80] = 51; // LeftArrow
    t[81] = 50; // DownArrow
    t[82] = 49; // UpArrow

    // Numpad.
    t[84] = 98;  // KeypadSlash → NumpadDivide
    t[85] = 97;  // KeypadAsterisk → NumpadMultiply
    t[86] = 96;  // KeypadHyphen → NumpadSubtract
    t[87] = 95;  // KeypadPlus → NumpadAdd
    t[88] = 94;  // KeypadEnter
    t[89] = 84; t[90] = 85; t[91] = 86; t[92] = 87; t[93] = 88;
    t[94] = 89; t[95] = 90; t[96] = 91; t[97] = 92;
    t[98] = 83;  // Keypad0
    t[99] = 93;  // KeypadPeriod → NumpadDecimal
    t[103] = 99; // KeypadEqualSign

    // F13–F20.
    t[104] = 75; t[105] = 76; t[106] = 77; t[107] = 78;
    t[108] = 79; t[109] = 80; t[110] = 81; t[111] = 82;

    t
};
