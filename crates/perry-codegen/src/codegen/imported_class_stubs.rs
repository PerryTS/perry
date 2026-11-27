//! Lightweight stub classes for the classes a module imports, so dispatch
//! tables can resolve cross-module class method calls.

use std::collections::HashMap;

use perry_hir::Module as HirModule;

use super::{namespace_member_class_key, CompileOptions};

/// The imported-class metadata `lower_module` builds before its dispatch
/// tables, each field named for the local it binds.
pub(super) struct ImportedClassStubs {
    pub(super) imported_class_stubs: Vec<perry_hir::Class>,
    pub(super) imported_stub_prefixes: Vec<String>,
    pub(super) shadowed_parent_stubs:
        Vec<(String, String, Option<String>, Vec<perry_hir::ClassField>)>,
    pub(super) imported_class_prefix: HashMap<String, String>,
    pub(super) imported_class_source_name: HashMap<String, String>,
}

pub(super) fn build_imported_class_stubs(
    hir: &HirModule,
    opts: &CompileOptions,
    class_table: &HashMap<String, &perry_hir::Class>,
    class_ids: &mut HashMap<String, u32>,
) -> ImportedClassStubs {
    // Imported classes: build lightweight stub `Class` objects so the
    // codegen dispatch tables (class_table, method_names, class_ids)
    // can resolve cross-module class method calls. The actual method
    // bodies live in the other module's .o — here we only need the
    // metadata for dispatch and the extern LLVM declarations for the
    // linker.
    let mut imported_class_stubs: Vec<perry_hir::Class> = Vec::new();
    // Issue #26 / #321: the source-module prefix of each entry in
    // `imported_class_stubs`, kept index-parallel. Effect (and other heavily
    // modular packages) export same-named classes from different modules —
    // e.g. `class Type` exists in BOTH `SchemaAST.ts` (fields `type,
    // annotations`) and `ParseResult.ts` (fields `_tag, ast, actual,
    // message`). Both arrive here as separate stubs that collide by name.
    // The packed-keys / field-count chain walks below resolve a class's
    // parent by name (`.find(|c| c.name == parent)`), which silently picks
    // whichever same-named stub appears first in the Vec. That makes
    // `PropertySignature ← OptionalType ← Type` inherit ParseResult.Type's
    // fields instead of SchemaAST.Type's, polluting the schema AST that
    // decode/encode/is later walk. A class's `extends` clause resolves in
    // *its own* module's scope, so we disambiguate by preferring the parent
    // stub whose source prefix matches the child's.
    let mut imported_stub_prefixes: Vec<String> = Vec::new();
    // Issue #26 / #321: imported classes that are shadowed by a same-named
    // LOCAL class (so they're intentionally kept OUT of `class_table` /
    // `class_ids` / `imported_class_stubs` to preserve local dispatch
    // precedence) but are still needed to resolve the parent layout of OTHER
    // imported classes. Tuple: (name, source_prefix, parent_name, fields).
    // Consulted only by `resolve_parent`.
    let mut shadowed_parent_stubs: Vec<(
        String,
        String,
        Option<String>,
        Vec<perry_hir::ClassField>,
    )> = Vec::new();
    // Fallback id range for imported classes whose source_class_id is None
    // (legacy callers that didn't populate it). Start above the max local
    // HIR id so we don't collide with local class ids.
    let next_class_id = hir.classes.iter().map(|c| c.id).max().unwrap_or(0) + 1;
    for (idx, ic) in opts.imported_classes.iter().enumerate() {
        // Prefer the source module's class id so `instanceof` on an
        // imported class matches the id stamped onto real instances
        // by the source module's constructor. Fall back to a freshly
        // assigned id when the caller didn't pass one.
        let class_id = ic
            .source_class_id
            .unwrap_or_else(|| next_class_id + (idx as u32));
        let effective_name = ic.effective_name();
        let exported_name = ic.local_alias.as_deref().unwrap_or(&ic.name);

        // Namespace member class identity must be scoped to the namespace.
        // A flat `class_ids[member]` lookup lets a class exported by one
        // namespace hijack an equal-named value/function in another (Effect's
        // SchemaAST.Boolean class versus Schema.Boolean schema value). The
        // driver already resolved every member to its origin prefix, and an
        // ImportedClass carries that same prefix plus its consumer-visible
        // alias, so record the exact `(namespace, member)` identity here.
        for ((namespace, member), source_prefix) in &opts.namespace_member_prefixes {
            if source_prefix == &ic.source_prefix && member == exported_name {
                class_ids
                    .entry(namespace_member_class_key(namespace, member))
                    .or_insert(class_id);
            }
        }

        // Skip if already defined locally (local definition takes precedence).
        if class_table.contains_key(&effective_name) {
            // Issue #26 / #321: a locally-shadowed import is still needed for
            // *parent resolution* of OTHER imported classes. Effect's
            // ParseResult.ts declares its own local `class Type`
            // (`{_tag,ast,actual,message}`) AND imports SchemaAST's
            // `OptionalType extends Type`, whose real parent is SchemaAST's
            // `Type` (`{type,annotations}`). The local `Type` correctly
            // shadows `class_table`/`class_ids` for ParseResult's own code,
            // but the imported `OptionalType`'s field layout must still
            // resolve to SchemaAST's `Type`. Record the shadowed import in a
            // side list keyed by source prefix so `resolve_parent`
            // can find it WITHOUT polluting the name-keyed dispatch maps.
            if !ic.field_names.is_empty() || ic.parent_name.is_some() {
                shadowed_parent_stubs.push((
                    effective_name.clone(),
                    ic.source_prefix.clone(),
                    ic.parent_name.clone(),
                    ic.field_names
                        .iter()
                        .map(|name| perry_hir::ClassField {
                            byte_offset: 0, /*HPOS*/
                            origin: perry_hir::ClassFieldOrigin::Definition,
                            name: name.clone(),
                            key_expr: None,
                            ty: perry_hir::types::Type::Any,
                            init: None,
                            is_private: false,
                            is_readonly: false,
                            decorators: Vec::new(),
                        })
                        .collect::<Vec<_>>(),
                ));
            }
            continue;
        }

        // Assign a class id for dispatch / instanceof.
        //
        // Refs #665: `or_insert` (first-writer-wins) instead of `insert`
        // (last-writer-wins). When two different classes are both
        // default-imported in the same file, both register under
        // `effective_name = "default"`. `class_table.entry().or_insert()`
        // below already keeps the first stub for that key; the side maps
        // must agree, otherwise the method registry builds symbols mixing
        // the FIRST writer's methods with the LAST writer's prefix +
        // canonical name, producing fnames the linker can't resolve.
        class_ids.entry(effective_name.clone()).or_insert(class_id);
        // A lexical alias also exposes the canonical binding for legacy
        // source-name lookups. Namespace members do not: `ns.Service` must
        // never claim the unrelated bare `Service` binding in this module.
        if ic.namespace.is_none() && ic.local_alias.is_some() && !class_ids.contains_key(&ic.name) {
            class_ids.insert(ic.name.clone(), class_id);
        }

        let imported_getters: Vec<perry_hir::Function> = ic
            .getter_names
            .iter()
            .enumerate()
            .map(|(index, prop)| perry_hir::Function {
                byte_offset: 0, /*HPOS*/
                id: 0,
                name: format!("get_{}", prop),
                type_params: Vec::new(),
                params: Vec::new(),
                return_type: ic
                    .getter_return_types
                    .get(index)
                    .cloned()
                    .unwrap_or(perry_hir::types::Type::Any),
                body: Vec::new(),
                is_async: false,
                is_generator: false,
                is_strict: true,
                was_plain_async: false,
                was_unrolled: false,
                construct_only: false,
                is_exported: false,
                captures: Vec::new(),
                decorators: Vec::new(),
            })
            .collect();
        let imported_setters: Vec<perry_hir::Function> = ic
            .setter_names
            .iter()
            .map(|prop| perry_hir::Function {
                byte_offset: 0, /*HPOS*/
                id: 0,
                name: format!("set_{}", prop),
                type_params: Vec::new(),
                params: Vec::new(),
                return_type: perry_hir::types::Type::Any,
                body: Vec::new(),
                is_async: false,
                is_generator: false,
                is_strict: true,
                was_plain_async: false,
                was_unrolled: false,
                construct_only: false,
                is_exported: false,
                captures: Vec::new(),
                decorators: Vec::new(),
            })
            .collect();

        // Build a stub Class with the minimum fields the codegen needs.
        // Imported accessor bodies execute from the source module; carrying
        // their names here keeps dispatch and field inference conservative.
        let stub = perry_hir::Class {
            id: 0, // imported — no local ClassId
            name: effective_name.clone(),
            // #6812: width hints don't cross module metadata; imported stubs
            // fall back to runtime learned sizing.
            alloc_width_hint: 0,
            // #7575: monomorphization is per-module, so an imported stub never
            // stands in for a specialization — its defining module registers
            // the origin edge itself.
            specialized_from: None,
            type_params: Vec::new(),
            extends: None,
            extends_name: ic.effective_parent_name(),
            native_extends: None,
            extends_expr: None,
            heritage_lexically_shadowed: false,
            fields: ic
                .field_names
                .iter()
                .enumerate()
                .map(|(i, name)| perry_hir::ClassField {
                    byte_offset: 0, /*HPOS*/
                    origin: perry_hir::ClassFieldOrigin::Definition,
                    name: name.clone(),
                    key_expr: None,
                    // Use the real declared type when the source-side
                    // populated `field_types`; fall back to `Any` otherwise.
                    // Real types let `receiver_class_name`'s `PropertyGet`
                    // recursion identify chained imported-class field
                    // dispatch (e.g. `vm.viewport.scroll.scrollTop`).
                    ty: ic
                        .field_types
                        .get(i)
                        .cloned()
                        .unwrap_or(perry_hir::types::Type::Any),
                    init: None,
                    is_private: false,
                    is_readonly: false,
                    decorators: Vec::new(),
                })
                .collect(),
            constructor: None,
            methods: ic
                .method_names
                .iter()
                .enumerate()
                .map(|(index, m)| perry_hir::Function {
                    byte_offset: 0, /*HPOS*/
                    id: 0,
                    name: m.clone(),
                    type_params: Vec::new(),
                    params: Vec::new(),
                    return_type: ic
                        .method_return_types
                        .get(index)
                        .cloned()
                        .unwrap_or(perry_hir::types::Type::Any),
                    body: Vec::new(),
                    is_async: false,
                    is_generator: false,
                    is_strict: true,
                    was_plain_async: false,
                    was_unrolled: false,
                    construct_only: false,
                    is_exported: false,
                    captures: Vec::new(),
                    decorators: Vec::new(),
                })
                .collect(),
            getters: ic
                .getter_names
                .iter()
                .cloned()
                .zip(imported_getters)
                .collect(),
            setters: ic
                .setter_names
                .iter()
                .cloned()
                .zip(imported_setters)
                .collect(),
            static_accessor_names: Vec::new(),
            static_accessor_fn_ids: Vec::new(),
            static_fields: Vec::new(),
            static_methods: ic
                .static_method_names
                .iter()
                .enumerate()
                .map(|(index, name)| {
                    // The body/default expressions remain in the producer,
                    // but imported static dispatch still needs the declared
                    // width and trailing rest/`arguments` shape to forward
                    // arguments with the producer's ABI. Synthetic params are
                    // sufficient because no imported stub body is compiled.
                    let declared = ic
                        .static_method_param_counts
                        .get(index)
                        .copied()
                        .unwrap_or(0);
                    let has_rest = ic
                        .static_method_has_rest
                        .get(index)
                        .copied()
                        .unwrap_or(false);
                    let has_user_rest = ic
                        .static_method_has_user_rest
                        .get(index)
                        .copied()
                        .unwrap_or(false);
                    let last_is_synthetic_arguments = ic
                        .static_method_has_synthetic_arguments
                        .get(index)
                        .copied()
                        .unwrap_or(false);
                    let params = (0..declared)
                        .map(|param_index| {
                            let is_last = param_index + 1 == declared;
                            let is_user_rest = has_user_rest
                                && if last_is_synthetic_arguments {
                                    param_index + 2 == declared
                                } else {
                                    is_last
                                };
                            let is_synthetic_rest = is_last
                                && last_is_synthetic_arguments
                                && has_rest
                                && !has_user_rest;
                            let is_legacy_rest = is_last
                                && !last_is_synthetic_arguments
                                && has_rest
                                && !has_user_rest;
                            perry_hir::Param {
                                byte_offset: 0, /*HPOS*/
                                id: param_index as perry_hir::types::LocalId,
                                name: format!("arg{param_index}"),
                                ty: perry_hir::types::Type::Any,
                                default: None,
                                decorators: Vec::new(),
                                is_rest: is_user_rest || is_synthetic_rest || is_legacy_rest,
                                arguments_object: (is_last && last_is_synthetic_arguments).then(
                                    || perry_hir::ArgumentsObjectMeta {
                                        strict: true,
                                        simple_parameters: false,
                                        mapped_parameter_ids: Vec::new(),
                                        restricted_callee: true,
                                    },
                                ),
                            }
                        })
                        .collect();
                    perry_hir::Function {
                        byte_offset: 0, /*HPOS*/
                        id: 0,
                        name: name.clone(),
                        type_params: Vec::new(),
                        params,
                        return_type: ic
                            .static_method_return_types
                            .get(index)
                            .cloned()
                            .unwrap_or(perry_hir::types::Type::Any),
                        body: Vec::new(),
                        is_async: false,
                        is_generator: false,
                        is_strict: true,
                        was_plain_async: false,
                        was_unrolled: false,
                        construct_only: false,
                        is_exported: false,
                        captures: Vec::new(),
                        decorators: Vec::new(),
                    }
                })
                .collect(),
            computed_members: Vec::new(),
            decorators: Vec::new(),
            is_exported: false,
            aliases: Vec::new(),
            is_nested: false,
            decl_stmt: None,
        };
        imported_class_stubs.push(stub);
        imported_stub_prefixes.push(ic.source_prefix.clone());
    }
    // Issue #309: break inheritance-chain cycles in imported_class_stubs.
    // Effect (and other heavily-modular TypeScript packages) declare
    // same-named classes across modules (e.g. multiple `class Base extends X`
    // inside IIFEs in Data.ts, plus `class Class extends Base` in
    // Effectable.ts). When pulled into a single importing module's
    // class_table by name, the chains can form a cycle:
    //     local Base → extends "Class" (imported stub from Effectable)
    //     imported Class → parent_name "Base" (resolves back to local Base)
    //     → cycle.
    // Every chain-walking site in codegen assumed acyclic inheritance, so
    // a single such cycle causes either an OOM (Vec-accumulating walks like
    // `apply_field_initializers_recursive`) or a CPU-hang (counter walks
    // like `class_field_global_index`). We break the cycle once at this
    // central point by detecting it via DFS over the (local ∪ stub) union
    // and dropping `extends_name` on the FIRST imported stub that closes
    // the cycle. All downstream chain walks then operate on a guaranteed-
    // acyclic graph. The fundamental name-collision problem (Data.ts's
    // local "Base" being a different class than Effectable.ts's "Base"
    // even though they share a name) is left unfixed — that requires
    // module-prefixing class names in HIR and is a separate refactor; the
    // cycle break here is purely defensive.
    {
        let local_class_names: std::collections::HashSet<&str> =
            hir.classes.iter().map(|c| c.name.as_str()).collect();
        let mut stub_idx_by_name: std::collections::HashMap<String, usize> =
            std::collections::HashMap::new();
        for (idx, stub) in imported_class_stubs.iter().enumerate() {
            stub_idx_by_name.entry(stub.name.clone()).or_insert(idx);
        }
        // For each stub, walk the chain in the union name space. If the
        // walk revisits a name OR exceeds a sane depth cap, drop this
        // stub's parent so the cycle dies here.
        let mut to_drop: Vec<usize> = Vec::new();
        for (idx, stub) in imported_class_stubs.iter().enumerate() {
            let mut visited: std::collections::HashSet<String> = std::collections::HashSet::new();
            visited.insert(stub.name.clone());
            let mut cur = stub.extends_name.clone();
            let mut depth: usize = 0;
            let mut cycle = false;
            while let Some(name) = cur {
                depth += 1;
                if depth > 64 {
                    cycle = true;
                    break;
                }
                if !visited.insert(name.clone()) {
                    cycle = true;
                    break;
                }
                // Parent resolution: prefer LOCAL class over imported stub
                // (matches `class_table.entry().or_insert()` semantics
                // below).
                cur = if local_class_names.contains(name.as_str()) {
                    hir.classes
                        .iter()
                        .find(|c| c.name == name)
                        .and_then(|c| c.extends_name.clone())
                } else if let Some(&pidx) = stub_idx_by_name.get(&name) {
                    imported_class_stubs[pidx].extends_name.clone()
                } else {
                    None
                };
            }
            if cycle {
                to_drop.push(idx);
            }
        }
        for idx in to_drop {
            imported_class_stubs[idx].extends_name = None;
        }
    }

    // Add imported class stubs to the class_table (references into the
    // Vec we just built — the Vec lives for the remainder of compile_module).
    // Also build a map from class name → source module prefix so method
    // dispatch generates the correct cross-module symbol name.
    //
    // Skip imports that collide by name with a LOCAL class (#431). The
    // local class shadows the import in `class_table` (the
    // `class_table.entry().or_insert()` loop below preserves the local
    // entry), so this map must not point a local-class lookup at an
    // import's source prefix — doing so makes `compile_method` mangle
    // the LOCAL methods under the IMPORTED module's prefix while the
    // dispatch-table builder (line ~3614) still references them under
    // the local prefix, leaving `@perry_method_<local>__<C>__<m>`
    // undefined at link time. This is the cross-module sibling of
    // #336's intra-module collision; #336 disambiguated the
    // `@perry_class_keys_*` global, but the method-body prefix needs
    // the same fix for cross-module name reuse (Effect's `Class` /
    // `Refinement` / `Composite` / `ParseError` /
    // `PropertySignatureTransformation` / `DroppingStrategy` cases).
    let mut imported_class_prefix: HashMap<String, String> = HashMap::new();
    // Issue #568: when `import { Widget as PublicWidget }` (or the
    // re-export shape `export { Widget as PublicWidget }` followed by
    // `import { PublicWidget }`) renames a cross-module class, the stub
    // pushed into `class_table` carries `name = effective_name` (the
    // alias). Method-symbol mangling needs the SOURCE-side name (the
    // canonical `ic.name`) so the LLVM call resolves to the symbol the
    // source module's `.o` actually exports. This side map lets the
    // method-registry loop below recover the source name.
    let mut imported_class_source_name: HashMap<String, String> = HashMap::new();
    for ic in &opts.imported_classes {
        let effective_name = ic.effective_name();
        if hir.classes.iter().any(|c| c.name == effective_name) {
            continue;
        }
        // Refs #665: first-writer-wins to match `class_table`'s
        // `.or_insert()` semantics (see the class-id loop above). When two
        // different classes are both default-imported, both register under
        // `effective_name = "default"`; using `.insert()` would let the
        // LAST writer's source_prefix / canonical name win, while
        // `class_table["default"]` keeps the FIRST writer's stub. The
        // method-registry builder reads both, and the mismatch produces
        // method symbols mangled under the wrong class — the linker can't
        // resolve them and the build fails with "undefined value".
        imported_class_prefix
            .entry(effective_name.clone())
            .or_insert_with(|| ic.source_prefix.clone());
        if effective_name != ic.name {
            imported_class_source_name
                .entry(effective_name)
                .or_insert_with(|| ic.name.clone());
        }
    }
    ImportedClassStubs {
        imported_class_stubs,
        imported_stub_prefixes,
        shadowed_parent_stubs,
        imported_class_prefix,
        imported_class_source_name,
    }
}
