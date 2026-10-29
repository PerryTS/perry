//! Compiler-only projection of R onto physical realm cells. Nothing in this
//! plan is a runtime registry: emission consumes it and writes cells/relocations.
use std::collections::{BTreeMap, BTreeSet};

use perry_hir::realm_manifest::{
    RealmDeclaration, RealmProperty, RealmStorageFacts, RealmValue, RuntimeLayoutFacts,
};
use perry_hir::realm_reach::RealmReach;

#[derive(Clone, Debug)]
pub struct RealmPlan {
    /// Physical cells, keyed by their source-owned graph identity. The global
    /// is always present, with only the properties selected by R.
    pub cells: BTreeMap<String, RealmDeclaration>,
    /// Non-image births such as live process arrays and native providers.
    /// A provider must be wired by its owner; it is never permission to run
    /// an installer for the rest of the realm.
    pub providers: BTreeMap<String, RealmDeclaration>,
    /// Native dispatch receivers have a source-owned namespace layout, rather
    /// than a sampled ordinary-object header in the reflective manifest.
    pub native_namespaces: BTreeMap<String, RealmDeclaration>,
    pub runtime_layouts: BTreeMap<String, RuntimeLayoutFacts>,
    pub atoms: BTreeSet<String>,
    /// String keys, including the existing private function-state key.
    pub key_atoms: BTreeSet<String>,
    pub symbols: BTreeSet<String>,
}

impl RealmPlan {
    pub fn from_reach(reach: &RealmReach) -> Result<Self, String> {
        let declarations = crate::realm_manifest::REALM
            .iter()
            .map(|entry| {
                serde_json::from_str(entry.facts)
                    .map_err(|error| format!("realm {}: {error}", entry.name))
            })
            .collect::<Result<Vec<RealmDeclaration>, String>>()?;
        Self::from_declarations(reach, declarations)
    }

