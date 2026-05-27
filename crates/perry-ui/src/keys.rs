//! Canonical key naming for perry/ui keyboard events.
//!
//! Each key has a stable `u16` id and a `&'static str` JS-visible name. The id
//! lets backends route events without ever touching the string in the hot path:
//! native dispatch maps platform scancodes → `KeyCode` (u16) → callback. The
//! string is materialised once (lazy intern on the runtime side) and reused
//! for every subsequent event.
//!
//! Modifier bits follow the existing `registerKeyboardShortcut` contract:
//! 1=Cmd/Win, 2=Shift, 4=Alt/Option, 8=Control.

/// Stable identifier for a normalised keyboard key.
///
/// `0` is reserved as `Unknown`. Ranges are grouped so callers can do cheap
/// classification (`is_letter`, `is_digit`, …) without a table.
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct KeyCode(pub u16);

impl KeyCode {
    pub const UNKNOWN: KeyCode = KeyCode(0);

    #[inline]
    pub const fn raw(self) -> u16 {
        self.0
    }

    #[inline]
    pub fn is_letter(self) -> bool {
        (1..=26).contains(&self.0)
    }

    #[inline]
    pub fn is_digit(self) -> bool {
        (27..=36).contains(&self.0)
    }

    #[inline]
    pub fn is_function(self) -> bool {
        (37..=48).contains(&self.0)
    }
}

/// Modifier bit layout, shared with `registerKeyboardShortcut` / `registerGlobalHotkey`.
pub mod modifiers {
    pub const CMD: u32 = 1;
    pub const SHIFT: u32 = 2;
    pub const ALT: u32 = 4;
    pub const CTRL: u32 = 8;
}

macro_rules! key_table {
    ( $( $id:literal => $name:literal ),* $(,)? ) => {
        /// All `(KeyCode, name)` pairs in id order. `id == index + 1`.
        pub const KEY_TABLE: &[(u16, &str)] = &[ $( ($id, $name) ),* ];

        /// Lookup a canonical key name from its id. Returns `""` for unknown.
        #[inline]
        pub fn name(code: KeyCode) -> &'static str {
            let idx = code.0 as usize;
            if idx == 0 || idx > KEY_TABLE.len() { return ""; }
            KEY_TABLE[idx - 1].1
        }

        /// Lookup a canonical key id from a name. Returns `KeyCode::UNKNOWN` if not found.
        pub fn from_name(s: &str) -> KeyCode {
            // Linear scan: ~80 entries, dominated by 1-3 char compares. Faster
            // than HashMap on this size and no allocation at startup.
            for &(id, n) in KEY_TABLE {
                if n == s { return KeyCode(id); }
            }
            KeyCode::UNKNOWN
        }
    };
}

key_table! {
    // Letters (1-26) — lowercase to match Web KeyboardEvent.key when unmodified.
    1  => "a",  2  => "b",  3  => "c",  4  => "d",  5  => "e",  6  => "f",
    7  => "g",  8  => "h",  9  => "i",  10 => "j",  11 => "k",  12 => "l",
    13 => "m",  14 => "n",  15 => "o",  16 => "p",  17 => "q",  18 => "r",
    19 => "s",  20 => "t",  21 => "u",  22 => "v",  23 => "w",  24 => "x",
    25 => "y",  26 => "z",

    // Digits (27-36).
    27 => "0", 28 => "1", 29 => "2", 30 => "3", 31 => "4",
    32 => "5", 33 => "6", 34 => "7", 35 => "8", 36 => "9",

    // Function keys (37-48).
    37 => "F1", 38 => "F2", 39 => "F3", 40 => "F4", 41 => "F5",  42 => "F6",
    43 => "F7", 44 => "F8", 45 => "F9", 46 => "F10", 47 => "F11", 48 => "F12",

    // Arrows + whitespace + edit.
    49 => "ArrowUp",
    50 => "ArrowDown",
    51 => "ArrowLeft",
    52 => "ArrowRight",
    53 => "Space",
    54 => "Enter",
    55 => "Tab",
    56 => "Escape",
    57 => "Backspace",
    58 => "Delete",

    // Navigation.
    59 => "Home",
    60 => "End",
    61 => "PageUp",
    62 => "PageDown",
    63 => "Insert",

    // Standard punctuation that needs a name (the rest comes through onTextInput).
    64 => "Minus",
    65 => "Equal",
    66 => "BracketLeft",
    67 => "BracketRight",
    68 => "Backslash",
    69 => "Semicolon",
    70 => "Quote",
    71 => "Comma",
    72 => "Period",
    73 => "Slash",
    74 => "Backquote",

    // Extended function keys (full-size keyboards / external ones).
    75 => "F13", 76 => "F14", 77 => "F15", 78 => "F16",
    79 => "F17", 80 => "F18", 81 => "F19", 82 => "F20",

    // Numeric keypad (distinct from top-row digits so apps can tell them apart).
    83 => "Numpad0", 84 => "Numpad1", 85 => "Numpad2", 86 => "Numpad3",
    87 => "Numpad4", 88 => "Numpad5", 89 => "Numpad6", 90 => "Numpad7",
    91 => "Numpad8", 92 => "Numpad9",
    93 => "NumpadDecimal",
    94 => "NumpadEnter",
    95 => "NumpadAdd",
    96 => "NumpadSubtract",
    97 => "NumpadMultiply",
    98 => "NumpadDivide",
    99 => "NumpadEqual",
    100 => "NumpadClear",
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn letters_are_lowercase_single_char() {
        for i in 1u16..=26 {
            let n = name(KeyCode(i));
            assert_eq!(n.len(), 1);
            assert!(n.chars().next().unwrap().is_ascii_lowercase());
        }
    }

    #[test]
    fn ids_are_dense_and_unique() {
        for (idx, (id, name)) in KEY_TABLE.iter().enumerate() {
            assert_eq!(*id as usize, idx + 1, "table must be id-dense at {name}");
        }
        let mut names: Vec<&str> = KEY_TABLE.iter().map(|(_, n)| *n).collect();
        names.sort_unstable();
        for w in names.windows(2) {
            assert_ne!(w[0], w[1], "duplicate key name: {}", w[0]);
        }
    }

    #[test]
    fn roundtrip() {
        for &(id, n) in KEY_TABLE {
            let code = from_name(n);
            assert_eq!(code, KeyCode(id), "lookup({n})");
            assert_eq!(name(code), n);
        }
    }

    #[test]
    fn classification() {
        assert!(from_name("a").is_letter());
        assert!(!from_name("a").is_digit());
        assert!(from_name("7").is_digit());
        assert!(from_name("F7").is_function());
        assert!(!from_name("ArrowUp").is_letter());
        assert_eq!(from_name("not-a-key"), KeyCode::UNKNOWN);
    }
}
