//! Compiler-only projection of R onto physical realm cells. Nothing in this
//! plan is a runtime registry: emission consumes it and writes cells/relocations.
use std::collections::{BTreeMap, BTreeSet};

use perry_hir::realm_manifest::{
    RealmDeclaration, RealmProperty, RealmStorageFacts, RealmValue, RuntimeLayoutFacts,
};
use perry_hir::realm_reach::RealmReach;

#[derive(Clone, Debug)]
pub struct RealmPlan {
    /// Physical cells keyed by source identity. A program whose realm-creating
    /// operation never executes emits none; R still owns its runtime layouts.
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
        let index = perry_hir::realm_reach_index::RealmIndex::new(crate::realm_manifest::REALM);
        let declarations = index
            .rows()
            .map(|entry| {
                serde_json::from_value(entry.facts().clone())
                    .map_err(|error| format!("realm {}: {error}", entry.name()))
            })
            .collect::<Result<Vec<RealmDeclaration>, String>>()?;
        Self::from_declarations(reach, declarations, &index)
    }

    fn from_declarations(
        reach: &RealmReach,
        declarations: Vec<RealmDeclaration>,
        index: &perry_hir::realm_reach_index::RealmIndex,
    ) -> Result<Self, String> {
        let mut all = BTreeMap::new();
        for declaration in declarations {
            let name = declaration.name.clone();
            if all.insert(name.clone(), declaration).is_some() {
                return Err(format!("duplicate realm declaration {name}"));
            }
        }
        let executing = reach.executing(index);
        let mut global = all
            .get("%Global%")
            .ok_or("realm has no global cell")?
            .clone();
        select_properties(&mut global, reach, index, &executing)?;

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
            // The global cell's own layout is in every R (RealmReach::close).
            if name == "%Global%" || (!reach.is_full() && !reach.names.contains(name)) {
                continue;
            }
            if !reach.installed_in_exec(index, name, &executing)
                || declaration.birth_domain == perry_hir::realm_manifest::RealmBirthDomain::Instance
            {
                // Instance body templates have reach, but their identity is born
                // only with the owner allocation, never in the realm image.
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
                let mut selected = declaration.clone();
                if !index
                    .last(name)
                    .is_some_and(|row| row.runtime_layout_holder())
                {
                    select_properties(&mut selected, reach, index, &executing)?;
                }
                result.cells.insert(name.clone(), selected);
            }
        }
        for declaration in result
            .cells
            .values_mut()
            .chain(result.native_namespaces.values_mut())
        {
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
        result.refresh_atoms()?;
        Ok(result)
    }

    pub(crate) fn refresh_atoms(&mut self) -> Result<(), String> {
        self.atoms.clear();
        self.key_atoms.clear();
        self.symbols.clear();
        for declaration in self.cells.values().chain(self.native_namespaces.values()) {
            if declaration
                .header
                .as_ref()
                .is_some_and(|header| header.gc_type == 4)
                && crate::codegen::realm_image::properties::function_has_prototype_state(
                    declaration,
                )?
            {
                self.atoms.insert("p".into());
                self.key_atoms.insert("p".into());
            }
            for property in &declaration.properties {
                if property.symbol {
                    self.symbols.insert(property.key.clone());
                } else {
                    self.atoms.insert(property.key.clone());
                    self.key_atoms.insert(property.key.clone());
                }
                if let Some(value) = &property.value {
                    collect_value(value, &mut self.atoms, &mut self.symbols);
                }
            }
            for value in declaration.captures.iter().chain(
                declaration
                    .storage
                    .iter()
                    .flat_map(|storage| storage.values()),
            ) {
                collect_value(value, &mut self.atoms, &mut self.symbols);
            }
        }
        Ok(())
    }
}

/// Native ownership follows the source birth's own values and captures, never
/// [[Prototype]] or operation births. Other declared materialize roots own
/// their graph independently. This classification stages native graphs until
/// their namespace membership protocol can back the same materialize point.
fn native_namespace_birth(row: &RealmDeclaration) -> bool {
    row.native_namespace.is_some()
        || (row.kind == perry_hir::realm_manifest::RealmKind::Namespace && row.module.is_some())
}

