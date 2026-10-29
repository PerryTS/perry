//! Generate a link-and-ABI test from the same manifest the compiler consumes.
//! Included only under cfg(test); no lookup inventory enters a runtime archive.
use perry_runtime_manifest::realm_json::Value;
use std::collections::BTreeMap;
use std::fmt::Write;
use std::path::Path;

#[path = "realm_layout_facts.rs"]
mod realm_layout_facts;

pub fn write(manifest: &Value, out: &Path) -> Result<(), String> {
    let declarations = manifest
        .get("declarations")
        .and_then(Value::array)
        .ok_or("realm declarations missing")?;
    let mut infos = BTreeMap::new();
    for declaration in declarations {
        let Some(symbol) = declaration.get("info_symbol").and_then(Value::string) else {
            continue;
        };
        if !symbol.starts_with("perry_fn_info_perry-runtime::") {
            continue;
        }
        let facts = declaration
            .get("function_info")
            .ok_or("export has no function info")?;
        if let Some(old) = infos.insert(symbol, facts) {
            for field in [
                "params",
                "rest_fixed",
                "flags",
                "length",
                "declared",
                "plain_params",
            ] {
                if old.get(field) != facts.get(field) {
                    return Err(format!("conflicting ABI for {symbol}"));
                }
            }
        }
    }
    if infos.is_empty() {
        return Err("realm has no runtime info exports".into());
    }
    let mut source = String::from("#[test]\nfn every_runtime_realm_info_export_links_with_declared_abi() {\nunsafe extern \"C\" {\n");
    for (i, symbol) in infos.keys().enumerate() {
        writeln!(
            source,
            "#[link_name = {symbol:?}] static INFO_{i}: crate::closure::JsFunctionInfo;"
        )
        .unwrap();
    }
    source.push_str("}\nlet infos: &[(&str, &crate::closure::JsFunctionInfo, [u64; 6])] = &[\n");
    for (i, (symbol, info)) in infos.iter().enumerate() {
        let facts = [
            "params",
            "rest_fixed",
            "flags",
            "length",
            "declared",
            "plain_params",
        ]
        .into_iter()
        .map(|field| {
            info.get(field)
                .map(Value::encode)
                .ok_or_else(|| format!("missing {field} for {symbol}"))
        })
        .collect::<Result<Vec<_>, _>>()?;
        writeln!(
            source,
            "({symbol:?}, unsafe {{ &INFO_{i} }}, [{}]),",
            facts.join(", ")
        )
        .unwrap();
    }
    source.push_str("] ;\nfor (symbol, info, expected) in infos {\nlet actual = [u64::from(info.params), u64::from(info.rest_fixed), u64::from(info.flags & !crate::codegen_abi::FN_HAS_METADATA), u64::from(info.length), u64::from(info.declared), u64::from(info.plain_params().wrapping_add(1))];\nassert_eq!(actual, *expected, \"{symbol}\");\nassert!(!info.code.is_null(), \"{symbol}\");\n}\n}\n");
    std::fs::write(out.join("realm_info_exports_test.rs"), source).map_err(|e| e.to_string())?;
    realm_layout_facts::write(declarations, out)?;
    write_set_storage(declarations, out)?;
    write_namespace_storage(declarations, out)
}

