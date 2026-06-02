//! Windows keyboard event hook for `onKeyDown` / `onKeyUp` (issue #1864).
//!
//! Called from the existing `GetMessage`/`TranslateMessage` pump in `app_run`
//! whenever a `WM_KEYDOWN` / `WM_SYSKEYDOWN` / `WM_KEYUP` / `WM_SYSKEYUP`
//! arrives. `wParam` (a `VK_*` virtual key code) is translated to our
//! canonical [`KeyCode`] via [`VK_LUT`] and forwarded to the shared
//! dispatcher in [`perry_ui::key_dispatch`].
//!
//! Modifier state is queried live via `GetKeyState` so it stays accurate
//! even for events that bypass `TranslateMessage` (e.g. our shortcut
//! interceptor returned `true` for the same VK).

#![cfg(target_os = "windows")]

use perry_ui::key_dispatch;
use perry_ui::keys::KeyCode;
use windows::Win32::UI::Input::KeyboardAndMouse::GetKeyState;

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

/// Called from the message pump for WM_KEY{DOWN,UP} / WM_SYSKEY{DOWN,UP}.
/// `vk` is the wParam virtual-key code; `lparam` is the event's LPARAM so we
/// can extract the auto-repeat flag (bit 30: previous state).
pub fn dispatch_message(vk: u16, lparam: isize, is_down: bool) {
    let code = vk_to_keycode(vk);
    let mods = read_modifiers_live();
    // LPARAM bit 30 is set when the key was already down — used as "is repeat"
    // for WM_KEYDOWN. Always `false` on WM_KEYUP.
    let is_repeat = is_down && (lparam & (1 << 30)) != 0;
    key_dispatch::on_key_event(code, mods, is_down, is_repeat);
}

#[inline]
fn read_modifiers_live() -> u32 {
    let pressed = |vk: i32| (unsafe { GetKeyState(vk) } as u16) & 0x8000 != 0;
    let mut m = 0u32;
    let ctrl = pressed(0x11); // VK_CONTROL
    if ctrl {
        m |= 1 | 8;
    } // Win has no Cmd; Ctrl serves both bits.
    if pressed(0x10) {
        m |= 2;
    } // VK_SHIFT
    if pressed(0x12) {
        m |= 4;
    } // VK_MENU (Alt)
    m
}

#[inline]
fn vk_to_keycode(vk: u16) -> KeyCode {
    if (vk as usize) < VK_LUT.len() {
        KeyCode(VK_LUT[vk as usize])
    } else {
        KeyCode::UNKNOWN
    }
}

const KC_UNK: u16 = 0;

/// Indexed by Windows virtual key code (`VK_*`). Slot index = `wParam`.
#[rustfmt::skip]
static VK_LUT: [u16; 256] = {
    let mut t = [KC_UNK; 256];

    // VK_0..VK_9 = 0x30..0x39 — top-row digits.
    t[0x30] = 27; t[0x31] = 28; t[0x32] = 29; t[0x33] = 30; t[0x34] = 31;
    t[0x35] = 32; t[0x36] = 33; t[0x37] = 34; t[0x38] = 35; t[0x39] = 36;

    // VK_A..VK_Z = 0x41..0x5A.
    let mut c: u8 = 0x41;
    let mut id: u16 = 1;
    while c <= 0x5A { t[c as usize] = id; c += 1; id += 1; }

    // Whitespace / edit / escape.
    t[0x08] = 57; // VK_BACK
    t[0x09] = 55; // VK_TAB
    t[0x0D] = 54; // VK_RETURN
    t[0x1B] = 56; // VK_ESCAPE
    t[0x20] = 53; // VK_SPACE

    // Navigation.
    t[0x21] = 61; // VK_PRIOR (PageUp)
    t[0x22] = 62; // VK_NEXT  (PageDown)
    t[0x23] = 60; // VK_END
    t[0x24] = 59; // VK_HOME
    t[0x25] = 51; // VK_LEFT
    t[0x26] = 49; // VK_UP
    t[0x27] = 52; // VK_RIGHT
    t[0x28] = 50; // VK_DOWN
    t[0x2D] = 63; // VK_INSERT
    t[0x2E] = 58; // VK_DELETE

    // F1..F20 (VK_F1 = 0x70).
    let mut i: usize = 0;
    while i < 12 { t[0x70 + i] = 37 + i as u16; i += 1; }
    let mut j: usize = 0;
    while j < 8 { t[0x7C + j] = 75 + j as u16; j += 1; } // VK_F13..VK_F20

    // Numpad (VK_NUMPAD0 = 0x60).
    let mut k: usize = 0;
    while k < 10 { t[0x60 + k] = 83 + k as u16; k += 1; }
    t[0x6A] = 97; // VK_MULTIPLY
    t[0x6B] = 95; // VK_ADD
    t[0x6D] = 96; // VK_SUBTRACT
    t[0x6E] = 93; // VK_DECIMAL
    t[0x6F] = 98; // VK_DIVIDE
    t[0x0C] = 100; // VK_CLEAR (Numpad-5 with NumLock off)

    // OEM punctuation (US layout — VK_OEM_* codes are layout-dependent but
    // these are the standard mappings).
    t[0xBA] = 69; // VK_OEM_1 = ;:
    t[0xBB] = 65; // VK_OEM_PLUS = =+
    t[0xBC] = 71; // VK_OEM_COMMA
    t[0xBD] = 64; // VK_OEM_MINUS
    t[0xBE] = 72; // VK_OEM_PERIOD
    t[0xBF] = 73; // VK_OEM_2 = /?
    t[0xC0] = 74; // VK_OEM_3 = `~
    t[0xDB] = 66; // VK_OEM_4 = [{
    t[0xDC] = 68; // VK_OEM_5 = \|
    t[0xDD] = 67; // VK_OEM_6 = ]}
    t[0xDE] = 70; // VK_OEM_7 = '"

    t
};