pub(crate) fn native_owned_cells(
    all: &BTreeMap<String, RealmDeclaration>,
) -> Result<BTreeSet<String>, String> {
    let mut owned = BTreeSet::new();
    let roots = all
        .values()
        .filter(|row| native_namespace_birth(row) || row.native_birth_owner.is_some())
        .map(|row| row.name.clone())
        .collect::<BTreeSet<_>>();
    for root in &roots {
        if let Some(owner) = &all[root].native_birth_owner {
            if !all.get(owner).is_some_and(native_namespace_birth) {
                return Err(format!(
                    "native birth {root} has no namespace owner {owner}"
                ));
            }
        }
        let mut todo = vec![root.clone()];
        let mut seen = BTreeSet::new();
        while let Some(name) = todo.pop() {
            if !seen.insert(name.clone()) {
                continue;
            }
            let row = all
                .get(&name)
                .ok_or_else(|| format!("native birth references absent cell {name}"))?;
            if name != *root
                && !roots.contains(&name)
                && (row.constructor.is_some() || row.materialize_family.is_some())
            {
                continue;
            }
            owned.insert(name);
            for property in &row.properties {
                todo.extend(property.get.iter().chain(&property.set).cloned());
                if let Some(RealmValue::Reference(target) | RealmValue::RawReference(target)) =
                    &property.value
                {
                    todo.push(target.clone());
                }
            }
            for value in row
                .captures
                .iter()
                .chain(row.storage.iter().flat_map(|storage| storage.values()))
            {
                if let RealmValue::Reference(target) | RealmValue::RawReference(target) = value {
                    todo.push(target.clone());
                }
            }
        }
    }
    Ok(owned)
}

/// Add physical realm rows after the close contains the actual lowered EXEC
/// set. Every module receives this same final ordinal plan.
pub fn image_ordinals(
    preliminary: &crate::layout_ordinals::ProgramLayoutOrdinals,
    reach: &RealmReach,
    target: Option<&str>,
) -> Result<crate::layout_ordinals::ProgramLayoutOrdinals, String> {
    use crate::layout_ordinals::{LayoutRole, OrdinalInput, ProgramLayoutOrdinals};
    let host;
    let target = if let Some(target) = target {
        target
    } else {
        host = inkwell::targets::TargetMachine::get_default_triple()
            .as_str()
            .to_string_lossy()
            .into_owned();
        &host
    };
    let image = crate::codegen::realm_image::plan::ImagePlan::new(reach, target)?;
    if image.realm.cells.is_empty() {
        if preliminary.realm_image {
            return Err("preliminary plan already contains a realm image".into());
        }
        return Ok(preliminary.clone());
    }
    let input = |entry: &crate::layout_ordinals::OrdinalEntry| OrdinalInput {
        key: entry.key.clone(),
        requested_shape_id: entry.requested_shape_id,
        band: entry.band,
        proto_kind: entry.proto_kind,
        role: entry.role,
    };
    if preliminary
        .entries
        .iter()
        .any(|entry| entry.role == LayoutRole::LazyImage)
    {
        return Err("preliminary plan already contains lazy realm rows".into());
    }
    let native = preliminary
        .entries
        .iter()
        .filter(|entry| entry.role == LayoutRole::Native)
        .map(input);
    let program = preliminary
        .entries
        .iter()
        .filter(|entry| entry.role != LayoutRole::Native)
        .map(input)
        .chain(image.rows.inputs());
    let mut result = ProgramLayoutOrdinals::assign(native, program)?;
    result.realm_image = true;
    result.prototype_serial_end = result.prototype_serial_end.max(image.metadata.serial_end);
    Ok(result)
}

