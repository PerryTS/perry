//! tvOS keyboard event hook (issue #1864).
//!
//! Identical to the iOS path: overrides `pressesBegan:` / `pressesEnded:` on
//! `PerryViewController`, maps `UIKey.keyCode` (HID usage) → `KeyCode`, and
//! delegates to the shared dispatcher in `perry_ui::key_dispatch`.
//!
//! On tvOS most users don't have a physical keyboard, but Bluetooth keyboards
//! pair fine and the hook handles them. `UIPress` events for the Siri Remote
//! / game controllers carry a `nil` `key` and are ignored here.

use objc2::msg_send;
use objc2::runtime::{AnyClass, AnyObject};
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

unsafe extern "C" fn vc_can_become_first_responder(
    _this: *mut AnyObject,
    _sel: *const c_void,
) -> bool {
    true
}

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
    let enumerator: *mut NSEnumerator<AnyObject> = msg_send![presses, objectEnumerator];
    if enumerator.is_null() {
        return;
    }
    loop {
        let press: *mut AnyObject = msg_send![enumerator, nextObject];
        if press.is_null() {
            break;
        }
        let key: *mut AnyObject = msg_send![press, key];
        // Remote / controller presses carry `nil` for `key`. Skip them — they
        // belong to a separate input model.
        if key.is_null() {
            continue;
        }
        let hid: i64 = msg_send![key, keyCode];
        let ns_mods: u64 = msg_send![key, modifierFlags];
        let code = hid_to_keycode(hid);
        let mods = perry_mods_from_ui(ns_mods);
        key_dispatch::on_key_event(code, mods, is_down, false);
    }
}

#[inline]
fn perry_mods_from_ui(ns_mods: u64) -> u32 {
    let mut m = 0u32;
    if ns_mods & (1 << 20) != 0 {
        m |= 1;
    }
    if ns_mods & (1 << 17) != 0 {
        m |= 2;
    }
    if ns_mods & (1 << 19) != 0 {
        m |= 4;
    }
    if ns_mods & (1 << 18) != 0 {
        m |= 8;
    }
    m
}

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

pub fn make_first_responder() {
    unsafe {
        let Some(app_cls) = AnyClass::get(c"UIApplication") else {
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
    }
}

const KC_UNK: u16 = 0;

#[inline]
fn hid_to_keycode(hid: i64) -> KeyCode {
    if (0..HID_LUT.len() as i64).contains(&hid) {
        KeyCode(HID_LUT[hid as usize])
    } else {
        KeyCode::UNKNOWN
    }
}

#[rustfmt::skip]
static HID_LUT: [u16; 128] = {
    let mut t = [KC_UNK; 128];
    t[4]  = 1;  t[5]  = 2;  t[6]  = 3;  t[7]  = 4;  t[8]  = 5;  t[9]  = 6;
    t[10] = 7;  t[11] = 8;  t[12] = 9;  t[13] = 10; t[14] = 11; t[15] = 12;
    t[16] = 13; t[17] = 14; t[18] = 15; t[19] = 16; t[20] = 17; t[21] = 18;
    t[22] = 19; t[23] = 20; t[24] = 21; t[25] = 22; t[26] = 23; t[27] = 24;
    t[28] = 25; t[29] = 26;
    t[30] = 28; t[31] = 29; t[32] = 30; t[33] = 31; t[34] = 32;
    t[35] = 33; t[36] = 34; t[37] = 35; t[38] = 36;
    t[39] = 27;
    t[40] = 54; t[41] = 56; t[42] = 57; t[43] = 55; t[44] = 53;
    t[45] = 64; t[46] = 65; t[47] = 66; t[48] = 67; t[49] = 68;
    t[51] = 69; t[52] = 70; t[53] = 74; t[54] = 71; t[55] = 72; t[56] = 73;
    t[58] = 37; t[59] = 38; t[60] = 39; t[61] = 40; t[62] = 41; t[63] = 42;
    t[64] = 43; t[65] = 44; t[66] = 45; t[67] = 46; t[68] = 47; t[69] = 48;
    t[73] = 63; t[74] = 59; t[75] = 61; t[76] = 58;
    t[77] = 60; t[78] = 62; t[79] = 52; t[80] = 51; t[81] = 50; t[82] = 49;
    t[84] = 98; t[85] = 97; t[86] = 96; t[87] = 95; t[88] = 94;
    t[89] = 84; t[90] = 85; t[91] = 86; t[92] = 87; t[93] = 88;
    t[94] = 89; t[95] = 90; t[96] = 91; t[97] = 92;
    t[98] = 83; t[99] = 93; t[103] = 99;
    t[104] = 75; t[105] = 76; t[106] = 77; t[107] = 78;
    t[108] = 79; t[109] = 80; t[110] = 81; t[111] = 82;
    t
};
