//! Expression codegen — Phase 2.
//!
//! Scope: numeric expressions (literals, LocalGet, Binary add/sub/mul/div,
//! Compare, direct FuncRef calls) plus the `console.log(<expr>)` sink. All
//! values are raw LLVM `double` — no NaN-boxing, no strings, no objects.
//!
//! Anything outside the supported shape returns an explicit "unsupported"
//! error so a user running `--backend llvm` on richer TypeScript gets a
//! one-line explanation instead of a silent broken binary.

use anyhow::{bail, Result};
use perry_hir::{BinaryOp, Expr};
use perry_types::Type as HirType;

use crate::block::LlBlock;
use crate::codegen::AppMetadata;
use crate::collectors::NativeRegionFactGraph;
use crate::function::LlFunction;
use crate::native_value::{
    AliasState, BoundedBufferIndex, BoundsProof, BoundsState, BufferAccessFacts, BufferAccessMode,
    BufferViewSlot, GuardedBufferIndex, LoweredValue, MaterializationReason, NativeAbiTypeRecord,
    NativeFactUse, NativeRep, NativeRepRecord, NativeValueState, PodLayoutManifest,
    PodRecordViewManifest, ScalarConversionRecord,
};
use crate::strings::StringPool;
use crate::type_analysis::is_numeric_expr;
use crate::types::{DOUBLE, I32, I64, PTR};

// Issue #1098: expr.rs split into expr/ submodules. These are pure
// mechanical moves of self-contained helper clusters out of this file;
// `lower_expr` and the foundational types (`FnCtx`, `FlatConstInfo`)
// remain here. `pub(crate) use` keeps the public surface stable so
// existing `crate::expr::X` paths resolve unchanged.
mod array_literal;
mod buffer_access;
mod buffer_views;
mod channel;
mod helpers;
mod i32_fast_path;
mod index;
mod nanbox_inline;
mod native_memory;
mod native_record;
mod object_literal;
mod pod_layout_constants;
mod pod_record;
mod property_get_names;
mod range_facts;
mod strings;
mod typed_feedback;
mod url_helpers;
mod v8_interop;
mod write_barrier;

pub(crate) use crate::native_value::materialize_js_value;
pub(crate) use array_literal::lower_array_literal;
pub(crate) use buffer_access::{
    access_facts_for_spec, emit_buffer_access_pointer, lower_buffer_access_proof,
    lower_buffer_load, lower_buffer_store, lower_typed_array_load, lower_typed_array_store,
    BufferAccessSpec,
};
pub(crate) use buffer_views::{
    alias_buffer_view_slot, attach_native_owned_view_fact, buffer_access_materialization_reason,
    buffer_view_lowered_value, downgrade_buffer_alias, downgrade_buffer_aliases_in_expr,
    invalidate_native_owned_views_for_dispose, native_arena_canonical_owner_id,
    record_native_arena_owner_assignment, update_buffer_view_for_assignment,
};
#[allow(unused_imports)] // ChannelReduction kept reachable for surface stability
pub(crate) use channel::{
    extract_array_of_object_shape, lower_channel_reduction, try_match_channel_reduction,
    variant_name, ChannelReduction,
};
pub(crate) use helpers::{
    array_store_needs_layout_note, array_store_needs_write_barrier, buffer_alias_metadata_suffix,
    expr_has_numeric_pointer_free_array_layout, expr_produces_non_pointer_bits_by_construction,
    is_global_this_builtin_function_name, is_global_this_builtin_name,
    lower_expr_with_expected_type, lower_js_args_array, proxy_build_args_array, unbox_str_handle,
    unbox_to_i64,
};
pub(crate) use i32_fast_path::{
    can_lower_expr_as_i32, is_known_finite, lower_expr_as_i32, lower_expr_native,
    try_flat_const_2d_int, try_lower_flat_const_index_get,
};
pub(crate) use index::lower_index_set_fast;
pub(crate) use nanbox_inline::{
    i32_bool_to_nanbox, nanbox_bigint_inline, nanbox_pointer_inline, nanbox_pointer_inline_pub,
    nanbox_string_inline,
};
pub(crate) use native_record::raw_f64_layout_fact;
pub(crate) use object_literal::lower_object_literal;
pub(crate) use pod_record::{
    lower_and_store_initial_pod_field, lower_pod_local_reassignment, materialize_pod_local,
    try_lower_pod_field_get, try_lower_pod_field_set,
};
pub(crate) use range_facts::{
    bounds_for_buffer_access_width, effective_alias_state_for_access,
    guarded_buffer_indices_for_condition, int_range_expr, invalidate_local_write_facts,
    record_int_facts_for_let, record_int_facts_for_local_set, record_int_facts_for_update,
    while_condition_range_fact, IntRange, IntRangeFact,
};
pub(crate) use strings::emit_string_literal_global;
pub(crate) use typed_feedback::{
    emit_typed_feedback_register_site, native_region_slug, TypedFeedbackContract, TypedFeedbackKind,
};
pub(crate) use url_helpers::lower_url_string_getter;
pub(crate) use v8_interop::{
    emit_v8_export_call, emit_v8_member_method_call, import_origin_suffix, try_static_class_name,
};
pub(crate) use write_barrier::{
    emit_array_numeric_write_note_on_block, emit_jsvalue_slot_store_on_block,
    emit_jsvalue_slot_store_scalar_aware_on_block, emit_layout_note_slot_on_block,
    emit_root_heap_word_store_on_block, emit_root_nanbox_store_on_block, emit_write_barrier,
    emit_write_barrier_slot_on_block, lower_array_super_init, lower_event_emitter_subclass_init,
    lower_node_stream_super_init, lower_stream_super_init,
};

// Issue #1098 phase 3: the `FnCtx` definition stays in this trunk, but its
// bulky `record_lowered_value*` method family, the shadow-slot free helpers,
// and the `lower_expr` dispatch table moved into siblings to keep this file
// under 2000 lines. Inherent methods (`record_value`) need no re-export.
mod dispatch;
mod record_value;
mod shadow_slot;

pub(crate) use dispatch::{lower_expr, lower_math_operand};
pub(crate) use shadow_slot::{
    emit_shadow_slot_bind_for_local, emit_shadow_slot_clear, emit_shadow_slot_update_for_expr,
    expr_is_known_non_pointer_shadow_value,
};

/// One in-flight inline-constructor return target. See
/// `FnCtx::inline_ctor_return`.
#[derive(Clone)]
pub(crate) struct InlineCtorReturn {
    /// `alloca` (as `%name`) holding the constructed instance, overwritten by
    /// an explicit `return <object>` (spec return-override). Loaded as the
    /// `new`-expression's value after the body's `after_label` block.
    pub result_slot: String,
    /// Label of the block that follows the inlined constructor body. Every
    /// `return` inside the body branches here instead of emitting `ret`.
    pub after_label: String,
    /// True for a derived class (`class X extends Y`). A derived ctor that
    /// `return`s a non-object, non-undefined value throws a TypeError; a base
    /// ctor silently ignores it and keeps `this`.
    pub is_derived: bool,
}