/// Every ordinary cell, including the global, uses the logical member rule.
/// Fixed runtime layouts and native namespaces keep their physical slot order.
fn select_properties(
    declaration: &mut RealmDeclaration,
    reach: &RealmReach,
    index: &perry_hir::realm_reach_index::RealmIndex,
    executing: &BTreeSet<String>,
) -> Result<(), String> {
    if let Some(header) = &declaration.header {
        if perry_abi::gc_type_is_object!(header.gc_type) {
            crate::codegen::realm_image::header::object_word(declaration, header)?;
        }
    }
    let holder = index
        .last(&declaration.name)
        .expect("selected realm holder is indexed");
    declaration.properties.retain_mut(|property| {
        let enumerable = property.attributes & 2 != 0;
        property.get = property.get.take().filter(|name| {
            reach.keeps_member_in_exec(index, holder, &property.key, name, enumerable, executing)
        });
        property.set = property.set.take().filter(|name| {
            reach.keeps_member_in_exec(index, holder, &property.key, name, enumerable, executing)
        });
        let value = match &property.value {
            Some(RealmValue::Reference(name) | RealmValue::RawReference(name)) => reach
                .keeps_member_in_exec(index, holder, &property.key, name, enumerable, executing),
            Some(_) => true,
            None => false,
        };
        value || property.get.is_some() || property.set.is_some()
    });
    let strings: BTreeSet<_> = declaration
        .properties
        .iter()
        .filter(|p| !p.symbol)
        .map(|p| p.key.clone())
        .collect();
    let symbols: BTreeSet<_> = declaration
        .properties
        .iter()
        .filter(|p| p.symbol)
        .map(|p| p.key.clone())
        .collect();
    declaration
        .own_key_order
        .retain(|key| strings.contains(key));
    declaration
        .own_symbol_order
        .retain(|key| symbols.contains(key));
    let projected_word = declaration
        .header
        .as_ref()
        .filter(|header| perry_abi::gc_type_is_object!(header.gc_type))
        .map(|header| crate::codegen::realm_image::header::object_birth_word(declaration, header))
        .transpose()?;
    if let (Some(word), Some(header)) = (projected_word, &mut declaration.header) {
        header.reserved = word;
    }
    Ok(())
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
        (!property.attributes & 7) | (property.attributes & 64)
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
        perry_hir::realm_reach::scan(
            &module,
            &perry_hir::realm_reach_index::RealmIndex::new(crate::realm_manifest::REALM),
        )
    }

    #[test]
    fn reflection_fixture_scans_own_the_values_node_reads() {
        let index = perry_hir::realm_reach_index::RealmIndex::new(crate::realm_manifest::REALM);
        for (source, names) in [
            ("const B: any = Buffer; console.log(typeof B.alloc, typeof B.prototype.readUInt8);", &["Buffer.alloc", "Buffer.prototype.readUInt8"][..]),
            ("console.log(Object.getOwnPropertyNames(Buffer.prototype).length, Object.keys(Error).length);", &["Buffer.prototype.readUInt8", "Error.captureStackTrace"][..]),
            ("for (const k in Promise) console.log(k); console.log(JSON.stringify(Object.getOwnPropertyDescriptors(Set)));", &["Promise.resolve", "Set.prototype.add"][..]),
            ("\"x\".constructor.fromCharCode(65);", &["String.fromCharCode"][..]),
        ] {
            let module = perry_parser::parse_typescript(source, "realm.ts").unwrap();
            let reach = perry_hir::realm_reach::scan(&module, &index);
            assert!(!reach.is_full(), "fixture unexpectedly selected top: {source}");
            let mut reclosed = reach.clone();
            reclosed.close(&index);
            assert_eq!(reclosed, reach, "reclose changed {source}");
            for name in names {
                assert!(reach.whole.contains(*name), "{source}: {name}: {reach:?}");
            }
        }
        let formatted = reach("function show(value) { console.log(value); }");
        assert_eq!(
            formatted.keyless_read,
            Some(perry_hir::realm_reach::KeylessRead::All)
        );
        assert!(index
            .first("%ArrayIteratorPrototype%")
            .unwrap()
            .runtime_spelled_key("next"));
    }

    #[test]
    fn realm_hello0_image_zero_preserves_the_closed_logical_reach() {
        let r = reach("console.log('hi');");
        let before = r.clone();
        let image =
            crate::codegen::realm_image::plan::ImagePlan::new(&r, "x86_64-unknown-linux-gnu")
                .unwrap();
        let plan = image.realm;
        assert!(plan.cells.is_empty());
        assert!(plan.providers.is_empty());
        assert!(plan.native_namespaces.is_empty());
        assert!(plan.atoms.is_empty());
        assert!(plan.key_atoms.is_empty());
        assert!(plan.symbols.is_empty());
        assert_eq!(r, before);
        let preliminary = crate::layout_ordinals::ProgramLayoutOrdinals::assign(
            crate::layout_ordinals::native_inputs(),
            Vec::new(),
        )
        .unwrap();
        let final_plan =
            image_ordinals(&preliminary, &r, Some("x86_64-unknown-linux-gnu")).unwrap();
        assert_eq!(final_plan, preliminary);
        assert!(!final_plan.realm_image);
        assert_eq!(final_plan.lazy_count, 0);
        assert!(final_plan
            .table_span_initializer()
            .ends_with("ptr null, ptr null, ptr null"));
    }

    #[test]
    fn hello_logical_projection_keeps_its_filtered_global() {
        let r = reach("console.log('hi');");
        let index = perry_hir::realm_reach_index::RealmIndex::new(crate::realm_manifest::REALM);
        let declarations = index
            .rows()
            .map(|row| serde_json::from_value(row.facts().clone()).unwrap())
            .collect();
        let plan = RealmPlan::from_declarations(&r, declarations, &index).unwrap();
        for ctor in ["Object", "Function", "Array"] {
            assert!(!plan.cells.contains_key(ctor), "{ctor}");
        }
        for name in plan.cells.keys().filter(|n| *n != "%Global%") {
            assert!(r.names.contains(name), "{name}");
        }
        assert!(plan.cells["%Global%"]
            .properties
            .iter()
            .any(|p| p.key == "console"));
        assert!(!plan.cells["%Global%"]
            .properties
            .iter()
            .any(|p| p.key == "Array"));
        assert!(!plan.cells["%Global%"]
            .properties
            .iter()
            .any(|p| matches!(p.key.as_str(), "global" | "globalThis")));
    }

    #[test]
    fn named_array_keeps_its_constructor_prototype_and_symbols() {
        let reach = reach("Array; console.log('hi');");
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
        let features = RealmPlan::from_reach(&reach(
            "console.log('hi'); Object.getOwnPropertyDescriptor; RegExp.prototype.exec;",
        ))
        .unwrap();
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
                features
                    .runtime_layouts
                    .contains_key(&format!("%Layout:{layout}%")),
                "{layout}"
            );
        }
        // The descriptor and RegExp member bodies own these layouts.
        for layout in ["REGEX_EXEC_NAMED", "REGEX_EXEC_INDICES_NAMED"] {
            assert!(features
                .runtime_layouts
                .contains_key(&format!("%Layout:{layout}%")));
        }
        let arithmetic = RealmPlan::from_reach(&reach("1 + 2;")).unwrap();
        assert!(!arithmetic
            .runtime_layouts
            .contains_key("%Layout:GLOBAL_OBJECT%"));
        let core = RealmPlan::from_reach(&reach("globalThis.Object;")).unwrap();
        // Global materialization executes its allocation owner.
        assert!(core.runtime_layouts.contains_key("%Layout:GLOBAL_OBJECT%"));
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
                !features
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
    fn every_global_key_reached_alone_closes_its_plan() {
        // A global key's value (a reference or an accessor's getter) must be
        // born by the key's own R node: a program that reads only that key
        // gets a closed plan. `navigator` became an accessor whose getter no
        // node birthed.
        let index = perry_hir::realm_reach_index::RealmIndex::new(crate::realm_manifest::REALM);
        let global: RealmDeclaration = index
            .first("%Global%")
            .map(|entry| serde_json::from_value(entry.facts().clone()).unwrap())
            .unwrap();
        let keys: Vec<_> = global
            .properties
            .iter()
            .filter(|property| !property.symbol)
            .map(|property| property.key.clone())
            .collect();
        assert!(keys.iter().any(|key| key == "navigator"));
        let mut failures = Vec::new();
        for key in &keys {
            let reach = reach(&format!("console.log(typeof globalThis.{key});"));
            if reach.is_full() {
                continue;
            }
            if let Err(error) = RealmPlan::from_reach(&reach) {
                failures.push(format!("{key}: {error}"));
            }
        }
        assert!(failures.is_empty(), "{failures:#?}");
    }

    #[test]
    fn an_absent_birth_is_an_error_not_implicit_reachability() {
        let mut reach = reach("Array; console.log('hi');");
        assert!(reach.names.remove("Array.prototype"));
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

    #[test]
    fn worker_notification_owns_the_canonical_process_without_selecting_full_realm() {
        let worker = crate::realm_manifest::REALM
            .iter()
            .find(|entry| entry.name == "node:worker_threads.Worker")
            .unwrap();
        for birth in [
            "process",
            "%Layout:RECORD_F426_0%",
            "%Layout:RECORD_Q436_0%",
        ] {
            assert!(
                worker.births.contains(&birth),
                "missing direct Worker birth {birth}"
            );
        }
        let reach =
            reach("import { Worker } from 'node:worker_threads'; new Worker('', { eval: true });");
        assert!(!reach.is_full());
        assert!(reach.names.contains("process"));
        let plan = RealmPlan::from_reach(&reach).unwrap();
        assert!(plan.cells.contains_key("process"));
        for name in ["RECORD_Q435_0", "RECORD_Q439_0", "RECORD_Q440_0"] {
            assert!(plan
                .runtime_layouts
                .contains_key(&format!("%Layout:{name}%")));
        }
        for name in ["RECORD_F427_0", "RECORD_F429_0"] {
            assert!(!plan
                .runtime_layouts
                .contains_key(&format!("%Layout:{name}%")));
        }
    }

    #[test]
    fn worker_diagnostic_capabilities_own_only_their_shared_result_layouts() {
        let worker_reach =
            reach("import { Worker } from 'node:worker_threads'; new Worker('', { eval: true });");
        assert!(!worker_reach.is_full());
        let worker = RealmPlan::from_reach(&worker_reach).unwrap();
        for record in ["RECORD_F426_0", "RECORD_Q436_0"] {
            assert!(
                worker
                    .runtime_layouts
                    .contains_key(&format!("%Layout:{record}%")),
                "missing Worker result {record}"
            );
        }
        for record in ["RECORD_F427_0", "RECORD_F429_0"] {
            assert!(
                !worker
                    .runtime_layouts
                    .contains_key(&format!("%Layout:{record}%")),
                "unrelated result {record}"
            );
        }
        let perf = RealmPlan::from_reach(&reach(
            "import { performance } from 'node:perf_hooks'; performance.eventLoopUtilization();",
        ))
        .unwrap();
        assert!(perf.runtime_layouts.contains_key("%Layout:RECORD_F426_0%"));
        let process = RealmPlan::from_reach(&reach("process.cpuUsage();")).unwrap();
        assert!(process
            .runtime_layouts
            .contains_key("%Layout:RECORD_Q436_0%"));
    }
}

