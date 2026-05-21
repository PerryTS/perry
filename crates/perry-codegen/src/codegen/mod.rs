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
use crate::types::{LlvmType, DOUBLE, I64, VOID};

mod artifacts;
mod closure;
mod entry;
mod function;
mod helpers;
mod method;
mod opts;
mod string_pool;

pub use helpers::resolve_target_triple;
pub(crate) use helpers::{default_target_triple, write_barriers_enabled};
pub(crate) use opts::CrossModuleCtx;
pub use opts::{AppMetadata, CompileOptions, ImportedClass, NamespaceEntry, NamespaceEntryKind};

use artifacts::{emit_module_artifacts, ModuleArtifactsCtx};
use function::compile_function;
use helpers::{
    collect_return_class, emit_buffer_alias_metadata, sanitize, scoped_fn_name, scoped_method_name,
};

// Collector and boxing-analysis walkers live in dedicated modules.
use crate::boxed_vars::{collect_boxed_vars, collect_let_types_in_stmts};
use crate::collectors::{collect_closures_in_stmts, collect_let_ids, collect_ref_ids_in_stmts};

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
    // Set the per-instruction FMF emission mode for this build before
    // any LlBlock methods run. All modules in a single program build
    // share the same `fast_math` setting, so writing the AtomicBool
    // here (potentially redundantly when rayon parallelizes module
    // compiles) is safe — every store writes the same value.
    crate::block::FAST_MATH.store(opts.fast_math, std::sync::atomic::Ordering::Relaxed);

    let triple = opts.target.clone().unwrap_or_else(default_target_triple);

    let mut llmod = LlModule::new(&triple);
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

        // Build a stub Class with the minimum fields the codegen needs.
        // Most fields are empty — only name, extends_name, and methods
        // are consulted by dispatch.
        let stub = perry_hir::Class {
            id: 0, // imported — no local ClassId
            name: effective_name.to_string(),
            type_params: Vec::new(),
            extends: None,
            extends_name: ic.parent_name.clone(),
            native_extends: None,
            extends_expr: None,
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
                    was_plain_async: false,
                    was_unrolled: false,
                    is_exported: false,
                    captures: Vec::new(),
                    decorators: Vec::new(),
                })
                .collect(),
            getters: Vec::new(),
            setters: Vec::new(),
            static_fields: Vec::new(),
            static_methods: Vec::new(),
            decorators: Vec::new(),
            is_exported: false,
            aliases: Vec::new(),
        };
        imported_class_stubs.push(stub);
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
    for f in &hir.functions {
        // Include both truly-async functions and those transformed from
        // async to generator (was_plain_async=true, is_async=false after
        // the v0.5.371 async-to-generator pass) — both return Promises
        // so is_promise_expr must recognize their call sites.
        if f.is_async || f.was_plain_async {
            local_async_funcs.insert(f.id);
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
    // (global_name, packed_keys_string, total_field_count, pointer_mask_words).
    // Used by emit_string_pool to emit the build-call sequence.
    let mut class_keys_init_data: Vec<(String, String, u32, Vec<u64>)> = Vec::new();
    let mut class_keys_globals_map: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    for c in &hir.classes {
        let global_name = format!("perry_class_keys_{}__{}", module_prefix, sanitize(&c.name),);
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
        let mut parent_chain: Vec<String> = Vec::new();
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
        let lookup_class_chain_link =
            |name: &str| -> Option<(Vec<perry_hir::ClassField>, Option<String>)> {
                if let Some(parent) = hir.classes.iter().find(|cls| cls.name == name) {
                    return Some((parent.fields.clone(), parent.extends_name.clone()));
                }
                if let Some(stub) = imported_class_stubs.iter().find(|cls| cls.name == name) {
                    return Some((stub.fields.clone(), stub.extends_name.clone()));
                }
                None
            };
        let mut p = c.extends_name.clone();
        while let Some(parent_name) = p {
            if let Some((parent_fields, parent_extends)) = lookup_class_chain_link(&parent_name) {
                parent_chain.push(parent_name.clone());
                total_field_count += count_keyable(&parent_fields);
                p = parent_extends;
            } else {
                break;
            }
        }
        // Walk from deepest ancestor to direct parent.
        for parent_name in parent_chain.iter().rev() {
            if let Some((parent_fields, _)) = lookup_class_chain_link(parent_name) {
                for f in &parent_fields {
                    if f.key_expr.is_some() {
                        continue;
                    }
                    packed_keys.push_str(&f.name);
                    packed_keys.push('\0');
                }
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
        let (_typed_slot_count, typed_mask_words) =
            crate::typed_shape::class_typed_layout(&class_table, &c.name);
        class_keys_init_data.push((
            global_name,
            packed_keys,
            total_field_count,
            typed_mask_words,
        ));
    }
    // Same naming convention for IMPORTED class stubs. Pack the field
    // names so the importing module allocates the right inline slot count
    // and the slot index for each field matches what the source module's
    // constructor wrote. Without this, the object is allocated 0 inline
    // slots and `this.field = v` in the cross-module constructor writes
    // past the object, while reads on the importing side return undefined.
    for c in imported_class_stubs.iter() {
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
        let global_name = format!("perry_class_keys_{}__{}", module_prefix, sanitize(&c.name),);
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
        let mut parent_chain: Vec<String> = Vec::new();
        let mut p = c.extends_name.clone();
        while let Some(parent_name) = p {
            if let Some(parent) = imported_class_stubs
                .iter()
                .find(|cls| cls.name == parent_name)
            {
                parent_chain.push(parent_name.clone());
                total_field_count += parent.fields.len() as u32;
                p = parent.extends_name.clone();
            } else if let Some(parent) = hir.classes.iter().find(|cls| cls.name == parent_name) {
                parent_chain.push(parent_name.clone());
                total_field_count += parent.fields.len() as u32;
                p = parent.extends_name.clone();
            } else {
                break;
            }
        }
        for parent_name in parent_chain.iter().rev() {
            if let Some(parent) = imported_class_stubs
                .iter()
                .find(|cls| cls.name == *parent_name)
            {
                for f in &parent.fields {
                    packed_keys.push_str(&f.name);
                    packed_keys.push('\0');
                }
            } else if let Some(parent) = hir.classes.iter().find(|cls| cls.name == *parent_name) {
                for f in &parent.fields {
                    packed_keys.push_str(&f.name);
                    packed_keys.push('\0');
                }
            }
        }
        for f in &c.fields {
            packed_keys.push_str(&f.name);
            packed_keys.push('\0');
        }
        let (_typed_slot_count, typed_mask_words) =
            crate::typed_shape::class_typed_layout(&class_table, &c.name);
        class_keys_init_data.push((
            global_name,
            packed_keys,
            total_field_count,
            typed_mask_words,
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
            method_param_counts.insert((cls.name.clone(), sm.name.clone()), sm.params.len());
            let has_rest = sm.params.iter().any(|p| p.is_rest);
            if has_rest {
                method_has_rest.insert((cls.name.clone(), sm.name.clone()), true);
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
    let cross_module = CrossModuleCtx {
        namespace_imports: opts.namespace_imports.iter().cloned().collect(),
        namespace_reexport_named_imports: opts.namespace_reexport_named_imports.clone(),
        namespace_member_prefixes: opts.namespace_member_prefixes,
        imported_async_funcs: opts.imported_async_funcs,
        local_async_funcs,
        type_aliases: opts.type_aliases,
        imported_func_param_counts: opts.imported_func_param_counts,
        import_function_origin_names: opts.import_function_origin_names.clone(),
        import_function_v8_specifiers: opts.import_function_v8_specifiers.clone(),
        // Issue #841: see CrossModuleCtx field docs.
        import_function_node_submodule: opts.import_function_node_submodule.clone(),
        namespace_node_submodules: opts.namespace_node_submodules.clone(),
        namespace_v8_specifiers: opts.namespace_v8_specifiers.clone(),
        imported_func_has_rest: opts.imported_func_has_rest,
        imported_func_return_types: opts.imported_func_return_types,
        func_returns_class: func_returns_class_map,
        method_param_counts,
        method_has_rest,
        class_keys_globals: class_keys_globals_map,
        imported_class_ctors: opts
            .imported_classes
            .iter()
            .map(|ic| {
                let effective_name = ic.local_alias.as_deref().unwrap_or(&ic.name);
                let ctor_name = format!("{}__{}_constructor", ic.source_prefix, ic.name);
                (
                    effective_name.to_string(),
                    (ctor_name, ic.constructor_param_count),
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
            let (translations, key_count, _locale_count, _locale_codes, default_locale_idx) =
                arc.as_ref();
            crate::expr::I18nLowerCtx {
                translations: translations.clone(),
                key_count: *key_count,
                default_locale_idx: *default_locale_idx,
            }
        }),
        imported_vars: opts.imported_vars,
        needs_stdlib: opts.needs_stdlib,
        needs_geisterhand: opts.needs_geisterhand,
        geisterhand_port: opts.geisterhand_port,
        needs_js_runtime: opts.needs_js_runtime,
        compile_time_constants,
        app_metadata: opts.app_metadata.clone(),
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
        // FFI manifest: each `native_library_functions` entry is
        // `(function_name, param_kinds, return_kind)` from the package.json
        // `nativeLibrary.functions` declaration. Build a name → (params, returns)
        // map so `lower_call` can emit the correct LLVM signature for direct
        // calls to native C/Rust functions (matters when the C ABI differs
        // from Perry's all-double default — e.g. `*mut View` returns in `x0`,
        // not `d0`).
        ffi_signatures: opts
            .native_library_functions
            .iter()
            .map(|(name, params, ret)| (name.clone(), (params.clone(), ret.clone())))
            .collect(),
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
        interfaces: hir
            .interfaces
            .iter()
            .map(|i| (i.name.clone(), i.clone()))
            .collect(),
        namespace_entries: opts.namespace_entries.clone(),
        dynamic_import_path_to_prefix: opts.dynamic_import_path_to_prefix.clone(),
        deferred_module_prefixes: opts.deferred_module_prefixes.clone(),
        module_init_deps: opts.module_init_deps.clone(),
        is_dynamic_import_target: opts.is_dynamic_import_target,
    };

    // Module-level globals registry. Pre-walk:
    //   1. Collect every LocalId referenced from any function or method
    //      body (LocalGet / LocalSet / Update). Those that aren't a
    //      function/method's own param or Let must be module-level.
    //   2. Walk hir.init's top-level Lets and globalize ONLY the ones in
    //      that set. Lets that are only referenced from main itself stay
    //      as cheap stack alloca (preserves perf for the bench
    //      benchmarks that don't share state with helper functions).
    let mut referenced_from_fn: std::collections::HashSet<u32> = std::collections::HashSet::new();
    // Helper that handles "params + lets define a scope, refs minus
    // defines flow out". Used for every function/method/closure body.
    let scan_body = |params: &[perry_hir::Param],
                     body: &[perry_hir::Stmt],
                     out: &mut std::collections::HashSet<u32>| {
        let mut local_defs: std::collections::HashSet<u32> = params.iter().map(|p| p.id).collect();
        collect_let_ids(body, &mut local_defs);
        let mut refs: std::collections::HashSet<u32> = std::collections::HashSet::new();
        collect_ref_ids_in_stmts(body, &mut refs);
        for r in refs {
            if !local_defs.contains(&r) {
                out.insert(r);
            }
        }
    };
    for f in &hir.functions {
        scan_body(&f.params, &f.body, &mut referenced_from_fn);
    }
    for c in &hir.classes {
        for m in &c.methods {
            scan_body(&m.params, &m.body, &mut referenced_from_fn);
        }
        if let Some(ctor) = &c.constructor {
            scan_body(&ctor.params, &ctor.body, &mut referenced_from_fn);
        }
    }
    // Also walk every closure body. A self-referencing recursive
    // closure (`let f = (n) => f(n-1)`) needs `f` to be globalized
    // so the closure body can see the live storage instead of a
    // stale snapshot. Without this, the closure auto-capture sees
    // `f` is not yet declared and bails with "local not in scope".
    {
        let mut closures: Vec<(perry_types::FuncId, perry_hir::Expr)> = Vec::new();
        let mut seen: std::collections::HashSet<perry_types::FuncId> =
            std::collections::HashSet::new();
        for f in &hir.functions {
            collect_closures_in_stmts(&f.body, &mut seen, &mut closures);
        }
        for c in &hir.classes {
            for m in &c.methods {
                collect_closures_in_stmts(&m.body, &mut seen, &mut closures);
            }
            for (_, getter_fn) in &c.getters {
                collect_closures_in_stmts(&getter_fn.body, &mut seen, &mut closures);
            }
            for (_, setter_fn) in &c.setters {
                collect_closures_in_stmts(&setter_fn.body, &mut seen, &mut closures);
            }
            for sm in &c.static_methods {
                collect_closures_in_stmts(&sm.body, &mut seen, &mut closures);
            }
            if let Some(ctor) = &c.constructor {
                collect_closures_in_stmts(&ctor.body, &mut seen, &mut closures);
            }
            // Class field initializers (`private foo = (x) => this.bar(x)`) are
            // hoisted into the constructor at codegen time via
            // `apply_field_initializers_recursive`, so any closure literal inside
            // an `init` expression gets a `js_closure_alloc(@perry_closure_*)`
            // emission. We must walk the inits too, otherwise the body never
            // gets compiled and clang errors with "use of undefined value" (#261).
            for field in &c.fields {
                if let Some(init) = &field.init {
                    collect_closures_in_stmts(
                        &[perry_hir::Stmt::Expr(init.clone())],
                        &mut seen,
                        &mut closures,
                    );
                }
            }
            // #338: same gap as the main compile loop — static field inits
            // (`static make = (x) => ...`) need walking so the global-
            // detection pre-walk sees their captures and globalises any
            // module-level lets the closure body references.
            for field in &c.static_fields {
                if let Some(init) = &field.init {
                    collect_closures_in_stmts(
                        &[perry_hir::Stmt::Expr(init.clone())],
                        &mut seen,
                        &mut closures,
                    );
                }
            }
        }
        collect_closures_in_stmts(&hir.init, &mut seen, &mut closures);
        for (_, closure_expr) in &closures {
            if let perry_hir::Expr::Closure { params, body, .. } = closure_expr {
                scan_body(params, body, &mut referenced_from_fn);
            }
        }
    }

    let mut module_globals: HashMap<u32, String> = HashMap::new();
    // Module global types: propagated to every FnCtx so functions that
    // access module globals (via LocalGet/LocalSet) see the correct
    // declared type. Without this, `editorInstance` (Named("Editor"))
    // in render.ts has its type only in the entry function's FnCtx,
    // so method calls in other functions fall through to the generic
    // dispatch instead of the class method registry.
    let mut module_global_types: HashMap<u32, perry_types::Type> = HashMap::new();
    // Collect exported variable names so we can create external
    // globals + getter functions for cross-module access.
    let exported_var_names: std::collections::HashSet<String> =
        hir.exported_objects.iter().cloned().collect();
    for s in &hir.init {
        if let perry_hir::Stmt::Let { id, name, ty, .. } = s {
            // Always record the declared type for module-level lets
            // so all functions see it (not just the entry function).
            if !matches!(ty, perry_types::Type::Any) {
                module_global_types.insert(*id, ty.clone());
            }
            if referenced_from_fn.contains(id) || exported_var_names.contains(name) {
                // Use external linkage for exported vars so other
                // modules can reference them. Internal for the rest.
                let is_exported = exported_var_names.contains(name);
                let global_name = format!("perry_global_{}__{}", module_prefix, id);
                // Use the compile-time constant value if one was registered
                // (e.g., __platform__, __plugins__). Otherwise default to 0.0.
                let init_value = if let Some(cv) = cross_module.compile_time_constants.get(id) {
                    format!("{:.1}", cv)
                } else {
                    "0.0".to_string()
                };
                // Use default (external) linkage for ALL module globals.
                // `internal` linkage lets clang -O3 assume the global is
                // never written by optnone functions (setjmp/try-catch),
                // causing it to constant-fold reads to 0.0. With external
                // linkage, the optimizer can't make cross-TU assumptions.
                // The module-unique name (perry_global_<prefix>__N)
                // prevents symbol collisions across modules.
                llmod.add_global(&global_name, DOUBLE, &init_value);
                module_globals.insert(*id, global_name.clone());

                // For exported variables, also emit a trivial getter
                // function `perry_fn_<prefix>__<name>` that returns
                // the global. The ExternFuncRef wrapper in importing
                // modules calls this symbol — without it, exported
                // constants (like `export const Key = { ... }`) cause
                // linker errors because the wrapper tries to call a
                // function that doesn't exist.
                // Skip the getter for names that are also functions — the
                // compiled function body will provide the correct symbol.
                // Without this, `export function isSetupComplete()` gets
                // a trivial getter that wraps a broken _i64 stub (returns 0)
                // instead of the real function that reads the module global.
                let is_also_function = hir
                    .functions
                    .iter()
                    .any(|f| f.is_exported && f.name == *name);
                // Also skip the value-getter when this name is already an
                // exported function alias (e.g. `export const async = _async`
                // or `export { _void as void }`). For those the #460 forwarding
                // wrapper below emits a `perry_fn_<modprefix>__<name>`
                // definition that actually calls the underlying function;
                // emitting a getter here on top would be a redef and is
                // semantically wrong (it'd return the closure value instead
                // of invoking it).
                let is_function_alias = hir.exported_functions.iter().any(|(exp, _)| exp == name);
                if is_exported && !is_also_function && !is_function_alias {
                    let fn_name = format!("perry_fn_{}__{}", module_prefix, sanitize(name),);
                    let getter = llmod.define_function(&fn_name, DOUBLE, vec![]);
                    let _ = getter.create_block("entry");
                    let blk = getter.block_mut(0).unwrap();
                    let val = blk.load(DOUBLE, &format!("@{}", global_name));
                    blk.ret(DOUBLE, &val);

                    // #460: also emit a duplicate getter under any renamed
                    // export targeting this local. `export { _await as await }`
                    // means consumers compute the callee symbol from the
                    // exported name `await` — without an alias getter the
                    // link fails on `_perry_fn_<mod>__<keyword>`. The wrapper
                    // returns the same global value the local-name getter
                    // returns; callers that invoke it as a function get the
                    // closure handle (matching status quo for non-renamed
                    // `export const f = aFunctionRef` exports).
                    for export in &hir.exports {
                        if let perry_hir::Export::Named { local, exported } = export {
                            if local == name && exported != name {
                                let alias_fn =
                                    format!("perry_fn_{}__{}", module_prefix, sanitize(exported));
                                if alias_fn == fn_name {
                                    continue;
                                }
                                let g = llmod.define_function(&alias_fn, DOUBLE, vec![]);
                                let _ = g.create_block("entry");
                                let b = g.block_mut(0).unwrap();
                                let v = b.load(DOUBLE, &format!("@{}", global_name));
                                b.ret(DOUBLE, &v);
                            }
                        }
                    }
                }
            }
        }
    }

    // Phase E: register and emit static class fields as module globals.
    // Each `static foo: T = init` becomes `@perry_static_<modprefix>__
    // <class>__<field>` initialized to 0.0. The init expression runs
    // in compile_module_entry's main/init function before user code.
    let mut static_field_globals: HashMap<(String, String), String> = HashMap::new();
    for c in &hir.classes {
        for sf in &c.static_fields {
            // Computed-key static fields (`static [Symbol.for(...)] = init`)
            // are stored in a runtime side table by
            // `init_static_fields_late`; they don't get a string-named
            // global. Refs #420, #894.
            if sf.key_expr.is_some() {
                continue;
            }
            let name = format!(
                "perry_static_{}__{}__{}",
                module_prefix,
                sanitize(&c.name),
                sanitize(&sf.name),
            );
            // External linkage so importing modules can reference the same
            // global. Static class fields are spec-level shared state across
            // the whole program (same `Symbol.X` value seen everywhere); they
            // must be a single defining global, not per-module copies.
            // Refs #420: drizzle's `Sub extends Base` reads `[Base.Symbol.X]`
            // when Sub is in a different file from Base; without external
            // linkage, the importing module's `StaticFieldGet { Base, Symbol }`
            // had no symbol to resolve and silently produced 0.0.
            llmod.add_global(&name, DOUBLE, "0.0");
            static_field_globals.insert((c.name.clone(), sf.name.clone()), name);
        }
    }
    // Register foreign static-field globals from imported classes. The source
    // module emits the defining external global (above); the consumer just
    // declares a reference and adds it to its own `static_field_globals` map
    // so `Expr::StaticFieldGet/Set` lowering finds it.
    // Track which globals we've already emitted to avoid double-declarations.
    let mut external_globals_emitted: std::collections::HashSet<String> =
        std::collections::HashSet::new();
    for ic in &opts.imported_classes {
        let effective_name = ic.local_alias.as_deref().unwrap_or(&ic.name);
        // Skip imported-class entries whose source matches this module's
        // prefix — the local-class loop above already emitted the defining
        // global. Re-declaring as external would produce a duplicate-symbol
        // error in the LLVM IR (clang rejects `@x = global` next to `@x =
        // external global`). Same-named local classes also win.
        if ic.source_prefix == module_prefix {
            // Still register in the static_field_globals map so HIR lookups
            // by the imported alias resolve to the local definition.
            for sf_name in &ic.static_field_names {
                let key = (effective_name.to_string(), sf_name.clone());
                if !static_field_globals.contains_key(&key) {
                    let global_name = format!(
                        "perry_static_{}__{}__{}",
                        module_prefix,
                        sanitize(&ic.name),
                        sanitize(sf_name),
                    );
                    static_field_globals.insert(key, global_name);
                }
            }
            continue;
        }
        if hir.classes.iter().any(|c| c.name == ic.name) {
            continue;
        }
        for sf_name in &ic.static_field_names {
            let global_name = format!(
                "perry_static_{}__{}__{}",
                ic.source_prefix,
                sanitize(&ic.name),
                sanitize(sf_name),
            );
            // Declare external (not define) — the source module owns the
            // defining global. Skip if already declared (multiple imports of
            // the same class).
            if external_globals_emitted.insert(global_name.clone()) {
                llmod.add_external_global(&global_name, DOUBLE);
            }
            // Register under both the alias (if any) and the source name so
            // either resolves.
            static_field_globals.insert(
                (effective_name.to_string(), sf_name.clone()),
                global_name.clone(),
            );
            if effective_name != ic.name {
                static_field_globals.insert((ic.name.clone(), sf_name.clone()), global_name);
            }
        }
    }

    // Method registry: (class_name, method_name) → LLVM function name.
    // Built from `class.methods` so the dispatch in `lower_call` knows
    // which mangled function name to call for `obj.method(args)`. Method
    // names are also scoped by module prefix.
    let mut method_names: HashMap<(String, String), String> = HashMap::new();
    for c in class_table.values() {
        // Use the source module prefix for imported classes so the method
        // symbol name matches where the method was actually compiled.
        let class_prefix = imported_class_prefix.get(&c.name).unwrap_or(&module_prefix);
        // Issue #568: when `c` is the stub for an imported renamed class
        // (`export { Widget as PublicWidget }` consumed via
        // `import { PublicWidget }`), `c.name` is the local alias
        // ("PublicWidget"). The source module emits its symbols mangled
        // with the ORIGINAL name ("Widget"); the consumer-side LLVM
        // symbol must match. `mangle_class_name` is the source-side
        // canonical name; the dispatch-table KEY stays `c.name` so
        // `receiver_class_name` lookups (which see the renamed type)
        // still hit.
        let mangle_class_name = imported_class_source_name
            .get(&c.name)
            .map(|s| s.as_str())
            .unwrap_or(c.name.as_str());
        for m in &c.methods {
            let llvm_name = scoped_method_name(class_prefix, mangle_class_name, &m.name);
            method_names.insert((c.name.clone(), m.name.clone()), llvm_name.clone());
            // Refs #486: also register self-binding aliases (e.g. `_X` from
            // `var X = class _X`) so static method dispatch on a receiver typed
            // as `_X` (the inner name) finds the same LLVM symbol as the
            // canonical `X`-typed dispatch.
            for alias in &c.aliases {
                method_names
                    .entry((alias.clone(), m.name.clone()))
                    .or_insert_with(|| llvm_name.clone());
            }
        }
        // Constructor: register as a method so compile_method can find it.
        // Emitted for ALL classes (even without explicit constructors)
        // so cross-module `new` can call the constructor.
        {
            let ctor_method_name = format!("{}_constructor", c.name);
            method_names.insert(
                (c.name.clone(), ctor_method_name.clone()),
                format!("{}__{}_constructor", class_prefix, mangle_class_name),
            );
        }
        // Getters: register under the property name with a `__get_`
        // prefix to avoid colliding with a regular method of the same
        // name. The dispatch site for `obj.prop` checks the getter
        // map first, then falls back to the regular method registry.
        for (prop, f) in &c.getters {
            method_names.insert(
                (c.name.clone(), format!("__get_{}", prop)),
                scoped_method_name(
                    class_prefix,
                    mangle_class_name,
                    &format!("__get_{}", f.name),
                ),
            );
        }
        for (prop, f) in &c.setters {
            method_names.insert(
                (c.name.clone(), format!("__set_{}", prop)),
                scoped_method_name(
                    class_prefix,
                    mangle_class_name,
                    &format!("__set_{}", f.name),
                ),
            );
        }
        // Static methods. Registered under their plain method name
        // so `Counter.increment()` (StaticMethodCall) can look them
        // up the same way as instance methods, but emitted as
        // `perry_static_<modprefix>__<class>__<method>` (no `this`).
        // The class/method names are sanitized so private methods
        // (`#helper`) produce a valid LLVM identifier.
        for sm in &c.static_methods {
            method_names.insert(
                (c.name.clone(), sm.name.clone()),
                format!(
                    "perry_static_{}__{}__{}",
                    class_prefix,
                    sanitize(mangle_class_name),
                    sanitize(&sm.name),
                ),
            );
        }
    }

    // Phase F: register imported class methods in the method_names
    // registry and pre-declare them as extern LLVM functions so the
    // linker can resolve cross-module method calls.
    for ic in &opts.imported_classes {
        let effective_name = ic.local_alias.as_deref().unwrap_or(&ic.name);
        // Skip if locally defined — local methods take precedence.
        if hir.classes.iter().any(|c| c.name == *effective_name) {
            continue;
        }
        let src = &ic.source_prefix;

        for (method_idx, method_name) in ic.method_names.iter().enumerate() {
            // The source module emitted its methods as
            // `perry_method_<source_prefix>__<class>__<method>`.
            // Use the canonical class name (ic.name) for the symbol
            // since that's how the source module mangled it.
            let llvm_fn = format!(
                "perry_method_{}__{}__{}",
                sanitize(src),
                sanitize(&ic.name),
                sanitize(method_name),
            );
            method_names
                .entry((effective_name.to_string(), method_name.clone()))
                .or_insert_with(|| llvm_fn.clone());

            // Declare extern: `double method(double this, double arg0, …)`.
            // Pre-#235 this was hardcoded to 6 doubles ("safe upper bound").
            // The bug: call sites that passed fewer args (the common case for
            // methods with default params) made the callee read garbage from
            // uninitialized arg-register slots — typically a real heap pointer
            // from a prior call's leftover state. Dereferencing that garbage
            // for `options.session` etc. silently hung in the dispatch chain.
            // We now read the actual arity from the parallel
            // `method_param_counts` Vec populated by the source side. If the
            // source module didn't populate it (legacy or out-of-sync build),
            // fall back to 6 to preserve compat.
            // Total arity = explicit params + 1 implicit `this`.
            let arity = ic
                .method_param_counts
                .get(method_idx)
                .copied()
                .map(|n| n + 1)
                .unwrap_or(6);
            let param_types: Vec<crate::types::LlvmType> =
                std::iter::repeat_n(DOUBLE, arity).collect();
            llmod.declare_function(&llvm_fn, DOUBLE, &param_types);
        }

        // Cross-module getters. The dispatch site at
        // `expr.rs::PropertyGet` looks up `(class, "__get_<prop>")` in
        // `method_names`; without this loop the entry is missing for
        // imported classes and `obj.prop` silently falls through to
        // `undefined`. The source module mangles getters as
        // `perry_method_<src>__<class>____get_get_<prop>` (the inner
        // `get_<prop>` is the HIR function name from
        // `lower_getter_method`, then codegen prepends `__get_`).
        for prop in &ic.getter_names {
            let inner_fn_name = format!("get_{}", prop);
            let llvm_fn = scoped_method_name(
                &sanitize(src),
                &ic.name,
                &format!("__get_{}", inner_fn_name),
            );
            method_names
                .entry((effective_name.to_string(), format!("__get_{}", prop)))
                .or_insert_with(|| llvm_fn.clone());
            // Getters take only `this` (NaN-boxed double) and return double.
            llmod.declare_function(&llvm_fn, DOUBLE, &[DOUBLE]);
        }

        // Cross-module setters. Symmetric to getters: source-side
        // mangling is `perry_method_<src>__<class>____set_set_<prop>`.
        for prop in &ic.setter_names {
            let inner_fn_name = format!("set_{}", prop);
            let llvm_fn = scoped_method_name(
                &sanitize(src),
                &ic.name,
                &format!("__set_{}", inner_fn_name),
            );
            method_names
                .entry((effective_name.to_string(), format!("__set_{}", prop)))
                .or_insert_with(|| llvm_fn.clone());
            // Setters take `this` plus the new value, both NaN-boxed
            // doubles, and return double (the assigned value).
            llmod.declare_function(&llvm_fn, DOUBLE, &[DOUBLE, DOUBLE]);
        }

        // Constructor: declared as
        // `<source_prefix>__<class>_constructor(i64 this, double arg0, …) → void`
        let ctor_fn = format!("{}__{}_constructor", sanitize(src), sanitize(&ic.name),);
        let mut ctor_params: Vec<crate::types::LlvmType> = vec![DOUBLE];
        for _ in 0..ic.constructor_param_count {
            ctor_params.push(DOUBLE);
        }
        llmod.declare_function(&ctor_fn, VOID, &ctor_params);

        // Cross-module static methods. Source module emits these as
        // `perry_static_<source_prefix>__<class>__<method>` (no `this`
        // receiver). Register them in `method_names` under the same
        // (class, method) key the StaticMethodCall lowering looks up.
        for sm in &ic.static_method_names {
            let llvm_fn = format!(
                "perry_static_{}__{}__{}",
                sanitize(src),
                sanitize(&ic.name),
                sanitize(sm),
            );
            method_names
                .entry((effective_name.to_string(), sm.clone()))
                .or_insert_with(|| llvm_fn.clone());
            // Declare conservatively with 6 double params; LLVM's direct-call
            // resolution doesn't require an exact arity match for declarations.
            let param_types: Vec<crate::types::LlvmType> = std::iter::repeat_n(DOUBLE, 6).collect();
            llmod.declare_function(&llvm_fn, DOUBLE, &param_types);
        }
    }

    // Resolve user function names up-front so body lowering can emit
    // forward/recursive calls without worrying about emission order.
    // Names are scoped by module prefix to avoid cross-module collisions.
    let mut func_names: HashMap<u32, String> = HashMap::new();
    let mut func_signatures: HashMap<u32, (usize, bool, bool)> = HashMap::new();
    let mut func_synthetic_arguments: std::collections::HashSet<u32> =
        std::collections::HashSet::new();
    for f in &hir.functions {
        func_names.insert(f.id, scoped_fn_name(&module_prefix, &f.name));
        let has_rest = f.params.iter().any(|p| p.is_rest);
        if f.params
            .last()
            .map(|p| p.is_rest && p.name == "arguments")
            .unwrap_or(false)
        {
            func_synthetic_arguments.insert(f.id);
        }
        let returns_number = matches!(
            f.return_type,
            perry_types::Type::Number | perry_types::Type::Int32
        );
        func_signatures.insert(f.id, (f.params.len(), has_rest, returns_number));
    }

    // Module-level boxed_vars: union of every per-function/method/
    // closure/module-init boxed set. We compute this once here because
    // closures emitted in `compile_closure` need to know whether their
    // transitively-captured ids from an enclosing function were boxed
    // at the creation site. Since HIR LocalIds are globally unique
    // across the module, a single union set is enough: each id either
    // lives in a box or it doesn't, irrespective of which function
    // owns it.
    let mut module_boxed_vars: std::collections::HashSet<u32> = std::collections::HashSet::new();
    for f in &hir.functions {
        module_boxed_vars.extend(collect_boxed_vars(&f.body));
    }
    for c in &hir.classes {
        for m in &c.methods {
            module_boxed_vars.extend(collect_boxed_vars(&m.body));
        }
        for (_, getter_fn) in &c.getters {
            module_boxed_vars.extend(collect_boxed_vars(&getter_fn.body));
        }
        for (_, setter_fn) in &c.setters {
            module_boxed_vars.extend(collect_boxed_vars(&setter_fn.body));
        }
        for sm in &c.static_methods {
            module_boxed_vars.extend(collect_boxed_vars(&sm.body));
        }
        if let Some(ctor) = &c.constructor {
            module_boxed_vars.extend(collect_boxed_vars(&ctor.body));
        }
    }
    module_boxed_vars.extend(collect_boxed_vars(&hir.init));

    // Module-wide LocalId → Type map. Used by closure bodies to
    // learn the types of captured vars from the enclosing scope.
    // HIR LocalIds are globally unique within the module, so a
    // single flat map works.
    let mut module_local_types: HashMap<u32, perry_types::Type> = HashMap::new();
    collect_let_types_in_stmts(&hir.init, &mut module_local_types);
    for f in &hir.functions {
        for p in &f.params {
            module_local_types.insert(p.id, p.ty.clone());
        }
        collect_let_types_in_stmts(&f.body, &mut module_local_types);
    }
    for c in &hir.classes {
        for m in &c.methods {
            for p in &m.params {
                module_local_types.insert(p.id, p.ty.clone());
            }
            collect_let_types_in_stmts(&m.body, &mut module_local_types);
        }
        for (_, getter_fn) in &c.getters {
            for p in &getter_fn.params {
                module_local_types.insert(p.id, p.ty.clone());
            }
            collect_let_types_in_stmts(&getter_fn.body, &mut module_local_types);
        }
        for (_, setter_fn) in &c.setters {
            for p in &setter_fn.params {
                module_local_types.insert(p.id, p.ty.clone());
            }
            collect_let_types_in_stmts(&setter_fn.body, &mut module_local_types);
        }
        if let Some(ctor) = &c.constructor {
            for p in &ctor.params {
                module_local_types.insert(p.id, p.ty.clone());
            }
            collect_let_types_in_stmts(&ctor.body, &mut module_local_types);
        }
        for sm in &c.static_methods {
            for p in &sm.params {
                module_local_types.insert(p.id, p.ty.clone());
            }
            collect_let_types_in_stmts(&sm.body, &mut module_local_types);
        }
    }

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

    // Pre-walk for closures: every `Expr::Closure` in the program needs
    // its body emitted as a top-level LLVM function so the closure
    // creation site can take its address. Collect them all first, then
    // emit each via `compile_closure` (Phase D.1).
    //
    // We must walk every container that the compile loop below also
    // compiles — methods, ctors, getters, setters, static_methods —
    // otherwise a closure body in (say) a `get size() { return arr.filter(...).length }`
    // ends up referenced by `js_closure_alloc(@perry_closure_*)` but
    // never defined, and clang errors with "use of undefined value".
    let mut closures: Vec<(perry_types::FuncId, perry_hir::Expr)> = Vec::new();
    {
        let mut seen: std::collections::HashSet<perry_types::FuncId> =
            std::collections::HashSet::new();
        for f in &hir.functions {
            collect_closures_in_stmts(&f.body, &mut seen, &mut closures);
        }
        for c in &hir.classes {
            for m in &c.methods {
                collect_closures_in_stmts(&m.body, &mut seen, &mut closures);
            }
            for (_, getter_fn) in &c.getters {
                collect_closures_in_stmts(&getter_fn.body, &mut seen, &mut closures);
            }
            for (_, setter_fn) in &c.setters {
                collect_closures_in_stmts(&setter_fn.body, &mut seen, &mut closures);
            }
            for sm in &c.static_methods {
                collect_closures_in_stmts(&sm.body, &mut seen, &mut closures);
            }
            if let Some(ctor) = &c.constructor {
                collect_closures_in_stmts(&ctor.body, &mut seen, &mut closures);
            }
            // Class field initializers (`private foo = (x) => this.bar(x)`) are
            // hoisted into the constructor at codegen time via
            // `apply_field_initializers_recursive`, so any closure literal inside
            // an `init` expression gets a `js_closure_alloc(@perry_closure_*)`
            // emission. We must walk the inits too, otherwise the body never
            // gets compiled and clang errors with "use of undefined value" (#261).
            for field in &c.fields {
                if let Some(init) = &field.init {
                    collect_closures_in_stmts(
                        &[perry_hir::Stmt::Expr(init.clone())],
                        &mut seen,
                        &mut closures,
                    );
                }
            }
            // #338: static fields with closure inits (`static make = (x) =>
            // ...`) emit `js_closure_alloc(@perry_closure_*)` at module-init
            // time too — the codegen path that initialises
            // `@perry_static_<class>__<field>` globals. Pre-fix this loop
            // walked instance fields (`c.fields`) only, so closures inside
            // `c.static_fields[i].init` were never collected and clang
            // errored on the undefined `@perry_closure_*` reference.
            // Surfaced on Effect's `SchemaAST.ts` (Union.make / Union.unify)
            // and any class shipping arrow-style static helpers.
            for field in &c.static_fields {
                if let Some(init) = &field.init {
                    collect_closures_in_stmts(
                        &[perry_hir::Stmt::Expr(init.clone())],
                        &mut seen,
                        &mut closures,
                    );
                }
            }
        }
        collect_closures_in_stmts(&hir.init, &mut seen, &mut closures);
    }

    // Build closure rest param index: for each closure that has a rest
    // parameter, record its func_id → rest param position. Used by
    // the closure call site in `lower_call` to bundle trailing args.
    let closure_rest_params: HashMap<u32, usize> = closures
        .iter()
        .filter_map(|(fid, expr)| {
            if let perry_hir::Expr::Closure { params, .. } = expr {
                params.iter().position(|p| p.is_rest).map(|idx| (*fid, idx))
            } else {
                None
            }
        })
        .collect();

    // Refs #915 (gap 1 from #899): closures whose rest param is the
    // HIR-synthesized `arguments` need to bundle ALL passed args into
    // the rest slot at dispatch time — JS spec semantics for
    // `arguments.length` count every passed arg, not just the trailing
    // tail after the fixed params. The runtime side reads this through
    // `js_register_closure_synthetic_arguments` (vs the regular
    // `js_register_closure_rest`).
    let closure_synthetic_arguments: std::collections::HashSet<u32> = closures
        .iter()
        .filter_map(|(fid, expr)| {
            if let perry_hir::Expr::Closure { params, .. } = expr {
                let last_is_synth_args = params
                    .last()
                    .map(|p| p.is_rest && p.name == "arguments")
                    .unwrap_or(false);
                if last_is_synth_args {
                    Some(*fid)
                } else {
                    None
                }
            } else {
                None
            }
        })
        .collect();

    // Refs #421: declared param count for every non-rest closure. Used by
    // `emit_string_pool` to register each closure's arity so the runtime can
    // pad missing args with TAG_UNDEFINED in the dynamic-dispatch path.
    let closure_arities: HashMap<u32, u32> = closures
        .iter()
        .filter_map(|(fid, expr)| {
            if let perry_hir::Expr::Closure { params, .. } = expr {
                if params.iter().any(|p| p.is_rest) {
                    return None;
                }
                Some((*fid, params.len() as u32))
            } else {
                None
            }
        })
        .collect();

    // Integer specialization: for pure numeric recursive functions (like
    // fibonacci), emit an i64 variant that uses integer registers and
    // integer arithmetic. The f64 wrapper calls fptosi → i64_fn → sitofp.
    let mut i64_specialized: std::collections::HashSet<u32> = std::collections::HashSet::new();
    for f in &hir.functions {
        // Skip integer specialization for functions that access module globals.
        // The i64 body emitter can't handle module global loads (it produces
        // `ret 0` instead of reading the global), creating a broken stub
        // that shadows the real compiled function.
        let uses_module_globals = f.body.iter().any(|s| {
            fn walks(s: &perry_hir::Stmt, mg: &HashMap<u32, String>) -> bool {
                match s {
                    perry_hir::Stmt::Return(Some(perry_hir::Expr::LocalGet(id))) => {
                        mg.contains_key(id)
                    }
                    perry_hir::Stmt::Expr(perry_hir::Expr::LocalGet(id)) => mg.contains_key(id),
                    _ => false,
                }
            }
            walks(s, &module_globals)
        });
        if crate::collectors::is_integer_specializable(f) && !uses_module_globals {
            if let Some(llvm_name) = func_names.get(&f.id) {
                let i64_name = format!("{}_i64", llvm_name);
                crate::collectors::emit_i64_function(&mut llmod, f, &i64_name);
                // Emit the f64 wrapper that calls the i64 version.
                // Mark as alwaysinline so LLVM exposes the integer ops
                // to callers — critical for vectorizing clamp patterns.
                let params: Vec<(LlvmType, String)> = f
                    .params
                    .iter()
                    .map(|p| (DOUBLE, format!("%arg{}", p.id)))
                    .collect();
                let wrapper = llmod.define_function(llvm_name, DOUBLE, params);
                wrapper.force_inline = true;
                let _ = wrapper.create_block("entry");
                let blk = wrapper.block_mut(0).unwrap();
                let mut i64_args: Vec<(LlvmType, String)> = Vec::new();
                for p in &f.params {
                    let i64_v = blk.fptosi(DOUBLE, &format!("%arg{}", p.id), I64);
                    i64_args.push((I64, i64_v));
                }
                let refs: Vec<(LlvmType, &str)> =
                    i64_args.iter().map(|(t, v)| (*t, v.as_str())).collect();
                let i64_result = blk.call(I64, &i64_name, &refs);
                let f64_result = blk.sitofp(I64, &i64_result, DOUBLE);
                blk.ret(DOUBLE, &f64_result);
                i64_specialized.insert(f.id);
            }
        }
    }

    // Lower each user function into the module (skip i64-specialized ones).
    for f in &hir.functions {
        if i64_specialized.contains(&f.id) {
            continue;
        }
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
            if &f.name == exported_name {
                continue;
            }
            let alias_sym = format!("perry_fn_{}__{}", module_prefix, sanitize(exported_name));
            let target_sym = match func_names.get(func_id) {
                Some(s) => s.clone(),
                None => continue,
            };
            if alias_sym == target_sym {
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
        closure_rest_params: &closure_rest_params,
        closure_synthetic_arguments: &closure_synthetic_arguments,
        closure_arities: &closure_arities,
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