/// Per-function codegen context. Held briefly during lowering, never stored.
pub(crate) struct FnCtx<'a> {
    /// Function being built (blocks, params, registers).
    pub func: &'a mut LlFunction,
    /// Stable slug for native-region ids derived from this module.
    pub module_slug: String,
    /// Source-level function name for native-representation records. Top-level
    /// module code uses `module_init`.
    pub source_function: String,
    pub source_function_slug: String,
    /// Stable id for the labeled loop currently being lowered.
    pub active_region_id: Option<String>,
    /// Full native-region fact graph collected for this lowered HIR region.
    ///
    /// Existing fields below borrow individual subgraphs for compatibility
    /// with older lowering consumers. New native-lowering decisions should
    /// prefer this structured graph so representation, range, bounds, alias,
    /// escape, shape, constants, and materialization-hazard facts stay tied
    /// to the same collector snapshot.
    pub native_facts: &'a NativeRegionFactGraph,
    /// Map from HIR LocalId → LLVM alloca pointer (e.g. `%r3`).
    pub locals: std::collections::HashMap<u32, String>,
    /// Map from HIR LocalId → static HIR Type. Used by `is_string_expr` and
    /// future type-aware dispatch sites (Phase B's "native instance flag
    /// tracking" extension). Populated from function params and `Stmt::Let`
    /// declarations as they're lowered.
    pub local_types: std::collections::HashMap<u32, HirType>,
    /// Index into `func.blocks()` pointing at the block currently receiving
    /// instructions. Lowering fns update this when control flow splits.
    pub current_block: usize,
    /// True while lowering an expression statement whose resulting JS value
    /// will be discarded.
    pub discard_expr_value: bool,
    /// HIR FuncId → LLVM function name. Resolved at the top of
    /// `compile_module` so `FuncRef(id)` calls know what to emit.
    pub func_names: &'a std::collections::HashMap<u32, String>,
    /// Module-wide string literal pool. Disjoint borrow from `func` because
    /// it lives in `codegen.rs` as a separate variable, not inside the
    /// LlModule that `func` was derived from. See `crate::strings` for the
    /// design rationale.
    pub strings: &'a mut StringPool,
    /// Stack of loop targets for `break` / `continue` lowering. Each entry is
    /// `(continue_label, break_label, try_depth_at_entry)`, pushed on loop
    /// entry, popped on exit; innermost loop on top. `for`: continue → update
    /// block, break → exit; `while`/`do-while`: continue → cond, break → exit.
    ///
    /// The third field is `ctx.try_depth` at loop entry, so a `break`/`continue`
    /// out of open `try` frames emits a matching `js_try_end` per exited frame
    /// (like `Stmt::Return`), keeping the runtime TRY_DEPTH balanced. Without
    /// it, a state-machine suspend (lowered to a `break` out of the dispatch
    /// loop's real `try`) leaked a slot per awaited try/catch (panic at 128).
    pub loop_targets: Vec<(String, String, usize)>,
    /// Map from label name → (continue_label, break_label, try_depth_at_entry).
    /// Populated by `Stmt::Labeled` when the body is a loop; read by
    /// `Stmt::LabeledBreak`/`LabeledContinue`. Third field balances try frames
    /// as in `loop_targets`.
    pub label_targets: std::collections::HashMap<String, (String, String, usize)>,
    /// Pending labels set by enclosing `Stmt::Labeled` nodes just before
    /// lowering the body. A label *chain* like `outer: inner: for (...)`
    /// stacks both labels here (outer pushed first, then inner) before the
    /// loop is reached. The next loop/switch that runs consumes *all* of
    /// them and registers each in `label_targets`, so `break outer` /
    /// `continue inner` both resolve to that same loop's blocks. Stored
    /// outermost-first; the innermost label is `.last()`.
    pub pending_labels: Vec<String>,
    /// Map from class name → HIR Class definition. Built once in
    /// `compile_module` from `hir.classes`. Used by `Expr::New` to look up
    /// the field count, constructor body, and (eventually) method table.
    pub classes: &'a std::collections::HashMap<String, &'a perry_hir::Class>,
    /// Map from interface name → HIR Interface definition. Built once
    /// from `hir.interfaces` and threaded via `cross_module.interfaces`.
    /// Consulted by `static_type_of` / `receiver_class_name` so a
    /// `PropertyGet` whose receiver is interface-typed (e.g.
    /// `s.pending` where `s: State` and `State` is an interface with
    /// `pending: number[]`) resolves to the property's declared type.
    /// Without this, the array fast-path in `lower_array_method` and
    /// the `arr.length = N` setter path silently fall through to
    /// generic dispatch — see issue #655.
    pub interfaces: &'a std::collections::HashMap<String, perry_hir::Interface>,
    /// Stack of `this` slot pointers — set when lowering inside a class
    /// constructor body. `Expr::This` loads from the top entry.
    pub this_stack: Vec<String>,
    /// Stack of lexical `new.target` slot pointers. Arrow closures that
    /// reference `new.target` capture the enclosing value here.
    pub new_target_stack: Vec<String>,
    /// Stack of class names currently being lowered. Pushed when entering
    /// a constructor body. `Expr::SuperCall` looks at the top entry to
    /// find the parent class's constructor to inline. Same depth as
    /// `this_stack` (one entry per nested `new`).
    pub class_stack: Vec<String>,
    /// Method registry: `(class_name, method_name) → LLVM function name`.
    /// Built by `compile_module` from `hir.classes[*].methods`. Used by
    /// `lower_call` to dispatch `obj.method(args)` to the right
    /// `perry_method_<class>_<name>` function.
    pub methods: &'a std::collections::HashMap<(String, String), String>,
    /// Module-level globals: `LocalId → global symbol name (without @)`.
    /// Built by `compile_module` from top-level `Stmt::Let` declarations
    /// in `hir.init`. Used by `LocalGet`/`LocalSet`/`Update`/`Stmt::Let`
    /// — when a local id is in this map, it refers to a module-level
    /// `internal global double 0.0` instead of a stack alloca, so the
    /// value is visible to all functions in the module (essential for
    /// patterns like `let failures = 0; function eq() { failures++; }`).
    pub module_globals: &'a std::collections::HashMap<u32, String>,
    /// Imported function name → source module's symbol prefix. Used by
    /// `ExternFuncRef` lowering in `lower_call` to generate scoped
    /// cross-module calls.
    pub import_function_prefixes: &'a std::collections::HashMap<String, String>,
    /// Issue #678: Imported function name → original export name in the
    /// origin module. Set when the import traverses a re-export rename
    /// (`export { default as render } from './render.js'`). Looked up at
    /// every `perry_fn_<source_prefix>__<suffix>` construction site to
    /// pick the right suffix. Absent entries (the common case) mean the
    /// origin name matches the consumer's imported name; callers should
    /// treat a missing entry as identity by calling
    /// `import_origin_suffix(import_function_origin_names, name)`.
    pub import_function_origin_names: &'a std::collections::HashMap<String, String>,
    /// Issue #678 followup: Imported function name → module specifier for
    /// imports that resolved to a `ModuleKind::Interpreted` (V8-fallback)
    /// module. When a name is present here, every codegen site that
    /// would otherwise form `perry_fn_<src>__<name>` routes through the
    /// runtime bridge `js_call_v8_export(specifier, name, args, argc)`
    /// instead — there is no native symbol to call. Sparse map; absent
    /// entries (the common case) mean the import resolves natively.
    pub import_function_v8_specifiers: &'a std::collections::HashMap<String, String>,
    /// Issue #841: Named-import → `(submodule_key, exported_name)` map
    /// for the five Node submodules Perry recognizes but has no
    /// perry-stdlib / compiled-source backing for —
    /// `node:timers/promises`, `node:readline/promises`,
    /// `node:stream/promises`, `node:stream/consumers`, `node:sys`.
    /// The `Expr::ExternFuncRef` value-form catch-all probes this BEFORE
    /// falling to the `TAG_TRUE` sentinel and, when hit, emits a call to
    /// `js_node_submodule_export_as_function(submod_bytes, submod_len,
    /// name_bytes, name_len)` so `typeof X === "function"` holds.
    pub import_function_node_submodule: &'a std::collections::HashMap<String, (String, String)>,
    /// Issue #841 companion: Local namespace alias → submodule key for
    /// `import * as ns from "node:<submod>"`. Codegen's namespace
    /// lowering paths route through
    /// `js_node_submodule_namespace(submod_bytes, submod_len)` so the
    /// namespace value reports `typeof === "object"` and per-property
    /// accesses (`ns.X`) read the same function singletons named
    /// imports produce.
    pub namespace_node_submodules: &'a std::collections::HashMap<String, String>,
    /// Issue #678 followup (namespace branch): see
    /// `CompileOptions::namespace_v8_specifiers`. Local namespace alias →
    /// V8 module specifier for `import * as ns from "<v8-module>"`. When
    /// `ns.member(args)` is lowered and the namespace local appears here,
    /// codegen emits a `js_call_v8_export(specifier, member, args, argc)`
    /// bridge call instead of falling to the `double_literal(0.0)` stub.
    /// Unblocks ramda (`import * as R`), date-fns, jose, effect — packages
    /// where consumers use a wildcard namespace for ergonomics but the
    /// source module fell back to V8.
    pub namespace_v8_specifiers: &'a std::collections::HashMap<String, String>,
    /// Closure capture map: when lowering inside a closure body, this
    /// holds `LocalId → capture_index`. `LocalGet`/`LocalSet`/`Update`
    /// of an id in this map routes through the runtime
    /// `js_closure_get/set_capture_f64(this_closure, idx)` calls
    /// instead of an alloca slot.
    pub closure_captures: std::collections::HashMap<u32, u32>,
    /// Inside a closure body, the LLVM SSA value name for the current
    /// closure pointer (`%this_closure`). `Expr::LocalGet` of a captured
    /// id uses this as the first arg to `js_closure_get_capture_f64`.
    pub current_closure_ptr: Option<String>,
    /// Map from (enum_name, member_name) → enum value. Built once in
    /// `compile_module` from `hir.enums`. Used by `Expr::EnumMember`
    /// to lower enum references to constants.
    pub enums: &'a std::collections::HashMap<(String, String), perry_hir::EnumValue>,
    /// Whether the enclosing function is `async`. When true, every
    /// `Stmt::Return(value)` wraps `value` in `js_promise_resolved`
    /// before returning, so callers can `await` the result.
    pub is_async_fn: bool,
    /// Whether `this` reads should preserve exact strict-mode receiver values.
    pub is_strict_fn: bool,
    /// Static class fields: `(class_name, field_name) → llvm global
    /// symbol`. Built once in `compile_module`. Used by
    /// `Expr::StaticFieldGet/Set` to load/store the global.
    pub static_field_globals: &'a std::collections::HashMap<(String, String), String>,
    /// Per-class id for object headers. Each user class gets a
    /// unique non-zero id (anonymous objects use 0). Used by
    /// `lower_new` and the virtual method dispatch helper.
    pub class_ids: &'a std::collections::HashMap<String, u32>,
    /// Per-class `keys_array` global variable names. Each entry is
    /// `class_name → @perry_class_keys_<modprefix>__<sanitized_class>`.
    /// Built once at module init via `js_build_class_keys_array` and
    /// stored in the global. `compile_new` looks up the class here
    /// and emits a direct global load + `js_object_alloc_class_inline_keys`
    /// call (skipping the SHAPE_CACHE lookup AND the
    /// `js_object_alloc_class_with_keys` runtime function entirely on
    /// the hot allocation path). When a class is missing from this
    /// map, `compile_new` falls back to the slower
    /// `js_object_alloc_class_with_keys` path.
    pub class_keys_globals: &'a std::collections::HashMap<String, String>,
    /// Issue #26 / #321: authoritative total inline-field count per class,
    /// matching the keys-array length the `class_keys_globals` global holds.
    /// `lower_new` prefers this over the name-keyed `ctx.classes` field-count
    /// walk, which mis-resolves same-named cross-module parents (effect's
    /// `Type` in SchemaAST.ts vs ParseResult.ts).
    pub class_field_counts: &'a std::collections::HashMap<String, u32>,
    /// Issue #26 / #321: authoritative root→leaf ancestor chain per class
    /// (prefix-disambiguated). `apply_field_initializers_recursive` uses this
    /// to write the correct inherited fields instead of walking the name-keyed
    /// `ctx.classes` chain (which mis-picks same-named cross-module parents).
    pub class_init_chains:
        &'a std::collections::HashMap<String, Vec<(String, Vec<perry_hir::ClassField>)>>,
    /// Imported class constructor metadata, keyed by effective imported class name.
    pub imported_class_ctors: &'a std::collections::HashMap<String, crate::codegen::ImportedCtor>,
    /// Per-function param signature: `(declared_param_count,
    /// has_rest_param)`. Used by FuncRef call sites to know whether
    /// to bundle trailing arguments into a rest array.
    pub func_signatures: &'a std::collections::HashMap<u32, (usize, bool, bool, bool)>,
    /// Function declarations where Perry appended a synthetic trailing
    /// `arguments` binding. Unlike a real rest parameter, it must receive
    /// every actual argument while fixed parameters still receive their
    /// normal positional values.
    pub func_synthetic_arguments: &'a std::collections::HashSet<u32>,
    /// Refs #915 (gap 3 / #321 follow-up): factory functions in THIS
    /// module — those whose body unconditionally returns a `ClassRef`
    /// (or transitively returns another such factory). Maps function
    /// id → produced class name. Lets `lower_call`'s static-method
    /// dispatch tower recognise `Literal(...).pipe(...)` (where
    /// `Literal` is a factory) and route the `.pipe` lookup through
    /// the produced class's static methods, matching the post-#912
    /// `Cls = make(); Cls.pipe(...)` shape.
    pub func_returns_class: &'a std::collections::HashMap<u32, String>,
    /// LocalIds that must be stored in heap boxes (`js_box_alloc`)
    /// instead of stack allocas. A local gets boxed when at least
    /// one closure captures it AND it's written to (either by the
    /// enclosing function or inside a closure). Boxing guarantees
    /// that all readers — inc()/get() on a shared counter, for
    /// instance — observe each other's writes. See `collect_boxed_
    /// vars` for the detection rule.
    ///
    /// For ids in this set:
    /// - Stmt::Let allocates a box via `js_box_alloc(init)` and
    ///   stores the box pointer (i64) in a local alloca slot.
    /// - LocalGet reads the slot, unboxes, and calls `js_box_get`.
    /// - LocalSet/Update reads the slot, unboxes, and calls
    ///   `js_box_set`.
    /// - Closure creation captures the box pointer directly so
    ///   the closure body sees the same storage.
    pub boxed_vars: std::collections::HashSet<u32>,
    /// LocalIds whose slot+box was allocated up-front via `Stmt::
    /// PreallocateBoxes` (issue #569). When a later `Stmt::Let` is
    /// processed for an id in this set, codegen skips the slot/box
    /// allocation and just `js_box_set`s the init value into the
    /// pre-allocated box. The id is added to `boxed_vars` automatically
    /// so subsequent `LocalGet`/`LocalSet`/`Update` go through the box.
    pub prealloc_boxes: std::collections::HashSet<u32>,
    /// Closure rest param index: closure `FuncId` → index of the rest
    /// parameter. Built once in `compile_module` from the collected
    /// closures. Used by the closure call site in `lower_call` to
    /// bundle trailing arguments into an array before calling
    /// `js_closure_callN`.
    pub closure_rest_params: &'a std::collections::HashMap<u32, usize>,
    /// LocalId → closure FuncId mapping. Populated in `Stmt::Let`
    /// when the init expression is `Expr::Closure { func_id, .. }`.
    /// Used by the closure call site in `lower_call` to look up the
    /// callee's rest param info from `closure_rest_params`.
    pub local_closure_func_ids: std::collections::HashMap<u32, u32>,
    /// LocalId → closure declared parameter count. Paired with
    /// `local_closure_func_ids` for guarded direct closure calls: direct
    /// calls only fire when the static arity exactly matches the call site.
    pub local_closure_param_counts: std::collections::HashMap<u32, usize>,
    /// LocalId → compile-time options object fields for immutable locals
    /// initialized from object literals / anonymous-shape literals. This lets
    /// native constructor lowering read `const init = {...}; new Request(url,
    /// init)` with the same field extractor used for inline object literals.
    pub option_object_locals: std::collections::HashMap<u32, Vec<(String, Expr)>>,
    /// LocalIds of immutable locals provably initialized from an object
    /// literal (`const o = { … }`, including method-bearing literals that
    /// lower to an object-building IIFE). #5271: a builtin-named method on
    /// such a receiver (`o.trim()`, joi's `internals.trim(v, s)`) is the
    /// object's OWN method, never `String.prototype.<m>` — so the static
    /// String-method fast path must NOT claim it even when the call's arity
    /// happens to match the String builtin.
    pub object_literal_locals: std::collections::HashSet<u32>,

    // ── Cross-module import plumbing (Phase F) ──────────────────────
    /// Locals that are namespace imports (`import * as X from "./mod"`).
    /// Codegen uses this to know that `X.foo()` should be dispatched as
    /// a cross-module call rather than an object method call.
    pub namespace_imports: &'a std::collections::HashSet<String>,
    /// Issue #321: subset of `namespace_imports` populated only by the
    /// "named import resolves to a `export * as Foo from "./Foo"`" branch
    /// in `compile.rs`. The StaticMethodCall arm uses this to decide
    /// whether to route var-shape members through `js_closure_callN`
    /// (safe for the user-import shape) vs. preserving the pre-fix
    /// direct-call (silently-wrong-but-doesn't-throw) path used by
    /// `import * as` namespaces in effect's internal modules.
    pub namespace_reexport_named_imports: &'a std::collections::HashSet<String>,
    /// Issue #680: per-namespace member resolution. Keyed by
    /// `(namespace_local_name, member_name)` → `source_prefix`. Consulted
    /// by namespace member access lowering to disambiguate when the same
    /// export name appears in multiple `import * as X / Y` sources.
    pub namespace_member_prefixes: &'a std::collections::HashMap<(String, String), String>,
    /// Names of imported functions that are async. Used to wrap
    /// cross-module calls in promise machinery.
    // #854: cross-module async-import wrapping context; currently routed via
    // other async-detection paths, so this borrowed field is not read yet.
    #[allow(dead_code)]
    pub imported_async_funcs: &'a std::collections::HashSet<String>,
    /// FuncIds of locally-defined async functions in this module.
    /// Used by `is_promise_expr` to recognize that `let p = asyncFn();`
    /// produces a Promise so subsequent `p.then(cb)` chains route
    /// through `js_promise_then` instead of `js_native_call_method`.
    pub local_async_funcs: &'a std::collections::HashSet<u32>,
    /// Locally-defined generator wrapper FuncIds after generator lowering.
    /// Used by direct `FuncRef` calls to re-link returned iterator objects to
    /// the same closure-cached prototype that `g.prototype` reads expose.
    pub local_generator_funcs: &'a std::collections::HashSet<u32>,
    /// FuncIds whose body reads dynamic `this` — see
    /// `CrossModuleCtx::funcs_reading_dynamic_this` (#3576).
    pub funcs_reading_dynamic_this: &'a std::collections::HashSet<u32>,
    /// Type alias map (name → Type) aggregated from all modules. Used
    /// to resolve `Named` types in function signatures and dispatch.
    pub type_aliases: &'a std::collections::HashMap<String, perry_types::Type>,
    /// Imported function parameter counts, keyed by function name.
    /// Used for rest-param bundling on cross-module calls.
    pub imported_func_param_counts: &'a std::collections::HashMap<String, usize>,
    /// Issue #608 — imported function names with a trailing `...rest`
    /// parameter. The cross-module call site uses this to pack trailing
    /// args into a real rest array before the call.
    pub imported_func_has_rest: &'a std::collections::HashSet<String>,
    /// #1816 — imported functions whose trailing param is the synthesized
    /// `arguments` rest; the cross-module call bundles ALL args into it.
    pub imported_func_synthetic_arguments: &'a std::collections::HashSet<String>,
    /// Imported function return types, keyed by local function name.
    /// Used for type-aware dispatch on cross-module call results.
    pub imported_func_return_types: &'a std::collections::HashMap<String, perry_types::Type>,
    /// Per-method explicit param counts, keyed by `(class_name, method_name)`.
    /// Built from BOTH local `hir.classes` AND `opts.imported_classes`.
    /// `lower_call.rs` dispatch sites use this to pad missing trailing args
    /// with TAG_UNDEFINED so the callee's default-param desugaring fires
    /// correctly. See issue #235 for the failure mode.
    pub method_param_counts: &'a std::collections::HashMap<(String, String), usize>,
    /// Closes #484: per-`(class, method)` rest-parameter flag. Used by
    /// `lower_call.rs`'s static / dynamic dispatch arms to bundle
    /// trailing args into a `js_array_alloc(n)` rest array when the
    /// method's last declared param is `...rest`. Without this
    /// information the call site emits `args.len()` doubles and the
    /// callee's `args` ends up as raw uninitialized stack-slot
    /// junk — `args.length` then panics with "Cannot read properties
    /// of undefined". Same shape as `func_signatures`'s `has_rest`
    /// bit but for class-method dispatch.
    pub method_has_rest: &'a std::collections::HashMap<(String, String), bool>,
    /// FFI manifest: `name -> (params, return)` from `package.json`
    /// `nativeLibrary.functions`. Descriptors use the shared native-library
    /// ABI vocabulary. `lower_call` consults
    /// this at native-library call sites so handle-returning functions
    /// (`*mut View`-typed C entries) declare an `i64` LLVM return type that
    /// reads the C ABI's `x0` register. Without it, the call defaults to
    /// `double` (reads `d0`) and observes 0 instead of the real handle.
    pub ffi_signatures: &'a std::collections::HashMap<
        String,
        (
            Vec<perry_api_manifest::NativeAbiType>,
            perry_api_manifest::NativeAbiType,
        ),
    >,
    /// Issue #5621: ergonomic camelCase binding → manifest `js_<pkg>_*`
    /// symbol. `try_lower_extern_func_call` rewrites the binding through
    /// this map so a camelCase native-library export routes to its real
    /// FFI symbol and the `ffi_signatures` lookups above hit.
    pub ffi_aliases: &'a std::collections::HashMap<String, String>,
    /// Per-module map: local class/binding name → import source spec.
    /// Used by `lower_builtin_new` to disambiguate ambiguously-named
    /// built-in constructors. See issue #602.
    pub imported_class_sources: &'a std::collections::HashMap<String, String>,
    /// Per-module alias → original imported export name (renamed named imports
    /// only). Used by `lower_new` to recover the canonical built-in constructor
    /// name when a bundle aliases the import (`import { AsyncLocalStorage as xQ5
    /// }`). See `CompileOptions::imported_class_original_names`.
    pub imported_class_original_names: &'a std::collections::HashMap<String, String>,
    /// Number of currently-open `try { ... }` blocks at the current
    /// lowering position. Incremented before lowering a try body,
    /// decremented after. `Stmt::Return` emits `js_try_end()` this many
    /// times before the actual `ret` so the runtime's TRY_DEPTH counter
    /// stays balanced — without this, an early `return` inside a try
    /// body leaks one slot in the runtime's setjmp jump-buffer table
    /// per call. Once 128 leaks accumulate the runtime panics with
    /// "Try block nesting too deep".
    pub try_depth: usize,

    /// Stack of in-flight inline-constructor return targets. When a class
    /// constructor body is inlined at a `new C(...)` site (see
    /// `lower_call/new.rs`), an explicit `return` inside that body must NOT
    /// emit a function-level `ret` (that would terminate the *enclosing*
    /// function). Instead `Stmt::Return` stores the spec return-override
    /// result into `result_slot` and branches to `after_label`; the
    /// new-expression then loads `result_slot` as its value. One entry per
    /// nested inline ctor; the innermost (`last()`) governs a `return`.
    pub inline_ctor_return: Vec<InlineCtorReturn>,

    /// Cross-module function declarations to add to `LlModule` after
    /// lowering finishes. Each entry is `(llvm_name, return_type, param_types)`.
    /// Pushed by `lower_call` whenever it emits a `call @perry_fn_<src>__<name>`,
    /// drained by the caller (compile_function/method/closure/module_entry)
    /// once the `&mut LlFunction` borrow on `LlModule` is released.
    ///
    /// This replaces the old pre-walker (`collect_extern_func_refs_in_*`)
    /// which had to mirror the entire HIR Expr/Stmt grammar to find every
    /// cross-module call. Lazy emission tracks declares at the actual
    /// emission point so any path the lowering reaches automatically gets
    /// its declare — no walker to keep in sync.
    pub pending_declares: Vec<(String, crate::types::LlvmType, Vec<crate::types::LlvmType>)>,

    /// LocalIds that are provably integer-valued — i.e., initialized from
    /// an integer literal and never the target of a `LocalSet` (only the
    /// `Update` expression and reads are allowed). Populated once per
    /// function by `crate::collectors::collect_integer_locals` at each
    /// `compile_*` entry point.
    ///
    /// Used by `BinaryOp::Mod` lowering to emit integer modulo via
    /// `fptosi → srem → sitofp` instead of `frem double`. `frem` lowers to
    /// a libm `fmod()` call on ARM (no hardware instruction), costing
    /// ~15ns per iteration — integer modulo is a single `msub` after
    /// LLVM's SCEV hoists the conversions. Turned factorial
    /// (`sum += i % 1000` in a 100M loop) from 1550ms → ~150ms on ARM.
    pub integer_locals: &'a std::collections::HashSet<u32>,

    /// LocalIds whose writes are all explicit `>>> 0` u32 casts. These locals
    /// can use the same i32 bit-pattern slot as signed integer locals for
    /// bitwise consumers, but ordinary JS reads must convert with `uitofp` so
    /// values above INT32_MAX remain observable as unsigned numbers.
    pub unsigned_i32_locals: &'a std::collections::HashSet<u32>,

    /// Gen-GC Phase A sub-phase 3a: pointer-typed local → shadow-
    /// frame slot index. Empty when `PERRY_SHADOW_STACK` is off.
    /// Sub-phase 3b uses this map at `Stmt::Let` / `LocalSet`
    /// lowering sites to emit `js_shadow_slot_set(idx, bits)` so
    /// the frame reflects the live pointer state at the following
    /// safepoint. Today — just tracked, not consumed.
    pub shadow_slot_map: std::collections::HashMap<u32, u32>,
    /// Top-level statement index → shadow-frame slot indices that can be
    /// cleared after lowering that statement. Built once per user function
    /// from HIR local-reference last-use information.
    pub shadow_slot_clears_after_stmt: std::collections::HashMap<usize, Vec<u32>>,

    /// Cached pointer to this function's `InlineArenaState` slot —
    /// allocated lazily on the first `new ClassName()` site that uses
    /// the inline bump-allocator path. The slot lives in the function
    /// entry block (via `LlFunction::entry_init_call_ptr`) and holds
    /// the result of a one-time `js_inline_arena_state()` call. Each
    /// subsequent `new` in the function loads from this slot instead
    /// of paying a TLS access per allocation.
    ///
    /// `None` until the first `new` lowers; thereafter `Some(slot_name)`
    /// (e.g. `"%r3"`).
    pub arena_state_slot: Option<String>,

    /// Per-class cached `keys_array` global slots. The
    /// `@perry_class_keys_<class>` global is set once at module init,
    /// then read on every `new ClassName()`. LLVM's LICM doesn't hoist
    /// the load out of the loop because the inline-alloc slow path
    /// calls into the runtime and LLVM can't prove the call doesn't
    /// modify the global. We hoist it manually here: the first `new`
    /// site for each class allocates a stack slot, emits a load+store
    /// at function entry (via `entry_init_load_global`), and
    /// subsequent sites for the same class load from the slot.
    pub class_keys_slots: std::collections::HashMap<String, String>,

    /// Per-arr-local cached `arr.length` slots — populated by
    /// `lower_for` when it spots the well-known shape
    /// `for (...; i < arr.length; ...) { body }` and proves via
    /// `stmt_preserves_array_length` that the body doesn't change
    /// `arr.length`. The `PropertyGet { object: LocalGet(arr_id),
    /// property: "length" }` lowering checks this map and, if found,
    /// emits a `load double, ptr <slot>` instead of unboxing the
    /// array and doing a fresh `load i32` of the length field.
    ///
    /// Saves the per-iteration length reload (which LLVM's LICM
    /// declines to do because the IndexSet slow path is an external
    /// call that LLVM can't prove won't modify the length).
    pub cached_lengths: std::collections::HashMap<u32, String>,

    /// `(counter_local_id, array_local_id)` pairs that are guaranteed
    /// inbounds inside the current loop nest — populated by
    /// `lower_for` when it detects the same `for (...; i < arr.length;
    /// ...)` shape that drives `cached_lengths`. The IndexSet codegen
    /// (`lower_index_set_fast`) checks this set: if `arr[i] = expr`
    /// where `(i, arr)` is in the set, the IndexSet skips its
    /// runtime bound check + cap check + realloc fallback entirely
    /// and emits a single inline-store sequence.
    ///
    /// The for-loop guarantees `i < arr.length` is true at the cond
    /// check, and `stmt_preserves_array_length` already proved the
    /// body can't change `arr.length` or reassign `i`, so the
    /// IndexSet site can rely on `i < arr.length` without rechecking.
    pub bounded_index_pairs: Vec<BoundedIndexPair>,

    /// Parallel i32 counter slots for integer loop counters that are
    /// used as bounded array indices. When a for-loop counter is in
    /// `integer_locals` AND appears in `bounded_index_pairs`, `lower_for`
    /// allocates a parallel i32 alloca tracked here. The `Expr::Update`
    /// lowering increments the i32 slot alongside the normal double slot,
    /// and the IndexGet/IndexSet bounded fast-path loads the i32 directly
    /// instead of emitting a `fptosi double → i32` on every iteration.
    ///
    /// Eliminates ~3 cycles per iteration on M-series (fcvtzs latency)
    /// on hot array-walking loops like `for (let i = 0; i < arr.length;
    /// i++) arr[i] = expr`.
    pub i32_counter_slots: std::collections::HashMap<u32, String>,

    /// LocalIds that appear anywhere inside an `index` subexpression of an
    /// array/buffer/typed-array access (`arr[i]`, `buf[k+1]`, `uint8[j]`,
    /// `arr.at(n)`, etc.). Populated once per function by
    /// `crate::collectors::collect_index_used_locals` at each `compile_*`
    /// entry point.
    ///
    /// Used as a gate on the Let-site i32 shadow allocation (issue #140):
    /// without this guard, every mutable integer-valued local got a parallel
    /// i32 slot — fine for real loop counters (`for (let i=0; i<arr.length;
    /// i++) arr[i] = v`, where the i32 load skips a `fptosi` per iteration)
    /// but harmful for pure accumulators (`sum = sum + 1`), where the shadow
    /// turns a clean `load/fadd/store` body into a dual `load/add/store +
    /// dead sitofp+store` body that LLVM's autovectorizer refuses to fold
    /// into a SIMD reduction, especially with the `asm sideeffect`
    /// loop-preservation barrier from issue #74 in place.
    pub index_used_locals: &'a std::collections::HashSet<u32>,

    /// (Issue #436) Locals where every write (Stmt::Let init, LocalSet,
    /// Update) has a strictly-i32-bounded rhs per
    /// `is_strictly_i32_bounded_expr`. Excludes the dangerous
    /// Add/Sub/Mul-of-int-stable arm (the #435 accumulator-overflow
    /// shape) but includes pure bitwise ops (`a & b`, `a ^ b`, `a >> n`),
    /// the explicit i32 coerces (`expr | 0`, `expr >>> 0`), Buffer-byte
    /// loads, MathImul, Update (i++/i--), and calls to clamp /
    /// returns_integer functions.
    ///
    /// Used at the Let-site `needs_i32_slot` gate alongside
    /// `index_used_locals`: a local qualifies for the i32 fast path if
    /// it's transitively-index-used OR strictly-i32-bounded. Image_conv's
    /// FNV-1a `h` accumulator is the latter case — its writes are
    /// `(h ^ dst[i]) | 0` (explicit coerce) and `imul32(h, K)`
    /// (returns_integer call), both strict, so `h` stays on i32 even
    /// though it's never used as an array index.
    pub strictly_i32_bounded_locals: &'a std::collections::HashSet<u32>,

    /// Compile-time i18n resolution context. When `Some`, the
    /// `Expr::I18nString` lowering looks up the translation for the
    /// default locale at compile time and emits the resolved string
    /// (with runtime interpolation for `{name}` placeholders). When
    /// `None`, the lowering falls back to the verbatim key string.
    ///
    /// The data is owned by `compile_module` (built once from
    /// `opts.i18n_table`) and threaded through every `FnCtx`
    /// instantiation as a shared borrow.
    pub i18n: &'a Option<I18nLowerCtx>,

    /// Issue #100: per-site target prefix for `Expr::DynamicImport`.
    /// Maps the path-string from `DynamicImport::paths` to the
    /// sanitized module prefix whose `@__perry_ns_<prefix>` global the
    /// dispatcher must load. Empty if this module performs no dynamic
    /// imports — the empty-map branch keeps codegen safe against a
    /// stray `DynamicImport` node leaking past the resolver.
    pub dynamic_import_path_to_prefix: &'a std::collections::HashMap<String, String>,

    /// Local-variable class aliases: `let_name → class_name` for any
    /// `Stmt::Let { name, init: Some(Expr::ClassRef(class_name)) }`
    /// in the current function. Also propagated through `LocalGet`
    /// chains (`const A = SomeClass; const B = A; new B()`) by
    /// looking up the source local's name via `local_id_to_name`.
    /// Populated by the Stmt::Let lowering in
    /// `crates/perry-codegen/src/stmt.rs` and consulted by `lower_new`
    /// when an `Expr::New { class_name }` lookup in `ctx.classes`
    /// misses — `let C = SomeClass; new C()` then reroutes through
    /// `lower_new("SomeClass", args)` instead of falling back to the
    /// empty-object placeholder.
    ///
    /// Owned per-function: each `compile_function`/`compile_method`/
    /// `compile_closure`/etc. instantiation gets a fresh empty map.
    /// Aliases don't escape function boundaries because the let
    /// binding's scope ends with the function.
    pub local_class_aliases: std::collections::HashMap<String, String>,

    /// Refs #740: when an object literal embeds a class reference in a
    /// field (`const O = { Inner: class extends Base {…} }`), record
    /// `local_id_of_O → { "Inner" → "__anon_class_N" }` so subsequent
    /// `new O.Inner(args)` and `let C = O.Inner; new C(args)` reads can
    /// resolve back to the underlying class. Without this, both fall
    /// through to the empty-object placeholder.
    pub local_class_field_aliases:
        std::collections::HashMap<u32, std::collections::HashMap<String, String>>,

    /// `LocalId → name` lookup table for chained class alias
    /// resolution. The HIR's `Stmt::Let { name, .. }` gives us the
    /// (id, name) pair at lowering time, but the rest of FnCtx tracks
    /// locals by id only (e.g. `ctx.locals: HashMap<u32, String>` is
    /// id → SSA slot, `ctx.local_types` is id → HIR type). To handle
    /// `let B = A; new B()` where `A` is itself a class alias, we
    /// need to look up the *name* of the LocalGet's id so we can
    /// check `ctx.local_class_aliases` (which is keyed by name).
    /// Populated by Stmt::Let alongside `ctx.local_class_aliases`.
    pub local_id_to_name: std::collections::HashMap<u32, String>,

    /// Names of imports that are exported variables (not functions).
    /// When an ExternFuncRef with one of these names appears as a value,
    /// the codegen calls the getter instead of wrapping as a closure.
    pub imported_vars: &'a std::collections::HashSet<String>,

    /// Compile-time constant values for specific module globals. When a
    /// global is a known compile-time constant (e.g., `__platform__`),
    /// its LocalId maps to the constant f64 value here. `lower_if` checks
    /// this to constant-fold comparisons like `if (__platform__ === 1)`
    /// and skip emitting dead branches — essential because those branches
    /// may reference extern FFI functions that don't exist on the current
    /// target (e.g., iOS-only `hone_get_documents_dir` on macOS).
    pub compile_time_constants: &'a std::collections::HashMap<u32, f64>,
    /// Effective LLVM target triple for this compile. Used by a few
    /// platform-sensitive Node compatibility folds.
    pub target_triple: &'a str,
    /// App metadata backing compile-time `perry/system` introspection APIs.
    pub app_metadata: &'a AppMetadata,

    /// Scalar-replaced non-escaping objects. When `let p = new Point(x, y)`
    /// and `p` never escapes, instead of heap-allocating, each field gets a
    /// stack alloca. Map: local_id → (field_name → alloca_slot).
    /// PropertyGet/PropertySet on these locals load/store from the allocas.
    pub scalar_replaced: std::collections::HashMap<u32, std::collections::HashMap<String, String>>,

    /// Exact closed POD record locals lowered to verifier-backed native stack
    /// bytes. The ordinary JS slot for the same local holds the lazily
    /// materialized object, initialized to undefined until a dynamic escape.
    pub pod_records: std::collections::HashMap<u32, crate::native_value::PodLocal>,

    /// Native-arena-backed packed POD record views. The ordinary JS slot holds
    /// the small GC-visible wrapper; native-call lowering consumes this map to
    /// emit the paired `(data_ptr, record_count)` ABI slots.
    pub pod_views: std::collections::HashMap<u32, crate::native_value::PodViewLocal>,

    /// Stack for tracking which local is the target of a scalar-replaced
    /// constructor being inlined. Pushed when entering a scalar-replaced
    /// ctor body, popped on exit. PropertySet on `this` inside the ctor
    /// routes to the alloca in `scalar_replaced[top]`.
    pub scalar_ctor_target: Vec<u32>,

    /// Non-escaping `new` locals identified by escape analysis. Maps
    /// local_id → class_name for `let p = new Point(...)` where `p`
    /// is only used in PropertyGet/PropertySet. The Stmt::Let lowering
    /// intercepts these to emit scalar-replaced field allocas.
    pub non_escaping_news: std::collections::HashMap<u32, String>,

    /// Fields that are actually observed on each scalar-replaced `new` local.
    /// For synthetic anonymous-shape classes, `Stmt::Let` can allocate only
    /// these slots while still evaluating constructor args/stores for side
    /// effects.
    pub non_escaping_new_used_fields:
        std::collections::HashMap<u32, std::collections::HashSet<String>>,

    /// Scalar-replaced non-escaping array literals. When `let arr =
    /// [a, b, c]` and `arr` is only read at constant indices (and for
    /// `.length`), each slot becomes a stack alloca. Map: local_id →
    /// `[slot_0, slot_1, ..., slot_(N-1)]`. IndexGet on
    /// `LocalGet(id), Integer(k)` loads directly from `slots[k]`, and
    /// `PropertyGet LocalGet(id), "length"` folds to the constant N.
    pub scalar_replaced_arrays: std::collections::HashMap<u32, Vec<String>>,

    /// Non-escaping array literals identified by escape analysis. Maps
    /// local_id → length. Used by the Stmt::Let lowering to intercept
    /// `let arr = [a, b, c]` and emit per-index allocas instead of a
    /// heap array, and by `.length` reads to fold to the constant.
    pub non_escaping_arrays: std::collections::HashMap<u32, u32>,
    pub non_escaping_array_used_indices:
        std::collections::HashMap<u32, std::collections::HashSet<u32>>,

    /// Non-escaping object literals identified by escape analysis. Maps
    /// local_id → field names (declaration order, deduplicated). Used by
    /// the Stmt::Let lowering to intercept `let o = { a: x, b: y }` and
    /// emit per-field allocas. PropertyGet/Set on the local's fields
    /// already resolve through `scalar_replaced`, so no separate read path
    /// is required.
    pub non_escaping_object_literals: std::collections::HashMap<u32, Vec<String>>,
    pub non_escaping_object_literal_used_fields:
        std::collections::HashMap<u32, std::collections::HashSet<String>>,

    /// (Issue #50) Module-level const 2D int arrays folded into a flat
    /// `[N x i32]` LLVM constant. Maps local_id → (flat_global_name, rows,
    /// cols). Populated at module compile, before any function lowering.
    /// The `IndexGet` lowering uses this to replace
    /// `IndexGet(IndexGet(LocalGet(id), i), j)` with a direct GEP + load
    /// of the flat global, eliminating the arena pointer chase and the
    /// per-access NaN-box unwrap.
    pub flat_const_arrays: &'a std::collections::HashMap<u32, FlatConstInfo>,

    /// Clamp-pattern function IDs. Call sites emit smin/smax inline.
    pub clamp3_functions: &'a std::collections::HashSet<u32>,
    pub clamp_u8_functions: &'a std::collections::HashSet<u32>,
    pub integer_returning_functions: &'a std::collections::HashSet<u32>,
    pub i32_identity_functions: &'a std::collections::HashSet<u32>,

    /// True if `perry_transform::unroll_static_loops` expanded any
    /// static-trip-count for-loop in the function this FnCtx is lowering
    /// (or in `module.init` for the module-init lowering). Read by the
    /// channel-vector SIMD reduction gate in `lower_stmts` to decide
    /// whether to skip the manual `<4 x i32>` reduction in favour of
    /// LLVM's auto-vectorizer + constant-folding. The unroll exposes the
    /// kernel coefficients as compile-time literals; the manual SIMD
    /// pre-commits to a `<4 x i32>` shape that fights LLVM's freedom to
    /// pick mul-by-shift / mul-by-1-elimination across the unrolled
    /// body. See `image_convolution`'s blur kernel: post-unroll without
    /// manual SIMD = 310-320 ms vs with manual SIMD = 350-360 ms.
    pub was_unrolled: bool,

    /// (Issue #51) Counter for per-site inline cache globals.
    pub ic_site_counter: u32,

    /// (Issue #51) Names of IC globals created during lowering. After
    /// the function is emitted, the caller emits `@<name> = private
    /// global [2 x i64] zeroinitializer` for each entry.
    pub ic_globals: Vec<String>,

    /// Issue #179 typed-parse: raw rodata globals emitted by
    /// `JsonParseTyped` codegen. Each entry is the full LLVM IR line
    /// `@<name> = private unnamed_addr constant [N x i8] c"..."` to
    /// append after the function finishes. Mirrors the `ic_globals`
    /// drain pattern. Also: counter for unique names at each call
    /// site in this function.
    pub typed_parse_rodata: Vec<String>,
    pub typed_parse_counter: u32,

    /// (Issue #50) Per-function row aliases. When a function declares
    /// `let krow = X[i]` where `X` is in `flat_const_arrays`, this map
    /// records `krow_id → (X_id, <cloned row_index expr>)`. The
    /// `IndexGet` lowering then recognises `krow[j]` as a flat-const
    /// access and emits the same fast path as the inline `X[i][j]`
    /// shape.
    pub array_row_aliases: std::collections::HashMap<u32, (u32, Box<perry_hir::Expr>)>,

    /// Pre-computed `ptr`-typed data-base-pointer slots for Buffer/Uint8Array
    /// locals. When HIR facts prove a non-mutable local owns a fresh u8 buffer,
    /// the lowering computes the data pointer (handle + 8, past the
    /// BufferHeader) once and stores it in a
    /// `ptr`-typed alloca. `Uint8ArrayGet/Set` then emits
    /// `getelementptr inbounds i8, ptr %base, i32 %idx` instead of the
    /// `inttoptr(handle + offset)` chain — giving LLVM proper pointer
    /// provenance so the LoopVectorizer can identify array bounds and
    /// auto-vectorize.
    ///
    /// Value: `(ptr_alloca, alias_scope_idx)` — the scope index is used
    /// to attach `!alias.scope` / `!noalias` metadata that proves
    /// different buffers don't alias (fixes the vectorizer's "unsafe
    /// dependent memory operations" remark).
    pub buffer_data_slots: std::collections::HashMap<u32, (String, u32)>,
    /// Codegen-level native buffer views keyed by LocalId. This is the
    /// representation model behind `buffer_data_slots`: raw pointer access can
    /// exist with `AliasState::Unknown`, while noalias metadata requires a
    /// proven/guarded alias state at the consumer.
    pub buffer_view_slots: std::collections::HashMap<u32, BufferViewSlot>,
    /// Local owner-handle aliases for native arenas. Values are canonical
    /// owner local ids used by native-owned typed-array view proof state.
    pub native_arena_owner_aliases: std::collections::HashMap<u32, u32>,
    /// Owner-handle aliases whose canonical owner is path-dependent after
    /// control-flow merge. Hazards through these locals conservatively
    /// invalidate every native-owned view.
    pub native_arena_ambiguous_owner_aliases: std::collections::HashSet<u32>,
    /// Benchmark/debug switch that forces tracked buffers through the existing
    /// helper fallback instead of native GEP/load/store lowering.
    pub disable_buffer_fast_path: bool,
    /// LocalId facts of the form `n = min(src.length, dst.length)`.
    pub min_length_bounds: std::collections::HashMap<u32, Vec<u32>>,
    /// Loop-local facts proving a buffer index is bounded inside the current
    /// loop body.
    pub bounded_buffer_index_pairs: Vec<BoundedBufferIndex>,
    /// Branch/loop-condition facts proving `index + width <= view.length`.
    /// These are scoped like loop facts and consumed only for accesses whose
    /// required width does not exceed the guarded width.
    pub guarded_buffer_index_pairs: Vec<GuardedBufferIndex>,
    pub buffer_hazard_reasons: std::collections::HashMap<u32, MaterializationReason>,
    /// Local aliases that preserve an i32 index, e.g. `const j = i | 0`.
    pub native_i32_aliases: std::collections::HashMap<u32, u32>,
    /// Immutable numeric aliases used by the range-based buffer proof. These
    /// remain HIR expressions so loop-local range facts can be applied at the
    /// eventual access site.
    pub int_range_aliases: std::collections::HashMap<u32, perry_hir::Expr>,
    /// Scoped local integer ranges derived from loop/while guards.
    pub int_range_facts: Vec<IntRangeFact>,
    /// Monotonic source for loop-local proof scopes. Loop exit removes only
    /// facts created with its exact scope id, so invalidation of older facts
    /// cannot make newer inner-loop facts survive via shifted vector indices.
    pub next_loop_proof_scope_id: u32,
    /// Mutable locals known to be non-negative at the current point. While
    /// guards provide the upper bound; this set supplies the lower bound.
    pub nonnegative_integer_locals: std::collections::HashSet<u32>,
    /// Native representation records drained into `LlModule` after this
    /// function/method/closure/module-init body has been lowered.
    pub native_rep_records: Vec<NativeRepRecord>,
    /// Immutable locals whose initializer creates a fresh u8 buffer backing
    /// store. Collected once as a HIR fact and consumed by Let lowering to seed
    /// direct data-pointer slots plus noalias metadata.
    pub known_noalias_buffer_locals: &'a std::collections::HashSet<u32>,
    /// Starting alias-scope id for buffers registered in this function.
    /// Seeded from `LlModule::buffer_alias_counter` at FnCtx creation so
    /// scope ids don't collide across functions in the same LLVM module.
    /// New scopes are allocated as `base + buffer_data_slots.len()`;
    /// after the function finishes lowering the caller bumps the module
    /// counter by the number of slots it used (closes #71).
    pub buffer_alias_base: u32,
}