#[cfg(test)]
#[path = "realm_plan_selection_tests.rs"]
mod selection_tests;

#[cfg(test)]
mod native_ownership_tests {
    use super::*;

    #[test]
    fn realm_native_owner_is_source_graph_not_name_parent_or_body_birth() {
        let declarations: Vec<RealmDeclaration> = serde_json::from_str(r#"[
          {"name":"%Global%","kind":"namespace"},
          {"name":"Host","kind":"constructor","constructor":{"identity":7,"prototype_callable":false},
           "native_birth_owner":"Receiver","prototype":"UnrelatedCtor","births":["BodyBirth"],
           "properties":[{"key":"prototype","symbol":false,"attributes":0,"value":{"kind":"reference","value":"HostProto"}},
                         {"key":"f","symbol":false,"attributes":7,"value":{"kind":"reference","value":"Member"}}]},
          {"name":"Receiver","kind":"exotic","native_namespace":"test.host"},
          {"name":"HostProto","kind":"prototype","prototype":"ParentProto"},
          {"name":"Member","kind":"method","captures":[{"kind":"reference","value":"Receiver"}]},
          {"name":"UnrelatedCtor","kind":"constructor","constructor":{"identity":8,"prototype_callable":false}},
          {"name":"ParentProto","kind":"prototype"},
          {"name":"BodyBirth","kind":"prototype"},
          {"name":"PlainModule","kind":"namespace","module":"test.plain","properties":[
            {"key":"value","symbol":false,"attributes":7,"value":{"kind":"reference","value":"PlainValue"}}]},
          {"name":"PlainValue","kind":"method"}
        ]"#).unwrap();
        let all = declarations
            .into_iter()
            .map(|row| (row.name.clone(), row))
            .collect();
        let owned = native_owned_cells(&all).unwrap();
        assert_eq!(
            owned,
            BTreeSet::from([
                "Host".into(),
                "HostProto".into(),
                "Member".into(),
                "Receiver".into(),
                "PlainModule".into(),
                "PlainValue".into()
            ])
        );
        let mut bad = all.clone();
        bad.get_mut("Host").unwrap().native_birth_owner = Some("ParentProto".into());
        assert!(native_owned_cells(&bad)
            .unwrap_err()
            .contains("no namespace owner"));
    }
    #[test]
    fn realm_native_instances_declare_their_prototype_birth_owner() {
        let index = perry_hir::realm_reach_index::RealmIndex::new(crate::realm_manifest::REALM);
        let all = index
            .rows()
            .map(|entry| {
                let cell: RealmDeclaration = serde_json::from_value(entry.facts().clone()).unwrap();
                (cell.name.clone(), cell)
            })
            .collect();
        let owned = native_owned_cells(&all).unwrap();
        for name in [
            "node:http.globalAgent.<prototype>",
            "node:https.globalAgent.<prototype>",
            "node:worker_threads.locks.<prototype>",
            "Buffer",
            "%BufferSpeciesConstructor%",
            "Buffer[\"@@species\"].<get>",
        ] {
            assert!(owned.contains(name), "{name}");
        }
        assert!(!owned.contains("Object.prototype"));
    }

