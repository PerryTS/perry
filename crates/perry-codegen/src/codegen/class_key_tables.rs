//! Per-class keys-array globals and the layout tables derived from each
//! class's source-prefix-disambiguated ancestor chain.

use std::collections::HashMap;

use perry_hir::Module as HirModule;

use super::{helpers, static_shape_ids};
use crate::module::LlModule;
use crate::strings::StringPool;
use crate::types::I64;

/// The class layout tables `lower_module` builds, each field named for the
/// local it binds.
pub(super) struct ClassKeyTables {
    pub(super) class_keys_init_data: Vec<(String, String, u32, Vec<u64>, Vec<u64>)>,
    pub(super) class_keys_globals_map: std::collections::HashMap<String, String>,
    pub(super) class_field_counts_map: std::collections::HashMap<String, u32>,
    pub(super) class_init_chains_map:
        std::collections::HashMap<String, Vec<(String, Vec<perry_hir::ClassField>)>>,
    pub(super) defined_class_keys_len: usize,
}

pub(super) fn build_class_key_tables(
    hir: &HirModule,
    module_prefix: &String,
    llmod: &mut LlModule,
    strings: &mut StringPool,
    imported_class_stubs: &[perry_hir::Class],
    imported_stub_prefixes: &[String],
    shadowed_parent_stubs: &[(String, String, Option<String>, Vec<perry_hir::ClassField>)],
) -> ClassKeyTables {
    // Per-class keys-array globals: each class gets a single internal
    // global `@perry_class_keys_<modprefix>__<class>` that holds the
    // shared keys_array pointer (built ONCE at module init via
    // js_build_class_keys_array). Every `new ClassName()` site then
    // emits a direct global load + inline allocator call, bypassing
    // the per-call SHAPE_CACHE lookup AND the runtime
    // js_object_alloc_class_with_keys function entirely on the hot
    // allocation path.
    //
    // Per-class init data:
    // (global_name, packed_keys_string, total_field_count, raw_f64_mask_words,
    // pointer_mask_words).
    // Used by emit_string_pool to emit the build-call sequence.
    let mut class_keys_init_data: Vec<(String, String, u32, Vec<u64>, Vec<u64>)> = Vec::new();
    let mut class_keys_globals_map: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    // Issue #26 / #321: the authoritative total inline-field count for each
    // class, as computed by the source-prefix-disambiguated chain walk that
    // builds the packed-keys global below. The `new ClassName()` site
    // (`lower_new`) recomputes a field count by walking `ctx.classes`
    // (a name-keyed map that can only hold ONE same-named parent stub),
    // which mis-sizes the allocation and stamps a wrong `field_count` in
    // the object header when same-named parents collide (effect's `Type`).
    // `lower_new` consults this map first so the allocated slot count and the
    // header `field_count` match the keys array length the global holds.
    let mut class_field_counts_map: std::collections::HashMap<String, u32> =
        std::collections::HashMap::new();
    // Issue #26 / #321: the authoritative, source-prefix-disambiguated
    // ancestor chain for each class, root → leaf, as `(class_name, fields)`.
    // `apply_field_initializers_recursive` (lower_new) otherwise walks the
    // chain via the name-keyed `ctx.classes`, which mis-resolves same-named
    // cross-module parents (effect's `Type`) and writes that wrong parent's
    // fields onto the instance as `undefined` — surfacing as spurious
    // enumerable keys. Consulting this chain makes constructor field-init
    // write exactly the layout the keys array describes.
    let mut class_init_chains_map: std::collections::HashMap<
        String,
        Vec<(String, Vec<perry_hir::ClassField>)>,
    > = std::collections::HashMap::new();

    // Issue #26 / #321: resolve a parent class name to its layout, disambiguating
    // same-named imported classes by source module. A class's `extends` clause
    // resolves in its OWN module's scope, so when several modules export a
    // same-named class (effect's `Type` in SchemaAST.ts vs ParseResult.ts), we
    // prefer the candidate whose source prefix matches the child's. Searches
    // `imported_class_stubs` first (the live stubs that also populate
    // `class_table`), then `shadowed_parent_stubs` (imports kept out of
    // `class_table` because a local class shadows the name — still valid
    // parents for OTHER imports). Returns `(fields, extends_name, source_prefix)`.
    // `child_prefix = None` (or no same-prefix hit) falls back to the first
    // by-name match — the legacy behavior.
    let resolve_parent = |parent_name: &str,
                          child_prefix: Option<&str>|
     -> Option<(Vec<perry_hir::ClassField>, Option<String>, String)> {
        // Same-prefix preference over the live stubs.
        if let Some(cp) = child_prefix {
            if let Some(i) = imported_class_stubs
                .iter()
                .enumerate()
                .position(|(i, cls)| cls.name == parent_name && imported_stub_prefixes[i] == cp)
            {
                let s = &imported_class_stubs[i];
                return Some((
                    s.fields.clone(),
                    s.extends_name.clone(),
                    imported_stub_prefixes[i].clone(),
                ));
            }
            // Same-prefix preference over the shadowed list.
            if let Some((_, p, ext, fields)) = shadowed_parent_stubs
                .iter()
                .find(|(n, p, _, _)| n == parent_name && p == cp)
            {
                return Some((fields.clone(), ext.clone(), p.clone()));
            }
        }
        // Fallback: first by-name match in the live stubs.
        if let Some(i) = imported_class_stubs
            .iter()
            .position(|cls| cls.name == parent_name)
        {
            let s = &imported_class_stubs[i];
            return Some((
                s.fields.clone(),
                s.extends_name.clone(),
                imported_stub_prefixes[i].clone(),
            ));
        }
        // Last resort: a shadowed import by name (still better than picking the
        // local class for a cross-module import's parent).
        shadowed_parent_stubs
            .iter()
            .find(|(n, _, _, _)| n == parent_name)
            .map(|(_, p, ext, fields)| (fields.clone(), ext.clone(), p.clone()))
    };

    // Source components use the injective encoder. Reserve a distinct global
    // for any repeated internal class name; the real-name keyed map carries
    // that selected global to allocation sites and capability harvesting.
    let mut used_class_keys_globals: std::collections::HashSet<String> =
        std::collections::HashSet::new();
    let mut keys_class_ids: HashMap<String, u32> = HashMap::new();
    for c in &hir.classes {
        let global_name = helpers::unique_class_keys_global(
            &module_prefix,
            &c.name,
            &mut used_class_keys_globals,
        );
        // #10399: the keys array, the ShapeId and the header image are all
        // composed by module init, so with a Worker in the program they are
        // per-thread — which is also what #10399 Paths 2 and 3 were patching
        // up at runtime (a foreign keys array / foreign ShapeId).
        llmod.add_internal_module_state_global(&global_name, I64, "0");
        // #8772: the immutable class ShapeId is a producer-authored
        // whole-program capability. Generic callers in other modules load it
        // to guard reverse-discovered direct method arms. The keys array stays
        // private; only the opaque process-unique identity is exported.
        // #8122: the inline-`new` header image, composed at module init
        // (`string_pool.rs`) for the classes `class_header_images` admits.

        // Build the packed-keys string. Format: each field name
        // followed by `\0`. Parent classes contribute their fields
        // first (walking from deepest ancestor down) so the slot
        // order matches `class_field_global_index`'s assumption.
        let mut packed_keys = String::new();
        // Skip computed-key fields (`[Symbol.for("k")] = …`) and private
        // fields. Computed keys are evaluated at construction time; private
        // fields live in class-id-qualified runtime storage installed by
        // `js_private_field_add`. Neither is a public inline shape key.
        // Including either synthetic/source spelling in packed keys leaks it
        // through reflection and inflates/misaligns the inline-slot layout.
        let count_keyable = |fields: &[perry_hir::ClassField]| -> u32 {
            fields
                .iter()
                .filter(|f| f.key_expr.is_none() && !f.is_private)
                .count() as u32
        };
        let mut total_field_count = count_keyable(&c.fields);
        // (parent_name, resolved_fields) captured during the chain walk so we
        // don't re-resolve by name (which could re-pick the wrong same-named
        // stub). Refs #26.
        let mut parent_chain: Vec<(String, Vec<perry_hir::ClassField>)> = Vec::new();
        // Resolver that finds a parent's `(fields_vec, next_extends)` either
        // in the local HIR or, failing that, in the imported_class_stubs
        // built earlier in this fn (which carry `ic.field_names` as full
        // ClassField records). Issue #485: without falling back to imports,
        // a local subclass that extends an IMPORTED parent ends up with a
        // packed_keys / total_field_count that omits the parent's fields,
        // so instances get allocated with too-few inline slots and the
        // parent's cross-module ctor's `this.field = ...` writes overflow
        // the object header — making `f.field` read undefined on the
        // importing side even though the parent's ctor "ran".
        // Issue #26 / #321: resolve a parent name to its fields, threading
        // the child's source prefix so same-named imported stubs disambiguate
        // by module (effect's duplicate `Type`). Local classes take priority
        // (they're defined in THIS module). Returns the resolved parent's
        // `(fields, extends_name, source_prefix)` so the next hop can keep
        // disambiguating in the right module's scope.
        let lookup_class_chain_link =
            |name: &str,
             child_prefix: Option<&str>|
             -> Option<(Vec<perry_hir::ClassField>, Option<String>, Option<String>)> {
                // Issue #26: if the child belongs to THIS module (child_prefix ==
                // module_prefix, the common case for a local class's own ancestor
                // chain), prefer the LOCAL same-named class — that's what its
                // `extends` clause refers to. Only fall back to the local class
                // for a cross-module child when no source-matched import exists.
                let child_is_local = child_prefix
                    .map(|cp| cp == module_prefix.as_str())
                    .unwrap_or(true);
                if child_is_local {
                    if let Some(parent) = hir.classes.iter().find(|cls| cls.name == name) {
                        return Some((
                            parent.fields.clone(),
                            parent.extends_name.clone(),
                            Some(module_prefix.clone()),
                        ));
                    }
                }
                if let Some((fields, ext, prefix)) = resolve_parent(name, child_prefix) {
                    return Some((fields, ext, Some(prefix)));
                }
                // Cross-module child with no source-matched import: last resort is
                // any local same-named class.
                if let Some(parent) = hir.classes.iter().find(|cls| cls.name == name) {
                    return Some((
                        parent.fields.clone(),
                        parent.extends_name.clone(),
                        Some(module_prefix.clone()),
                    ));
                }
                None
            };
        let mut p = c.extends_name.clone();
        // The child here is a local class `c`, so its `extends` resolves in
        // this module's scope first.
        let mut child_prefix: Option<String> = Some(module_prefix.clone());
        while let Some(parent_name) = p {
            if let Some((parent_fields, parent_extends, resolved_prefix)) =
                lookup_class_chain_link(&parent_name, child_prefix.as_deref())
            {
                parent_chain.push((parent_name.clone(), parent_fields.clone()));
                total_field_count += count_keyable(&parent_fields);
                p = parent_extends;
                child_prefix = resolved_prefix;
            } else {
                break;
            }
        }
        // Walk from deepest ancestor to direct parent. We captured the exact
        // resolved fields above, so no second by-name resolution is needed
        // (which would risk re-picking the wrong same-named stub).
        for (_parent_name, parent_fields) in parent_chain.iter().rev() {
            for f in parent_fields {
                if f.key_expr.is_some() || f.is_private {
                    continue;
                }
                packed_keys.push_str(&f.name);
                packed_keys.push('\0');
            }
        }
        for f in &c.fields {
            if f.key_expr.is_some() || f.is_private {
                continue;
            }
            packed_keys.push_str(&f.name);
            packed_keys.push('\0');
        }
        class_keys_globals_map.insert(c.name.clone(), global_name.clone());
        // Issue #26: record the authoritative root→leaf init chain. `parent_chain`
        // was pushed direct-parent-first, so reverse it (deepest ancestor first),
        // then append the leaf class `c` (with its own fields, init exprs intact).
        let chain: Vec<(String, Vec<perry_hir::ClassField>)> = {
            let mut chain: Vec<(String, Vec<perry_hir::ClassField>)> =
                parent_chain.iter().rev().cloned().collect();
            chain.push((c.name.clone(), c.fields.clone()));
            class_init_chains_map.insert(c.name.clone(), chain.clone());
            for alias in &c.aliases {
                class_init_chains_map
                    .entry(alias.clone())
                    .or_insert_with(|| chain.clone());
            }
            chain
        };
        // Refs #486: register self-binding aliases (`_X` from `var X = class _X`)
        // so the inline-alloc fast path at lower_call.rs:2532 finds the keys
        // global when the class is referenced by its inner name. Without this,
        // `new _X()` would fall into the slower `js_object_alloc_class_with_keys`
        // path that builds packed_keys at the call site — which works but is
        // unnecessarily slow.
        for alias in &c.aliases {
            class_keys_globals_map
                .entry(alias.clone())
                .or_insert_with(|| global_name.clone());
        }
        // Refs #5094: derive the GC raw-f64/pointer masks from the SAME
        // prefix-disambiguated chain that built `packed_keys` above, so mask
        // bits stay aligned with the actual slot layout when same-named
        // cross-module parents exist (the name-keyed `class_typed_layout`
        // walk picks whichever stub won the bare-name race).
        let typed_layout = crate::typed_shape::class_typed_layout_from_chain(&chain);
        class_field_counts_map.insert(c.name.clone(), total_field_count);
        for alias in &c.aliases {
            class_field_counts_map
                .entry(alias.clone())
                .or_insert(total_field_count);
        }
        keys_class_ids.insert(global_name.clone(), c.id);
        class_keys_init_data.push((
            global_name,
            packed_keys,
            total_field_count,
            typed_layout.raw_f64_mask_words,
            typed_layout.pointer_mask_words,
        ));
    }
    let defined_class_keys_len = class_keys_init_data.len();
    static_shape_ids::set_module_keys_class_ids(keys_class_ids);
    // Same naming convention for IMPORTED class stubs. Pack the field
    // names so the importing module allocates the right inline slot count
    // and the slot index for each field matches what the source module's
    // constructor wrote. Without this, the object is allocated 0 inline
    // slots and `this.field = v` in the cross-module constructor writes
    // past the object, while reads on the importing side return undefined.
    for (c_idx, c) in imported_class_stubs.iter().enumerate() {
        if hir.classes.iter().any(|local| local.name == c.name) {
            continue;
        }
        // Skip duplicate imported stubs of the same name. Two namespace
        // re-exports of the same class (e.g., `export * as A from "./mod"`
        // and `export * as B from "./mod"`) can register the same class
        // twice in `imported_class_stubs`. Without this guard, codegen
        // would emit `@perry_class_keys_<modprefix>__<name>` twice and
        // clang would reject the IR with "redefinition of global". See #336.
        if class_keys_globals_map.contains_key(&c.name) {
            continue;
        }
        let global_name = helpers::unique_class_keys_global(
            &module_prefix,
            &c.name,
            &mut used_class_keys_globals,
        );
        // #10399: composed by module init -> per-thread when the program
        // constructs a Worker.
        llmod.add_internal_module_state_global(&global_name, I64, "0");
        // #8122: the inline-`new` header image, composed at module init
        // (`string_pool.rs`) for the classes `class_header_images` admits.
        class_keys_globals_map.insert(c.name.clone(), global_name.clone());
        let mut packed_keys = String::new();
        let keyable_count = |fields: &[perry_hir::ClassField]| -> u32 {
            fields
                .iter()
                .filter(|f| f.key_expr.is_none() && !f.is_private)
                .count() as u32
        };
        let mut total_field_count = keyable_count(&c.fields);
        // Issue #485: imported subclass stubs also need their parent's
        // fields prepended to the packed-keys, so allocations on this
        // importing side reserve enough inline slots for parent +
        // child. Without this, `new Sub()` in the importing module
        // allocates 0 slots when Sub has no own fields and the
        // cross-module ctor's `this.parentField = v` writes past the
        // object header — exactly the same shape collapse the local-
        // class branch above guards against.
        //
        // Issue #26 / #321: capture each ancestor's resolved fields during
        // the walk and disambiguate same-named parent stubs by the child's
        // source prefix (effect's duplicate `Type` in SchemaAST.ts vs
        // ParseResult.ts). `child_prefix` starts as THIS stub's own source
        // prefix and follows the resolved parent's prefix at each hop, since
        // each class's `extends` resolves in its own module's scope.
        let mut parent_chain: Vec<(String, Vec<perry_hir::ClassField>)> = Vec::new();
        let mut p = c.extends_name.clone();
        let mut child_prefix: Option<String> = Some(imported_stub_prefixes[c_idx].clone());
        while let Some(parent_name) = p {
            // Imported child: resolve the parent among imports first (prefix-
            // disambiguated, including locally-shadowed imports), so a same-
            // named LOCAL class does NOT hijack an imported chain (effect's
            // ParseResult.ts local `Type` vs SchemaAST's `Type`). Refs #26.
            if let Some((parent_fields, parent_extends, parent_prefix)) =
                resolve_parent(&parent_name, child_prefix.as_deref())
            {
                parent_chain.push((parent_name.clone(), parent_fields.clone()));
                total_field_count += keyable_count(&parent_fields);
                p = parent_extends;
                child_prefix = Some(parent_prefix);
            } else if let Some(parent) = hir.classes.iter().find(|cls| cls.name == parent_name) {
                parent_chain.push((parent_name.clone(), parent.fields.clone()));
                total_field_count += keyable_count(&parent.fields);
                p = parent.extends_name.clone();
                child_prefix = Some(module_prefix.clone());
            } else {
                break;
            }
        }
        for (_parent_name, parent_fields) in parent_chain.iter().rev() {
            for f in parent_fields {
                if f.key_expr.is_some() || f.is_private {
                    continue;
                }
                packed_keys.push_str(&f.name);
                packed_keys.push('\0');
            }
        }
        for f in &c.fields {
            if f.key_expr.is_some() || f.is_private {
                continue;
            }
            packed_keys.push_str(&f.name);
            packed_keys.push('\0');
        }
        class_field_counts_map
            .entry(c.name.clone())
            .or_insert(total_field_count);
        // Issue #26: authoritative root→leaf init chain for the imported class
        // (prefix-disambiguated parents + this stub's own fields as the leaf).
        // Refs #5094: the GC raw-f64/pointer masks derive from this same chain
        // (not the name-keyed `class_typed_layout` walk) so mask bits stay
        // aligned with the packed-keys slot layout under same-named
        // cross-module parents.
        let typed_layout = {
            let mut chain: Vec<(String, Vec<perry_hir::ClassField>)> =
                parent_chain.iter().rev().cloned().collect();
            chain.push((c.name.clone(), c.fields.clone()));
            let typed_layout = crate::typed_shape::class_typed_layout_from_chain(&chain);
            class_init_chains_map.entry(c.name.clone()).or_insert(chain);
            typed_layout
        };
        class_keys_init_data.push((
            global_name,
            packed_keys,
            total_field_count,
            typed_layout.raw_f64_mask_words,
            typed_layout.pointer_mask_words,
        ));
    }
    // Every key a class key list above names is one of this module's pool
    // literals. A key text's image literal is the program's atom of that text
    // (`js_string_register_property_atom`), and read sites pass the image
    // itself. The lists are built in this module's string init, which can run
    // before the module whose pool holds the text (an imported class's keys
    // are built by the importer, which initializes first); the runtime then
    // minted a second atom for the text, which the objects' key lists held and
    // no read site passed (zod's `spa` under a marked import: the probe
    // answered absent and `this.spa.bind(this)` threw). As this module's own
    // literal, the key's image is registered as the atom before any list
    // holds it.
    for (_, packed_keys, _, _, _) in &class_keys_init_data {
        for key in packed_keys.split('\0').filter(|k| !k.is_empty()) {
            strings.intern(key);
        }
    }
    ClassKeyTables {
        class_keys_init_data,
        class_keys_globals_map,
        class_field_counts_map,
        class_init_chains_map,
        defined_class_keys_len,
    }
}
