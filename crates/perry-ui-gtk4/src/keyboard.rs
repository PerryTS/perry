//! GTK4 keyboard event hook for `onKeyDown` / `onKeyUp` (issue #1864).
//!
//! Attaches a `GtkEventControllerKey` to the application window and routes
//! `key-pressed` / `key-released` signals to the shared dispatcher in
//! [`perry_ui::key_dispatch`]. Key name normalisation (GDK → canonical
//! [`KeyCode`]) lives in [`gdk_name_to_keycode`].
//!
//! Attached **once per window** as part of `app_run`'s post-creation hooks
//! (alongside `install_shortcuts_on_window`). Callbacks registered via
//! `set_on_key_down`/`set_on_key_up` before the window exists are kept in
//! the dispatcher's thread-local maps and start firing as soon as the
//! controller is live.

use gtk4::glib;
use gtk4::prelude::*;
use gtk4::{ApplicationWindow, EventControllerKey};
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

/// Attach one EventControllerKey to the given window. Called from `app_run`
/// right after window creation, mirroring `install_shortcuts_on_window`.
pub fn install_on_window(window: &ApplicationWindow) {
    let controller = EventControllerKey::new();

    controller.connect_key_pressed(move |_, keyval, _keycode, modifier| {
        let name = keyval.name().map(|n| n.to_string()).unwrap_or_default();
        let code = gdk_name_to_keycode(&name);
        let mods = perry_mods_from_gdk(modifier);
        // GTK4 doesn't expose isARepeat on the signal; passing `false` keeps
        // the contract conservative — apps that need edge-only detection
        // dedup against their own held set.
        key_dispatch::on_key_event(code, mods, true, false);
        glib::Propagation::Proceed
    });

    controller.connect_key_released(move |_, keyval, _keycode, modifier| {
        let name = keyval.name().map(|n| n.to_string()).unwrap_or_default();
        let code = gdk_name_to_keycode(&name);
        let mods = perry_mods_from_gdk(modifier);
        key_dispatch::on_key_event(code, mods, false, false);
    });

    // `modifiers` signal fires for modifier-only transitions (Shift held alone),
    // so `current_modifiers()` stays accurate without any physical key event.
    controller.connect_modifiers(|_, state| {
        key_dispatch::update_modifiers(perry_mods_from_gdk(state));
        glib::Propagation::Proceed
    });

    window.add_controller(controller);
}

#[inline]
fn perry_mods_from_gdk(state: gtk4::gdk::ModifierType) -> u32 {
    use gtk4::gdk::ModifierType;
    let mut m = 0u32;
    // On Linux, Cmd is conventionally remapped to Ctrl in the API contract.
    if state.contains(ModifierType::CONTROL_MASK) {
        m |= 1 | 8;
    }
    if state.contains(ModifierType::SHIFT_MASK) {
        m |= 2;
    }
    if state.contains(ModifierType::ALT_MASK) {
        m |= 4;
    }
    m
}

/// GDK key name → canonical KeyCode. GDK names cover way more than our
/// canonical set; we map the subset that appears in `Key`. Unknown names
/// return `KeyCode::UNKNOWN`, which the dispatcher silently drops.
fn gdk_name_to_keycode(name: &str) -> KeyCode {
    // Letters / digits / single-char punctuation: GDK lowercases them.
    if name.len() == 1 {
        let c = name.chars().next().unwrap();
        if c.is_ascii_alphabetic() {
            return KeyCode((c.to_ascii_lowercase() as u8 - b'a' + 1) as u16);
        }
        if c.is_ascii_digit() {
            // '0'->27, '1'->28, …
            let id = if c == '0' {
                27
            } else {
                27 + (c as u16 - b'0' as u16)
            };
            return KeyCode(id);
        }
    }

    // F1..F20.
    if let Some(rest) = name.strip_prefix('F') {
        if let Ok(n) = rest.parse::<u16>() {
            if (1..=12).contains(&n) {
                return KeyCode(36 + n);
            }
            if (13..=20).contains(&n) {
                return KeyCode(62 + n);
            }
        }
    }

    // Numpad: GDK uses "KP_0".."KP_9", "KP_Decimal", "KP_Enter", "KP_Add", etc.
    if let Some(rest) = name.strip_prefix("KP_") {
        if rest.len() == 1 {
            if let Some(d) = rest.chars().next().and_then(|c| c.to_digit(10)) {
                return KeyCode(83 + d as u16);
            }
        }
        return KeyCode(match rest {
            "Decimal" => 93,
            "Enter" => 94,
            "Add" => 95,
            "Subtract" => 96,
            "Multiply" => 97,
            "Divide" => 98,
            "Equal" => 99,
            "Begin" | "Clear" => 100, // GDK "Clear" / Numpad-5 "Begin"
            _ => 0,
        });
    }

    // Named keys — GDK uses CamelCase for most.
    KeyCode(match name {
        "Up" => 49,
        "Down" => 50,
        "Left" => 51,
        "Right" => 52,
        "space" => 53,
        "Return" => 54,
        "Tab" => 55,
        "Escape" => 56,
        "BackSpace" => 57,
        "Delete" => 58,
        "Home" => 59,
        "End" => 60,
        "Page_Up" => 61,
        "Page_Down" => 62,
        "Insert" => 63,
        "minus" => 64,
        "equal" => 65,
        "bracketleft" => 66,
        "bracketright" => 67,
        "backslash" => 68,
        "semicolon" => 69,
        "apostrophe" => 70,
        "comma" => 71,
        "period" => 72,
        "slash" => 73,
        "grave" => 74,
        _ => 0,
    })
}