    #[test]
    fn realm_instance_birth_keeps_body_reach_without_an_image_identity() {
        let index = perry_hir::realm_reach_index::RealmIndex::new(crate::realm_manifest::REALM);
        let ast =
            perry_parser::parse_typescript("console.log(globalThis.Intl.Collator);", "instance.ts")
                .unwrap();
        let mut reach = perry_hir::realm_reach::scan(&ast, &index);
        reach.own("%CollatorCompare%");
        reach.close(&index);
        let row = index.last("%CollatorCompare%").unwrap();
        let declaration: RealmDeclaration = serde_json::from_value(row.facts().clone()).unwrap();
        assert_eq!(
            declaration.birth_domain,
            perry_hir::realm_manifest::RealmBirthDomain::Instance
        );
        assert!(reach.names.contains(row.name()));
        assert!(reach.executes(&index, row));
        let plan = RealmPlan::from_reach(&reach).unwrap();
        assert!(!plan.cells.contains_key(row.name()));
        assert!(!plan.providers.contains_key(row.name()));
    }
    #[test]
    fn realm_nullish_read_exec_keeps_the_caught_type_error_constructor() {
        let index = perry_hir::realm_reach_index::RealmIndex::new(crate::realm_manifest::REALM);
        let mut reach = RealmReach::default();
        reach.exec("js_object_get_field_value");
        reach.receiver_keys.insert("constructor".into());
        reach.receiver_keys.insert("name".into());
        reach.close(&index);
        assert!(reach.executing(&index).contains("js_throw_type_error_property_access"));
        assert!(reach.names.contains("TypeError.prototype"));
        assert!(reach.names.contains("TypeError"));
        let plan = RealmPlan::from_reach(&reach).unwrap();
        assert!(plan.cells["TypeError.prototype"].properties.iter().any(|p| p.key == "constructor"));
    }