    fn from_declarations(
        reach: &RealmReach,
        declarations: Vec<RealmDeclaration>,
    ) -> Result<Self, String> {
        let mut all = BTreeMap::new();
        for declaration in declarations {
            let name = declaration.name.clone();
            if all.insert(name.clone(), declaration).is_some() {
                return Err(format!("duplicate realm declaration {name}"));
            }
        }
        let mut global = all
            .get("%Global%")
            .ok_or("realm has no global cell")?
            .clone();
        if !reach.is_full() {
            global.properties.retain(|property| {
                reach.names.contains(&property.key)
                    || match &property.value {
                        Some(RealmValue::Reference(name)) => {
                            name == "%Global%" || reach.names.contains(name)
                        }
                        // Primitive global constants are always observable at
                        // their fixed values and require no additional cells.
                        Some(RealmValue::Undefined | RealmValue::NumberBits(_)) => true,
                        _ => false,
                    }
            });
            let strings: BTreeSet<_> = global
                .properties
                .iter()
                .filter(|p| !p.symbol)
                .map(|p| p.key.clone())
                .collect();
            let symbols: BTreeSet<_> = global
                .properties
                .iter()
                .filter(|p| p.symbol)
                .map(|p| p.key.clone())
                .collect();
            global.own_key_order.retain(|key| strings.contains(key));
            global.own_symbol_order.retain(|key| symbols.contains(key));
        }
        let mut result = Self {
            cells: BTreeMap::from([("%Global%".into(), global)]),
            providers: BTreeMap::new(),
            native_namespaces: BTreeMap::new(),
            runtime_layouts: BTreeMap::new(),
            atoms: BTreeSet::new(),
            key_atoms: BTreeSet::new(),
            symbols: BTreeSet::new(),
        };
        for (name, declaration) in &all {
            if name == "%Global%" || (!reach.is_full() && !reach.names.contains(name)) {
                continue;
            }
            if let Some(RealmStorageFacts::RuntimeLayout(layout)) = &declaration.storage {
                result.runtime_layouts.insert(name.clone(), layout.clone());
            } else if matches!(declaration.value, Some(RealmValue::Dynamic(_))) {
                result.providers.insert(name.clone(), declaration.clone());
            } else if name.starts_with("%NativeNamespace:")
                && declaration.native_namespace.is_some()
            {
                result
                    .native_namespaces
                    .insert(name.clone(), declaration.clone());
            } else if declaration.header.is_some() {
                result.cells.insert(name.clone(), declaration.clone());
            }
        }
        for declaration in result
            .cells
            .values_mut()
            .chain(result.native_namespaces.values_mut())
        {
            if let Some(module) = &declaration.native_namespace {
                result.atoms.insert(module.clone());
                result.atoms.insert("__module__".into());
            }
            // The witness stores descriptors independently of enumeration
            // order. Slot order follows the declared string/symbol orders.
            declaration.properties = ordered_properties(declaration)?;
            // Native dispatch reads the module identity from physical slot 0.
            // It is omitted from public reflection by the namespace's own-key
            // protocol, just as on the heap birth. Keep that storage in the
            // physical plan, ahead of the already ordered public properties.
            if let Some(module) = &declaration.native_namespace {
                if declaration
                    .properties
                    .iter()
                    .any(|p| !p.symbol && p.key == "__module__")
                {
                    return Err(format!(
                        "native namespace {} redeclares its internal module slot",
                        declaration.name
                    ));
                }
                declaration.properties.insert(
                    0,
                    RealmProperty {
                        key: "__module__".into(),
                        symbol: false,
                        attributes: 7,
                        value: Some(RealmValue::String(module.clone())),
                        get: None,
                        set: None,
                    },
                );
            }
            if declaration
                .header
                .as_ref()
                .is_some_and(|header| header.gc_type == 4)
                && crate::codegen::realm_image::properties::function_has_prototype_state(
                    declaration,
                )?
            {
                result.atoms.insert("p".into());
                result.key_atoms.insert("p".into());
            }
            for property in &declaration.properties {
                if property.symbol {
                    result.symbols.insert(property.key.clone());
                } else {
                    result.atoms.insert(property.key.clone());
                    result.key_atoms.insert(property.key.clone());
                }
                if let Some(value) = &property.value {
                    collect_value(value, &mut result.atoms, &mut result.symbols);
                }
            }
            for capture in declaration
                .captures
                .iter()
                .chain(declaration.storage.iter().flat_map(|s| s.values()))
            {
                collect_value(capture, &mut result.atoms, &mut result.symbols);
            }
        }
        // R must already be closed. Do not make an absent declaration appear
        // reachable by following a missing edge here: fix the birth facts.
        for declaration in result
            .cells
            .values()
            .chain(result.native_namespaces.values())
        {
            let mut references = declaration.prototype.iter().collect::<Vec<_>>();
            for property in &declaration.properties {
                references.extend(property.get.iter().chain(property.set.iter()));
                if let Some(RealmValue::Reference(name) | RealmValue::RawReference(name)) =
                    &property.value
                {
                    references.push(name);
                }
            }
            for capture in declaration
                .captures
                .iter()
                .chain(declaration.storage.iter().flat_map(|s| s.values()))
            {
                if let RealmValue::Reference(name) | RealmValue::RawReference(name) = capture {
                    references.push(name);
                }
            }
            for target in references {
                if !result.cells.contains_key(target)
                    && !result.providers.contains_key(target)
                    && !result.native_namespaces.contains_key(target)
                {
                    return Err(format!(
                        "realm birth closure is incomplete: {} -> {target}",
                        declaration.name
                    ));
                }
            }
        }
        Ok(result)
    }
}

fn collect_value(value: &RealmValue, atoms: &mut BTreeSet<String>, symbols: &mut BTreeSet<String>) {
    match value {
        RealmValue::String(text) | RealmValue::Bytes(text) => {
            atoms.insert(text.clone());
        }
        RealmValue::Symbol(name) => {
            symbols.insert(name.clone());
        }
        _ => {}
    }
}

