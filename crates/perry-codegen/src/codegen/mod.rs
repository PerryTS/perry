//! HIR → LLVM IR compilation entry point.
//!
//! Public contract:
//!
//! ```ignore
//! let opts = CompileOptions { target: None, is_entry_module: true };
//! let object_bytes: Vec<u8> = perry_codegen::compile_module(&hir, opts)?;
//! ```
//!
//! The returned bytes are a regular object file produced by `clang -c`.
//! Perry's linking stage in `crates/perry/src/commands/compile.rs`
//! links them against `libperry_runtime.a` and `libperry_stdlib.a`.
//!
//! Currently supported (Phases 1, 2, 2.1, A-strings):
//!
//! - User functions with typed `double` ABI
//! - Recursive and forward calls via `FuncRef`
//! - If/else, for loops, let, return
//! - Binary arithmetic (add/sub/mul/div/mod) and compare
//! - Update (++/--) and LocalSet
//! - `Date.now()` via `js_date_now`
//! - **String literals** via the hoisted `StringPool` (one allocation per
//!   literal at module init time, registered as a permanent GC root via
//!   `js_gc_register_global_root`; use sites are a single `load`)
//! - `console.log(<expr>)` — uses `js_console_log_number` for static number
//!   literals (optimized path) and `js_console_log_dynamic` for everything
//!   else (NaN-tag dispatch at runtime)
//!
//! Anything else (objects, arrays, classes, closures, async, imports, …)
//! errors with an actionable "Phase X not yet supported" message.

use std::collections::HashMap;

use anyhow::{Context, Result};
use perry_hir::Module as HirModule;

use crate::module::LlModule;
use crate::runtime_decls;
use crate::strings::StringPool;
use crate::types::{LlvmType, DOUBLE, I64};

pub(crate) mod arguments;
mod artifacts;
mod boxed_locals;
mod closure;
mod closure_collect;
mod entry;
mod func_registry;
mod function;
mod helpers;
mod i64_spec;
mod method;
mod method_registry;
mod module_globals_emit;
mod opts;
mod string_pool;
mod typed_abi;

pub(crate) use closure::emit_typed_string_capture_guard;
pub use helpers::resolve_target_triple;
pub(crate) use helpers::{
    decide_codegen_units, decide_full_outline_ic, default_target_triple, full_outline_ic_enabled,
    module_callable_count, set_full_outline_ic, write_barriers_enabled,
};
pub use opts::{
    AppMetadata, CompileOptions, FpContractMode, ImportedClass, NamespaceEntry, NamespaceEntryKind,
};
pub(crate) use opts::{CrossModuleCtx, ImportedCtor};
pub(crate) use typed_abi::{
    emit_typed_arg_guard, emit_typed_arg_to_raw, generic_closure_body_name,
    generic_function_body_name, generic_method_body_name, typed_f64_closure_name,
    typed_f64_function_name, typed_f64_method_name, typed_f64_receiver_method_info,
    typed_f64_receiver_method_name, typed_i1_closure_name, typed_i1_function_name,
    typed_i1_method_name, typed_i32_closure_name, typed_i32_function_name, typed_i32_method_name,
    typed_param_reps_match_args, typed_string_closure_name, typed_string_function_name,
    typed_string_method_name, TypedParamRep, TypedReceiverMethodInfo,
};

use artifacts::{emit_module_artifacts, ModuleArtifactsCtx};
use function::{
    compile_function, compile_typed_f64_function, compile_typed_i1_function,
    compile_typed_i32_function, compile_typed_string_function,
};
use helpers::{
    collect_return_class, emit_buffer_alias_metadata, function_body_returns_generator_object,
    sanitize, sanitize_member, scoped_fn_name, scoped_method_name, scoped_static_method_name,
};

// Collector and boxing-analysis walkers live in dedicated modules. The
// module-wide pre-walk passes that consumed them moved into the
// `boxed_locals` / `closure_collect` / `module_globals_emit` siblings, which
// import them directly; the trunk no longer references them.

pub(super) fn spec_function_length(params: &[perry_hir::Param]) -> usize {
    params
        .iter()
        .take_while(|p| !p.is_rest && p.default.is_none())
        .count()
}

fn should_record_typed_clone_rejection(reason: typed_abi::TypedCloneRejectionReason) -> bool {
    if std::env::var_os("PERRY_NATIVE_REPS_ALL_TYPED_CLONE_REJECTIONS").is_some() {
        return !matches!(reason, typed_abi::TypedCloneRejectionReason::NotClosure);
    }
    !matches!(
        reason,
        typed_abi::TypedCloneRejectionReason::NotClosure
            | typed_abi::TypedCloneRejectionReason::ReturnTypeNotF64
            | typed_abi::TypedCloneRejectionReason::ReturnTypeNotI32
            | typed_abi::TypedCloneRejectionReason::ReturnTypeNotI1
            | typed_abi::TypedCloneRejectionReason::ReturnTypeNotString
            | typed_abi::TypedCloneRejectionReason::NoReceiverField
    )
}

fn record_typed_clone_rejection(
    records: &mut Vec<crate::native_value::NativeRepRecord>,
    source_function: impl Into<String>,
    consumer: &'static str,
    reason: typed_abi::TypedCloneRejectionReason,
    notes: Vec<String>,
) {
    if !should_record_typed_clone_rejection(reason) {
        return;
    }
    records.push(crate::native_value::typed_clone_rejection_record(
        source_function,
        consumer,
        reason.as_str(),
        notes,
    ));
}

pub(crate) fn static_method_registry_key(method_name: &str) -> String {
    format!("__perry_static__{}", method_name)
}

