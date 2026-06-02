//! Shared Perry UI constants and public model metadata.
//!
//! This crate is intentionally tiny: it holds stable values that are consumed by
//! both runtime UI backends and HIR lowering. Keeping the public enum numbers in
//! one place prevents `types/perry/ui/index.d.ts`, compiler lowering, and the
//! event runtime from drifting silently.

/// One member of Perry's public `Key` const enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyMember {
    /// Stable numeric `Key` enum value.
    pub value: u16,
    /// TypeScript enum member name, e.g. `ArrowLeft` or `Digit0`.
    pub enum_name: &'static str,
    /// Runtime event/key name, e.g. `ArrowLeft`, `0`, or `a`.
    pub event_name: &'static str,
}

/// One member of a numeric Perry UI const enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UiConstEnumMember {
    /// TypeScript enum member name.
    pub name: &'static str,
    /// Stable numeric enum value.
    pub value: i64,
}

/// Canonical public `Key` const-enum values and runtime key names.
///
/// `KEY_MEMBERS` and `KEY_TABLE` are expanded from this single source so the
/// compiler-facing enum values and runtime event names cannot drift inside Rust.
macro_rules! key_members {
    (
        ($unknown_value:literal, $unknown_enum_name:literal, $unknown_event_name:literal),
        $(($value:literal, $enum_name:literal, $event_name:literal)),+ $(,)?
    ) => {
        /// Canonical public `Key` const-enum values and runtime key names.
        pub const KEY_MEMBERS: &[KeyMember] = &[
            KeyMember {
                value: $unknown_value,
                enum_name: $unknown_enum_name,
                event_name: $unknown_event_name,
            },
            $(KeyMember {
                value: $value,
                enum_name: $enum_name,
                event_name: $event_name,
            }),+
        ];

        /// All `(KeyCode, name)` pairs in id order, excluding `Unknown`.
        pub const KEY_TABLE: &[(u16, &str)] = &[
            $(($value, $event_name)),+
        ];
    };
}

key_members!(
    (0, "Unknown", ""),
    (1, "A", "a"),
    (2, "B", "b"),
    (3, "C", "c"),
    (4, "D", "d"),
    (5, "E", "e"),
    (6, "F", "f"),
    (7, "G", "g"),
    (8, "H", "h"),
    (9, "I", "i"),
    (10, "J", "j"),
    (11, "K", "k"),
    (12, "L", "l"),
    (13, "M", "m"),
    (14, "N", "n"),
    (15, "O", "o"),
    (16, "P", "p"),
    (17, "Q", "q"),
    (18, "R", "r"),
    (19, "S", "s"),
    (20, "T", "t"),
    (21, "U", "u"),
    (22, "V", "v"),
    (23, "W", "w"),
    (24, "X", "x"),
    (25, "Y", "y"),
    (26, "Z", "z"),
    (27, "Digit0", "0"),
    (28, "Digit1", "1"),
    (29, "Digit2", "2"),
    (30, "Digit3", "3"),
    (31, "Digit4", "4"),
    (32, "Digit5", "5"),
    (33, "Digit6", "6"),
    (34, "Digit7", "7"),
    (35, "Digit8", "8"),
    (36, "Digit9", "9"),
    (37, "F1", "F1"),
    (38, "F2", "F2"),
    (39, "F3", "F3"),
    (40, "F4", "F4"),
    (41, "F5", "F5"),
    (42, "F6", "F6"),
    (43, "F7", "F7"),
    (44, "F8", "F8"),
    (45, "F9", "F9"),
    (46, "F10", "F10"),
    (47, "F11", "F11"),
    (48, "F12", "F12"),
    (49, "ArrowUp", "ArrowUp"),
    (50, "ArrowDown", "ArrowDown"),
    (51, "ArrowLeft", "ArrowLeft"),
    (52, "ArrowRight", "ArrowRight"),
    (53, "Space", "Space"),
    (54, "Enter", "Enter"),
    (55, "Tab", "Tab"),
    (56, "Escape", "Escape"),
    (57, "Backspace", "Backspace"),
    (58, "Delete", "Delete"),
    (59, "Home", "Home"),
    (60, "End", "End"),
    (61, "PageUp", "PageUp"),
    (62, "PageDown", "PageDown"),
    (63, "Insert", "Insert"),
    (64, "Minus", "Minus"),
    (65, "Equal", "Equal"),
    (66, "BracketLeft", "BracketLeft"),
    (67, "BracketRight", "BracketRight"),
    (68, "Backslash", "Backslash"),
    (69, "Semicolon", "Semicolon"),
    (70, "Quote", "Quote"),
    (71, "Comma", "Comma"),
    (72, "Period", "Period"),
    (73, "Slash", "Slash"),
    (74, "Backquote", "Backquote"),
    (75, "F13", "F13"),
    (76, "F14", "F14"),
    (77, "F15", "F15"),
    (78, "F16", "F16"),
    (79, "F17", "F17"),
    (80, "F18", "F18"),
    (81, "F19", "F19"),
    (82, "F20", "F20"),
    (83, "Numpad0", "Numpad0"),
    (84, "Numpad1", "Numpad1"),
    (85, "Numpad2", "Numpad2"),
    (86, "Numpad3", "Numpad3"),
    (87, "Numpad4", "Numpad4"),
    (88, "Numpad5", "Numpad5"),
    (89, "Numpad6", "Numpad6"),
    (90, "Numpad7", "Numpad7"),
    (91, "Numpad8", "Numpad8"),
    (92, "Numpad9", "Numpad9"),
    (93, "NumpadDecimal", "NumpadDecimal"),
    (94, "NumpadEnter", "NumpadEnter"),
    (95, "NumpadAdd", "NumpadAdd"),
    (96, "NumpadSubtract", "NumpadSubtract"),
    (97, "NumpadMultiply", "NumpadMultiply"),
    (98, "NumpadDivide", "NumpadDivide"),
    (99, "NumpadEqual", "NumpadEqual"),
    (100, "NumpadClear", "NumpadClear"),
);

