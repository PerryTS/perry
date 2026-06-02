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

/// All `(KeyCode, name)` pairs in id order, excluding `Unknown`.
pub const KEY_TABLE: &[(u16, &str)] = perry_ui_model::KEY_TABLE;

/// Lookup a canonical key name from its id. Returns `""` for unknown.
#[inline]
pub fn name(code: KeyCode) -> &'static str {
    perry_ui_model::KEY_MEMBERS
        .get(code.0 as usize)
        .filter(|member| member.value == code.0)
        .map(|member| member.event_name)
        .unwrap_or("")
}

/// Lookup a canonical key id from a name. Returns `KeyCode::UNKNOWN` if not found.
pub fn from_name(s: &str) -> KeyCode {
    // Linear scan: ~100 entries, dominated by 1-3 char compares. Faster
    // than HashMap on this size and no allocation at startup.
    for member in perry_ui_model::KEY_MEMBERS {
        if member.event_name == s {
            return KeyCode(member.value);
        }
    }
    KeyCode::UNKNOWN
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
