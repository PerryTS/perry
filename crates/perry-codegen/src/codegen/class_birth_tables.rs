//! Birth facts of every class keys global: the inline-`new` header images,
//! the birth representations, the anonymous-shape key adds and the module's
//! layout families.

use std::collections::HashMap;

use perry_hir::Module as HirModule;

use super::CompileOptions;
use crate::collectors::ModuleDispatchFacts;

/// The class birth tables `lower_module` builds, each field named for the
/// local it binds.
pub(super) struct ClassBirthTables {
    pub(super) anon_key_adds: HashMap<String, std::collections::BTreeSet<String>>,
    pub(super) class_header_image_inits: std::collections::HashMap<String, (u32, u64, u32)>,
    pub(super) class_birth_reps_map: std::collections::HashMap<String, u64>,
}

#[allow(clippy::too_many_arguments)]
pub(super) fn build_class_birth_tables(
    hir: &HirModule,
    opts: &CompileOptions,
    class_table: &HashMap<String, &perry_hir::Class>,
    class_ids: &HashMap<String, u32>,
    class_keys_globals_map: &std::collections::HashMap<String, String>,
    class_field_counts_map: &std::collections::HashMap<String, u32>,
    class_init_chains_map: &std::collections::HashMap<
        String,
        Vec<(String, Vec<perry_hir::ClassField>)>,
    >,
    imported_class_stubs: &[perry_hir::Class],
    module_dispatch_facts: &ModuleDispatchFacts,
    module_prefix: &String,
    triple: &String,
) -> ClassBirthTables {
    // #8122: the inline-`new` header-image table. For every class with a keys
    // global (local or imported stub), derive the packed GcHeader word the
    // inline allocator will store — with `target_layout::inline_alloc_gc_packed`,
    // the SAME function the allocation site uses — from the same module-level
    // maps the site consults through its `FnCtx`. Module init composes
    // `[gc_packed | class_id | ShapeId << 32]` into the class's image global;
    // the site loads that instead of composing per site (or per call in a
    // recursive allocator, where the per-function compose measured +0.6% on
    // `tree`).
    //
    // Module init writes one image per KEYS global (aliases share one), so the
    // init table is keyed by keys global and the site table is DERIVED from it:
    // a class only gets a site entry if module init will actually compose its
    // image. A site entry with no init store would hand every instance a
    // zeroed header, so that direction of the dependency is load-bearing.
    let imported_stub_names: std::collections::HashSet<&str> = imported_class_stubs
        .iter()
        .map(|class| class.name.as_str())
        .collect();
    let mut anon_key_adds = crate::collectors::anon_key_adds::anon_receiver_added_keys(
        hir,
        &class_table,
        &module_dispatch_facts,
    );
    // A keys global owns ONE birth ShapeId. Equal-key classes and aliases
    // must reserve the maximum requested capacity before either consumer
    // derives its image. Deterministic sets also make equal-size ties stable.
    let mut adds_by_keys: std::collections::HashMap<String, std::collections::BTreeSet<String>> =
        std::collections::HashMap::new();
    for (name, keys) in &anon_key_adds {
        if let Some(global) = class_keys_globals_map.get(name) {
            let existing = adds_by_keys.entry(global.clone()).or_default();
            if (keys.len(), keys) > (existing.len(), &*existing) {
                *existing = keys.clone();
            }
        }
    }
    for (name, global) in class_keys_globals_map {
        if let Some(keys) = adds_by_keys.get(global) {
            anon_key_adds.insert(name.clone(), keys.clone());
        }
    }
    let class_header_image_inits: std::collections::HashMap<String, (u32, u64, u32)> = {
        let mut inits: std::collections::HashMap<String, (u32, u64, u32)> =
            std::collections::HashMap::new();
        for (class_name, keys_global) in class_keys_globals_map {
            let Some(&key_count) = class_field_counts_map.get(class_name) else {
                continue;
            };
            // In-object slack for constructor key-adds (see
            // `lower_call::new_alloc::constructor_added_key_count`): such a
            // class is born with a live bound of keys + slack, minted beside
            // this image by the string pool (`birth_live`, 0 = the key count).
            // The private fields' entries are key-adds of construction too
            // (#11791): reserve their inline slots the same way.
            let slack = if imported_stub_names.contains(class_name.as_str()) {
                0
            } else {
                class_table.get(class_name).map_or(0, |class| {
                    let lookup = |name: &str| class_table.get(name).copied();
                    crate::lower_call::new_alloc::birth_slack_in(class, &lookup, &anon_key_adds)
                })
            };
            let birth_live = if slack > 0 { key_count + slack } else { 0 };
            let field_count = if slack > 0 {
                key_count + slack
            } else {
                key_count
            };
            let Some(&class_id) = class_ids.get(class_name) else {
                continue;
            };
            let gc_packed = crate::target_layout::inline_alloc_gc_packed(
                &triple,
                crate::target_layout::class_instance_form(class_name),
                field_count,
            );
            match inits.get(keys_global) {
                // Two names (an alias) sharing one keys global must agree on
                // the word module init writes; if they do not, neither may use
                // the image — drop the keys global from the table.
                Some(&(existing_id, existing_gc, existing_live)) => {
                    if existing_id != class_id
                        || existing_gc != gc_packed
                        || existing_live != birth_live
                    {
                        inits.insert(keys_global.clone(), (u32::MAX, 0, 0));
                    }
                }
                None => {
                    inits.insert(keys_global.clone(), (class_id, gc_packed, birth_live));
                }
            }
        }
        inits.retain(|_, (class_id, _, _)| *class_id != u32::MAX);
        inits
    };
    // Charter step 5, T1: each class's birth rep (`typed_shape::class_birth_rep_in`),
    // keyed like the header-image inits by keys global. Two names sharing a
    // keys global that disagree get `Any` (0): every consumer reads this map,
    // so they still agree with each other.
    let class_birth_reps_map: std::collections::HashMap<String, u64> = {
        let mut reps: std::collections::HashMap<String, u64> = std::collections::HashMap::new();
        for (class_name, keys_global) in class_keys_globals_map {
            let rep = crate::typed_shape::class_birth_rep_in(
                &class_table,
                &class_keys_globals_map,
                &class_init_chains_map,
                imported_stub_names.contains(class_name.as_str()),
                class_name,
            );
            // I32 at rest (PR C): a class born with its keys carries its I32
            // lanes in its birth rep (`expr::rest_fields`); a class born
            // keyless gets them after its field definitions.
            let rep = match class_table.get(class_name) {
                Some(class)
                    if !imported_stub_names.contains(class_name.as_str())
                        && crate::lower_call::new_alloc::keys_defined_at_birth(class) =>
                {
                    let lanes = crate::expr::rest_fields::class_lanes(
                        &opts.program_facts.elision,
                        &opts.program_layouts,
                        &class_table,
                        &class_ids,
                        class_name,
                    );
                    crate::expr::rest_fields::with_lanes(rep, lanes)
                }
                _ => rep,
            };
            let entry = reps.entry(keys_global.clone()).or_insert(rep);
            if *entry != rep {
                *entry = 0;
            }
        }
        // I32 at rest (PR C): a class born with its keys keeps every lane the
        // facts give it; other modules read those lanes from the facts.
        for (class_name, keys_global) in class_keys_globals_map {
            let Some(class) = class_table.get(class_name) else {
                continue;
            };
            if imported_stub_names.contains(class_name.as_str())
                || !crate::lower_call::new_alloc::keys_defined_at_birth(class)
            {
                continue;
            }
            let lanes = crate::expr::rest_fields::class_lanes(
                &opts.program_facts.elision,
                &opts.program_layouts,
                &class_table,
                &class_ids,
                class_name,
            );
            let rep = reps.get(keys_global).copied().unwrap_or(0);
            assert_eq!(
                crate::expr::rest_fields::lanes_of_rep(rep) & lanes,
                lanes,
                "I32 at rest: class {class_name} lost its I32 lanes in its birth rep"
            );
        }
        reps
    };
    // Object model step 1: literal families born with the slot of their first
    // added key (the capacity `anon_key_adds` reserves becomes a layout
    // slot), and the carried descriptor of the state holding it.
    {
        let mut adds = std::collections::HashMap::new();
        let mut layouts: Vec<crate::layout_desc::FamilyLayout> = Vec::new();
        if crate::layout_desc::layouts_enabled(&opts.output_type, &triple) {
            let mut by_global: std::collections::HashMap<String, String> =
                std::collections::HashMap::new();
            let mut names: Vec<&String> = anon_key_adds.keys().collect();
            names.sort();
            for class_name in names {
                let added = &anon_key_adds[class_name];
                let (Some(first), Some(class), Some(keys_global)) = (
                    added.iter().next(),
                    class_table.get(class_name),
                    class_keys_globals_map.get(class_name),
                ) else {
                    continue;
                };
                if !class_name.starts_with("__AnonShape_")
                    || imported_stub_names.contains(class_name.as_str())
                    || class.extends_name.is_some()
                {
                    continue;
                }
                let Some(&key_count) = class_field_counts_map.get(class_name) else {
                    continue;
                };
                let Some(&(_, _, birth_live)) = class_header_image_inits.get(keys_global) else {
                    continue;
                };
                let rep = class_birth_reps_map.get(keys_global).copied().unwrap_or(0);
                let base: Vec<String> = class.fields.iter().map(|f| f.name.clone()).collect();
                // Every lane a plain Any or F64 lane: a ConstFn or deprecated
                // lane is a different family state.
                let plain_rep = (0..32).all(|lane| matches!((rep >> (2 * lane)) & 3, 0 | 1));
                let named_elsewhere = base.iter().any(|k| k == first)
                    || class.methods.iter().any(|m| &m.name == first)
                    || class
                        .getters
                        .iter()
                        .chain(&class.setters)
                        .any(|(k, _)| k == first);
                if base.len() as u32 != key_count
                    || birth_live <= key_count
                    || !plain_rep
                    || named_elsewhere
                    || first == "__proto__"
                    || !crate::layout_desc::key_is_atom(first)
                    || !base.iter().all(|k| crate::layout_desc::key_is_atom(k))
                {
                    continue;
                }
                let suffix = keys_global
                    .strip_prefix("perry_class_keys_")
                    .unwrap_or(keys_global);
                let desc_global = format!("perry_layout_fam_{suffix}");
                if let std::collections::hash_map::Entry::Vacant(v) =
                    by_global.entry(keys_global.clone())
                {
                    v.insert(first.clone());
                    let mut keys = base.clone();
                    keys.push(first.clone());
                    layouts.push(crate::layout_desc::FamilyLayout {
                        desc_global: desc_global.clone(),
                        keys,
                        slot_count: birth_live,
                        rep,
                        array_named: false,
                    });
                } else if by_global[keys_global] != *first {
                    continue;
                }
                adds.insert(
                    class_name.clone(),
                    crate::layout_desc::FamilyAdd {
                        key: first.clone(),
                        slot: key_count,
                        desc_global,
                        birth_keys: key_count,
                        birth_rep: rep,
                    },
                );
            }
        }
        let layouts_enabled = crate::layout_desc::layouts_enabled(&opts.output_type, &triple);
        if layouts_enabled {
            layouts.extend(crate::layout_desc::array_families::collect(
                hir,
                &module_prefix,
            ));
            crate::layout_desc::create_families::collect(hir, &module_prefix, &triple);
        } else {
            crate::layout_desc::create_families::clear();
        }
        crate::layout_desc::set_module_families(adds, layouts);
        if layouts_enabled {
            crate::layout_desc::set_array_layout_prefix(&module_prefix);
        }
    }
    ClassBirthTables {
        anon_key_adds,
        class_header_image_inits,
        class_birth_reps_map,
    }
}