/// Canonical public `Modifier` const-enum values.
pub const MODIFIER_MEMBERS: &[UiConstEnumMember] = &[
    UiConstEnumMember {
        name: "None",
        value: 0,
    },
    UiConstEnumMember {
        name: "Cmd",
        value: 1,
    },
    UiConstEnumMember {
        name: "Shift",
        value: 2,
    },
    UiConstEnumMember {
        name: "Alt",
        value: 4,
    },
    UiConstEnumMember {
        name: "Ctrl",
        value: 8,
    },
];

/// Canonical public `MouseButton` const-enum values.
pub const MOUSE_BUTTON_MEMBERS: &[UiConstEnumMember] = &[
    UiConstEnumMember {
        name: "Left",
        value: 0,
    },
    UiConstEnumMember {
        name: "Middle",
        value: 1,
    },
    UiConstEnumMember {
        name: "Right",
        value: 2,
    },
    UiConstEnumMember {
        name: "Back",
        value: 3,
    },
    UiConstEnumMember {
        name: "Forward",
        value: 4,
    },
];

/// Returns the numeric const-enum members for a Perry UI enum visible to TS.
pub fn const_enum_members(enum_name: &str) -> Option<Vec<UiConstEnumMember>> {
    match enum_name {
        "Key" => Some(
            KEY_MEMBERS
                .iter()
                .map(|member| UiConstEnumMember {
                    name: member.enum_name,
                    value: member.value as i64,
                })
                .collect(),
        ),
        "Modifier" => Some(MODIFIER_MEMBERS.to_vec()),
        "MouseButton" => Some(MOUSE_BUTTON_MEMBERS.to_vec()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    const UI_DTS: &str = include_str!("../../../types/perry/ui/index.d.ts");

    fn parse_const_enum(name: &str) -> BTreeMap<String, i64> {
        let needle = format!("export const enum {name} {{");
        let start = UI_DTS.find(&needle).expect("enum exists in d.ts") + needle.len();
        let rest = &UI_DTS[start..];
        let end = rest.find("}\n").expect("enum closes");
        let body = &rest[..end];
        let mut out = BTreeMap::new();
        for raw_line in body.lines() {
            let line = raw_line.trim().trim_end_matches(',');
            if line.is_empty() {
                continue;
            }
            for part in line.split(',') {
                let part = part.trim();
                if part.is_empty() {
                    continue;
                }
                let Some((member, value)) = part.split_once('=') else {
                    continue;
                };
                out.insert(
                    member.trim().to_string(),
                    value.trim().parse::<i64>().expect("numeric enum value"),
                );
            }
        }
        out
    }

    fn model_const_enum(name: &str) -> BTreeMap<String, i64> {
        const_enum_members(name)
            .expect("known model enum")
            .into_iter()
            .map(|member| (member.name.to_string(), member.value))
            .collect()
    }

    #[test]
    fn key_values_match_dts() {
        assert_eq!(model_const_enum("Key"), parse_const_enum("Key"));
    }

    #[test]
    fn modifier_values_match_dts() {
        assert_eq!(model_const_enum("Modifier"), parse_const_enum("Modifier"));
    }

    #[test]
    fn mouse_button_values_match_dts() {
        assert_eq!(
            model_const_enum("MouseButton"),
            parse_const_enum("MouseButton")
        );
    }

    #[test]
    fn runtime_key_names_are_id_ordered() {
        for (index, member) in KEY_MEMBERS.iter().enumerate() {
            assert_eq!(member.value as usize, index);
        }
    }
}