fn write_set_storage(declarations: &[Value], out: &Path) -> Result<(), String> {
    let names = [
        "process.allowedNodeEnvironmentFlags",
        "node:process.allowedNodeEnvironmentFlags",
    ];
    let mut expected = None;
    for name in names {
        let declaration = declarations
            .iter()
            .find(|d| d.get("name").and_then(Value::string) == Some(name))
            .ok_or_else(|| format!("missing Set declaration {name}"))?;
        let storage = declaration
            .get("storage")
            .ok_or_else(|| format!("missing Set storage {name}"))?;
        if storage.get("kind").and_then(Value::string) != Some("set") {
            return Err(format!("wrong Set storage {name}"));
        }
        let entries = storage
            .get("entries")
            .and_then(Value::array)
            .ok_or("missing Set entries")?;
        let strings = entries
            .iter()
            .map(|entry| {
                if entry.get("kind").and_then(Value::string) != Some("string") {
                    return Err("non-string flags entry");
                }
                entry
                    .get("value")
                    .and_then(Value::string)
                    .ok_or("missing flag value")
            })
            .collect::<Result<Vec<_>, _>>()?;
        if let Some(previous) = &expected {
            if previous != &strings {
                return Err("process flag declarations disagree".into());
            }
        } else {
            expected = Some(strings);
        }
    }
    let expected = expected.unwrap();
    // Build the serialized scalar index with the emitter's insertion history.
    // The runtime test compares this against real JS string hashing and births.
    let mut image_index = perry_abi::OwnedSetIndex::new(perry_abi::SMALL_SET_SCAN_MAX as usize + 1);
    for (slot, text) in expected.iter().enumerate() {
        let hash = text.bytes().fold(0xcbf2_9ce4_8422_2325u64, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
        });
        image_index.insert_hash(perry_abi::set_string_index_hash(hash), slot as u32);
    }
    let buckets = image_index
        .buckets()
        .iter()
        .map(|bucket| (bucket.hash, bucket.index))
        .collect::<Vec<_>>();
    let counts = (image_index.live_len(), image_index.occupied_len());
    let source = format!(
        r#"
#[test]
fn manifest_set_entries_match_the_owning_runtime_birth() {{
    crate::gc::initialize_runtime_fixture();
    let scope = crate::gc::RuntimeHandleScope::new();
    let set = scope.root_nanbox_f64(super::process_allowed_flags_value());
    let expected: &[&str] = &{expected:?};
    let set_ptr = crate::value::js_nanbox_get_pointer(set.get_nanbox_f64()) as *const crate::set::SetHeader;
    assert_eq!(crate::set::js_set_size(set_ptr) as usize, expected.len());
    let expected_buckets: &[(u32, u32)] = &{buckets:?};
    let (actual_buckets, live, occupied) = unsafe {{ crate::set::test_index_buckets(set_ptr) }};
    assert_eq!(actual_buckets, expected_buckets, "image index differs from runtime birth");
    assert_eq!((live, occupied), {counts:?});
    for (index, text) in expected.iter().enumerate() {{
        let value = unsafe {{ crate::set::set_value_raw(set_ptr, index as u32) }};
        let actual = crate::string::with_string_value_bytes(value, |bytes| bytes.to_vec()).unwrap();
        assert_eq!(actual, text.as_bytes(), "Set insertion order at {{index}}");
    }}
}}
"#
    );
    std::fs::write(out.join("realm_set_storage_test.rs"), source).map_err(|e| e.to_string())
}

fn write_namespace_storage(declarations: &[Value], out: &Path) -> Result<(), String> {
    let mut expected = String::new();
    for declaration in declarations {
        let Some(name) = declaration.get("name").and_then(Value::string) else {
            continue;
        };
        if !name.starts_with("%NativeNamespace:") {
            continue;
        }
        let module = declaration
            .get("native_namespace")
            .and_then(Value::string)
            .ok_or("native receiver lacks its module identity")?;
        let header = declaration
            .get("header")
            .ok_or("native receiver lacks its header facts")?;
        let reserved = header
            .get("reserved")
            .ok_or("native receiver lacks reserved facts")?
            .encode();
        writeln!(expected, "({name:?}, {module:?}, {reserved}u16),").unwrap();
    }
    let source = format!(
        r#"
#[test]
fn realm_native_receiver_facts_match_namespace_births() {{
    crate::gc::initialize_runtime_fixture();
    let _no_move = crate::gc::GcSuppressScope::new();
    for (name, module, reserved) in [ {expected} ] {{
        let value = super::js_create_native_module_namespace(module.as_ptr(), module.len());
        let obj = crate::value::js_nanbox_get_pointer(value) as *const crate::object::ObjectHeader;
        unsafe {{
            let header = crate::gc::header_from_trusted_user_ptr(obj.cast());
            assert_eq!((*header).obj_type, crate::gc::GC_TYPE_OBJECT, "{{name}}");
            assert_eq!((*header)._reserved, reserved, "{{name}}");
            assert_eq!((*obj).class_id, super::NATIVE_MODULE_CLASS_ID, "{{name}}");
            assert_eq!(super::read_native_module_name(obj).as_deref(), Some(module), "{{name}}");
        }}
    }}
}}
"#
    );
    std::fs::write(out.join("realm_native_storage_test.rs"), source).map_err(|e| e.to_string())
}