fn ordered_properties(declaration: &RealmDeclaration) -> Result<Vec<RealmProperty>, String> {
    let mut remaining: BTreeMap<_, _> = declaration
        .properties
        .iter()
        .map(|property| ((property.symbol, property.key.as_str()), property))
        .collect();
    if remaining.len() != declaration.properties.len() {
        return Err(format!("duplicate properties on {}", declaration.name));
    }
    let mut ordered = Vec::with_capacity(remaining.len());
    for (symbol, keys) in [
        (false, &declaration.own_key_order),
        (true, &declaration.own_symbol_order),
    ] {
        for key in keys {
            let Some(property) = remaining.remove(&(symbol, key.as_str())) else {
                // Perry's function own-name enumeration synthesizes prototype
                // for some bodies even when no own descriptor exists. The
                // oracle records that distinction. Do not invent a bag slot.
                if !symbol
                    && key == "prototype"
                    && declaration
                        .header
                        .as_ref()
                        .is_some_and(|header| header.gc_type == 4)
                {
                    continue;
                }
                return Err(format!(
                    "invalid realm key order on {}: {key}",
                    declaration.name
                ));
            };
            ordered.push(property.clone());
        }
    }
    if !remaining.is_empty() {
        return Err(format!(
            "realm key order omits properties on {}",
            declaration.name
        ));
    }
    Ok(ordered)
}

/// Runtime key-list entries encode descriptor restrictions, whereas the
/// manifest records ECMAScript writable/enumerable/configurable bits.
pub fn property_entry(property: &RealmProperty) -> u8 {
    if property.value.is_some() {
        !property.attributes & 7
    } else {
        // A fresh accessor's internal writable bit is true. Missing halves
        // are represented by undefined values in its four-word pair.
        (!property.attributes & 6)
            | 8
            | if property.get.is_some() { 16 } else { 0 }
            | if property.set.is_some() { 32 } else { 0 }
    }
}

