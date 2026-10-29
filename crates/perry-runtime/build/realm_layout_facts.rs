//! Test-only verification of compiler manifest facts against compiled births.
use perry_runtime_manifest::realm_json::Value;
use std::fmt::Write;
use std::path::Path;

pub fn write(declarations: &[Value], out: &Path) -> Result<(), String> {
    let mut source = String::from(
        "#[test]\nfn all_runtime_layout_manifest_facts_match_compiled_descriptors() {\nunsafe {\n",
    );
    let mut count = 0;
    for declaration in declarations {
        let Some(storage) = declaration.get("storage") else {
            continue;
        };
        if storage.get("kind").and_then(Value::string) != Some("runtime_layout") {
            continue;
        }
        let symbol = storage
            .get("symbol")
            .and_then(Value::string)
            .ok_or("layout lacks symbol")?;
        writeln!(source, "unsafe extern \"C\" {{ #[link_name = {symbol:?}] static D{count}: super::PerryLayoutDesc; }}").unwrap();
        let variants = storage
            .get("variants")
            .and_then(Value::array)
            .ok_or("layout lacks variants")?;
        for variant in variants {
            let cfg = match variant.get("target").and_then(Value::string) {
                Some("any") => "",
                Some("windows") => "#[cfg(windows)]",
                Some("non_windows") => "#[cfg(not(windows))]",
                _ => return Err("invalid layout target facts".into()),
            };
            let scalar = [
                "family",
                "variant",
                "object_kind",
                "integrity",
                "rep",
                "class_id",
                "parent_class_id",
            ]
            .iter()
            .map(|field| {
                variant
                    .get(field)
                    .map(Value::encode)
                    .ok_or_else(|| format!("missing layout {field}"))
            })
            .collect::<Result<Vec<_>, _>>()?
            .join(", ");
            let prototype = variant.get("prototype").ok_or("layout lacks prototype")?;
            let prototype = match prototype.get("kind").and_then(Value::string) {
                Some("object") => "(0, 0)".into(),
                Some("null") => "(1, 0)".into(),
                Some("serial") => format!(
                    "(2, {})",
                    prototype
                        .get("serial")
                        .ok_or("missing prototype serial")?
                        .encode()
                ),
                _ => return Err("invalid layout prototype".into()),
            };
            let mut keys = Vec::new();
            for key in variant
                .get("keys")
                .and_then(Value::array)
                .ok_or("layout lacks keys")?
            {
                let atom = key.get("atom").ok_or("key lacks atom")?;
                let kind = atom.get("kind").ok_or("atom lacks kind")?.encode();
                let index = atom.get("index").ok_or("atom lacks index")?.encode();
                let text = atom
                    .get("text")
                    .and_then(Value::string)
                    .map_or("None".into(), |v| format!("Some({v:?})"));
                let registered = atom
                    .get("registered")
                    .map_or("None".into(), |v| format!("Some({})", v.encode()));
                keys.push(format!(
                    "({kind}, {index}, {text}, {registered}, {}, {})",
                    key.get("slot").ok_or("missing slot")?.encode(),
                    key.get("attributes").ok_or("missing attributes")?.encode()
                ));
            }
            writeln!(source, "{cfg}\ncheck_source_layout(std::ptr::addr_of!(D{count}), {symbol:?}, ({scalar}), {prototype}, &[{}]);", keys.join(", ")).unwrap();
        }
        count += 1;
    }
    if count != perry_abi::NATIVE_LAYOUT_DESCRIPTORS.len() {
        return Err(format!(
            "layout fact coverage {count} differs from native ABI"
        ));
    }
    source.push_str("}\n}\n");
    source.push_str(r#"
unsafe fn check_source_layout(desc: *const super::PerryLayoutDesc, name: &str,
    scalar: (u32, u16, u8, u8, u64, u32, u32), prototype: (u8, u64),
    keys: &[(u8, u32, Option<&str>, Option<bool>, u32, u8)]) {
    let d = unsafe { &*desc };
    assert_eq!((d.family, d.variant, d.kind, d.integrity, d.rep, d.class_id, d.parent_class_id), scalar, "{name}");
    assert!(d.methods.is_null(), "{name}");
    assert_eq!((d.key_count as usize, d.slot_count as usize), (keys.len(), keys.len()), "{name}");
    match prototype.0 {
        0 => assert_eq!(d.proto as usize, 0, "{name}"),
        1 => assert_eq!(d.proto as usize, 1, "{name}"),
        _ => { assert!(d.proto as usize >= 4, "{name}"); assert_eq!(unsafe { (*d.proto).serial }, prototype.1, "{name}"); }
    }
    for (key, &(kind, index, text, registered, slot, attrs)) in unsafe { std::slice::from_raw_parts(d.keys, keys.len()) }.iter().zip(keys) {
        let atom = unsafe { &*key.atom };
        assert_eq!((atom.kind, atom.index, key.slot, key.attrs), (kind, index, slot, attrs), "{name}");
        if let Some(expected) = text {
            assert!(!atom.string.is_null(), "{name}");
            let bytes = unsafe { std::slice::from_raw_parts(crate::string::string_data(atom.string), (*atom.string).byte_len as usize) };
            assert_eq!(bytes, expected.as_bytes(), "{name}");
        } else { assert!(atom.string.is_null(), "{name}"); }
        let hash = match kind {
            crate::codegen_abi::ATOM_KIND_STRING => super::atom_hash(text.unwrap().as_bytes()),
            crate::codegen_abi::ATOM_KIND_WELL_KNOWN_SYMBOL => crate::codegen_abi::well_known_symbol_atom_hash(index),
            crate::codegen_abi::ATOM_KIND_PROGRAM_SYMBOL => {
                let (owner, actual_registered) = super::RUNTIME_SYMBOL_ATOMS[index as usize];
                assert!(std::ptr::eq(owner, atom), "{name}");
                assert_eq!(registered, Some(actual_registered), "{name}");
                super::runtime_symbol_atom_hash(index)
            }
            _ => panic!("invalid atom kind for {name}"),
        };
        assert_eq!(atom.hash, hash, "{name}");
    }
}
"#);
    std::fs::write(out.join("realm_layout_facts_test.rs"), source).map_err(|e| e.to_string())
}