/// (Issue #50) Info about a flat-folded const 2D int array.
#[derive(Debug, Clone)]
pub struct FlatConstInfo {
    pub global_name: String,
    pub rows: usize,
    pub cols: usize,
}

/// Per-module i18n table snapshot used by the LLVM codegen to resolve
/// `Expr::I18nString` against the default locale at compile time.
///
/// `translations` is a flat 2D array `[locale_idx * key_count + string_idx]`
/// matching `perry_transform::i18n::I18nStringTable::translations`. The
/// codegen uses `default_locale_idx` to pick a row.
#[derive(Debug, Clone)]
pub struct I18nLowerCtx {
    pub translations: Vec<String>,
    pub key_count: usize,
    pub default_locale_idx: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct BoundedIndexPair {
    pub index_local_id: u32,
    pub array_local_id: u32,
    pub scope_id: u32,
}

impl<'a> FnCtx<'a> {
    pub fn next_loop_proof_scope_id(&mut self) -> u32 {
        let id = self.next_loop_proof_scope_id;
        self.next_loop_proof_scope_id = self
            .next_loop_proof_scope_id
            .checked_add(1)
            .expect("loop proof scope id overflow");
        id
    }

    pub fn block(&mut self) -> &mut LlBlock {
        self.func
            .block_mut(self.current_block)
            .expect("current_block index points at a valid block")
    }

