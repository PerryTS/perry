//! Linker-safe names shared by JS symbol definitions and references.
//!
//! Components are UTF-8 bytes. `.uHH` escapes are disjoint from plain names
//! because `.` is itself escaped. `$` remains reserved for generated suffixes.
//! These names work unquoted in LLVM IR and in ELF, Mach-O, COFF and Wasm.
//! Reflection always uses HIR/source names, never these encoded components.

pub fn encode_identifier(name: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = String::with_capacity(name.len());
    for byte in name.bytes() {
        if byte.is_ascii_alphanumeric() || byte == b'_' {
            out.push(char::from(byte));
        } else {
            out.push_str(".u");
            out.push(char::from(HEX[usize::from(byte >> 4)]));
            out.push(char::from(HEX[usize::from(byte & 15)]));
        }
    }
    out
}

/// Keep historical punctuation folding for ASCII paths, then encode Unicode
/// bytes with the same component encoder. Callers pass this canonical prefix to
/// all constructors below, without encoding it a second time.
pub fn module_prefix(name: &str) -> String {
    let normalized: String = name
        .chars()
        .map(|c| {
            if c.is_ascii() && !c.is_ascii_alphanumeric() && c != '_' {
                '_'
            } else {
                c
            }
        })
        .collect();
    let mut prefix = encode_identifier(&normalized);
    if prefix.as_bytes().first().is_some_and(u8::is_ascii_digit) {
        prefix.insert(0, '_');
    }
    prefix
}

pub fn function(prefix: &str, name: &str) -> String {
    format!("perry_fn_{prefix}__{}", encode_identifier(name))
}

pub fn method(prefix: &str, class: &str, name: &str) -> String {
    format!(
        "perry_method_{prefix}__{}__{}",
        encode_identifier(class),
        encode_identifier(name)
    )
}

pub fn static_method(prefix: &str, class_id: u32, class: &str, name: &str) -> String {
    format!(
        "perry_static_{prefix}__{}__c{class_id}__{}",
        encode_identifier(class),
        encode_identifier(name)
    )
}

/// Static fields and the legacy static-method ABI (without a source class id).
pub fn static_member(prefix: &str, class: &str, name: &str) -> String {
    format!(
        "perry_static_{prefix}__{}__{}",
        encode_identifier(class),
        encode_identifier(name)
    )
}

pub fn constructor(prefix: &str, class: &str) -> String {
    format!("{prefix}__{}_constructor", encode_identifier(class))
}

pub fn default_fields(prefix: &str, class: &str) -> String {
    format!("{prefix}__{}_default_fields", encode_identifier(class))
}

pub fn closure(prefix: &str, id: impl std::fmt::Display) -> String {
    format!("perry_closure_{prefix}__{id}")
}

pub fn function_wrapper(prefix: &str, name: &str) -> String {
    format!("__perry_wrap_{}", function(prefix, name))
}

pub fn class_first_evaluation(template: &str) -> String {
    format!("@perry_class_first_eval.{}", encode_identifier(template))
}

pub fn class_keys(prefix: &str, class: &str) -> String {
    format!("{}{}", class_keys_prefix(prefix), encode_identifier(class))
}

pub fn class_keys_prefix(prefix: &str) -> String {
    format!("perry_class_keys_{prefix}__")
}

pub fn class_environment(prefix: &str, class: &str, slot: impl std::fmt::Display) -> String {
    format!(
        "perry_classenv_{prefix}__{}__{slot}",
        encode_identifier(class)
    )
}

pub fn namespace_value(prefix: &str, namespace: &str, member: &str) -> String {
    format!(
        "__perry_namespace_value_{prefix}__{}",
        encode_identifier(&format!("{namespace}:{member}"))
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decode(encoded: &str) -> String {
        let mut bytes = Vec::new();
        let mut rest = encoded;
        while !rest.is_empty() {
            if let Some(escape) = rest.strip_prefix(".u") {
                bytes.push(u8::from_str_radix(&escape[..2], 16).unwrap());
                rest = &escape[2..];
            } else {
                bytes.push(rest.as_bytes()[0]);
                rest = &rest[1..];
            }
        }
        String::from_utf8(bytes).unwrap()
    }

    #[test]
    fn ascii_components_are_unchanged() {
        for name in ["", "ABC_xyz0123456789", "_", "u__24_", "012"] {
            assert_eq!(encode_identifier(name), name);
        }
        assert_eq!(encode_identifier("$"), ".u24");
        assert_eq!(encode_identifier("É"), ".uC3.u89");
        assert_eq!(module_prefix("src/05_file.ts"), "src_05_file_ts");
        assert_eq!(module_prefix("src/Étage.ts"), "src_.uC3.u89tage_ts");
        assert_ne!(module_prefix("Ü.ts"), module_prefix("Ö.ts"));
    }

    #[test]
    fn encoding_is_injective_and_reversible() {
        let mut seen = std::collections::HashSet::new();
        // Every Unicode scalar, including every possible escape-marker byte.
        for c in (0..=0x10ffff).filter_map(char::from_u32) {
            let name = c.to_string();
            let encoded = encode_identifier(&name);
            assert!(seen.insert(encoded.clone()), "collision for {name:?}");
            assert_eq!(decode(&encoded), name);
            assert!(!encoded.contains('$'));
        }
        for name in [
            "$",
            "u__24_",
            ".u24",
            "Étage",
            "grüße",
            "#℘",
            "a.b",
            "a$generic",
            "a.u24generic",
        ] {
            assert_eq!(decode(&encode_identifier(name)), name);
        }
        assert_ne!(encode_identifier("$"), encode_identifier("u__24_"));
        assert_ne!(encode_identifier("$"), encode_identifier(".u24"));
        assert_ne!(
            function("m", "f$generic"),
            format!("{}$generic", function("m", "f"))
        );
    }
}
