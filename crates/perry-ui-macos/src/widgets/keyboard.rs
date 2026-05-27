//! macOS event hook for continuous keyboard events (issue #1864).
//!
//! Installs a single `NSEvent` local monitor on first registration and
//! translates each event to a canonical [`KeyCode`] via the static
//! [`MAC_VK_LUT`]. All dispatch logic — held-key bitset, focused-widget
//! callback cache, modifier snapshot, JS closure invocation — lives in
//! [`perry_ui::key_dispatch`] and is shared with every other platform backend.

use perry_ui::key_dispatch;
use perry_ui::keys::KeyCode;
use std::cell::Cell;

use objc2::msg_send;
use objc2::runtime::AnyObject;

thread_local! {
    /// One-time install guard for the NSEvent local monitor.
    static MONITOR_INSTALLED: Cell<bool> = const { Cell::new(false) };
}

// --- Public API (called from the FFI shims in lib_ffi/interactivity.rs) -----

pub fn set_on_key_down(handle: i64, callback: f64) {
    ensure_monitor_installed();
    key_dispatch::set_on_key_down(handle, callback);
}

pub fn set_on_key_up(handle: i64, callback: f64) {
    ensure_monitor_installed();
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

// --- Internal ---------------------------------------------------------------

/// Decode NSEvent.modifierFlags → Perry's bitfield (1=Cmd, 2=Shift, 4=Alt, 8=Ctrl).
#[inline]
fn perry_mods_from_ns(ns_flags: u64) -> u32 {
    let mut m = 0u32;
    if ns_flags & (1 << 20) != 0 {
        m |= 1;
    } // Cmd
    if ns_flags & (1 << 17) != 0 {
        m |= 2;
    } // Shift
    if ns_flags & (1 << 19) != 0 {
        m |= 4;
    } // Alt/Option
    if ns_flags & (1 << 18) != 0 {
        m |= 8;
    } // Control
    m
}

fn handle_key_event(is_down: bool, event: *const AnyObject) {
    if event.is_null() {
        return;
    }
    unsafe {
        let keycode: u16 = msg_send![event, keyCode];
        let code = mac_keycode_to_perry(keycode);
        let ns_flags: u64 = msg_send![event, modifierFlags];
        let mods = perry_mods_from_ns(ns_flags);
        let is_repeat: bool = if is_down {
            msg_send![event, isARepeat]
        } else {
            false
        };

        crate::catch_callback_panic(
            if is_down { "onKeyDown" } else { "onKeyUp" },
            std::panic::AssertUnwindSafe(|| {
                key_dispatch::on_key_event(code, mods, is_down, is_repeat);
            }),
        );
    }
}

fn ensure_monitor_installed() {
    let already = MONITOR_INSTALLED.with(|c| c.replace(true));
    if already {
        return;
    }
    unsafe {
        // NSEventMask: keyDown = 1<<10, keyUp = 1<<11, flagsChanged = 1<<12.
        let mask: u64 = (1 << 10) | (1 << 11) | (1 << 12);

        let block = block2::RcBlock::new(move |event: *const AnyObject| -> *const AnyObject {
            if event.is_null() {
                return event;
            }
            let event_type: u64 = msg_send![event, type];
            match event_type {
                10 => handle_key_event(true, event),
                11 => handle_key_event(false, event),
                12 => {
                    // flagsChanged: refresh modifier snapshot without
                    // firing any callback.
                    let ns_flags: u64 = msg_send![event, modifierFlags];
                    key_dispatch::update_modifiers(perry_mods_from_ns(ns_flags));
                }
                _ => {}
            }
            event // never consume — let the rest of the responder chain run.
        });

        let ns_event_cls = objc2::class!(NSEvent);
        let _: *const AnyObject = msg_send![
            ns_event_cls,
            addLocalMonitorForEventsMatchingMask: mask,
            handler: &*block
        ];
        std::mem::forget(block);
    }
}

// --- macOS virtual keycode → KeyCode LUT ------------------------------------

const KC_UNK: u16 = 0;

#[inline]
fn mac_keycode_to_perry(vk: u16) -> KeyCode {
    if (vk as usize) < MAC_VK_LUT.len() {
        KeyCode(MAC_VK_LUT[vk as usize])
    } else {
        KeyCode::UNKNOWN
    }
}

/// Indexed by macOS HIToolbox virtual keycode (`kVK_*`). Values are the
/// `KeyCode.0` ids declared in `perry_ui::keys`. Slots left at `KC_UNK` (0)
/// are ignored.
#[rustfmt::skip]
static MAC_VK_LUT: [u16; 128] = {
    let mut t = [KC_UNK; 128];

    // Letters (kVK_ANSI_*).
    t[0x00] = 1;  t[0x0B] = 2;  t[0x08] = 3;  t[0x02] = 4;  t[0x0E] = 5;  t[0x03] = 6;
    t[0x05] = 7;  t[0x04] = 8;  t[0x22] = 9;  t[0x26] = 10; t[0x28] = 11; t[0x25] = 12;
    t[0x2E] = 13; t[0x2D] = 14; t[0x1F] = 15; t[0x23] = 16; t[0x0C] = 17; t[0x0F] = 18;
    t[0x01] = 19; t[0x11] = 20; t[0x20] = 21; t[0x09] = 22; t[0x0D] = 23; t[0x07] = 24;
    t[0x10] = 25; t[0x06] = 26;

    // Digits.
    t[0x1D] = 27; t[0x12] = 28; t[0x13] = 29; t[0x14] = 30; t[0x15] = 31;
    t[0x17] = 32; t[0x16] = 33; t[0x1A] = 34; t[0x1C] = 35; t[0x19] = 36;

    // F1–F12.
    t[0x7A] = 37; t[0x78] = 38; t[0x63] = 39; t[0x76] = 40; t[0x60] = 41; t[0x61] = 42;
    t[0x62] = 43; t[0x64] = 44; t[0x65] = 45; t[0x6D] = 46; t[0x67] = 47; t[0x6F] = 48;

    // Arrows / whitespace / edit.
    t[0x7E] = 49; t[0x7D] = 50; t[0x7B] = 51; t[0x7C] = 52;
    t[0x31] = 53; t[0x24] = 54; t[0x30] = 55; t[0x35] = 56;
    t[0x33] = 57; // kVK_Delete (= Backspace on Mac)
    t[0x75] = 58; // kVK_ForwardDelete

    // Navigation.
    t[0x73] = 59; t[0x77] = 60; t[0x74] = 61; t[0x79] = 62;
    t[0x72] = 63; // kVK_Help — used as Insert on Mac

    // Punctuation.
    t[0x1B] = 64; t[0x18] = 65; t[0x21] = 66; t[0x1E] = 67;
    t[0x2A] = 68; t[0x29] = 69; t[0x27] = 70; t[0x2B] = 71;
    t[0x2F] = 72; t[0x2C] = 73; t[0x32] = 74;

    // F13–F20.
    t[0x69] = 75; t[0x6B] = 76; t[0x71] = 77; t[0x6A] = 78;
    t[0x40] = 79; t[0x4F] = 80; t[0x50] = 81; t[0x5A] = 82;

    // Numpad.
    t[0x52] = 83;  t[0x53] = 84;  t[0x54] = 85;  t[0x55] = 86;
    t[0x56] = 87;  t[0x57] = 88;  t[0x58] = 89;  t[0x59] = 90;
    t[0x5B] = 91;  t[0x5C] = 92;
    t[0x41] = 93;  // NumpadDecimal
    t[0x4C] = 94;  // NumpadEnter
    t[0x45] = 95;  // NumpadAdd
    t[0x4E] = 96;  // NumpadSubtract
    t[0x43] = 97;  // NumpadMultiply
    t[0x4B] = 98;  // NumpadDivide
    t[0x51] = 99;  // NumpadEqual
    t[0x47] = 100; // NumpadClear

    t
};