/// Compile a Perry HIR module to an object file via LLVM IR.
///
/// CRITICAL (#686): `hir` MUST be `&HirModule` (shared reference), never
/// `&mut`. The caller computes `perry_hir::stable_hash::hash_module(hir)`
/// just before this call to derive the per-module object cache key. If
/// codegen ever mutated the HIR mid-compile, the cached `.o` would no
/// longer correspond to the hashed input and stale entries would be
/// served on subsequent builds. The `&` here is the load-bearing
/// guarantee — do not change to `&mut` without also moving the cache
/// hash to AFTER codegen.
pub fn compile_module(hir: &HirModule, opts: CompileOptions) -> Result<Vec<u8>> {
    let triple = opts.target.clone().unwrap_or_else(default_target_triple);
    let fp_flags = crate::block::FpFlags::new(opts.fast_math, opts.fp_contract_mode);

    // #5334 lever B: decide ONCE, up front, whether this module is large enough
    // to full-outline its class-field IC diamonds (read per-site during
    // lowering via `full_outline_ic_enabled()`). Thread-local, so it must be set
    // afresh for every module — including the `false` case, to clear any prior
    // module's decision on this thread.
    set_full_outline_ic(decide_full_outline_ic(module_callable_count(hir)));

    let mut llmod = LlModule::new_with_fp_flags(&triple, fp_flags);
    // Null guard global: a zeroed i32 used as a safe dereference target
    // when a NaN-unboxed pointer is null/invalid. Prevents segfaults from
    // uninitialized locals or unhandled expressions producing 0.0/TAG_UNDEFINED.
    llmod.add_internal_global("perry_null_guard_zero", crate::types::I32, "0");
    runtime_decls::declare_phase1(&mut llmod);

    // Derive a per-module symbol prefix from the HIR module name:
    //
    //     self.module_symbol_prefix = hir.name.replace(|c: char|
    //         !c.is_alphanumeric() && c != '_', "_");
    //
    // Every emitted symbol that could collide across modules
    // (user functions, class methods, string pool globals, handle slots,
    // module-level globals) gets prefixed with this. The entry module's
    // `main` is the only globally-named symbol — non-entry modules emit
    // `<prefix>__init` instead.
    let module_prefix = sanitize(&hir.name);

    // Imports are no longer a hard error — Phase F.1 supports multi-
    // module compilation. Cross-module function CALLS via ExternFuncRef
    // still land in Phase F.2; for now they'll error at the use site
    // with a specific message.

    // Phase C.2: classes (and inheritance!) are supported. Perry's HIR
    // lowering aggressively pre-resolves both methods and super calls
    // into inline statements at the constructor/method body, so the
    // LLVM codegen mostly sees a flat object-allocation + field-set
    // pattern. We let everything through and let the expression-level
    // codegen error at any specific construct it doesn't know how to
    // handle.

    // Module-wide string literal pool. Owned by the codegen so that
    // `compile_function` and `compile_main` can take split borrows of
    // (&mut LlFunction, &mut StringPool) without confusing the borrow
    // checker — the pool lives outside LlModule. The module prefix
    // becomes part of every emitted global so multi-module programs
    // don't collide on `.str.0.handle`.
    let mut strings = StringPool::with_prefix(module_prefix.clone());
    // #5247: install per-module source-location context for the dynamic
    // call-dispatch throw path, but only under `--debug-symbols` (which sets
    // `opts.debug_locations` + `opts.module_source`). Off by default — no
    // source clone, no per-call emission.
    if opts.debug_locations {
        if let Some(src) = opts.module_source.clone() {
            strings.set_debug_location_ctx(Some((hir.name.clone(), src)));
            // #5247 (CJS-wrap coordinate skew): `src` is the WRAPPED source for
            // a CommonJS module; subtract the wrapper-prefix line count when
            // resolving offsets so the rendered line is in original coordinates.
            strings.set_debug_source_line_offset(opts.debug_source_line_offset);
        }
    }

    // Class lookup table for `Expr::New`. Indexed by class name —
    // the HIR has unique names per module.
    let mut class_table: HashMap<String, &perry_hir::Class> =
        hir.classes.iter().map(|c| (c.name.clone(), c)).collect();
    // Refs #486: also register class-expression self-binding aliases so
    // `lookup_new("_X")` and other code paths that consult `class_table` by
    // name find the underlying class. See `class_ids` block below for the
    // companion id-map registration and the broader rationale.
    for c in &hir.classes {
        for alias in &c.aliases {
            class_table.entry(alias.clone()).or_insert(c);
        }
    }

    // Class id assignment: each user class gets an integer id
    // starting at 1 (0 is reserved for anonymous object literals).
    // Used by lower_new to tag the object header so virtual
    // dispatch and instanceof can read the actual class at runtime.
    //
    // We use the HIR `ClassId` (assigned by `LoweringContext::fresh_class`)
    // rather than a per-module enumerate index, because in multi-module
    // compilation the HIR counter is shared across modules (compile.rs
    // threads `next_class_id` through `lower_module_with_class_id_and_types`).
    // Importing modules look up imported classes via their HIR id (passed
    // as `ImportedClass.source_class_id`); using the HIR id here too means
    // the source module stamps the same id on `new C()` instances that
    // importing modules check against in `e instanceof C`.
    let mut class_ids: HashMap<String, u32> =
        hir.classes.iter().map(|c| (c.name.clone(), c.id)).collect();
    // Refs #486: register class-expression self-binding aliases (e.g. the
    // `_X` in `var X = class _X { ... }`) so `new _X()` from inside the class
    // body resolves to the same class id as `new X()` would. Without this,
    // lower_new("_X") falls into the placeholder path and stamps class_id=0
    // on the new instance, breaking method dispatch.
    for c in &hir.classes {
        for alias in &c.aliases {
            class_ids.entry(alias.clone()).or_insert(c.id);
        }
    }

    // Enum lookup table for `Expr::EnumMember`. Each (enum_name,
    // member_name) maps to its EnumValue, which the codegen lowers
    // to either a numeric or string constant. Built once here.
    let mut enum_table: HashMap<(String, String), perry_hir::EnumValue> = hir
        .enums
        .iter()
        .flat_map(|e| {
            e.members
                .iter()
                .map(move |m| ((e.name.clone(), m.name.clone()), m.value.clone()))
        })
        .collect();

    // ── Phase F: merge imported cross-module definitions ──────────
    //
    // Imported enums: add their members to the enum_table so
    // `Expr::EnumMember` can resolve them in this module.
    for (enum_name, members) in &opts.imported_enums {
        for (member_name, value) in members {
            enum_table
                .entry((enum_name.clone(), member_name.clone()))
                .or_insert_with(|| value.clone());
        }
    }

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
        let effective_name = ic.local_alias.as_deref().unwrap_or(&ic.name);

        // Skip if already defined locally (local definition takes precedence).
        if class_table.contains_key(effective_name) {
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
                    effective_name.to_string(),
                    ic.source_prefix.clone(),
                    ic.parent_name.clone(),
                    ic.field_names
                        .iter()
                        .map(|name| perry_hir::ClassField {
                            name: name.clone(),
                            key_expr: None,
                            ty: perry_types::Type::Any,
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
        class_ids
            .entry(effective_name.to_string())
            .or_insert(class_id);
        // Also register the canonical name if aliased.
        if ic.local_alias.is_some() && !class_ids.contains_key(&ic.name) {
            class_ids.insert(ic.name.clone(), class_id);
        }

        let imported_getters: Vec<perry_hir::Function> = ic
            .getter_names
            .iter()
            .map(|prop| perry_hir::Function {
                id: 0,
                name: format!("get_{}", prop),
                type_params: Vec::new(),
                params: Vec::new(),
                return_type: perry_types::Type::Any,
                body: Vec::new(),
                is_async: false,
                is_generator: false,
                is_strict: true,
                was_plain_async: false,
                was_unrolled: false,
                is_exported: false,
                captures: Vec::new(),
                decorators: Vec::new(),
            })
            .collect();
        let imported_setters: Vec<perry_hir::Function> = ic
            .setter_names
            .iter()
            .map(|prop| perry_hir::Function {
                id: 0,
                name: format!("set_{}", prop),
                type_params: Vec::new(),
                params: Vec::new(),
                return_type: perry_types::Type::Any,
                body: Vec::new(),
                is_async: false,
                is_generator: false,
                is_strict: true,
                was_plain_async: false,
                was_unrolled: false,
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
            name: effective_name.to_string(),
            type_params: Vec::new(),
            extends: None,
            extends_name: ic.parent_name.clone(),
            native_extends: None,
            extends_expr: None,
            heritage_lexically_shadowed: false,
            fields: ic
                .field_names
                .iter()
                .enumerate()
                .map(|(i, name)| perry_hir::ClassField {
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
                        .unwrap_or(perry_types::Type::Any),
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
                .map(|m| perry_hir::Function {
                    id: 0,
                    name: m.clone(),
                    type_params: Vec::new(),
                    params: Vec::new(),
                    return_type: perry_types::Type::Any,
                    body: Vec::new(),
                    is_async: false,
                    is_generator: false,
                    is_strict: true,
                    was_plain_async: false,
                    was_unrolled: false,
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
            static_methods: Vec::new(),
            computed_members: Vec::new(),
            decorators: Vec::new(),
            is_exported: false,
            aliases: Vec::new(),
            is_nested: false,
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
        let effective_name = ic.local_alias.as_deref().unwrap_or(&ic.name);
        if hir.classes.iter().any(|c| c.name == *effective_name) {
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
            .entry(effective_name.to_string())
            .or_insert_with(|| ic.source_prefix.clone());
        if effective_name != ic.name {
            imported_class_source_name
                .entry(effective_name.to_string())
                .or_insert_with(|| ic.name.clone());
        }
    }
    for stub in &imported_class_stubs {
        class_table.entry(stub.name.clone()).or_insert(stub);
    }

    // Local async function FuncIds — populated below from `hir.functions`
    // (the per-function loop further down). Built here so the CrossModuleCtx
    // construction is complete before the FnCtx instances reference it.
    let mut local_async_funcs: std::collections::HashSet<u32> = std::collections::HashSet::new();
    let mut local_generator_funcs: std::collections::HashSet<u32> =
        std::collections::HashSet::new();
    let mut funcs_reading_dynamic_this: std::collections::HashSet<u32> =
        std::collections::HashSet::new();
    for f in &hir.functions {
        // Include both truly-async functions and those transformed from
        // async to generator (was_plain_async=true, is_async=false after
        // the v0.5.371 async-to-generator pass) — both return Promises
        // so is_promise_expr must recognize their call sites.
        if f.is_async || f.was_plain_async {
            local_async_funcs.insert(f.id);
        }
        if function_body_returns_generator_object(&f.body) {
            local_generator_funcs.insert(f.id);
        }
        if perry_hir::analysis::body_reads_dynamic_this(&f.body) {
            funcs_reading_dynamic_this.insert(f.id);
        }
    }

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

    // Distinct source class names can `sanitize()` to the SAME symbol — e.g.
    // `$X` and `_X` both become `_X` (minified bundles use `$`/`_` heavily).
    // Two such classes are genuinely different (different shapes), so each needs
    // its OWN keys-global; emitting `@perry_class_keys_<prefix>__<sanitized>`
    // twice makes clang reject the IR ("redefinition of global"). Track every
    // emitted name and disambiguate collisions with a numeric suffix. The
    // (real-name-keyed) `class_keys_globals_map` stores the unique name, so every
    // `new ClassName()` site still resolves to the right global.
    let mut used_class_keys_globals: std::collections::HashSet<String> =
        std::collections::HashSet::new();
    fn unique_global(base: String, used: &mut std::collections::HashSet<String>) -> String {
        if used.insert(base.clone()) {
            return base;
        }
        let mut n = 1u32;
        loop {
            let candidate = format!("{base}_{n}");
            if used.insert(candidate.clone()) {
                return candidate;
            }
            n += 1;
        }
    }

    for c in &hir.classes {
        let global_name = unique_global(
            format!("perry_class_keys_{}__{}", module_prefix, sanitize(&c.name)),
            &mut used_class_keys_globals,
        );
        llmod.add_internal_global(&global_name, I64, "0");

        // Build the packed-keys string. Format: each field name
        // followed by `\0`. Parent classes contribute their fields
        // first (walking from deepest ancestor down) so the slot
        // order matches `class_field_global_index`'s assumption.
        let mut packed_keys = String::new();
        // Skip computed-key fields (`[Symbol.for("k")] = …`): their key is an
        // expression evaluated at runtime, not a stable string, so they don't
        // get an inline slot. Including their synthetic `__computed_field_*`
        // names in the packed keys would surface them as enumerable own
        // properties via Object.keys() and inflate the inline-slot count.
        // Their values are stored via `apply_field_initializers_recursive`'s
        // IndexSet path → js_object_set_field / js_object_set_symbol_property.
        let count_keyable = |fields: &[perry_hir::ClassField]| -> u32 {
            fields.iter().filter(|f| f.key_expr.is_none()).count() as u32
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
                if f.key_expr.is_some() {
                    continue;
                }
                packed_keys.push_str(&f.name);
                packed_keys.push('\0');
            }
        }
        for f in &c.fields {
            if f.key_expr.is_some() {
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
        class_keys_init_data.push((
            global_name,
            packed_keys,
            total_field_count,
            typed_layout.raw_f64_mask_words,
            typed_layout.pointer_mask_words,
        ));
    }
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
        let global_name = unique_global(
            format!("perry_class_keys_{}__{}", module_prefix, sanitize(&c.name)),
            &mut used_class_keys_globals,
        );
        llmod.add_internal_global(&global_name, I64, "0");
        class_keys_globals_map.insert(c.name.clone(), global_name.clone());
        let mut packed_keys = String::new();
        let mut total_field_count = c.fields.len() as u32;
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
                total_field_count += parent_fields.len() as u32;
                p = parent_extends;
                child_prefix = Some(parent_prefix);
            } else if let Some(parent) = hir.classes.iter().find(|cls| cls.name == parent_name) {
                parent_chain.push((parent_name.clone(), parent.fields.clone()));
                total_field_count += parent.fields.len() as u32;
                p = parent.extends_name.clone();
                child_prefix = Some(module_prefix.clone());
            } else {
                break;
            }
        }
        for (_parent_name, parent_fields) in parent_chain.iter().rev() {
            for f in parent_fields {
                packed_keys.push_str(&f.name);
                packed_keys.push('\0');
            }
        }
        for f in &c.fields {
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

    // Derive __platform__ number from target triple:
    //   0 = macOS, 1 = iOS, 2 = Android, 3 = Windows, 4 = Linux,
    //   5 = Web, 6 = tvOS, 7 = watchOS, 8 = visionOS, 9 = HarmonyOS
    let platform_number: f64 = {
        let t = triple.to_lowercase();
        // HarmonyOS check must precede the plain `linux` arm: the OHOS triple is
        // `*-unknown-linux-ohos`, so a naive `contains("linux")` would classify it as 4.
        if t.contains("ohos") {
            9.0
        } else if t.contains("visionos") || t.contains("xros") {
            8.0
        } else if t.contains("watchos") {
            7.0
        } else if t.contains("ios") {
            1.0
        } else if t.contains("tvos") {
            6.0
        } else if t.contains("android") {
            2.0
        } else if t.contains("windows") || t.contains("mingw") || t.contains("msvc") {
            3.0
        } else if t.contains("linux") {
            4.0
        } else if t.contains("wasm") || t.contains("emscripten") {
            5.0
        } else {
            0.0
        } // macOS / darwin default
    };
    // Pre-scan hir.init for compile-time constant variables. These are
    // `declare const __platform__: number` / `declare const __plugins__: number`
    // that other backends (JS, WASM) inject at build time. The LLVM backend
    // uses these to constant-fold platform checks in `lower_if`, eliminating
    // dead branches that reference extern FFI functions absent on the target.
    let mut compile_time_constants: HashMap<u32, f64> = HashMap::new();
    for s in &hir.init {
        if let perry_hir::Stmt::Let {
            id,
            name,
            init: None,
            ..
        } = s
        {
            match name.as_str() {
                "__platform__" => {
                    compile_time_constants.insert(*id, platform_number);
                }
                "__plugins__" => {
                    compile_time_constants.insert(*id, 0.0);
                }
                _ => {}
            }
        }
    }

    // Issue #235: per-method explicit-param-count map covering BOTH local
    // classes (from `hir.classes`) AND imported classes (from
    // `opts.imported_classes`). Every method-call dispatch site in
    // `lower_call.rs` looks up here to pad missing trailing args with
    // TAG_UNDEFINED so the callee's default-param desugaring (`if (options
    // === undefined) options = {}`) fires correctly. Pre-fix the dispatch
    // tower passed only the user-provided args, leaving the callee to read
    // uninitialized arg-register slots for any param the caller skipped —
    // a real heap pointer from a prior call's leftover state, which when
    // dereferenced for `options.session` silently hung in the dispatch chain.
    let mut method_param_counts: std::collections::HashMap<(String, String), usize> =
        std::collections::HashMap::new();
    // Parallel `(class, method) → has_rest_param` map. Closes #484:
    // `b.with(1)` on `class Builder { with<T>(id, ...args: T extends void ?
    // [] | [void] : [T]): this }` left `args` as undefined because the
    // codegen-side dispatch table didn't track the rest bit, so the
    // call site never bundled trailing args (zero, in that test) into
    // a `js_array_alloc(0)` rest array. The conditional rest type is
    // a red herring — even `...args: any[]` would have shown the same
    // signature gap, except for the freestanding-function path which
    // already had `func_signatures.has_rest`.
    let mut method_has_rest: std::collections::HashMap<(String, String), bool> =
        std::collections::HashMap::new();
    for cls in &hir.classes {
        for m in &cls.methods {
            method_param_counts.insert((cls.name.clone(), m.name.clone()), m.params.len());
            let has_rest = m.params.iter().any(|p| p.is_rest);
            if has_rest {
                method_has_rest.insert((cls.name.clone(), m.name.clone()), true);
            }
        }
        // Issue #894: track static methods too. Effect's `static pipe()` /
        // `static annotations()` synthesize a trailing `...arguments` rest
        // param when the body reads `arguments`. The StaticMethodCall
        // lowering at `expr.rs::Expr::StaticMethodCall` reads
        // `method_has_rest` to decide whether to bundle trailing args into
        // a rest array; without this, `Cls.pipe(a, b)` calls the method
        // with 2 scalar args while the signature expects (rest_array),
        // and `arguments.length` reads garbage / undefined.
        for sm in &cls.static_methods {
            let key = static_method_registry_key(&sm.name);
            method_param_counts.insert((cls.name.clone(), key.clone()), sm.params.len());
            let has_rest = sm.params.iter().any(|p| p.is_rest);
            if has_rest {
                method_has_rest.insert((cls.name.clone(), key), true);
            }
        }
    }
    for ic in &opts.imported_classes {
        let effective_name = ic.local_alias.as_deref().unwrap_or(&ic.name).to_string();
        for (i, mname) in ic.method_names.iter().enumerate() {
            // Default to 0 if the source side hasn't populated method_param_counts
            // yet (legacy ImportedClass with no parallel Vec). 0 means "no padding".
            let count = ic.method_param_counts.get(i).copied().unwrap_or(0);
            // Register under the canonical class name and the local alias if any.
            method_param_counts.insert((ic.name.clone(), mname.clone()), count);
            if effective_name != ic.name {
                method_param_counts.insert((effective_name.clone(), mname.clone()), count);
            }
            // Issue #672: same propagation for the rest-flag side. Without this,
            // call sites to imported-class methods with `...rest` parameters
            // skipped the rest-array packing path, leaving trailing positional
            // args either dropped or silently spread into the next slot —
            // `c.cmd("SET", "k", "v")` reached the callee as `args = "k"`.
            if ic.method_has_rest.get(i).copied().unwrap_or(false) {
                method_has_rest.insert((ic.name.clone(), mname.clone()), true);
                if effective_name != ic.name {
                    method_has_rest.insert((effective_name.clone(), mname.clone()), true);
                }
            }
        }
    }

    // Refs #915 (gap 3 / #321 follow-up): tag functions whose body
    // unconditionally returns a `ClassRef` (or transitively returns
    // another such factory) so call sites of the form
    // `Literal(value).pipe(...)` can dispatch the `.pipe` lookup as a
    // static-method call on the returned class. Iterate until
    // fixed-point so `Literal(value)` (which calls `makeLiteralClass`)
    // resolves to the same class as `makeLiteralClass(...)`.
    let mut func_returns_class_map: std::collections::HashMap<u32, String> =
        std::collections::HashMap::new();
    let n_funcs_for_factory_pass = hir.functions.len();
    for _ in 0..n_funcs_for_factory_pass {
        let mut changed = false;
        for f in &hir.functions {
            if func_returns_class_map.contains_key(&f.id) {
                continue;
            }
            let mut produced: Option<String> = None;
            let mut disqualified = false;
            collect_return_class(
                &f.body,
                &mut produced,
                &mut disqualified,
                &func_returns_class_map,
            );
            if !disqualified {
                if let Some(class_name) = produced {
                    func_returns_class_map.insert(f.id, class_name);
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
    }

    // Build the cross-module context bundle from CompileOptions.
    let disable_buffer_fast_path = opts.disable_buffer_fast_path
        || std::env::var("PERRY_DISABLE_BUFFER_FAST_PATH")
            .ok()
            .as_deref()
            == Some("1");
    let mut typed_clone_rejection_records = Vec::new();
    let mut typed_f64_functions = std::collections::HashSet::new();
    let mut typed_i32_functions = std::collections::HashSet::new();
    let mut typed_i1_functions = std::collections::HashSet::new();
    let mut typed_string_functions = std::collections::HashSet::new();
    let mut typed_i1_function_param_reps = std::collections::HashMap::new();
    for f in &hir.functions {
        match typed_abi::typed_f64_function_rejection_reason(f) {
            None => {
                typed_f64_functions.insert(f.id);
                if let Some(reps) = typed_abi::typed_param_reps_for_params(&f.params) {
                    typed_i1_function_param_reps.insert(f.id, reps);
                }
            }
            Some(reason) => record_typed_clone_rejection(
                &mut typed_clone_rejection_records,
                f.name.clone(),
                "typed_f64_function_clone_decision",
                reason,
                vec![
                    "typed_clone_kind=typed_f64_function".to_string(),
                    format!("function_id={}", f.id),
                    format!("symbol={}", f.name),
                ],
            ),
        }
        match typed_abi::typed_i32_function_rejection_reason(f) {
            None => {
                typed_i32_functions.insert(f.id);
                if let Some(reps) = typed_abi::typed_param_reps_for_params(&f.params) {
                    typed_i1_function_param_reps.insert(f.id, reps);
                }
            }
            Some(reason) => record_typed_clone_rejection(
                &mut typed_clone_rejection_records,
                f.name.clone(),
                "typed_i32_function_clone_decision",
                reason,
                vec![
                    "typed_clone_kind=typed_i32_function".to_string(),
                    format!("function_id={}", f.id),
                    format!("symbol={}", f.name),
                ],
            ),
        }
        match typed_abi::typed_i1_function_rejection_reason(f) {
            None => {
                typed_i1_functions.insert(f.id);
                if let Some(reps) = typed_abi::typed_param_reps_for_params(&f.params) {
                    typed_i1_function_param_reps.insert(f.id, reps);
                }
            }
            Some(reason) => record_typed_clone_rejection(
                &mut typed_clone_rejection_records,
                f.name.clone(),
                "typed_i1_function_clone_decision",
                reason,
                vec![
                    "typed_clone_kind=typed_i1_function".to_string(),
                    format!("function_id={}", f.id),
                    format!("symbol={}", f.name),
                ],
            ),
        }
        match typed_abi::typed_string_function_rejection_reason(f) {
            None => {
                typed_string_functions.insert(f.id);
                if let Some(reps) = typed_abi::typed_param_reps_for_params(&f.params) {
                    typed_i1_function_param_reps.insert(f.id, reps);
                }
            }
            Some(reason) => record_typed_clone_rejection(
                &mut typed_clone_rejection_records,
                f.name.clone(),
                "typed_string_function_clone_decision",
                reason,
                vec![
                    "typed_clone_kind=typed_string_function".to_string(),
                    format!("function_id={}", f.id),
                    format!("symbol={}", f.name),
                ],
            ),
        }
    }
    let mut typed_f64_methods = std::collections::HashSet::new();
    let mut typed_i32_methods = std::collections::HashSet::new();
    let mut typed_i1_methods = std::collections::HashSet::new();
    let mut typed_string_methods = std::collections::HashSet::new();
    let mut typed_i1_method_param_reps = std::collections::HashMap::new();
    let mut typed_f64_receiver_methods = std::collections::HashMap::new();
    for class in &hir.classes {
        for method in &class.methods {
            let source_function = format!("{}::{}", class.name, method.name);
            match typed_abi::typed_f64_method_rejection_reason(method) {
                None => {
                    let key = (class.name.clone(), method.name.clone());
                    typed_f64_methods.insert(key.clone());
                    if let Some(reps) = typed_abi::typed_param_reps_for_params(&method.params) {
                        typed_i1_method_param_reps.insert(key, reps);
                    }
                }
                Some(reason) => record_typed_clone_rejection(
                    &mut typed_clone_rejection_records,
                    source_function.clone(),
                    "typed_f64_method_clone_decision",
                    reason,
                    vec![
                        "typed_clone_kind=typed_f64_method".to_string(),
                        format!("class={}", class.name),
                        format!("method={}", method.name),
                        format!("function_id={}", method.id),
                    ],
                ),
            }
            match typed_abi::typed_f64_receiver_method_info(class, method) {
                Some(info) => {
                    typed_f64_receiver_methods
                        .insert((class.name.clone(), method.name.clone()), info);
                }
                None => {
                    if let Some(reason) =
                        typed_abi::typed_f64_receiver_method_rejection_reason(class, method)
                    {
                        record_typed_clone_rejection(
                            &mut typed_clone_rejection_records,
                            source_function.clone(),
                            "typed_f64_receiver_method_clone_decision",
                            reason,
                            vec![
                                "typed_clone_kind=typed_f64_receiver_method".to_string(),
                                format!("class={}", class.name),
                                format!("method={}", method.name),
                                format!("function_id={}", method.id),
                            ],
                        );
                    }
                }
            }
            match typed_abi::typed_i1_method_rejection_reason(method) {
                None => {
                    let key = (class.name.clone(), method.name.clone());
                    typed_i1_methods.insert(key.clone());
                    if let Some(reps) = typed_abi::typed_param_reps_for_params(&method.params) {
                        typed_i1_method_param_reps.insert(key, reps);
                    }
                }
                Some(reason) => record_typed_clone_rejection(
                    &mut typed_clone_rejection_records,
                    source_function.clone(),
                    "typed_i1_method_clone_decision",
                    reason,
                    vec![
                        "typed_clone_kind=typed_i1_method".to_string(),
                        format!("class={}", class.name),
                        format!("method={}", method.name),
                        format!("function_id={}", method.id),
                    ],
                ),
            }
            match typed_abi::typed_i32_method_rejection_reason(method) {
                None => {
                    let key = (class.name.clone(), method.name.clone());
                    typed_i32_methods.insert(key.clone());
                    if let Some(reps) = typed_abi::typed_param_reps_for_params(&method.params) {
                        typed_i1_method_param_reps.insert(key, reps);
                    }
                }
                Some(reason) => record_typed_clone_rejection(
                    &mut typed_clone_rejection_records,
                    source_function.clone(),
                    "typed_i32_method_clone_decision",
                    reason,
                    vec![
                        "typed_clone_kind=typed_i32_method".to_string(),
                        format!("class={}", class.name),
                        format!("method={}", method.name),
                        format!("function_id={}", method.id),
                    ],
                ),
            }
            match typed_abi::typed_string_method_rejection_reason(method) {
                None => {
                    let key = (class.name.clone(), method.name.clone());
                    typed_string_methods.insert(key.clone());
                    if let Some(reps) = typed_abi::typed_param_reps_for_params(&method.params) {
                        typed_i1_method_param_reps.insert(key, reps);
                    }
                }
                Some(reason) => record_typed_clone_rejection(
                    &mut typed_clone_rejection_records,
                    source_function.clone(),
                    "typed_string_method_clone_decision",
                    reason,
                    vec![
                        "typed_clone_kind=typed_string_method".to_string(),
                        format!("class={}", class.name),
                        format!("method={}", method.name),
                        format!("function_id={}", method.id),
                    ],
                ),
            }
        }
    }
    let mut compiler_private_async_i32_control_locals = std::collections::HashSet::new();
    let mut compiler_private_async_i1_control_locals = std::collections::HashSet::new();
    crate::boxed_vars::collect_compiler_private_async_control_locals_in_stmts(
        &hir.init,
        &mut compiler_private_async_i32_control_locals,
        &mut compiler_private_async_i1_control_locals,
    );
    for f in &hir.functions {
        crate::boxed_vars::collect_compiler_private_async_control_locals_in_stmts(
            &f.body,
            &mut compiler_private_async_i32_control_locals,
            &mut compiler_private_async_i1_control_locals,
        );
    }
    for c in &hir.classes {
        for m in &c.methods {
            crate::boxed_vars::collect_compiler_private_async_control_locals_in_stmts(
                &m.body,
                &mut compiler_private_async_i32_control_locals,
                &mut compiler_private_async_i1_control_locals,
            );
        }
        for (_, getter_fn) in &c.getters {
            crate::boxed_vars::collect_compiler_private_async_control_locals_in_stmts(
                &getter_fn.body,
                &mut compiler_private_async_i32_control_locals,
                &mut compiler_private_async_i1_control_locals,
            );
        }
        for (_, setter_fn) in &c.setters {
            crate::boxed_vars::collect_compiler_private_async_control_locals_in_stmts(
                &setter_fn.body,
                &mut compiler_private_async_i32_control_locals,
                &mut compiler_private_async_i1_control_locals,
            );
        }
        if let Some(ctor) = &c.constructor {
            crate::boxed_vars::collect_compiler_private_async_control_locals_in_stmts(
                &ctor.body,
                &mut compiler_private_async_i32_control_locals,
                &mut compiler_private_async_i1_control_locals,
            );
        }
        for sm in &c.static_methods {
            crate::boxed_vars::collect_compiler_private_async_control_locals_in_stmts(
                &sm.body,
                &mut compiler_private_async_i32_control_locals,
                &mut compiler_private_async_i1_control_locals,
            );
        }
        for member in &c.computed_members {
            crate::boxed_vars::collect_compiler_private_async_control_locals_in_stmts(
                &member.function.body,
                &mut compiler_private_async_i32_control_locals,
                &mut compiler_private_async_i1_control_locals,
            );
        }
    }

    let mut cross_module = CrossModuleCtx {
        namespace_imports: opts.namespace_imports.iter().cloned().collect(),
        namespace_member_prefixes: opts.namespace_member_prefixes,
        namespace_member_origin_names: opts.namespace_member_origin_names,
        imported_async_funcs: opts.imported_async_funcs,
        local_async_funcs,
        local_generator_funcs,
        async_step_closures: hir.async_step_closures.iter().copied().collect(),
        funcs_reading_dynamic_this,
        type_aliases: opts.type_aliases,
        imported_func_param_counts: opts.imported_func_param_counts,
        import_function_origin_names: opts.import_function_origin_names.clone(),
        import_function_v8_specifiers: opts.import_function_v8_specifiers.clone(),
        // Issue #841: see CrossModuleCtx field docs.
        import_function_node_submodule: opts.import_function_node_submodule.clone(),
        namespace_node_submodules: opts.namespace_node_submodules.clone(),
        namespace_v8_specifiers: opts.namespace_v8_specifiers.clone(),
        imported_func_has_rest: opts.imported_func_has_rest,
        imported_func_synthetic_arguments: opts.imported_func_synthetic_arguments,
        imported_func_return_types: opts.imported_func_return_types,
        func_returns_class: func_returns_class_map,
        method_param_counts,
        method_has_rest,
        class_keys_globals: class_keys_globals_map,
        class_field_counts: class_field_counts_map,
        class_init_chains: class_init_chains_map,
        imported_class_ctors: opts
            .imported_classes
            .iter()
            .map(|ic| {
                let effective_name = ic.local_alias.as_deref().unwrap_or(&ic.name);
                let ctor_name = format!("{}__{}_constructor", ic.source_prefix, ic.name);
                (
                    effective_name.to_string(),
                    ImportedCtor {
                        symbol: ctor_name,
                        param_count: ic.constructor_param_count,
                        has_own_constructor: ic.has_own_constructor,
                        has_instance_fields: ic.has_instance_fields,
                        has_rest: ic.constructor_has_rest,
                    },
                )
            })
            .collect(),
        // Per-module i18n lowering context. Built from `opts.i18n_table`
        // when i18n is configured; `None` otherwise. The
        // `Expr::I18nString` lowering pulls the right translation row at
        // compile time using `default_locale_idx` and emits the resolved
        // string (with runtime interpolation for `{name}` placeholders).
        i18n: opts.i18n_table.as_ref().map(|arc| {
            // Tier 4.6: deref the `Arc<Tuple>` to access the inner
            // tuple fields. The `translations.clone()` here is still a
            // per-module Vec clone — wrapping the I18nLowerCtx field
            // in Arc too would eliminate it, but is a wider refactor
            // tracked as a follow-up.
            let (translations, key_count, _locale_count, locale_codes, default_locale_idx) =
                arc.as_ref();
            crate::expr::I18nLowerCtx {
                translations: translations.clone(),
                key_count: *key_count,
                default_locale_idx: *default_locale_idx,
                locale_codes: locale_codes.clone(),
            }
        }),
        imported_vars: opts.imported_vars,
        needs_stdlib: opts.needs_stdlib,
        needs_geisterhand: opts.needs_geisterhand,
        geisterhand_port: opts.geisterhand_port,
        compile_time_constants,
        target_triple: triple.clone(),
        app_metadata: opts.app_metadata.clone(),
        module_dispatch: crate::collectors::collect_module_dispatch_facts(hir),
        clamp3_functions: hir
            .functions
            .iter()
            .filter_map(|f| crate::collectors::detect_clamp3(f).map(|_| f.id))
            .collect(),
        clamp_u8_functions: hir
            .functions
            .iter()
            .filter(|f| crate::collectors::detect_clamp_u8(f))
            .map(|f| f.id)
            .collect(),
        returns_int_functions: hir
            .functions
            .iter()
            .filter(|f| crate::collectors::returns_integer(f))
            .map(|f| f.id)
            .collect(),
        i32_identity_functions: hir
            .functions
            .iter()
            .filter(|f| crate::collectors::returns_i32_identity_arg(f))
            .map(|f| f.id)
            .collect(),
        typed_f64_functions,
        typed_i32_functions,
        typed_i1_functions,
        typed_string_functions,
        typed_i1_function_param_reps,
        typed_f64_methods,
        typed_i32_methods,
        typed_i1_methods,
        typed_string_methods,
        typed_i1_method_param_reps,
        typed_f64_receiver_methods,
        typed_f64_closures: std::collections::HashSet::new(),
        typed_i32_closures: std::collections::HashSet::new(),
        typed_i1_closures: std::collections::HashSet::new(),
        typed_string_closures: std::collections::HashSet::new(),
        typed_string_closure_capture_counts: std::collections::HashMap::new(),
        typed_i1_closure_param_reps: std::collections::HashMap::new(),
        compiler_private_async_i32_control_locals,
        compiler_private_async_i1_control_locals,
        disable_buffer_fast_path,
        program_shadows_buffer_read_method:
            crate::lower_call::buffer_intrinsic::module_shadows_buffer_read_method(hir),
        flat_const_arrays: {
            // Issue #50: fold module-level `const X: number[][] = [[int, ...], ...]`
            // into a flat `[N x i32]` LLVM constant so `X[i][j]` / `krow[j]` can
            // load directly from `.rodata` instead of chasing the arena array
            // header. Qualifying locals are `Let { mutable: false }`, have a
            // rectangular int-literal 2D init, and are never mutated anywhere
            // in the module (LocalSet/Update/IndexSet/mutating methods).
            let mut map: std::collections::HashMap<u32, crate::expr::FlatConstInfo> =
                std::collections::HashMap::new();
            for s in &hir.init {
                if let perry_hir::Stmt::Let {
                    id,
                    init: Some(init),
                    mutable: false,
                    ..
                } = s
                {
                    if let Some((rows, cols, vals)) = crate::expr::try_flat_const_2d_int(init) {
                        let mut mutated = false;
                        if crate::collectors::has_any_mutation(&hir.init, *id) {
                            mutated = true;
                        }
                        if !mutated {
                            for f in &hir.functions {
                                if crate::collectors::has_any_mutation(&f.body, *id) {
                                    mutated = true;
                                    break;
                                }
                            }
                        }
                        if !mutated {
                            'outer: for c in &hir.classes {
                                for m in &c.methods {
                                    if crate::collectors::has_any_mutation(&m.body, *id) {
                                        mutated = true;
                                        break 'outer;
                                    }
                                }
                                if let Some(ctor) = &c.constructor {
                                    if crate::collectors::has_any_mutation(&ctor.body, *id) {
                                        mutated = true;
                                        break;
                                    }
                                }
                            }
                        }
                        if !mutated {
                            let gname = format!("perry_flat_{}__{}", module_prefix, id);
                            let init_str = format!(
                                "[{}]",
                                vals.iter()
                                    .map(|v| format!("i32 {}", v))
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            );
                            let ty = format!("[{} x i32]", rows * cols);
                            llmod.add_raw_global(format!(
                                "@{} = private unnamed_addr constant {} {}",
                                gname, ty, init_str
                            ));
                            map.insert(
                                *id,
                                crate::expr::FlatConstInfo {
                                    global_name: gname,
                                    rows,
                                    cols,
                                },
                            );
                        }
                    }
                }
            }
            map
        },
        // FFI manifest: each `native_library_functions` entry is a typed
        // native ABI signature from package.json `nativeLibrary.functions`.
        // Build a name → (params, returns) map so `lower_call` can emit the
        // correct LLVM signature for direct calls to native C/Rust functions
        // (matters when the C ABI differs from Perry's all-double default —
        // e.g. `*mut View` returns in `x0`, not `d0`).
        ffi_signatures: opts
            .native_library_functions
            .iter()
            .map(|(name, params, ret)| (name.clone(), (params.clone(), ret.clone())))
            .collect(),
        // Issue #5621: ergonomic camelCase binding → manifest symbol, so
        // `lower_call` can route a camelCase native-library export
        // (`requestAdapter`) to its real FFI symbol
        // (`js_webgpu_request_adapter`).
        ffi_aliases: opts.import_function_ffi_aliases.clone(),
        // Per-module local-name → import-source map. Walks `hir.imports`
        // and records every named/default import binding's source spec.
        // `lower_builtin_new` consults this to gate ambiguously-named
        // built-in arms (Client / Pool / Database / Redis / MongoClient /
        // Decimal) on the import source — `import Client from
        // "better-sqlite3"` should not dispatch through pg's Client arm.
        // See issue #602.
        imported_class_sources: {
            let mut map: std::collections::HashMap<String, String> =
                std::collections::HashMap::new();
            for import in &hir.imports {
                for spec in &import.specifiers {
                    match spec {
                        perry_hir::ImportSpecifier::Named { local, .. }
                        | perry_hir::ImportSpecifier::Default { local } => {
                            map.insert(local.clone(), import.source.clone());
                        }
                        perry_hir::ImportSpecifier::Namespace { .. } => {}
                    }
                }
            }
            map
        },
        // Per-module alias → original imported export name. Only renamed named
        // imports (`local != imported`) are recorded; this lets `lower_new`
        // recover the canonical built-in constructor name when a bundle aliases
        // the import (e.g. `import { AsyncLocalStorage as xQ5 }`). See the
        // field doc on `CompileOptions::imported_class_original_names`.
        imported_class_original_names: {
            let mut map: std::collections::HashMap<String, String> =
                std::collections::HashMap::new();
            for import in &hir.imports {
                for spec in &import.specifiers {
                    if let perry_hir::ImportSpecifier::Named { imported, local } = spec {
                        if local != imported {
                            map.insert(local.clone(), imported.clone());
                        }
                    }
                }
            }
            map
        },
        interfaces: hir
            .interfaces
            .iter()
            .map(|i| (i.name.clone(), i.clone()))
            .collect(),
        namespace_entries: opts.namespace_entries.clone(),
        dynamic_import_path_to_prefix: opts.dynamic_import_path_to_prefix.clone(),
        nextjs_path_init_modules: opts.nextjs_path_init_modules.clone(),
        deferred_module_prefixes: opts.deferred_module_prefixes.clone(),
        module_init_deps: opts.module_init_deps.clone(),
        is_dynamic_import_target: opts.is_dynamic_import_target,
    };

    let module_globals_emit::ModuleGlobals {
        module_globals,
        module_global_types,
        static_field_globals,
    } = module_globals_emit::emit_module_globals(
        &mut llmod,
        hir,
        &opts.imported_classes,
        &cross_module.compile_time_constants,
        &module_prefix,
    );

    // Method registry + cross-module method/getter/setter/ctor/static
    // extern declares. See `method_registry::build_method_names`.
    let method_names = method_registry::build_method_names(
        &mut llmod,
        hir,
        &opts.imported_classes,
        &class_table,
        &class_ids,
        &imported_class_prefix,
        &imported_class_source_name,
        &module_prefix,
    );

    // Resolve user function names + signatures up front. See
    // `func_registry::build_func_registry`.
    let func_registry::FuncRegistry {
        func_names,
        func_signatures,
        func_synthetic_arguments,
    } = func_registry::build_func_registry(hir, &module_prefix);

    // Module-wide boxed-var union + LocalId→Type map. See `boxed_locals`.
    let module_boxed_vars = boxed_locals::collect_module_boxed_vars(hir);
    // #6369: the *receiver-type oracle* for closure bodies — every module-wide
    // `Stmt::Let` type, with NO representation-driven filtering. `FnCtx.
    // local_types` is what `static_type_of` / `is_array_expr` /
    // `receiver_class_name` read to pick a specialized (guarded) access path,
    // and a binding's declared type is a fact about its VALUE — it holds no
    // matter whether the slot backing it is a plain alloca, a box cell, or a
    // module global (every read routes through the matching load, and every
    // specialized path is a runtime-guarded fast path with the generic
    // fallback intact). `compile_function` / `compile_method` already seed
    // `local_types` from the unfiltered `module_global_types`; closures are
    // the outlier, and the two filters below (both aimed squarely at the
    // typed-ABI *capture representation*) were silently dropping the type of
    // every captured binding from the closure oracle too — so a captured
    // `number[]` reached `arr[i]` as an unknown receiver and fell all the way
    // to `js_dyn_index_get` (27× slower than the same array passed as a
    // parameter, and no faster than an untyped array).
    let module_receiver_types = boxed_locals::collect_module_local_types(hir);
    let mut module_local_types = module_receiver_types.clone();
    // #5869 residual: a BOXED local's slot holds a BOX POINTER, never the
    // typed value — advertising its declared type to the typed-ABI layer
    // made the typed closure specializations (typed_f64/i1/i32/string
    // capture reps) read the capture RAW while the generic variant
    // box_get's, and the dispatcher picked the typed body: the call
    // returned box-pointer bits as a denormal number. Observable repro:
    //   let n = 0; let get: any = null;
    //   tag: { get = () => n; break tag; }
    //   n = 42; get()          // → 2.58e-311 instead of 42
    // (#5871's Labeled-descent fix EXPOSED this — the type became visible
    // for closures inside labeled blocks.) Removing boxed ids here
    // disqualifies every type-directed unboxed access on a boxed slot in
    // one place; consumers fall back to the generic (box-aware) paths.
    //
    // #6369: scoped to the typed-ABI copy (like the module-globals filter
    // below). The hazard is the unboxed *capture representation*, not the
    // receiver type — see `module_receiver_types` above.
    module_local_types.retain(|id, _| !module_boxed_vars.contains(id));
    // #5982 (#5466 regression): a MODULE-GLOBAL captured local is read by a
    // closure through `@perry_global_*`, NOT the closure's capture array —
    // `closure.rs` filters module globals OUT of `closure_captures`, so the
    // closure is `alloc_singleton` with no capture slots. But advertising the
    // local's type to the typed-ABI closure specialization
    // (`__typed_f64`/i32/…) made it read `js_closure_get_capture_bits(this,
    // 0)` — an UNSET slot (0) — while the generic variant correctly loads the
    // global; the dispatcher picked the typed body, so every closure returned
    // 0. Repro (bisected to #5466 representation lowering):
    //   for (let i=0;i<5;i++){ const c=i; fns.push(()=>c); }  // → 0,0,0,0,0
    // A module-global capture has no capture-slot representation, so — like a
    // boxed slot — it must not feed the type-directed unboxed *capture* path.
    //
    // #6039 originally stripped these ids from `module_local_types` outright,
    // but that map is ALSO the receiver-type oracle for every function body
    // (`FnCtx.local_types` → `static_type_of` / `is_array_expr`). Dropping a
    // module-global's declared type there mis-classified a captured array
    // receiver as untyped inside a closure, so `arr.every()` (undefined
    // callbackfn) fell to the generic dynamic dispatch that skips the
    // `js_validate_array_callback` throw — the test262 harness's
    // `assert.throws(TypeError, () => arr.every())` then saw no exception
    // (24 regressions: Array HOF + symbol-strict [[Set]], all via harness
    // closures). Only the typed-ABI *specialization* decision needs the
    // module-globals removed, so scope the filter to a dedicated copy — the
    // receiver oracle is `module_receiver_types` (#6369).
    let typed_abi_local_types: std::collections::HashMap<u32, perry_types::Type> =
        module_local_types
            .iter()
            .filter(|(id, _)| !module_globals.contains_key(id))
            .map(|(id, ty)| (*id, ty.clone()))
            .collect();

    // Cross-module function declares are emitted lazily by `lower_call`
    // via `FnCtx.pending_declares` (drained back into `llmod` at the
    // end of each compile_function/closure/method/static call). The
    // previous pre-walker (`collect_extern_func_refs_in_*`) had to
    // mirror the entire HIR Expr/Stmt grammar to find every cross-module
    // call shape — it missed `Expr::Closure` bodies, `Stmt::Try`/`Switch`,
    // and many other containers, which produced clang
    // "use of undefined value @perry_fn_*" errors when a call was hidden
    // inside an arrow callback. Lazy emission tracks declares at the
    // actual emission point so any path the lowering reaches is covered.

    // Closure collection + derived per-closure dispatch maps. See
    // `closure_collect::collect_module_closures`.
    let closure_collect::ModuleClosures {
        closures,
        closure_rest_params,
        closure_synthetic_arguments,
        closure_rest_and_arguments,
        closure_arities,
        closure_lengths,
        closure_arrow_functions,
    } = closure_collect::collect_module_closures(hir);

    cross_module.typed_f64_closures.clear();
    cross_module.typed_i32_closures.clear();
    cross_module.typed_i1_closures.clear();
    cross_module.typed_string_closures.clear();
    cross_module.typed_string_closure_capture_counts.clear();
    cross_module.typed_i1_closure_param_reps.clear();
    for (func_id, expr) in &closures {
        match typed_abi::typed_f64_closure_rejection_reason_with_types(expr, &typed_abi_local_types)
        {
            None => {
                cross_module.typed_f64_closures.insert(*func_id);
                if let perry_hir::Expr::Closure { params, .. } = expr {
                    if let Some(reps) = typed_abi::typed_param_reps_for_params(params) {
                        cross_module
                            .typed_i1_closure_param_reps
                            .insert(*func_id, reps);
                    }
                }
            }
            Some(reason) => record_typed_clone_rejection(
                &mut typed_clone_rejection_records,
                format!("closure#{func_id}"),
                "typed_f64_closure_clone_decision",
                reason,
                vec![
                    "typed_clone_kind=typed_f64_closure".to_string(),
                    format!("closure_func_id={func_id}"),
                    format!(
                        "symbol={}",
                        typed_f64_closure_name(&format!(
                            "perry_closure_{}__{}",
                            module_prefix, func_id
                        ))
                    ),
                ],
            ),
        }
        match typed_abi::typed_i1_closure_rejection_reason_with_types(expr, &typed_abi_local_types)
        {
            None => {
                cross_module.typed_i1_closures.insert(*func_id);
                if let perry_hir::Expr::Closure { params, .. } = expr {
                    if let Some(reps) = typed_abi::typed_param_reps_for_params(params) {
                        cross_module
                            .typed_i1_closure_param_reps
                            .insert(*func_id, reps);
                    }
                }
            }
            Some(reason) => record_typed_clone_rejection(
                &mut typed_clone_rejection_records,
                format!("closure#{func_id}"),
                "typed_i1_closure_clone_decision",
                reason,
                vec![
                    "typed_clone_kind=typed_i1_closure".to_string(),
                    format!("closure_func_id={func_id}"),
                    format!(
                        "symbol={}",
                        typed_i1_closure_name(&format!(
                            "perry_closure_{}__{}",
                            module_prefix, func_id
                        ))
                    ),
                ],
            ),
        }
        match typed_abi::typed_i32_closure_rejection_reason_with_types(expr, &typed_abi_local_types)
        {
            None => {
                cross_module.typed_i32_closures.insert(*func_id);
                if let perry_hir::Expr::Closure { params, .. } = expr {
                    if let Some(reps) = typed_abi::typed_param_reps_for_params(params) {
                        cross_module
                            .typed_i1_closure_param_reps
                            .insert(*func_id, reps);
                    }
                }
            }
            Some(reason) => record_typed_clone_rejection(
                &mut typed_clone_rejection_records,
                format!("closure#{func_id}"),
                "typed_i32_closure_clone_decision",
                reason,
                vec![
                    "typed_clone_kind=typed_i32_closure".to_string(),
                    format!("closure_func_id={func_id}"),
                    format!(
                        "symbol={}",
                        typed_i32_closure_name(&format!(
                            "perry_closure_{}__{}",
                            module_prefix, func_id
                        ))
                    ),
                ],
            ),
        }
        match typed_abi::typed_string_closure_rejection_reason_with_types(
            expr,
            &typed_abi_local_types,
        ) {
            None => {
                cross_module.typed_string_closures.insert(*func_id);
                if let perry_hir::Expr::Closure { params, .. } = expr {
                    if let Some(reps) = typed_abi::typed_param_reps_for_params(params) {
                        cross_module
                            .typed_i1_closure_param_reps
                            .insert(*func_id, reps);
                    }
                }
                let capture_count =
                    typed_abi::typed_string_closure_capture_reps(expr, &typed_abi_local_types)
                        .map(|captures| captures.len())
                        .unwrap_or(0);
                cross_module
                    .typed_string_closure_capture_counts
                    .insert(*func_id, capture_count);
            }
            Some(reason) => record_typed_clone_rejection(
                &mut typed_clone_rejection_records,
                format!("closure#{func_id}"),
                "typed_string_closure_clone_decision",
                reason,
                vec![
                    "typed_clone_kind=typed_string_closure".to_string(),
                    format!("closure_func_id={func_id}"),
                    format!(
                        "symbol={}",
                        typed_string_closure_name(&format!(
                            "perry_closure_{}__{}",
                            module_prefix, func_id
                        ))
                    ),
                ],
            ),
        }
    }

    // Integer-specialization pass. See `i64_spec::emit_i64_specializations`.
    let i64_specialized =
        i64_spec::emit_i64_specializations(&mut llmod, hir, &func_names, &module_globals);

    // From here on, this set means "a typed-f64 clone is present in the
    // module", not just "the HIR body was eligible." The i64 specializer owns
    // its public wrapper and may skip the ordinary f64 body entirely, so direct
    // call lowering must not branch to an unemitted typed-f64 clone.
    for f in &hir.functions {
        if i64_specialized.contains(&f.id) && cross_module.typed_f64_functions.contains(&f.id) {
            record_typed_clone_rejection(
                &mut typed_clone_rejection_records,
                f.name.clone(),
                "typed_f64_function_clone_decision",
                typed_abi::TypedCloneRejectionReason::I64Specialized,
                vec![
                    "typed_clone_kind=typed_f64_function".to_string(),
                    format!("function_id={}", f.id),
                    format!(
                        "symbol={}",
                        func_names.get(&f.id).map(String::as_str).unwrap_or(&f.name)
                    ),
                ],
            );
        }
        if i64_specialized.contains(&f.id) && cross_module.typed_i32_functions.contains(&f.id) {
            record_typed_clone_rejection(
                &mut typed_clone_rejection_records,
                f.name.clone(),
                "typed_i32_function_clone_decision",
                typed_abi::TypedCloneRejectionReason::I64Specialized,
                vec![
                    "typed_clone_kind=typed_i32_function".to_string(),
                    format!("function_id={}", f.id),
                    format!(
                        "symbol={}",
                        func_names.get(&f.id).map(String::as_str).unwrap_or(&f.name)
                    ),
                ],
            );
        }
        if i64_specialized.contains(&f.id) && cross_module.typed_i1_functions.contains(&f.id) {
            record_typed_clone_rejection(
                &mut typed_clone_rejection_records,
                f.name.clone(),
                "typed_i1_function_clone_decision",
                typed_abi::TypedCloneRejectionReason::I64Specialized,
                vec![
                    "typed_clone_kind=typed_i1_function".to_string(),
                    format!("function_id={}", f.id),
                    format!(
                        "symbol={}",
                        func_names.get(&f.id).map(String::as_str).unwrap_or(&f.name)
                    ),
                ],
            );
        }
    }
    cross_module
        .typed_f64_functions
        .retain(|id| !i64_specialized.contains(id));
    cross_module
        .typed_i32_functions
        .retain(|id| !i64_specialized.contains(id));
    cross_module
        .typed_i1_functions
        .retain(|id| !i64_specialized.contains(id));
    cross_module
        .typed_i1_function_param_reps
        .retain(|id, _| !i64_specialized.contains(id));

    // Emit internal typed-f64 clones before their public/generic wrappers. The
    // public wrapper keeps the JSValue ABI; it and direct proven numeric call
    // sites can call the internal clone.
    for f in &hir.functions {
        if !cross_module.typed_f64_functions.contains(&f.id) {
            continue;
        }
        compile_typed_f64_function(&mut llmod, f, &func_names)
            .with_context(|| format!("lowering typed-f64 clone for function '{}'", f.name))?;
    }

    // Emit internal typed-i32 clones before their public/generic wrappers. The
    // public wrapper keeps the JSValue ABI; it and direct proven Int32 call
    // sites guard and unbox into this clone, then re-box at the ABI boundary.
    for f in &hir.functions {
        if !cross_module.typed_i32_functions.contains(&f.id) {
            continue;
        }
        compile_typed_i32_function(&mut llmod, f, &func_names)
            .with_context(|| format!("lowering typed-i32 clone for function '{}'", f.name))?;
    }

    // Emit internal typed-i1 clones before their public/generic wrappers. The
    // public wrapper keeps the JSValue ABI; it and direct proven boolean call
    // sites guard and unbox into this clone, then re-box at the ABI boundary.
    for f in &hir.functions {
        if !cross_module.typed_i1_functions.contains(&f.id) {
            continue;
        }
        compile_typed_i1_function(&mut llmod, f, &func_names)
            .with_context(|| format!("lowering typed-i1 clone for function '{}'", f.name))?;
    }

    // Emit internal typed-string clones before their public/generic wrappers.
    // The clone keeps raw string handles in SSA and boxes only when returning
    // through the public JSValue ABI.
    for f in &hir.functions {
        if !cross_module.typed_string_functions.contains(&f.id) {
            continue;
        }
        compile_typed_string_function(&mut llmod, f, &func_names)
            .with_context(|| format!("lowering typed-string clone for function '{}'", f.name))?;
    }

    // Lower each user function into the module (skip i64-specialized ones).
    for f in &hir.functions {
        if i64_specialized.contains(&f.id) {
            continue;
        }
        let typed_public_trampoline = if cross_module.typed_f64_functions.contains(&f.id) {
            Some(typed_abi::TypedFunctionTrampolineKind::F64)
        } else if cross_module.typed_i32_functions.contains(&f.id) {
            Some(typed_abi::TypedFunctionTrampolineKind::I32)
        } else if cross_module.typed_i1_functions.contains(&f.id) {
            Some(typed_abi::TypedFunctionTrampolineKind::I1)
        } else if cross_module.typed_string_functions.contains(&f.id) {
            Some(typed_abi::TypedFunctionTrampolineKind::StringRef)
        } else {
            None
        };
        compile_function(
            &mut llmod,
            f,
            &func_names,
            &mut strings,
            &class_table,
            &method_names,
            &module_globals,
            &module_global_types,
            &opts.import_function_prefixes,
            &enum_table,
            &static_field_globals,
            &class_ids,
            &func_signatures,
            &func_synthetic_arguments,
            &module_boxed_vars,
            &closure_rest_params,
            &cross_module,
            typed_public_trampoline,
        )
        .with_context(|| format!("lowering function '{}'", f.name))?;
    }

    // Closes #460: emit forwarding wrappers for `export { local as exported }`
    // renames where the exported name differs from the function's local HIR
    // name. Without these, cross-module callers compute the callee symbol
    // from the *exported* name (`perry_fn_<src>__<exported>`) and link-fail
    // because the body was emitted under the *local* name. Bites contextual-
    // keyword renames the worst — Effect's `void_ as void`, `_async as async`,
    // `_await as await`, etc. all left link-undefined `_perry_fn_..._<keyword>`.
    {
        use std::collections::HashSet;
        let mut emitted_aliases: HashSet<String> = HashSet::new();
        let func_by_id: HashMap<u32, &perry_hir::Function> =
            hir.functions.iter().map(|f| (f.id, f)).collect();
        for (exported_name, func_id) in &hir.exported_functions {
            let Some(f) = func_by_id.get(func_id) else {
                continue;
            };
            // NOTE: do NOT early-skip when `f.name == exported_name`. The real
            // body is emitted under `scoped_fn_name` (the INJECTIVE
            // `sanitize_member`), but cross-module callers and the #461
            // undefined-stub / #836 verbatim-alias paths compute the symbol via
            // plain `sanitize`. For a non-plain name like `$constructor`
            // (`export function $constructor` in zod core, #5431) those two
            // manglings diverge — body at `perry_fn_<mod>__u__24constructor`,
            // callers at `perry_fn_<mod>___constructor` — even though local ==
            // exported. Without a forwarding alias the #461 loop below claims
            // `_constructor` with an undefined-returning stub and every
            // cross-module call resolves to it (function reference is fine,
            // every CALL returns `undefined`). The `alias_sym == target_sym`
            // check below is the correct guard: it skips the plain-name case
            // (where both manglings agree) while still emitting the alias when
            // they differ.
            let alias_sym = format!("perry_fn_{}__{}", module_prefix, sanitize(exported_name));
            let target_sym = match func_names.get(func_id) {
                Some(s) => s.clone(),
                None => continue,
            };
            if alias_sym == target_sym {
                continue;
            }
            // Guard against colliding with an already-emitted body symbol. Two
            // exports whose names sanitize to the same string (`$x` and `_x`)
            // would otherwise redefine the alias; the body of whichever is plain
            // already owns `alias_sym`, so skip rather than redefine.
            if llmod.has_function(&alias_sym) {
                continue;
            }
            if !emitted_aliases.insert(alias_sym.clone()) {
                continue;
            }
            let param_count = f.params.len();
            let wrap_params: Vec<(LlvmType, String)> = (0..param_count)
                .map(|i| (DOUBLE, format!("%a{}", i)))
                .collect();
            let wf = llmod.define_function(&alias_sym, DOUBLE, wrap_params);
            let _ = wf.create_block("entry");
            let blk = wf.block_mut(0).unwrap();
            let arg_names: Vec<String> = (0..param_count).map(|i| format!("%a{}", i)).collect();
            let call_args: Vec<(LlvmType, &str)> =
                arg_names.iter().map(|s| (DOUBLE, s.as_str())).collect();
            let result = blk.call(DOUBLE, &target_sym, &call_args);
            blk.ret(DOUBLE, &result);
        }
    }

    // Closes #461: emit an undefined-returning stub for every named export
    // that doesn't already have a `perry_fn_<modprefix>__<exported>` symbol.
    // The cross-module call site resolves any namespace property access to
    // `perry_fn_<src>__<name>` (lower_call.rs::ExternFuncRef path) — that
    // works for value exports because either the function body itself or
    // the variable getter at line 1099 claims the symbol. It does NOT work
    // for:
    //   * exported classes — `export class Union` produces method/keys
    //     symbols but no function-shaped getter, so `AST.Union` from a
    //     consumer link-fails on `_perry_fn_<SchemaAST>__Union`;
    //   * exported interfaces / type aliases — `export interface Order`
    //     is type-only at runtime, but type annotations like
    //     `order.Order<...>` leak into the value-position symbol resolver
    //     and link-fail on `_perry_fn_<Order_ts>__Order`.
    // The stub returns NaN-boxed undefined; that matches the consumer-side
    // no-op wrapper at line 1955 (which already returns undefined for
    // imported classes referenced as values) so the link- and runtime-
    // visible behavior of cross-module class/type references is symmetric.
    {
        use std::collections::HashSet;
        let mut emitted_stubs: HashSet<String> = HashSet::new();
        let stub_targets: Vec<String> = hir
            .exports
            .iter()
            .filter_map(|e| match e {
                perry_hir::Export::Named { exported, .. } => Some(exported.clone()),
                _ => None,
            })
            .collect();
        for exported in stub_targets {
            let stub_sym = format!("perry_fn_{}__{}", module_prefix, sanitize(&exported));
            if llmod.has_function(&stub_sym) {
                continue;
            }
            if !emitted_stubs.insert(stub_sym.clone()) {
                continue;
            }
            let wf = llmod.define_function(&stub_sym, DOUBLE, vec![]);
            let _ = wf.create_block("entry");
            let blk = wf.block_mut(0).unwrap();
            let undef = crate::nanbox::double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
            blk.ret(DOUBLE, &undef);
        }
    }

    // ── End of compile_module prelude (data + initial emission). ──
    // The remainder (closures, methods, ctors, statics, function /
    // ExternFuncRef / export-rename / unknown-func / method
    // closure-call wrappers, namespace globals + extern declares,
    // entry-fn emission, string-pool init) lives in
    // `artifacts::emit_module_artifacts`. Behavior is unchanged —
    // see the doc on that fn for the split rationale.
    emit_module_artifacts(ModuleArtifactsCtx {
        llmod: &mut llmod,
        strings: &mut strings,
        hir,
        import_function_prefixes: &opts.import_function_prefixes,
        imported_classes: &opts.imported_classes,
        is_entry_module: opts.is_entry_module,
        non_entry_module_prefixes: &opts.non_entry_module_prefixes,
        output_type: &opts.output_type,
        module_prefix: &module_prefix,
        class_table: &class_table,
        class_ids: &class_ids,
        enum_table: &enum_table,
        module_globals: &module_globals,
        module_global_types: &module_global_types,
        static_field_globals: &static_field_globals,
        method_names: &method_names,
        func_names: &func_names,
        func_signatures: &func_signatures,
        func_synthetic_arguments: &func_synthetic_arguments,
        module_boxed_vars: &module_boxed_vars,
        module_local_types: &module_local_types,
        module_receiver_types: &module_receiver_types,
        closure_rest_params: &closure_rest_params,
        closure_synthetic_arguments: &closure_synthetic_arguments,
        closure_rest_and_arguments: &closure_rest_and_arguments,
        closure_arities: &closure_arities,
        closure_lengths: &closure_lengths,
        closure_arrow_functions: &closure_arrow_functions,
        closures: &closures,
        class_keys_init_data: &class_keys_init_data,
        imported_class_stubs: &imported_class_stubs,
        cross_module: &cross_module,
    })?;

    // Emit the buffer alias-scope metadata once per module, covering every
    // scope id allocated across compile_function / compile_closure /
    // compile_method / compile_static_method / compile_module_entry. Must
    // run AFTER all function compilation so the counter reflects the true
    // total — otherwise functions whose scope ids exceed the init
    // function's count emit `!alias.scope !N` references with no matching
    // metadata definition (issue #71).
    let total_buffer_scopes = llmod.buffer_alias_counter;
    emit_buffer_alias_metadata(&mut llmod, total_buffer_scopes);
    llmod
        .native_rep_records
        .extend(typed_clone_rejection_records);

    let verify_native_regions = opts.verify_native_regions
        || std::env::var("PERRY_VERIFY_NATIVE_REGIONS").ok().as_deref() == Some("1");
    if verify_native_regions {
        crate::native_value::verify_native_rep_records(&llmod.native_rep_records)?;
    }

    crate::native_value::write_native_rep_artifact_if_enabled(
        &hir.name,
        &llmod.native_rep_records,
    )?;

    // #5391 codegen units: large modules split their object compilation into N
    // independently-compiled units so clang's peak RSS stays ~whole/N instead of
    // OOMing on one giant TU. Gated to large modules (default 1 unit = unchanged
    // behavior). `emit_ir_only` and `PERRY_SAVE_LL` want the whole-module text,
    // so they take the single-text path; the split path avoids materializing the
    // full ~1GB IR string at all (which would defeat the memory win).
    let n_units = if opts.emit_ir_only {
        1
    } else {
        decide_codegen_units(module_callable_count(hir))
    };
    if n_units > 1 {
        let units = llmod.render_codegen_units(n_units);
        log::debug!(
            "perry-codegen: split '{}' into {} codegen units",
            hir.name,
            units.len()
        );
        return crate::linker::compile_units_to_object(&units, opts.target.as_deref());
    }

    let ll_text = llmod.to_ir();
    log::debug!(
        "perry-codegen: emitted {} bytes of LLVM IR for '{}' ({} interned strings)",
        ll_text.len(),
        hir.name,
        strings.len()
    );
    // Save .ll files when PERRY_SAVE_LL=<dir> is set
    if let Ok(save_dir) = std::env::var("PERRY_SAVE_LL") {
        let filename = format!("{}/{}.ll", save_dir, module_prefix);
        let _ = std::fs::write(&filename, &ll_text);
    }
    if opts.emit_ir_only {
        Ok(ll_text.into_bytes())
    } else {
        crate::linker::compile_ll_to_object(&ll_text, opts.target.as_deref())
    }
}