    /// Create a new block and return its index, **without** switching the
    /// current_block pointer. The caller is responsible for deciding when
    /// to flip.
    pub fn new_block(&mut self, name: &str) -> usize {
        let _ = self.func.create_block(name);
        self.func.num_blocks() - 1
    }

    /// Label of a block by index — needed when emitting a branch.
    pub fn block_label(&self, idx: usize) -> String {
        self.func
            .blocks()
            .get(idx)
            .map(|b| b.label.clone())
            .expect("valid block index")
    }

    fn typed_feedback_site_id(&self, local_site_id: u32) -> u64 {
        let mut h = 0x811c9dc5u32;
        for b in self.strings.module_prefix().bytes() {
            h ^= b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        (((h & 0x7fff_ffff) as u64) << 32) | local_site_id as u64
    }

    pub fn current_block_label(&self) -> String {
        self.block_label(self.current_block)
    }

    pub fn region_id_for_label(&self, label: &str) -> String {
        format!(
            "{}.{}.{}",
            self.module_slug,
            self.source_function_slug,
            native_region_slug(label)
        )
    }
}

// Issue #1098 phase 2: lower_expr arm-bodies extracted into
// per-chunk sibling modules. The dispatch in `lower_expr` below routes each
// variant to its module's `lower(ctx, expr)` helper.
mod array_methods;
mod array_push;
mod arrays_finds;
mod bigint_set;
mod binary;
mod call_spread;
pub(crate) mod calls;
mod child_proc;
mod closure;
mod compare;
mod conditional;
mod dyn_extern_i18n;
mod env_clones;
mod fs_await;
mod index_get;
mod index_set;
mod instance_misc1;
pub(crate) use instance_misc1::builtin_parent_reserved_class_id;
mod class_field_inline_guard;
mod js_runtime;
mod literals_vars;
mod logical_collections;
mod math_simple;
mod misc_methods;
mod new_dynamic;
mod objects_arrays_lit;
mod os_uri_dates;
mod property_get;
mod property_set;
pub(crate) mod proxy_reflect;
mod static_field_meta;
mod static_method;
mod string_regex_proc;
mod super_method;
mod this_super_call;
pub(crate) use this_super_call::is_other_builtin_constructor_name;
mod unary;
mod url_main;