/// External cell symbols contain the graph name's UTF-8 bytes. Encoding is
/// injective; replacing punctuation by underscores would alias distinct paths.
pub fn cell_symbol(name: &str) -> String {
    if name == "%Global%" {
        return "perry_realm_global".into();
    }
    if let Some(owner) = name.strip_suffix(".prototype") {
        if !owner.is_empty()
            && owner
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        {
            return format!("perry_realm_{owner}_prototype");
        }
    }
    format!(
        "perry_realm_cell_{}",
        name.bytes()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reach(source: &str) -> RealmReach {
        let module = perry_parser::parse_typescript(source, "realm.cjs").unwrap();
        perry_hir::realm_reach::scan(&module, crate::realm_manifest::REALM)
    }

    #[test]
    fn hello_has_only_reached_cells_and_the_filtered_global() {
        let reach = reach("console.log('hi');");
        assert!(!reach.is_full());
        let plan = RealmPlan::from_reach(&reach).unwrap();
        assert!(plan.cells.contains_key("Array.prototype"));
        assert_eq!(plan.cells["Array"].info_symbol.as_deref(), Some(
            "perry_fn_info_perry-runtime::crate::object::global_this::builtin_thunks::global_this_array_thunk::1::with_rest(0 );"
        ));
        assert!(!plan.cells.contains_key("Map"));
        for name in plan.cells.keys().filter(|name| *name != "%Global%") {
            assert!(reach.names.contains(name), "unreached cell {name}");
        }
        let global = &plan.cells["%Global%"];
        assert!(global
            .properties
            .iter()
            .any(|property| property.key == "Array"));
        assert!(!global
            .properties
            .iter()
            .any(|property| property.key == "Map"));
        assert!(plan.symbols.contains("@@iterator"));
    }

    #[test]
    fn runtime_layouts_follow_reached_births_without_retaining_unrelated_kinds() {
        let hello = RealmPlan::from_reach(&reach("console.log('hi');")).unwrap();
        for layout in [
            "ARRAY_NAMED",
            "ARRAY_PROTOTYPE_NAMED",
            "OBJECT_PROTOTYPE",
            "FUNCTION_PROTOTYPE",
            "ITERATOR_RESULT",
            "RECORD_F327_0",
            "RECORD_F328_0",
        ] {
            assert!(
                hello
                    .runtime_layouts
                    .contains_key(&format!("%Layout:{layout}%")),
                "{layout}"
            );
        }
        // A string literal reaches String, whose declared matching births
        // include RegExp. Hello therefore correctly includes these layouts.
        for layout in ["REGEX_EXEC_NAMED", "REGEX_EXEC_INDICES_NAMED"] {
            assert!(hello
                .runtime_layouts
                .contains_key(&format!("%Layout:{layout}%")));
        }
        let core = RealmPlan::from_reach(&reach("1 + 2;")).unwrap();
        assert!(!core
            .runtime_layouts
            .contains_key("%Layout:REGEX_EXEC_NAMED%"));
        for layout in [
            "URL_COMPONENTS",
            "TIMEOUT",
            "IMMEDIATE",
            "SPAWN_SYNC_RESULT",
            "SQLITE_COLUMN_METADATA",
            "RECORD_F473_0",
        ] {
            assert!(
                !hello
                    .runtime_layouts
                    .contains_key(&format!("%Layout:{layout}%")),
                "{layout}"
            );
        }
        let regex = RealmPlan::from_reach(&reach("/hi/d.exec('hi');")).unwrap();
        for layout in [
            "REGEX_EXEC_NAMED",
            "REGEX_EXEC_INDICES_NAMED",
            "REGEX_INDICES_GROUPS_NAMED",
        ] {
            assert!(
                regex
                    .runtime_layouts
                    .contains_key(&format!("%Layout:{layout}%")),
                "{layout}"
            );
        }
        let timer = RealmPlan::from_reach(&reach("setTimeout(() => {}, 1);")).unwrap();
        assert!(timer.runtime_layouts.contains_key("%Layout:TIMEOUT%"));
        assert!(!timer.runtime_layouts.contains_key("%Layout:IMMEDIATE%"));
        let url = RealmPlan::from_reach(&reach("new URL('https://example.com');")).unwrap();
        assert!(url.runtime_layouts.contains_key("%Layout:URL_COMPONENTS%"));
        assert!(url
            .runtime_layouts
            .contains_key("%Layout:URL_SEARCH_PARAMS%"));
    }

    #[test]
    fn full_realm_contains_source_derived_internal_set_entries_and_atoms() {
        let plan = RealmPlan::from_reach(&reach("with ({}) {}")).unwrap();
        let storage = plan.cells["process.allowedNodeEnvironmentFlags"]
            .storage
            .as_ref()
            .unwrap();
        assert!(storage.values().len() > 200);
        for value in storage.values() {
            let RealmValue::String(text) = value else {
                panic!("flag was not a string")
            };
            assert!(plan.atoms.contains(text), "unemitted Set string {text}");
        }
        assert_eq!(
            Some(storage),
            plan.cells["node:process.allowedNodeEnvironmentFlags"]
                .storage
                .as_ref()
        );
    }

    #[test]
    fn feature_probes_keep_the_named_constructor_without_selecting_top() {
        let reach = reach("console.log(typeof Map === 'undefined');");
        assert!(!reach.is_full());
        let plan = RealmPlan::from_reach(&reach).unwrap();
        assert!(plan.cells.contains_key("Map"));
        assert!(plan.cells["%Global%"]
            .properties
            .iter()
            .any(|property| property.key == "Map"));
    }

    #[test]
    fn full_realm_preserves_key_order_and_live_provider_boundaries() {
        let reach = reach("with ({}) { console.log('hi'); }");
        assert!(reach.is_full());
        let plan = RealmPlan::from_reach(&reach).unwrap();
        assert!(plan.cells.contains_key("Temporal"));
        assert!(!plan.cells.contains_key("process.argv"));
        assert!(plan.providers.contains_key("process.argv"));
        for declaration in plan.cells.values().chain(plan.native_namespaces.values()) {
            let internal = usize::from(declaration.native_namespace.is_some());
            if let Some(module) = &declaration.native_namespace {
                assert_eq!(declaration.properties[0].key, "__module__");
                assert_eq!(
                    declaration.properties[0].value,
                    Some(RealmValue::String(module.clone()))
                );
                assert!(!declaration
                    .own_key_order
                    .iter()
                    .any(|key| key == "__module__"));
            }
            assert_eq!(
                declaration
                    .properties
                    .iter()
                    .skip(internal)
                    .filter(|p| !p.symbol)
                    .map(|p| &p.key)
                    .collect::<Vec<_>>(),
                declaration
                    .own_key_order
                    .iter()
                    .filter(|key| declaration
                        .properties
                        .iter()
                        .any(|property| !property.symbol && &property.key == *key))
                    .collect::<Vec<_>>()
            );
            assert_eq!(
                declaration
                    .properties
                    .iter()
                    .filter(|p| p.symbol)
                    .map(|p| &p.key)
                    .collect::<Vec<_>>(),
                declaration.own_symbol_order.iter().collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn an_absent_birth_is_an_error_not_implicit_reachability() {
        let mut reach = reach("console.log('hi');");
        reach.names.remove("Array.prototype");
        let error = RealmPlan::from_reach(&reach).unwrap_err();
        assert!(error.contains("Array.prototype"), "{error}");
    }

    #[test]
    fn descriptor_attributes_preserve_data_and_accessor_semantics() {
        let mut property = RealmProperty {
            key: "value".into(),
            symbol: false,
            attributes: 5,
            value: Some(RealmValue::Undefined),
            get: None,
            set: None,
        };
        assert_eq!(property_entry(&property), 2); // non-enumerable data
        property.attributes = 4;
        assert_eq!(property_entry(&property), 3); // also non-writable
        property.value = None;
        property.get = Some("getter".into());
        assert_eq!(property_entry(&property), 2 | 8 | 16);
        property.set = Some("setter".into());
        assert_eq!(property_entry(&property), 2 | 8 | 16 | 32);
        property.attributes = 0;
        assert_eq!(property_entry(&property), 2 | 4 | 8 | 16 | 32);
    }

    #[test]
    fn key_order_errors_are_not_hidden_by_function_synthesis() {
        let mut declaration: RealmDeclaration = serde_json::from_str(
            r#"{"name":"f","kind":"function","header":{"gc_type":4,"reserved":0},"properties":[{"key":"name","symbol":false,"attributes":4,"value":{"kind":"string","value":"f"}}],"own_key_order":["name","prototype"]}"#
        ).unwrap();
        assert_eq!(ordered_properties(&declaration).unwrap().len(), 1);
        declaration.own_key_order.push("missing".into());
        assert!(ordered_properties(&declaration).is_err());
        declaration.own_key_order.clear();
        assert!(ordered_properties(&declaration).is_err());
    }

    #[test]
    fn symbols_cannot_alias_after_punctuation_encoding() {
        assert_ne!(cell_symbol("A.b"), cell_symbol("A_b"));
        assert_ne!(cell_symbol("A[\"b\"]"), cell_symbol("A.b"));
        assert_eq!(cell_symbol("%Global%"), "perry_realm_global");
        assert_eq!(
            cell_symbol("Array.prototype"),
            "perry_realm_Array_prototype"
        );
        assert_ne!(
            cell_symbol("Array.prototype"),
            cell_symbol("Array_prototype")
        );
    }
}
