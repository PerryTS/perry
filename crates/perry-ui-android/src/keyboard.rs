//! Android keyboard event hook for `onKeyDown` / `onKeyUp` (issue #1864).
//!
//! Java side: `PerryActivity.dispatchKeyEvent` calls
//! `PerryBridge.nativeDispatchKey(keyCode, action, metaState, repeatCount)`,
//! which lands here as `Java_com_perry_app_PerryBridge_nativeDispatchKey`.
//! We translate Android's `KeyEvent.KEYCODE_*` via [`AKEYCODE_LUT`] to the
//! canonical [`KeyCode`] and forward to the shared dispatcher in
//! [`perry_ui::key_dispatch`].

use perry_ui::key_dispatch;
use perry_ui::keys::KeyCode;

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

/// JNI entry point. `action` matches `KeyEvent.ACTION_DOWN` (0) /
/// `ACTION_UP` (1); other actions (`ACTION_MULTIPLE` = 2) are ignored.
/// `meta_state` is the raw `KeyEvent.metaState` bitfield.
/// `repeat_count` ≥ 1 marks the OS auto-repeat case.
#[no_mangle]
pub extern "C" fn Java_com_perry_app_PerryBridge_nativeDispatchKey(
    _env: jni::JNIEnv,
    _class: jni::objects::JClass,
    key_code: jni::sys::jint,
    action: jni::sys::jint,
    meta_state: jni::sys::jint,
    repeat_count: jni::sys::jint,
) {
    let is_down = match action {
        0 => true,  // ACTION_DOWN
        1 => false, // ACTION_UP
        _ => return,
    };
    let code = akeycode_to_perry(key_code);
    let mods = perry_mods_from_meta(meta_state as u32);
    key_dispatch::on_key_event(code, mods, is_down, repeat_count > 0);
}

#[inline]
fn perry_mods_from_meta(meta: u32) -> u32 {
    // KeyEvent.META_* (relevant bits):
    //   META_SHIFT_ON   = 0x0001
    //   META_ALT_ON     = 0x0002
    //   META_CTRL_ON    = 0x1000
    //   META_META_ON    = 0x10000 (Cmd / Win)
    let mut m = 0u32;
    if meta & 0x10000 != 0 {
        m |= 1;
    } // Meta → Cmd
    if meta & 0x0001 != 0 {
        m |= 2;
    } // Shift
    if meta & 0x0002 != 0 {
        m |= 4;
    } // Alt
    if meta & 0x1000 != 0 {
        m |= 8;
    } // Ctrl
    m
}

#[inline]
fn akeycode_to_perry(kc: i32) -> KeyCode {
    if (0..AKEYCODE_LUT.len() as i32).contains(&kc) {
        KeyCode(AKEYCODE_LUT[kc as usize])
    } else {
        KeyCode::UNKNOWN
    }
}

const KC_UNK: u16 = 0;

/// Indexed by `android.view.KeyEvent.KEYCODE_*`. Highest mapped is
/// `KEYCODE_F12` = 142, plus numpad through 161; allow 256 for headroom.
#[rustfmt::skip]
static AKEYCODE_LUT: [u16; 256] = {
    let mut t = [KC_UNK; 256];

    // Digits: KEYCODE_0 = 7 .. KEYCODE_9 = 16.
    t[7] = 27;  t[8] = 28;  t[9] = 29;  t[10] = 30; t[11] = 31;
    t[12] = 32; t[13] = 33; t[14] = 34; t[15] = 35; t[16] = 36;

    // Arrows: DPAD_UP=19 DPAD_DOWN=20 DPAD_LEFT=21 DPAD_RIGHT=22.
    t[19] = 49; t[20] = 50; t[21] = 51; t[22] = 52;

    // Letters: KEYCODE_A = 29 .. KEYCODE_Z = 54.
    let mut c: usize = 29;
    let mut id: u16 = 1;
    while c <= 54 { t[c] = id; c += 1; id += 1; }

    // Punctuation.
    t[55] = 71; // KEYCODE_COMMA
    t[56] = 72; // KEYCODE_PERIOD
    t[62] = 53; // KEYCODE_SPACE
    t[61] = 55; // KEYCODE_TAB
    t[66] = 54; // KEYCODE_ENTER
    t[67] = 57; // KEYCODE_DEL (backspace)
    t[68] = 74; // KEYCODE_GRAVE
    t[69] = 64; // KEYCODE_MINUS
    t[70] = 65; // KEYCODE_EQUALS
    t[71] = 66; // KEYCODE_LEFT_BRACKET
    t[72] = 67; // KEYCODE_RIGHT_BRACKET
    t[73] = 68; // KEYCODE_BACKSLASH
    t[74] = 69; // KEYCODE_SEMICOLON
    t[75] = 70; // KEYCODE_APOSTROPHE
    t[76] = 73; // KEYCODE_SLASH

    // Escape / nav / edit (KEYCODE_ESCAPE=111 onwards).
    t[111] = 56; // ESCAPE
    t[112] = 58; // FORWARD_DEL
    t[122] = 59; // MOVE_HOME
    t[123] = 60; // MOVE_END
    t[124] = 63; // INSERT
    t[92]  = 61; // PAGE_UP
    t[93]  = 62; // PAGE_DOWN

    // F1..F12 = 131..142.
    let mut fi: usize = 0;
    while fi < 12 { t[131 + fi] = 37 + fi as u16; fi += 1; }

    // Numpad (KEYCODE_NUMPAD_0 = 144 .. KEYCODE_NUMPAD_9 = 153).
    let mut ni: usize = 0;
    while ni < 10 { t[144 + ni] = 83 + ni as u16; ni += 1; }
    t[154] = 98;  // NUMPAD_DIVIDE
    t[155] = 97;  // NUMPAD_MULTIPLY
    t[156] = 96;  // NUMPAD_SUBTRACT
    t[157] = 95;  // NUMPAD_ADD
    t[158] = 93;  // NUMPAD_DOT
    t[160] = 94;  // NUMPAD_ENTER
    t[161] = 99;  // NUMPAD_EQUALS

    t
};