    #[test]
    fn realm_match_all_body_keeps_its_allocated_iterator_prototype() {
        let index = perry_hir::realm_reach_index::RealmIndex::new(crate::realm_manifest::REALM);
        for callee in ["RegExp.prototype[\"@@matchAll\"]", "String.prototype.matchAll"] {
            let mut reach = RealmReach::default();
            reach.own(callee);
            reach.exec("js_runtime_install_regex_engine");
            reach.close(&index);
            assert!(reach.names.contains("%RegExpStringIteratorPrototype%"), "{callee}");
            assert!(RealmPlan::from_reach(&reach).unwrap().cells.contains_key("%RegExpStringIteratorPrototype%"));
        }
    }

    #[test]
    fn realm_hook_targets_follow_installer_exec_in_logical_and_physical_selection() {
        let index = perry_hir::realm_reach_index::RealmIndex::new(crate::realm_manifest::REALM);
        for (holder, target, installer, key) in [
            (
                "RegExp.prototype",
                "RegExp.prototype.exec",
                "js_runtime_install_regex_engine",
                "exec",
            ),
            (
                "ReadableStream.prototype",
                "ReadableStream.prototype.getReader",
                "js_stdlib_install_bundled_streams",
                "getReader",
            ),
        ] {
            let mut reach = RealmReach::default();
            reach.own(holder);
            reach.receiver_keys.insert(key.to_owned());
            reach.close(&index);
            assert!(
                !reach.names.contains(target),
                "inactive hook target {target}"
            );
            assert!(!reach.executing(&index).contains(
                index
                    .last(target)
                    .unwrap()
                    .body()
                    .unwrap()
                    .rsplit("::")
                    .next()
                    .unwrap()
            ));
            let plan = RealmPlan::from_reach(&reach).unwrap();
            assert!(!plan.cells[holder].properties.iter().any(|p| p.key == key));
            reach.exec(installer);
            reach.close(&index);
            assert!(reach.names.contains(target), "installed target {target}");
            let plan = RealmPlan::from_reach(&reach).unwrap();
            assert!(plan.cells[holder].properties.iter().any(|p| p.key == key));
            assert!(plan.cells.contains_key(target));
        }
    }
    #[test]
    fn realm_full_reach_does_not_install_optional_hook_members() {
        let index = perry_hir::realm_reach_index::RealmIndex::new(crate::realm_manifest::REALM);
        for (holder, target, installer, key) in [
            (
                "RegExp.prototype",
                "RegExp.prototype.exec",
                "js_runtime_install_regex_engine",
                "exec",
            ),
            (
                "ReadableStream.prototype",
                "ReadableStream.prototype.getReader",
                "js_stdlib_install_bundled_streams",
                "getReader",
            ),
        ] {
            let ast = perry_parser::parse_typescript("with ({}) {}", "full.cjs").unwrap();
            let mut reach = perry_hir::realm_reach::scan(&ast, &index);
            assert!(reach.is_full());
            assert!(reach.names.contains(target), "full R retains body reach");
            assert!(
                reach.executes(&index, index.last(target).unwrap()),
                "R5 unchanged"
            );
            let plan = RealmPlan::from_reach(&reach).unwrap();
            assert!(!plan.cells[holder].properties.iter().any(|p| p.key == key));
            assert!(!plan.cells.contains_key(target));
            reach.exec(installer);
            reach.close(&index);
            let plan = RealmPlan::from_reach(&reach).unwrap();
            assert!(plan.cells[holder].properties.iter().any(|p| p.key == key));
            assert!(plan.cells.contains_key(target));
        }
    }
    #[test]
    fn realm_full_plan_excludes_all_instance_birth_templates() {
        let index = perry_hir::realm_reach_index::RealmIndex::new(crate::realm_manifest::REALM);
        let ast = perry_parser::parse_typescript("with ({}) {}", "all.cjs").unwrap();
        let reach = perry_hir::realm_reach::scan(&ast, &index);
        assert!(reach.is_full());
        let image =
            crate::codegen::realm_image::plan::ImagePlan::new(&reach, "x86_64-unknown-linux-gnu")
                .unwrap();
        let mut instances = 0;
        for row in index.rows() {
            let cell: RealmDeclaration = serde_json::from_value(row.facts().clone()).unwrap();
            if cell.birth_domain == perry_hir::realm_manifest::RealmBirthDomain::Instance {
                instances += 1;
                assert!(reach.names.contains(row.name()));
                assert!(
                    !image.realm.cells.contains_key(row.name()),
                    "instance image cell {}",
                    row.name()
                );
                assert!(!image.realm.providers.contains_key(row.name()));
            }
        }
        assert!(instances >= 15, "all declared per-owner births covered");
    }
}
