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

pub mod reach_report;
pub(crate) mod realm_image;
pub mod source_need;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use anyhow::{Context, Result};
use perry_hir::Module as HirModule;

use crate::module::LlModule;
use crate::runtime_decls;
use crate::strings::StringPool;
use crate::types::{LlvmType, DOUBLE, I32, I64};

mod class_key_tables;
mod compile_progress;
mod imported_class_stubs;
mod method_signature_tables;
mod module_constants;
pub(super) use compile_progress::CompileProgress;

#[cfg(test)]
mod argument_shape_clone_tests;
pub(crate) mod arguments;
mod artifact_context;
mod artifact_display_names;
mod artifact_source_text;
mod artifacts;
pub(crate) mod boxed_locals;
pub(crate) mod cjs_exports;
mod class_artifacts;
pub(crate) mod class_hierarchy;
#[cfg(test)]
mod clone_suffix_tests;
pub(crate) mod closed_targets;
mod closure;
mod closure_capture_cells;
mod closure_captures;
mod closure_collect;
mod constructor_contracts;
mod deferred_namespace;
#[cfg(test)]
mod number_local_root_tests;
mod shared_class_method_bodies;
pub use constructor_contracts::{ConstructorContracts, ResolvedConstructorContracts};
mod ctor_arity;
pub use ctor_arity::{
    context_free_ctor_abi, context_free_ctor_param_count, CtorAbi, UNRESOLVED_PARENT_FWD_ARITY,
};
#[cfg(test)]
mod buffer_alias_metadata_tests;
mod checked_integer;
#[cfg(test)]
mod declared_string_add_tests;
#[cfg(test)]
mod emission_order_tests;
mod entry;
pub mod entry_outline;
mod export_value_wrappers;
pub(crate) mod facts_const_length;
pub(crate) mod facts_int32_locals;
pub(crate) mod facts_number_locals;
#[cfg(test)]
mod facts_number_locals_tests;
pub(crate) mod facts_rep;
pub(crate) mod fresh_class_templates;
pub(crate) mod func_registry;
mod function;
mod function_source_header;
#[cfg(test)]
mod guarded_falsy_default_method_tests;
#[cfg(test)]
mod guarded_undefined_method_tests;
#[cfg(test)]
mod hoisted_callback_method_tests;
#[cfg(test)]
mod imported_global_order_tests;
pub(crate) mod index_entry;
#[cfg(test)]
mod index_method_clone_tests;
mod indexed_method_artifacts;
mod integer_contract;
#[cfg(test)]
mod literal_method_this_tests;
mod ordinary_method_artifacts;
pub(crate) mod record_array;
pub(crate) mod record_sig;
pub(crate) mod signature_entry;
pub mod split_regions;
#[cfg(test)]
mod split_regions_literal_tests;
#[cfg(test)]
mod split_regions_tests;
#[cfg(test)]
mod string_accumulator_tests;
mod tdz_names;
// `pub(crate)` so `crate::linker` can read the inline-hot-small policy
// (`inline_hot_small_enabled` / `inline_hot_small_hint_threshold`).
pub(crate) mod helpers;
// #10399: worker entries supplied before any module codegen runs.
pub use helpers::{set_worker_entries, worker_entries};
pub(crate) mod accessor_numeric;
pub(crate) mod global_transfer;
pub(crate) mod inherited_accessors;
pub(crate) mod literal_constructor;
mod method;
mod method_registry;
mod method_trampolines;
mod module_globals_emit;
pub(crate) mod namespace_value_getters;
mod native_namespace_exports;
#[cfg(test)]
mod number_exactness_tests;
mod opts;
mod program_facts_view;
pub use program_facts_view::{
    AccessorKind, DirectAbi, ElisionFacts, FactsArrRep, FactsBody, FactsClass, FactsElemRep,
    FactsElementRead, FactsEnc, FactsField, FactsFieldReadCall, FactsIndexedOwner,
    FactsNumericExpr, FactsNumericFunction, FactsRecRep, FactsRecordBirth, FactsRecordFn,
    FactsRecordLanes, FactsRecordLayout, FactsRecv, FactsRep, FactsSignature, ProgramFactsView,
};
#[cfg(test)]
mod ordinary_param_guard_tests;
mod param_guard;
mod retained_source_pool;
mod spec_abi;
#[cfg(test)]
mod spec_preserve_none_tests;
mod spec_return_proof;
#[cfg(test)]
mod spec_self_recursion_tests;
#[cfg(test)]
mod spec_uint8_ta_ptr_tests;
pub(crate) mod static_constfn;
pub(crate) mod static_constfn_class;
pub(crate) mod static_fields;
pub(crate) mod static_private_class;
pub(crate) mod static_prototype;
pub(crate) mod static_shape_ids;
pub use static_shape_ids::{
    assign_static_shape_ids, decode_static_seed, encode_static_seed, take_module_static_seeds,
    BirthProto, BirthShape, ConstFnBirth, DefinedClassShape, ModuleBirth, ProgramClassShapeIds,
    TypedMasks, STATIC_SEED_FORMAT,
};
pub(crate) use static_shape_ids::{
    compatible_final_layout_words, compatible_final_layouts, key_may_be_constfn_at,
    slot_may_be_constfn, static_layout_word_for_keys_global, static_region_slots,
};
mod class_declarations;
mod string_pool;

/// The image-string address expression of a pool entry (object model step 0's
/// static atoms point at it).
pub(crate) fn string_pool_image_pointer(entry: &crate::strings::StringEntry) -> String {
    string_pool::image_pointer(entry)
}
pub(crate) use string_pool::{emit_image_record, image_record_symbol};
pub(crate) mod numeric_remainder;
#[cfg(test)]
mod testing_feature_gate_tests;
#[cfg(test)]
mod trusted_box_callback_tests;
mod typed_abi;
mod typed_abi_opt_report;
mod typed_entry;
pub(crate) use typed_entry::{
    emit_ordered_number_test, emit_plain_number_bits_test, emit_plain_number_test,
    typed_entry_arg_guard,
};
#[cfg(test)]
mod unknown_func_tests;

pub(crate) use closure::emit_typed_capture_guard;
pub(crate) use function::arena_threaded_function_body_name;
pub(crate) use helpers::{
    decide_codegen_units, decide_full_outline_ic, default_target_triple, full_outline_ic_enabled,
    module_callable_count, set_full_outline_ic, write_barriers_enabled,
};
pub use helpers::{resolve_target_triple, target_os_of, TARGET_NAMES};
pub use opts::{
    namespace_member_class_key, namespace_member_func_key, namespace_member_var_key, AppMetadata,
    CompileOptions, ExportedObjectLiteralCapability, FpContractMode, ImportedClass,
    ImportedObjectLiteral, ImportedObjectLiteralMethod, NamespaceEntry, NamespaceEntryKind,
    ObjectLiteralMethodCandidate,
};
pub(crate) use opts::{CrossModuleCtx, ImportedCtor};
pub(crate) use param_guard::scalar_descriptor_rep;
pub(crate) use spec_abi::{
    spec_abi_enabled, spec_entry_name, spec_function_name, SpecDispatch, SpecFnPlan,
};
pub(crate) use typed_abi::{
    emit_typed_arg_guard, emit_typed_arg_to_raw, emit_typed_f64_guard, emit_typed_i1_guard,
    emit_typed_i32_plain_guard_and_raw, emit_typed_string_guard, generic_closure_body_name,
    generic_function_body_name, generic_method_body_name, nonnegative_index_fast_array_method_name,
    nonnegative_index_fast_array_params, nonnegative_index_method_name,
    typed_arg_is_guard_candidate, typed_f64_closure_name, typed_f64_function_name,
    typed_f64_method_name, typed_f64_receiver_method_info, typed_f64_receiver_method_name,
    typed_i1_closure_name, typed_i1_function_name, typed_i1_method_name, typed_i32_closure_name,
    typed_i32_function_name, typed_i32_method_name, typed_param_reps_match_args,
    typed_string_closure_name, typed_string_function_name, typed_string_method_name, TypedParamRep,
    TypedReceiverMethodInfo,
};

use artifact_context::ModuleArtifactsCtx;
use artifacts::emit_module_artifacts;
use function::{
    compile_function, compile_typed_f64_function, compile_typed_i1_function,
    compile_typed_i32_function, compile_typed_string_function,
};
use helpers::{
    emit_buffer_alias_metadata, function_body_is_fixed_state_generator,
    function_body_returns_generator_object, sanitize,
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
    let source_function = source_function.into();
    // `--opt-report` (#6952): surface the specialized-ABI (RFC Phase 2)
    // decision, which is the only place params and returns get a
    // representation today. The `typed_*_clone_decision` consumers are the
    // older per-type clone mechanism and would report the same function up
    // to four times, so they stay out of the report and keep going to the
    // native-reps artifact only.
    if consumer == "spec_abi_entry_decision" && crate::opt_report::enabled() {
        let (why, tier, issue) = reason.opt_report_reason();
        crate::opt_report::deny_named(
            &source_function,
            crate::opt_report::RegionKind::Function,
            crate::opt_report::Denial {
                position: crate::opt_report::Position::Param,
                name: "(parameters + return)",
                local_id: None,
                analysis: crate::opt_report::Analysis::SpecAbi,
                rule: reason.as_str(),
                reason: why,
                tier,
                issue,
                loop_depth: 0,
                detail: None,
                byte_offset: None,
            },
        );
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

/// Return the exact LLVM symbol used for a top-level HIR function.
///
/// The module prefix and function component deliberately use different
/// manglers: module names keep the stable public ABI, while function names
/// use the injective member mangler so `$a` and `_a` cannot collide. Compile
/// orchestration that prepares cross-module metadata must call this helper
/// instead of reconstructing the symbol with the module-name sanitizer.
pub fn user_function_symbol(module_name: &str, function_name: &str) -> String {
    let module_prefix = sanitize(module_name);
    helpers::scoped_fn_name(&module_prefix, function_name)
}

mod module_compile;
pub use module_compile::{compile_module, compile_module_owned, module_birth_shapes};

fn ensure_layout_plan(hir: &HirModule, opts: &mut CompileOptions) -> Result<()> {
    if !opts.program_layout_ordinals.entries.is_empty() {
        return Ok(());
    }
    let births = module_birth_shapes(hir, opts.clone())?;
    if opts.static_shape_ids.is_empty() {
        opts.static_shape_ids = assign_static_shape_ids(
            births
                .iter()
                .map(|b| &b.shape)
                .filter(|s| !s.is_completed()),
        )
        .into_iter()
        .collect();
        opts.static_shape_ids.sort();
    }
    let assigned = assign_static_shape_ids(
        births
            .iter()
            .map(|b| &b.shape)
            .filter(|s| !s.is_completed()),
    );
    for (shape, id) in assigned {
        if !opts.static_shape_ids.iter().any(|(s, _)| *s == shape) {
            opts.static_shape_ids.push((shape, id));
        }
    }
    opts.static_shape_ids.sort();
    let mut inputs = crate::layout_ordinals::take_module_layout_inputs();
    inputs.extend(
        opts.static_shape_ids
            .iter()
            .map(|(s, id)| crate::layout_ordinals::identity_input(s, *id)),
    );
    opts.program_layout_ordinals = std::sync::Arc::new(
        crate::ProgramLayoutOrdinals::assign(crate::layout_ordinals::native_inputs(), inputs)
            .map_err(|e| anyhow::anyhow!(e))?,
    );
    Ok(())
}

/// Lower the module, or collect births and return before body lowering.
fn lower_module(
    hir: &HirModule,
    opts: CompileOptions,
    births: Option<&mut Vec<ModuleBirth>>,
) -> Result<Option<LoweredModule>> {
    let reach_census = reach_report::active();
    let _reach_scope = reach_census.as_ref().map(|_| reach_report::enter_module());
    let collect_births = births.is_some();
    crate::layout_ordinals::begin_module(
        &hir.name,
        opts.program_layout_ordinals.clone(),
        collect_births,
    );
    // No family facts of a previous module on this thread reach this one's
    // early analyses; this module's are installed once its births are known.
    crate::layout_desc::set_module_families(Default::default(), Vec::new());
    crate::layout_desc::create_families::clear();
    let (live_cjs_hir, cjs_property_exports) = cjs_exports::prepare(hir);
    let hir = live_cjs_hir.as_ref();
    // VR-3a: the facts' record bindings are their fields by
    // value (`value_records`); every lowering below sees the rewritten body.
    let mut opts = opts;
    let records_hir = crate::value_records::rewrite_module(hir, &mut opts.program_facts.elision);
    let hir = records_hir.as_ref().unwrap_or(hir);
    // The driver supplies whole-program ownership in opts. Direct callers
    // without callback prefixes also need local launch detection for their
    // string-preparation callback. All decisions remain per compile.
    let mut local_thread_use = false;
    if opts.thread_literal_module_prefixes.is_empty() {
        perry_hir::for_each_module_expr(hir, &mut |expr| {
            if matches!(expr, perry_hir::Expr::NativeMethodCall { module, method, .. }
                if module == "perry/thread"
                    && matches!(method.as_str(), "spawn" | "parallelMap" | "parallelFilter"))
            {
                local_thread_use = true;
            }
        });
    }
    let thread_agents = opts.program_has_thread_agents
        || !opts.thread_literal_module_prefixes.is_empty()
        || local_thread_use;
    let agent_strings_tls = opts.program_has_worker || thread_agents;
    let progress = CompileProgress::new(&hir.name, module_callable_count(hir), collect_births);
    let triple = opts.target.clone().unwrap_or_else(default_target_triple);
    // ConstFn lanes are one decision for the whole compile, from its options.
    let constfn_enabled = static_constfn::enabled(&opts);
    if let Some(refusal) = crate::target_layout::ilp32_codegen_refusal(&triple) {
        anyhow::bail!(refusal);
    }
    // `PERRY_REGION_DIAG=1`: report step 4b's regions and the statement-level
    // runs it does not reach, when this module's codegen ends.
    let _region_diag = (!collect_births).then(|| crate::expr::region_guard::ModuleDiag::start(hir));
    crate::stmt::region_loop::begin_module(hir);
    let fp_flags = crate::block::FpFlags::new(opts.fast_math, opts.fp_contract_mode);

    // #5334 lever B: decide ONCE, up front, whether this module is large enough
    // to full-outline its class-field IC diamonds (read per-site during
    // lowering via `full_outline_ic_enabled()`). Thread-local, so it must be set
    // afresh for every module — including the `false` case, to clear any prior
    // module's decision on this thread.
    set_full_outline_ic(decide_full_outline_ic(module_callable_count(hir)));
    // Initial-exec TLS only in an image that is never `dlopen`ed.
    crate::expr::agent_ptr::set_output_is_executable(opts.output_type == "executable");
    // #8595: report the module-entry outlining analysis when asked. Pure
    // diagnostic — no transform yet (see codegen/entry_outline.rs).
    if !collect_births {
        entry_outline::report_entry_outlining(hir);
    }
    // FEAT_JSCVT decision is per-target (apple-arm64 only) — same
    // set-per-module discipline as the outline gate above.
    helpers::set_jscvt_for_target(&triple);
    helpers::set_js_text_section_for_target(&triple);
    // Native roots are the default lowering wherever the runtime can walk the
    // frames, and the shadow stack elsewhere. Same per-module discipline.
    helpers::set_native_roots_for_target(&triple);
    // ILP32 (wasm32 WASI only): inline paths that bake in LP64 layouts take
    // their runtime-call arms or ILP32 offsets. Same per-module discipline.
    helpers::set_ilp32_for_target(&triple);

    // `--opt-report` (#6952): mark the closures that are iterating-builtin
    // callbacks before any region is lowered, so their denials carry the
    // per-element hotness column. No-op when the report is off.
    if !collect_births {
        crate::opt_report::scan_module(hir);
    }
    if let Some(source) = opts.module_source.as_deref().filter(|_| !collect_births) {
        crate::opt_report::register_module_source(&hir.name, source, opts.debug_source_line_offset);
    }
    // Module-wide fallback attribution scope. Per-region scopes nest inside
    // it and restore it on drop, so decisions taken outside any region (the
    // specialized-ABI entry decision) still know their module.
    let _opt_report_module_scope = crate::opt_report::enter_module(hir);

    let mut llmod = LlModule::new_with_fp_flags(&triple, fp_flags);
    llmod.program_has_worker = opts.program_has_worker;
    // Only a complete executable can prove that none of its compiled code
    // can run under a worker's termination handle. Reusable libraries and
    // modules lacking this whole-program proof retain interrupt polls.
    if opts.output_type == "executable" && !opts.program_has_worker {
        llmod.add_metadata_line("!perry.no_workers = !{!0}".to_string());
    }
    llmod.set_gc_effects(std::sync::Arc::clone(&opts.gc_effects));
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
    // Anonymous rodata constants (`add_string_constant`) carry the prefix
    // too: codegen-unit splitting promotes them to link-visible symbols,
    // and their per-module counter would otherwise collide across modules
    // at the final link (GNU ld: `multiple definition of .str.N`).
    llmod.set_symbol_prefix(&module_prefix);
    // wpfacts slice 1: contract builds only (empty otherwise).
    program_facts_view::emit_contract_section(
        &mut llmod,
        &module_prefix,
        &opts.program_facts.contract_section,
    );
    // Null guard global: a zeroed i32 used as a safe dereference target
    // when a NaN-unboxed pointer is null/invalid. Prevents segfaults from
    // uninitialized locals or unhandled expressions producing 0.0/TAG_UNDEFINED.
    // Defined AFTER the prefix is installed so its name is module-unique
    // (`perry_null_guard_zero_<prefix>`) — split units export it strong on
    // ELF/COFF, exactly like the string constants above.
    llmod.add_internal_global(&llmod.null_guard_global(), crate::types::I32, "0");

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
    let thread_literal_callback_prefix = opts
        .thread_literal_module_prefixes
        .first()
        .unwrap_or(&module_prefix);
    let mut strings = StringPool::with_prefix(module_prefix.clone());
    strings.set_thread_literal_callback_prefix(thread_literal_callback_prefix.clone());
    strings.set_image_literals(opts.output_type != "dylib" && opts.output_type != "staticlib");
    strings.tdz_binding_names = tdz_names::collect(hir);
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

    let imported_class_stubs::ImportedClassStubs {
        imported_class_stubs,
        imported_stub_prefixes,
        shadowed_parent_stubs,
        imported_class_prefix,
        imported_class_source_name,
    } = imported_class_stubs::build_imported_class_stubs(hir, &opts, &class_table, &mut class_ids);
    for stub in &imported_class_stubs {
        class_table.entry(stub.name.clone()).or_insert(stub);
    }

    // Local async function FuncIds — populated below from `hir.functions`
    // (the per-function loop further down). Built here so the CrossModuleCtx
    // construction is complete before the FnCtx instances reference it.
    let mut local_async_funcs: std::collections::HashSet<u32> = std::collections::HashSet::new();
    let mut local_generator_funcs: std::collections::HashSet<u32> =
        std::collections::HashSet::new();
    let mut fixed_generator_funcs: std::collections::HashSet<u32> =
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
            if function_body_is_fixed_state_generator(&f.body) {
                fixed_generator_funcs.insert(f.id);
            }
        }
        if perry_hir::analysis::body_reads_dynamic_this(&f.body) {
            funcs_reading_dynamic_this.insert(f.id);
        }
    }

    let class_key_tables::ClassKeyTables {
        class_keys_init_data,
        class_keys_globals_map,
        class_field_counts_map,
        class_init_chains_map,
        defined_class_keys_len,
    } = class_key_tables::build_class_key_tables(
        hir,
        &module_prefix,
        &mut llmod,
        &mut strings,
        &imported_class_stubs,
        &imported_stub_prefixes,
        &shadowed_parent_stubs,
    );

    let platform_number = module_constants::platform_number(&triple);
    progress.checkpoint("symbol tables and initial declarations");
    // #8595: after entry outlining, declaration-bearing statements live in
    // compiler-owned chunk functions. Analyses that model the module's source
    // environment use the reconstructed stream, not the compact call-only
    // `hir.init`, so immutable initializer facts and TDZ/prealloc metadata are
    // unchanged by the structural transform.
    let logical_entry_stmts = entry_outline::logical_entry_stmts(hir);

    let compile_time_constants =
        module_constants::compile_time_constants(&logical_entry_stmts, platform_number);

    // #7286 lever (c): interprocedural integer ranges for numeric function
    // parameters, computed once per module from the same folded top-level
    // `const` map the call-site arguments resolve through.
    let param_int_ranges_summary =
        crate::collectors::collect_param_int_ranges(hir, &compile_time_constants);

    let method_signature_tables::MethodSignatureTables {
        method_param_counts,
        method_has_rest,
        method_has_synthetic_arguments,
        method_arguments_length_only,
    } = method_signature_tables::build_method_signature_tables(hir, &opts);

    let func_returns_class_map = module_constants::func_returns_class_map(hir);

    progress.checkpoint("module constant and export analysis");

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
        // A split region is one body by design: no typed-ABI clone.
        if split_regions::is_region_function(f) {
            continue;
        }
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
    let nonnegative_index_methods: std::collections::HashMap<(String, String), Vec<u32>> = hir
        .classes
        .iter()
        .flat_map(|class| {
            class.methods.iter().filter_map(|method| {
                let params = typed_abi::nonnegative_index_method_params(method);
                (!params.is_empty()).then(|| ((class.name.clone(), method.name.clone()), params))
            })
        })
        .collect();
    // One bounded full-body version per eligible method. The erased optional
    // annotation only nominates a candidate; the public wrapper emitted in
    // `codegen/method.rs` guards the actual argument bits before the clone can
    // consume a `Type::Void` proof. Keep the module-wide cap explicit so a
    // source file with many optional loop filters cannot grow without bound.
    let mut guarded_undefined_method_candidates: Vec<_> = hir
        .classes
        .iter()
        .flat_map(|class| {
            class.methods.iter().filter_map(move |method| {
                param_guard::guarded_undefined_method_candidate(method).map(|candidate| {
                    (
                        candidate.body_nodes,
                        (class.name.clone(), method.name.clone()),
                        candidate.param_index,
                    )
                })
            })
        })
        .collect();
    let mut guarded_falsy_field_default_candidates: Vec<_> = hir
        .classes
        .iter()
        .flat_map(|class| {
            class.methods.iter().filter_map(|method| {
                let key = (class.name.clone(), method.name.clone());
                if !nonnegative_index_methods.contains_key(&key) {
                    return None;
                }
                param_guard::guarded_falsy_field_default_method_candidate(class, method)
                    .map(|candidate| (candidate.body_nodes, key, candidate))
            })
        })
        .collect();
    progress.checkpoint("cross-module and typed-ABI analysis");

    // Module-wide dispatch/barrier facts. Hoisted above the typed-clone
    // eligibility loop because representation-selection Phase 5a's
    // proven-`this` admission consults them (§5.2 shape barriers, the
    // freeze family, and `prototype_is_stable`). Moved into `CrossModuleCtx`
    // below — computed exactly once per module either way.
    let mut module_dispatch_facts = crate::collectors::collect_module_dispatch_facts_with(
        hir,
        &opts.program_facts.elision.native_member_calls,
    );
    let imported_return_shapes = opts
        .imported_classes
        .iter()
        .flat_map(|class| {
            class
                .return_shape_imports
                .iter()
                .map(move |local| (local.clone(), class.name.clone()))
        })
        .collect();
    module_dispatch_facts.install_imported_return_shapes(imported_return_shapes);
    module_dispatch_facts.install_array_record_protocol_dead(
        crate::expr::array_iteration_facts(&opts.program_facts.elision) == 0
            && opts.program_facts.elision.array_iterator_close_unwritten,
    );
    // Representation-selection Phase 5a: proven-`this` method clones.
    let mut pshape_methods: std::collections::HashMap<
        (String, String),
        crate::collectors::PtrShapeLocal,
    > = std::collections::HashMap::new();
    // #7142: the profitability subset the class-id dispatch tower may route to.
    let mut pshape_tower_routable: std::collections::HashSet<(String, String)> =
        std::collections::HashSet::new();
    // Phase 3b typed-receiver widening: chain-global field indexes need the
    // full class table — and it must be the SAME table dynamic dispatch's
    // call-site gating consults (`class_table`, incl. class-expression
    // aliases), or a chain resolvable only through an alias would gate a
    // clone call the emission loop never produced (undefined symbol at
    // link).
    let receiver_class_table = &class_table;
    for class in &hir.classes {
        for (prefix, accessors) in [("__get_", &class.getters), ("__set_", &class.setters)] {
            for (property, method) in accessors {
                if class.static_accessor_fn_ids.contains(&method.id) {
                    continue;
                }
                let mut method = method.clone();
                method.name = format!("{prefix}{property}");
                if let Some(fact) = crate::collectors::accessor_proven_this(
                    class,
                    &method,
                    receiver_class_table,
                    &module_dispatch_facts,
                ) {
                    pshape_methods.insert((class.name.clone(), method.name), fact);
                }
            }
        }
        for method in &class.methods {
            let source_function = format!("{}::{}", class.name, method.name);
            // Representation-selection Phase 5a: does this method admit a
            // proven-`this` clone? Uses the SAME class table as the
            // typed-receiver decision below, for the same reason — the two
            // routing sites gate on this map, and a chain resolvable only
            // through an alias would gate a call to a symbol the emission
            // loop never produced.
            if let Some(fact) = crate::collectors::method_proven_this(
                class,
                method,
                receiver_class_table,
                &module_dispatch_facts,
            ) {
                let key = (class.name.clone(), method.name.clone());
                // Indexed methods compose this receiver proof with their
                // exact nonnegative-i32 argument proof. Their published
                // The published proven-receiver entry guards the live
                // argument once, then selects its combined integer clone or
                // its receiver-safe generic body.
                pshape_methods.insert(key, fact);
            }
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
            match typed_abi::typed_f64_receiver_method_info(class, method, receiver_class_table) {
                Some(info) => {
                    typed_f64_receiver_methods
                        .insert((class.name.clone(), method.name.clone()), info);
                }
                None => {
                    if let Some(reason) = typed_abi::typed_f64_receiver_method_rejection_reason(
                        class,
                        method,
                        &receiver_class_table,
                    ) {
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
    // #7142: the tower routing site emits its own inline shape re-check, so it
    // only takes a clone where that clone deletes strictly more guarded work
    // than the check costs. Price these routes after all local clone facts are
    // known so nested `this.other()` calls can count an inherited clone too.
    let local_pshape_methods: std::collections::HashSet<(String, String)> =
        pshape_methods.keys().cloned().collect();
    for class in &hir.classes {
        for method in &class.methods {
            let key = (class.name.clone(), method.name.clone());
            if local_pshape_methods.contains(&key)
                && crate::collectors::pshape_tower_route_profitable(
                    class,
                    method,
                    receiver_class_table,
                    &local_pshape_methods,
                )
            {
                pshape_tower_routable.insert(key);
            }
        }
    }
    // Imported classes publish only clone names the defining module proved and
    // emitted. Installing those capabilities in the same registries lets both
    // the ordinary exact-class/shape guarded arm and profitable adapter-field
    // dispatch towers retain the receiver proof across ESM and npm boundaries.
    // The tower subset is producer-authored because only the defining module
    // can see enough of the body to price its additional keys-token check.
    for imported in &opts.imported_classes {
        let effective_name = imported.effective_name();
        if hir.classes.iter().any(|class| class.name == effective_name) {
            continue;
        }
        for method in &imported.proven_this_method_names {
            if !imported.method_names.contains(method) {
                continue;
            }
            pshape_methods.insert(
                (effective_name.clone(), method.clone()),
                crate::collectors::PtrShapeLocal {
                    family: None,
                    class_name: effective_name.clone(),
                    numeric_fields: std::collections::HashSet::new(),
                    report_name: crate::opt_report::enabled()
                        .then(|| format!("imported:{}", imported.source_prefix)),
                },
            );
            if imported.proven_this_tower_method_names.contains(method) {
                pshape_tower_routable.insert((effective_name.clone(), method.clone()));
            }
        }
    }
    // Keep this first implementation non-combinatorial. Existing typed/raw
    // method clone families have their own public trampolines and calling
    // conventions; composing those proofs is separate work. The optional
    // undefined version retains the ordinary boxed ABI throughout.
    guarded_undefined_method_candidates.retain(|(_, key, _)| {
        !typed_f64_methods.contains(key)
            && !typed_i32_methods.contains(key)
            && !typed_i1_methods.contains(key)
            && !typed_string_methods.contains(key)
            && !typed_f64_receiver_methods.contains_key(key)
            && !nonnegative_index_methods.contains_key(key)
    });
    guarded_undefined_method_candidates.sort_unstable_by(|left, right| left.cmp(right));
    let guarded_undefined_method_params: std::collections::HashMap<(String, String), usize> =
        guarded_undefined_method_candidates
            .into_iter()
            .take(16)
            .map(|(_, key, param_index)| (key, param_index))
            .collect();
    guarded_falsy_field_default_candidates
        .sort_unstable_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));
    let guarded_falsy_field_default_methods: std::collections::HashMap<_, _> =
        guarded_falsy_field_default_candidates
            .into_iter()
            .take(16)
            .map(|(_, key, candidate)| (key, candidate))
            .collect();
    // #8774: one non-combinatorial tagged-ABI clone per local method. Source
    // annotations or a unique unannotated field signature only nominate a
    // class; every routed call emits an exact runtime class+shape guard. Keep
    // this disjoint from typed/index/undefined clone families, whose
    // trampolines have separate routing conventions.
    // Argument layouts may come from an imported class stub. The clone itself
    // is still emitted only for a method body owned by this module; imported
    // field metadata, class id and class-keys/ShapeId globals provide the same
    // exact runtime guard and offsets as local metadata. This is required by
    // perform-ecs: `ECS.addComponentsToEntity` is local to ECS.ts while its
    // `Entity` parameter is declared in Entity.ts.
    let visible_class_names: std::collections::HashSet<&str> =
        receiver_class_table.keys().map(String::as_str).collect();
    let mut pshape_arg_methods = std::collections::HashMap::new();
    for class in &hir.classes {
        for method in &class.methods {
            let key = (class.name.clone(), method.name.clone());
            if typed_f64_methods.contains(&key)
                || typed_i32_methods.contains(&key)
                || typed_i1_methods.contains(&key)
                || typed_string_methods.contains(&key)
                || typed_f64_receiver_methods.contains_key(&key)
                || nonnegative_index_methods.contains_key(&key)
                || guarded_undefined_method_params.contains_key(&key)
                || guarded_falsy_field_default_methods.contains_key(&key)
            {
                continue;
            }
            let Some(plan) = crate::collectors::method_proven_shape_args(
                method,
                receiver_class_table,
                &visible_class_names,
            ) else {
                continue;
            };
            if !plan.args.is_empty() {
                pshape_arg_methods.insert(key, plan);
            }
        }
    }
    module_dispatch_facts.install_argument_shape_routes(pshape_arg_methods.iter().map(
        |(key, plan)| {
            (
                key.clone(),
                plan.args
                    .iter()
                    .map(|arg| {
                        (
                            arg.param_index,
                            arg.fact.class_name.clone(),
                            arg.preserves_containment,
                        )
                    })
                    .collect(),
            )
        },
    ));
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
    for (name, global) in &class_keys_globals_map {
        if let Some(keys) = adds_by_keys.get(global) {
            anon_key_adds.insert(name.clone(), keys.clone());
        }
    }
    let class_header_image_inits: std::collections::HashMap<String, (u32, u64, u32)> = {
        let mut inits: std::collections::HashMap<String, (u32, u64, u32)> =
            std::collections::HashMap::new();
        for (class_name, keys_global) in &class_keys_globals_map {
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
        for (class_name, keys_global) in &class_keys_globals_map {
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
        for (class_name, keys_global) in &class_keys_globals_map {
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
    // Step 5C: a literal's method slot holding a module function's value is a
    // ConstFn lane too; births and lowering read the same admitted set.
    static_constfn::set_module_function_values(hir, &module_prefix);
    static_constfn::set_module_constructions(hir, &module_prefix);
    static_constfn::set_module_object_create_protos(hir);
    if let Some(births) = births {
        *births = static_shape_ids::module_births(
            &module_prefix,
            &class_keys_init_data,
            defined_class_keys_len,
            &class_header_image_inits,
            &class_birth_reps_map,
            &class_ids,
        );
        let deferred: Vec<_> = births
            .iter()
            .filter(|b| b.shape.key_count != 0 && matches!(b.shape.proto, BirthProto::Class(_)))
            .map(|b| {
                let mut b = b.clone();
                b.keys_global.push_str("$deferred");
                b.defined = false;
                b.shape.keys.clear();
                b.shape.key_count = 0;
                b.shape.rep = 0;
                b.shape.typed = None;
                b
            })
            .collect();
        births.extend(deferred);
        if constfn_enabled {
            let reps = class_keys_globals_map
                .iter()
                .filter_map(|(name, keys)| {
                    class_birth_reps_map
                        .get(keys)
                        .map(|rep| (name.clone(), *rep))
                })
                .collect();
            let class_finals = static_constfn_class::module_class_finals(
                hir,
                &module_prefix,
                births,
                &class_keys_globals_map,
                &class_ids,
            );
            births.extend(class_finals);
            births.extend(static_prototype::collect(
                hir,
                &module_prefix,
                &class_ids,
                &opts.program_layouts,
            ));
            let widths = hir
                .classes
                .iter()
                .map(|class| {
                    let lookup = |name: &str| class_table.get(name).copied();
                    (
                        class.name.clone(),
                        class.fields.len() as u32
                            + crate::lower_call::new_alloc::birth_slack_in(
                                class,
                                &lookup,
                                &anon_key_adds,
                            ),
                    )
                })
                .collect();
            births.extend(static_constfn::module_literal_finals(
                hir,
                &module_prefix,
                &reps,
                &widths,
            ));
        }
        if opts.output_type == "executable" {
            let private_finals = static_private_class::module_private_finals(
                hir,
                &module_prefix,
                births,
                &class_keys_globals_map,
                &class_ids,
            );
            births.extend(private_finals);
        }
    }
    static_shape_ids::set_module_static_ids(
        &module_prefix,
        &class_keys_init_data,
        &class_header_image_inits,
        &class_birth_reps_map,
        &class_ids,
        &opts.static_shape_ids,
        &opts.program_class_shape_ids,
    );
    crate::layout_ordinals::emit_requests(&mut llmod, &opts.static_shape_ids);
    if !constfn_enabled {
        static_shape_ids::disable_static_final_shapes();
    }
    for (keys, _, count, _, _) in &class_keys_init_data {
        let shape_global = crate::typed_shape::shape_id_global_name_from_keys_global(keys);
        let requested = static_shape_ids::requested_shape_id_for_keys_global(keys);
        if let Some(id) = requested.filter(|_| !collect_births) {
            crate::layout_ordinals::emit_request(&mut llmod, id);
            let word = crate::layout_ordinals::static_word(id);
            llmod.add_raw_global(format!("@{shape_global} = constant i32 {word}, align 4"));
            if let Some(&(cid, packed, live)) = class_header_image_inits.get(keys) {
                let birth = static_shape_ids::static_birth_content(keys).expect("static content");
                let deferred = !matches!(birth.proto, BirthProto::Literal)
                    && *count != 0
                    && !class_table.values().any(|class| {
                        class_ids.get(&class.name) == Some(&cid)
                            && crate::lower_call::new_alloc::keys_defined_at_birth(class)
                    });
                let image_word = if deferred {
                    crate::layout_ordinals::deferred_word(&birth, live.max(*count)).1
                } else {
                    word
                };
                let image = crate::typed_shape::header_image_global_name_from_keys_global(keys);
                let header = u64::from(cid) | (u64::from(image_word) << 32);
                llmod.add_raw_global(format!(
                    "@{image} = internal constant <2 x i64> <i64 {packed}, i64 {header}>, align 8"
                ));
            }
        } else {
            llmod.add_module_state_global(&shape_global, I32, "0");
        }
    }
    let class_header_images_map: std::collections::HashMap<String, (String, u64, u32)> =
        class_keys_globals_map
            .iter()
            .filter_map(|(class_name, keys_global)| {
                let &(class_id, gc_packed, _) = class_header_image_inits.get(keys_global)?;
                static_shape_ids::requested_shape_id_for_keys_global(keys_global)?;
                Some((
                    class_name.clone(),
                    (
                        crate::typed_shape::header_image_global_name_from_keys_global(keys_global),
                        gc_packed,
                        class_id,
                    ),
                ))
            })
            .collect();

    let imported_object_literals: std::collections::HashMap<String, ImportedObjectLiteral> = opts
        .imported_classes
        .iter()
        .filter_map(|imported| {
            imported
                .object_literal
                .as_ref()
                .map(|object| (object.local_binding.clone(), object.clone()))
        })
        .collect();
    let mut imported_object_producers: std::collections::BTreeSet<(String, u32)> =
        imported_object_literals
            .values()
            .map(|object| (object.source_prefix.clone(), object.source_global_id))
            .collect();
    // #11409: collect before emitting. Candidate maps have randomized order;
    // declarations must be stable for byte-identical IR and object-cache keys.
    for candidate in opts.object_literal_method_candidates.values().flatten() {
        if candidate.source_prefix != module_prefix {
            imported_object_producers
                .insert((candidate.source_prefix.clone(), candidate.source_global_id));
        }
    }
    for (source_prefix, source_global_id) in &imported_object_producers {
        llmod.add_external_module_state_global(
            &format!("perry_global_{source_prefix}__{source_global_id}"),
            DOUBLE,
        );
    }

    // #8772: declare the opaque ShapeId slots published by concrete classes
    // in other modules. Local candidates already have a defining global in
    // this module and must not be redeclared as external. Both capability
    // families share the sorted set so aliases remain deduplicated.
    let mut imported_shapes = std::collections::BTreeSet::new();
    for candidate in opts.object_literal_method_candidates.values().flatten() {
        if candidate.source_prefix != module_prefix {
            imported_shapes.insert(&candidate.shape_id_global);
        }
    }
    for shape in imported_shapes {
        llmod.add_external_global(shape, I32);
    }

    // A function whose body reads its dynamic `this` gets a receiver-taking
    // body (`codegen/function.rs`); the arena-threaded wrapper scheme does
    // not compose with it, so those stay ordinary.
    let arena_threaded_functions: std::collections::HashSet<u32> =
        crate::collectors::collect_self_recursive_allocators(hir)
            .into_iter()
            .filter(|id| !funcs_reading_dynamic_this.contains(id))
            .collect();
    let collected = closure_collect::collect_module_closures(hir);
    let closures = collected.closures;
    if opts.program_has_tla {
        llmod.declare_function(
            "js_async_module_configure",
            crate::types::VOID,
            &[I64, I64, I64, I64, I64, I64],
        );
        llmod.declare_function(
            "js_async_module_visit",
            crate::types::VOID,
            &[I64, I64, I64],
        );
        llmod.declare_function("js_async_module_import", DOUBLE, &[I64]);
        llmod.declare_function("js_async_module_require", crate::types::VOID, &[I64]);
        llmod.declare_function("js_async_module_entry", crate::types::VOID, &[I64]);
        for prefix in opts.dynamic_import_path_to_prefix.values() {
            if !prefix.starts_with("__node_submod__") && !prefix.starts_with("__native_mod__") {
                llmod.declare_function(&format!("{prefix}__async_record"), I64, &[]);
            }
        }
    }
    // The births pre-pass (`module_birth_shapes`) runs this function up to
    // the layout emission below and lowers no body: registering there wrote
    // every module's parameter rows twice.
    let _life_census = (!collect_births)
        .then(|| crate::collectors::life_census::register(hir, &opts.program_facts.elision))
        .flatten();
    let mut cross_module = CrossModuleCtx {
        closure_writes: collected.closure_writes,
        numeric_order_functions: &hir.functions,
        numeric_order_closures: &closures,
        region_closures: hir
            .closure_display_names
            .iter()
            .filter(|(_, name)| name.contains(split_regions::REGION_MARK))
            .map(|(id, _)| *id)
            .collect(),
        wpfacts: opts.program_facts.elision.clone(),
        capture_type_proofs: Default::default(),
        program_layouts: Arc::clone(&opts.program_layouts),
        receiver_families: Arc::new(crate::layout_families::receiver_candidates(
            hir,
            &opts.program_layouts,
        )),
        program_has_worker: opts.program_has_worker,
        program_has_thread_agents: thread_agents,
        class_hierarchy: class_hierarchy::ClassHierarchy::default(),
        namespace_imports: opts.namespace_imports.iter().cloned().collect(),
        namespace_self_import: opts.namespace_self_import,
        namespace_source_prefixes: opts.namespace_source_prefixes,
        deferred_namespace_imports: opts.deferred_namespace_imports,
        namespace_member_nested: opts.namespace_member_nested.iter().cloned().collect(),
        cjs_interop_namespaces: opts.cjs_interop_namespaces.iter().cloned().collect(),
        namespace_member_prefixes: opts.namespace_member_prefixes,
        namespace_member_origin_names: opts.namespace_member_origin_names,
        imported_async_funcs: opts.imported_async_funcs,
        object_literal_method_candidates: Arc::clone(&opts.object_literal_method_candidates),
        local_async_funcs,
        local_generator_funcs,
        fixed_generator_funcs,
        async_step_closures: hir.async_step_closures.iter().copied().collect(),
        module_global_proven_types: std::collections::HashMap::new(),
        module_global_transfers: std::collections::HashMap::new(),
        funcs_reading_dynamic_this,
        literal_method_home_classes: crate::collectors::literal_method_home_classes(hir),
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
        method_has_synthetic_arguments,
        method_arguments_length_only,
        class_keys_globals: class_keys_globals_map,
        class_field_counts: class_field_counts_map,
        anon_key_adds,
        class_init_chains: class_init_chains_map,
        class_header_images: class_header_images_map,
        class_birth_reps: class_birth_reps_map,
        imported_class_ctors: opts
            .imported_classes
            .iter()
            .map(|ic| {
                let effective_name = ic.effective_name();
                let ctor_name = crate::symbol_names::constructor(&ic.source_prefix, &ic.name);
                (
                    effective_name,
                    ImportedCtor {
                        symbol: ctor_name,
                        param_count: ic.constructor_param_count,
                        has_own_constructor: ic.has_own_constructor,
                        has_instance_fields: ic.has_instance_fields,
                        has_rest: ic.constructor_has_rest,
                        has_synthetic_arguments: ic.constructor_has_synthetic_arguments,
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
            let (
                translations,
                key_count,
                _locale_count,
                locale_codes,
                default_locale_idx,
                currencies,
            ) = arc.as_ref();
            crate::expr::I18nLowerCtx {
                translations: translations.clone(),
                key_count: *key_count,
                default_locale_idx: *default_locale_idx,
                locale_codes: locale_codes.clone(),
                currencies: currencies.clone(),
            }
        }),
        imported_vars: opts.imported_vars,
        imported_object_literals,
        needs_stdlib: opts.needs_stdlib,
        program_is_synchronous: opts.program_is_synchronous,
        needs_geisterhand: opts.needs_geisterhand,
        geisterhand_port: opts.geisterhand_port,
        compile_time_constants,
        target_triple: triple.clone(),
        app_metadata: opts.app_metadata.clone(),
        module_dispatch: module_dispatch_facts,
        array_callback_shapes: std::collections::HashMap::new(),
        // Inline-hot-small pre-pass (#6850 follow-up): FuncIds with an in-loop
        // call site AND few total call sites, so small hot callees can earn
        // `inlinehint` while the call-site cap bounds duplication.
        hot_loop_callees: crate::collectors::collect_hot_loop_callees(
            hir,
            crate::codegen::helpers::inline_hot_small_max_call_sites(),
        ),
        // #7871: the allocator's own "is this hot" set — same in-loop proxy,
        // no call-site cap (the cap prices `inlinehint`'s duplication, which
        // the inline bump allocator does not incur), plus direct recursion.
        alloc_hot_functions: crate::collectors::collect_alloc_hot_functions(hir),
        arena_threaded_functions,
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
        param_int_ranges: param_int_ranges_summary,
        // Phase 2 spec-ABI plans are selected AFTER the i64-specialization
        // pass (mutual exclusion), below; start empty here.
        spec_abi_functions: std::collections::HashMap::new(),
        typed_methods: std::collections::HashMap::new(),
        spec_return_proofs: std::collections::HashMap::new(),
        spec_ta_bindings: std::collections::HashMap::new(),
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
        nonnegative_index_methods,
        guarded_undefined_method_params,
        guarded_falsy_field_default_methods,
        pshape_methods,
        pshape_arg_methods,
        pshape_tower_routable,
        typed_f64_closures: std::collections::HashSet::new(),
        typed_i32_closures: std::collections::HashSet::new(),
        typed_i1_closures: std::collections::HashSet::new(),
        typed_string_closures: std::collections::HashSet::new(),
        typed_closure_capture_reps: std::collections::HashMap::new(),
        typed_i1_closure_param_reps: std::collections::HashMap::new(),
        compiler_private_async_i32_control_locals,
        compiler_private_async_i1_control_locals,
        scope_map: Default::default(),
        string_accumulator_locals: Default::default(),
        contents_stable_bindings: Default::default(),
        loop_pushed_arrays: Default::default(),
        definite_init: Default::default(),
        disable_buffer_fast_path,
        flat_const_arrays: {
            // Issue #50: fold module-level `const X: number[][] = [[int, ...], ...]`
            // into a flat `[N x i32]` LLVM constant so `X[i][j]` / `krow[j]` can
            // load directly from `.rodata` instead of chasing the arena array
            // header. Qualifying locals are `Let { mutable: false }`, have a
            // rectangular int-literal 2D init, and are used only by direct
            // element reads or read-only row aliases. Any observable row value
            // or write keeps the normal heap representation authoritative.
            let mut map: std::collections::HashMap<u32, crate::expr::FlatConstInfo> =
                std::collections::HashMap::new();
            for s in logical_entry_stmts.iter().copied() {
                if let perry_hir::Stmt::Let {
                    id,
                    init: Some(init),
                    mutable: false,
                    ..
                } = s
                {
                    if let Some((rows, cols, vals)) = crate::expr::try_flat_const_2d_int(init) {
                        let mut safe =
                            crate::collectors::flat_const_array_uses_are_read_only(&hir.init, *id);
                        if safe {
                            for f in &hir.functions {
                                if !crate::collectors::flat_const_array_uses_are_read_only(
                                    &f.body, *id,
                                ) {
                                    safe = false;
                                    break;
                                }
                            }
                        }
                        if safe {
                            'outer: for c in &hir.classes {
                                for m in &c.methods {
                                    if !crate::collectors::flat_const_array_uses_are_read_only(
                                        &m.body, *id,
                                    ) {
                                        safe = false;
                                        break 'outer;
                                    }
                                }
                                if let Some(ctor) = &c.constructor {
                                    if !crate::collectors::flat_const_array_uses_are_read_only(
                                        &ctor.body, *id,
                                    ) {
                                        safe = false;
                                        break;
                                    }
                                }
                            }
                        }
                        if safe {
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
        // built-in arms (Client / Pool / Database /
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
        // Preserve the export identity of aliased constructors. The default
        // SQLite constructor has no named export to recover from its local name.
        imported_class_original_names: {
            let mut map = HashMap::new();
            for import in &hir.imports {
                for spec in &import.specifiers {
                    match spec {
                        perry_hir::ImportSpecifier::Named { imported, local }
                            if local != imported =>
                        {
                            map.insert(local.clone(), imported.clone());
                        }
                        perry_hir::ImportSpecifier::Default { local }
                            if import.has_native_value_bindings()
                                && import.source == "better-sqlite3" =>
                        {
                            map.insert(local.clone(), "default".to_string());
                        }
                        _ => {}
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
        lazy_path_init_modules: opts.lazy_path_init_modules.clone(),
        deferred_module_prefixes: opts.deferred_module_prefixes.clone(),
        module_init_deps: opts.module_init_deps.clone(),
        module_deferred_init_deps: opts.module_deferred_init_deps.clone(),
        module_deferred_readiness: opts.module_deferred_readiness.clone(),
        program_has_tla: opts.program_has_tla,
        module_graph_has_tla: opts.module_graph_has_tla,
        is_dynamic_import_target: opts.is_dynamic_import_target,
        namespace_adopts_cjs_exports: opts.namespace_adopts_cjs_exports,
    };

    let module_globals_emit::ModuleGlobals {
        module_globals,
        module_global_types,
        module_global_proven_types,
        static_field_globals,
        module_global_transfers,
    } = module_globals_emit::emit_module_globals(
        &mut llmod,
        hir,
        &opts.imported_classes,
        &cross_module.compile_time_constants,
        &module_prefix,
        &cjs_property_exports,
        global_transfer::enabled(thread_agents, opts.program_has_worker),
    );
    cross_module.module_global_proven_types = module_global_proven_types;
    cross_module.module_global_transfers = module_global_transfers;

    // Method registry + cross-module method/getter/setter/ctor/static
    // extern declares. See `method_registry::build_method_names`.
    let mut method_names = method_registry::build_method_names(
        &mut llmod,
        hir,
        &opts.imported_classes,
        &class_table,
        &class_ids,
        &imported_class_prefix,
        &imported_class_source_name,
        &module_prefix,
    );
    inherited_accessors::register(
        hir,
        &class_table,
        &class_ids,
        &mut method_names,
        &mut cross_module,
    );
    cross_module.class_hierarchy =
        class_hierarchy::ClassHierarchy::new(&class_table, &method_names);

    // Representation-selection Phase 5a: now that the method registry exists,
    // drop any proven-`this` clone whose pair never made it into it. (Symbol
    // collisions with user members are impossible since #6927's reserved-`$`
    // clone namespace, so registry presence is all that is checked.) Pruning
    // HERE — before `emit_module_artifacts` reads `cross_module.pshape_methods`
    // for both emission and call-site routing — keeps the two in lockstep, so
    // a call site can never route to a clone the emission loop cannot produce.
    crate::collectors::prune_unregistered_clones(&mut cross_module.pshape_methods, &method_names);

    // Resolve user function names + signatures up front. See
    // `func_registry::build_func_registry`.
    let func_registry::FuncRegistry {
        func_names,
        func_signatures,
        func_synthetic_arguments,
    } = func_registry::build_func_registry(hir, &module_prefix);

    progress.checkpoint("class dispatch and representation analysis");

    // Module-wide boxed-var union + LocalId→Type map. See `boxed_locals`.
    let module_boxed_vars = boxed_locals::collect_module_boxed_vars(hir);
    // Scope context objects: the groups `scope_env::group_scope_boxes` formed
    // (as preallocation statements), validated against this module's
    // globals. Every capture layout below is computed through this map.
    cross_module.scope_map =
        crate::scope_env::ScopeMap::build(hir, &module_boxed_vars, &module_globals);
    cross_module.string_accumulator_locals = crate::collectors::string_accumulator_locals(hir);
    cross_module.contents_stable_bindings = crate::collectors::contents_stable_bindings(hir);
    cross_module.loop_pushed_arrays = crate::collectors::loop_pushed_arrays(hir);
    cross_module.capture_type_proofs =
        crate::collectors::typed_locals::capture_proofs(hir, &module_boxed_vars, &module_globals);
    cross_module.definite_init = crate::collectors::definite_init::collect(hir);
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
    // (`$typed_f64`/i32/…) made it read `js_closure_get_capture_bits(this,
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
    let typed_abi_local_types: std::collections::HashMap<u32, perry_hir::types::Type> =
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

    crate::layout_desc::function_families::collect(
        hir,
        &module_prefix,
        &module_globals,
        &closures,
        &module_boxed_vars,
        &cross_module.scope_map,
        crate::layout_desc::layouts_enabled(&opts.output_type, &triple)
            && crate::target_layout::inline_births_admitted(&triple),
    );
    if collect_births {
        crate::layout_desc::emit_module_layouts(
            &mut llmod,
            &mut strings,
            hir,
            &module_prefix,
            &opts.output_type,
            &opts.program_layouts,
            opts.is_entry_module,
            &opts.namespace_entries,
            cross_module.namespace_adopts_cjs_exports,
        );
        return Ok(None);
    }
    // Step 5C: a method site whose static lane names one of these bodies
    // calls it directly, so it needs each body's exact parameter count.
    static_constfn::set_module_body_arities(&module_prefix, &collected.closure_arities);

    // #8103: closure bodies are emitted before their enclosing regions. Prove
    // inline array-callback element shapes module-wide now, while both sides
    // of the boundary are available, then inject the vetted parameter facts
    // when each closure is compiled.
    let mut array_callback_shapes = crate::collectors::collect_array_callback_shapes(
        hir,
        &closures,
        &module_boxed_vars,
        &module_globals,
        &module_receiver_types,
        &class_table,
        &cross_module.module_dispatch,
    );
    // Async/generator transforms clear the flags on the closure expression,
    // but preserve the original identity in these module sets. Their callback
    // parameters outlive the synchronous array HOF invocation and therefore
    // cannot inherit its region-local containment fact.
    array_callback_shapes.retain(|func_id, _| {
        !cross_module.async_step_closures.contains(func_id)
            && !cross_module.local_generator_funcs.contains(func_id)
    });
    cross_module.array_callback_shapes = array_callback_shapes;

    cross_module.typed_f64_closures.clear();
    cross_module.typed_i32_closures.clear();
    cross_module.typed_i1_closures.clear();
    cross_module.typed_string_closures.clear();
    cross_module.typed_closure_capture_reps.clear();
    cross_module.typed_i1_closure_param_reps.clear();
    for (func_id, expr) in &closures {
        // A closure region is one body by design: no typed-ABI clone.
        if cross_module.region_closures.contains(func_id) {
            continue;
        }
        if let Some(captures) =
            typed_abi::typed_f64_closure_capture_reps(expr, &typed_abi_local_types)
        {
            cross_module
                .typed_closure_capture_reps
                .insert(*func_id, captures.into_iter().map(|(_, rep)| rep).collect());
        }
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
                        typed_f64_closure_name(&crate::symbol_names::closure(
                            &module_prefix,
                            func_id
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
                        typed_i1_closure_name(&crate::symbol_names::closure(
                            &module_prefix,
                            func_id
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
                        typed_i32_closure_name(&crate::symbol_names::closure(
                            &module_prefix,
                            func_id
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
                        typed_string_closure_name(&crate::symbol_names::closure(
                            &module_prefix,
                            func_id
                        ))
                    ),
                ],
            ),
        }
    }

    // Trusted bodies deliberately use the ordinary closure ABI. Keep them
    // disjoint from all typed-clone families so registration can always name
    // the public body directly and so one closure never accumulates two
    // independent cloning policies.
    let trusted_box_exclusions: std::collections::HashSet<u32> = cross_module
        .async_step_closures
        .union(&cross_module.local_generator_funcs)
        .chain(cross_module.funcs_reading_dynamic_this.iter())
        .chain(cross_module.typed_f64_closures.iter())
        .chain(cross_module.typed_i32_closures.iter())
        .chain(cross_module.typed_i1_closures.iter())
        .chain(cross_module.typed_string_closures.iter())
        .chain(cross_module.region_closures.iter())
        .copied()
        .collect();
    let trusted_box_closures = closure_collect::select_trusted_box_closures(
        &closures,
        &collected.direct_call_closures,
        &module_boxed_vars,
        &module_globals,
        &trusted_box_exclusions,
        &cross_module.scope_map,
    );
    let versioned_loop_callbacks = closure_collect::select_versioned_loop_callbacks(
        &closures,
        &trusted_box_closures,
        &module_boxed_vars,
        &module_globals,
        &cross_module.scope_map,
    );

    // ---- Representation-selection Phase 2: specialized-ABI plan selection.
    // Runs AFTER the typed_abi clone sets so mutual exclusion is decidable;
    // the entries themselves are emitted below in the pre-public loop.
    // Bounded: one entry per function (the dominant tuple),
    // `PERRY_SPECIALIZED_ABI_MAX` per module.
    if spec_abi::spec_abi_enabled() {
        let no_int32 = std::collections::BTreeSet::new();
        let proven_int32 = if facts_int32_locals::enabled() {
            &cross_module.wpfacts.int32_inputs
        } else {
            &no_int32
        };
        let spec_facts = crate::collectors::collect_spec_abi_facts_with(
            hir,
            &cross_module.definite_init,
            proven_int32,
            &cross_module.wpfacts.native_member_calls,
        );
        let spec_budget = spec_abi::spec_abi_max();
        let mut spec_emitted = 0usize;
        for f in &hir.functions {
            let reject =
                |reason: typed_abi::TypedCloneRejectionReason,
                 records: &mut Vec<crate::native_value::NativeRepRecord>| {
                    record_typed_clone_rejection(
                        records,
                        f.name.clone(),
                        "spec_abi_entry_decision",
                        reason,
                        vec![
                            "typed_clone_kind=spec_abi_entry".to_string(),
                            format!("function_id={}", f.id),
                            format!("symbol={}", f.name),
                        ],
                    );
                };
            // #7111: a function whose call sites were all inlined away — and
            // then constant-folded by `unroll_static_loops` — has no entry in
            // `spec_facts.call_sites`, which is built by walking `hir.init` and
            // every body for direct `Call` expressions. This used to `continue`
            // BEFORE any rejection was constructed, so `--opt-report` said
            // nothing at all about the function: indistinguishable from "not
            // analysed" and from "analysed and denied". Say "moot" instead.
            // A split region has one call site and one body, already typed
            // by its caller's proofs: no specialized copy.
            if split_regions::is_region_function(f) {
                continue;
            }
            let sites = spec_facts.call_sites.get(&f.id);
            // A guard executed when a generator object is CREATED cannot
            // prove the argument when its body later runs. Likewise, an
            // async body may let external code mutate a reachable argument
            // across `await`. Async functions with no suspension execute
            // their entire body synchronously and are safe to clone.
            if f.is_generator
                || f.was_plain_async
                || (f.is_async && param_guard::body_contains_await(&f.body))
            {
                reject(
                    typed_abi::TypedCloneRejectionReason::AsyncOrGenerator,
                    &mut typed_clone_rejection_records,
                );
                continue;
            }
            if !f.captures.is_empty() {
                reject(
                    typed_abi::TypedCloneRejectionReason::Captures,
                    &mut typed_clone_rejection_records,
                );
                continue;
            }
            if f.params.iter().any(|p| p.default.is_some()) {
                reject(
                    typed_abi::TypedCloneRejectionReason::ParamDefault,
                    &mut typed_clone_rejection_records,
                );
                continue;
            }
            if f.params.iter().any(|p| p.is_rest) {
                reject(
                    typed_abi::TypedCloneRejectionReason::RestParam,
                    &mut typed_clone_rejection_records,
                );
                continue;
            }
            if f.params.iter().any(|p| p.arguments_object.is_some())
                || func_synthetic_arguments.contains(&f.id)
            {
                reject(
                    typed_abi::TypedCloneRejectionReason::ArgumentsObject,
                    &mut typed_clone_rejection_records,
                );
                continue;
            }
            if cross_module.funcs_reading_dynamic_this.contains(&f.id) {
                reject(
                    typed_abi::TypedCloneRejectionReason::SpecReadsDynamicThis,
                    &mut typed_clone_rejection_records,
                );
                continue;
            }
            if cross_module.typed_f64_functions.contains(&f.id)
                || cross_module.typed_i32_functions.contains(&f.id)
                || cross_module.typed_i1_functions.contains(&f.id)
                || cross_module.typed_string_functions.contains(&f.id)
            {
                reject(
                    typed_abi::TypedCloneRejectionReason::SpecTypedCloneOverlap,
                    &mut typed_clone_rejection_records,
                );
                continue;
            }
            // A specialized entry is a second full copy of the body. For a
            // giant body (a generated straight-line driver, a 400-case
            // switch) that doubles the dominant LLVM cost and its `.text`
            // for entry-bound parameter facts that a body this size
            // amortizes to nothing.
            if spec_abi::spec_body_too_large(&f.body) {
                reject(
                    typed_abi::TypedCloneRejectionReason::SpecBodyTooLarge,
                    &mut typed_clone_rejection_records,
                );
                continue;
            }
            // Callee-side demotion: params the raw ABI cannot accept keep the
            // boxed protocol (reassigned params would stale the entry-bound
            // proofs; closure-referenced params feed the capture machinery).
            let demoted = crate::collectors::callee_demoted_params(f);
            // (#8094) A descriptor proof describes a heap object and is
            // established once, at entry. Any call in this body can run code
            // that reaches that same object — not only through an argument we
            // hand over, but through any alias the caller arranged before we
            // were entered (a global, a closure, a field of another live
            // object). So a REFERENCE-typed parameter cannot keep its proof
            // across a call. Primitive parameters are immune: a callee has no
            // route to the caller's copy of a number, string or boolean.
            //
            // A write THROUGH the parameter in this body invalidates the same
            // proof without any call being involved (`node.left = x`,
            // `values[i] = v`), so it belongs here too — and ONLY here. It is
            // a claim about the object's CONTENTS; a raw slot (I32 / F64 /
            // TaPtr) makes no such claim, so putting this on `demoted` deletes
            // representation choices that predate this PR. Measured: with it
            // on `demoted`, `fill(values: Float64Array, nodes: number)` loses
            // `fill$spec_ta7x10000_i32` entirely, because writing an element
            // reads as "mutation" of the typed-array parameter.
            //
            // Both halves are the definition of a contents-stable binding
            // (`collectors::contents_stable_bindings`), shared with the
            // let-binding proofs: a parameter is an entry-aliased binding, so
            // it is stable only when no write reaches it (through it, an
            // alias or an escape) and the body runs no unknown code and stores
            // through no foreign root. A primitive keeps only the rebinding
            // half: no callee can reach the caller's copy.
            let guard_blocked: Vec<bool> = f
                .params
                .iter()
                .map(|p| {
                    if spec_return_proof::is_reference_like(&cross_module.type_aliases, &p.ty, 0) {
                        !cross_module.contents_stable_bindings.contains(&p.id)
                    } else {
                        crate::collectors::has_any_mutation(&f.body, p.id)
                    }
                })
                .collect();
            let declaration_guards = param_guard::declaration_guards(
                f.id,
                &module_prefix,
                &f.params,
                &demoted,
                &guard_blocked,
                &cross_module.type_aliases,
                &cross_module.interfaces,
                &class_table,
                &class_ids,
            );
            let plan = match sites
                .and_then(|sites| spec_abi::select_dominant_tuple(sites, f.params.len(), &demoted))
            {
                Some((mut reps, _matching_sites)) => {
                    // Raw slots already carry a by-construction proof. Boxed
                    // slots may additionally recover an ordinary declared
                    // parameter fact, but only through their runtime
                    // descriptor. TaPtr remains a construction-only ABI: an
                    // unknown/public caller has no equivalent cheap guard for
                    // the raw pointer + length contract, so mixed TaPtr plans
                    // keep their existing static-only route.
                    let mut inferred_guards = vec![None; reps.len()];
                    for (index, (rep, param)) in reps.iter_mut().zip(f.params.iter()).enumerate() {
                        if !matches!(rep, crate::collectors::SpecParamRep::NumberArray) {
                            continue;
                        }
                        *rep = crate::collectors::SpecParamRep::Boxed;
                        if crate::collectors::guarded_number_array_param_eligible(&f.body, param.id)
                        {
                            inferred_guards[index] = param_guard::inferred_guard(
                                perry_hir::types::Type::Array(Box::new(
                                    perry_hir::types::Type::Number,
                                )),
                                param_guard::guard_descriptor_name(&module_prefix, f.id, index),
                                &cross_module.type_aliases,
                                &cross_module.interfaces,
                                &class_table,
                                &class_ids,
                            );
                        }
                    }
                    let has_ta_ptr = reps
                        .iter()
                        .any(|rep| matches!(rep, crate::collectors::SpecParamRep::TaPtr { .. }));
                    let guards: Vec<_> = declaration_guards
                        .into_iter()
                        .zip(inferred_guards)
                        .zip(reps.iter())
                        .map(|((declared, inferred), rep)| {
                            if !matches!(rep, crate::collectors::SpecParamRep::Boxed) {
                                None
                            } else if has_ta_ptr {
                                inferred
                            } else {
                                inferred.or(declared)
                            }
                        })
                        .collect();
                    let dispatch = if has_ta_ptr {
                        // Raw typed-array pointers remain direct-call-only.
                        // Any inferred boxed-array descriptor is checked in
                        // the same call-site diamond as an i32 range check.
                        SpecDispatch::Static
                    } else if guards.iter().any(Option::is_some) {
                        SpecDispatch::Guarded
                    } else {
                        SpecDispatch::Static
                    };
                    SpecFnPlan {
                        reps,
                        dispatch,
                        guards,
                        signature: None,
                        sig_defaults: Vec::new(),
                        sig_public: Default::default(),
                        record: None,
                    }
                }
                None => {
                    // Tier B: declaration-derived tuple plus descriptors for
                    // boxed ordinary parameters. The raw tuple can be all
                    // Boxed here: the clone's win is its post-guard parameter
                    // facts rather than a calling-convention change.
                    let reps = spec_abi::declaration_tuple(&f.params, &demoted);
                    let guards: Vec<_> = declaration_guards
                        .into_iter()
                        .zip(reps.iter())
                        .map(|(guard, rep)| {
                            matches!(rep, crate::collectors::SpecParamRep::Boxed)
                                .then_some(guard)
                                .flatten()
                        })
                        .collect();
                    if spec_abi::spec_tuple_is_viable(&reps) || guards.iter().any(Option::is_some) {
                        SpecFnPlan {
                            reps,
                            dispatch: SpecDispatch::Guarded,
                            guards,
                            signature: None,
                            sig_defaults: Vec::new(),
                            sig_public: Default::default(),
                            record: None,
                        }
                    } else {
                        reject(
                            if sites.is_none() {
                                typed_abi::TypedCloneRejectionReason::SpecNoCallSites
                            } else {
                                typed_abi::TypedCloneRejectionReason::SpecTupleUnproven
                            },
                            &mut typed_clone_rejection_records,
                        );
                        continue;
                    }
                }
            };
            if spec_emitted >= spec_budget {
                reject(
                    typed_abi::TypedCloneRejectionReason::SpecBudgetExceeded,
                    &mut typed_clone_rejection_records,
                );
                continue;
            }
            spec_emitted += 1;
            cross_module.spec_abi_functions.insert(f.id, plan);
        }
        cross_module.spec_ta_bindings = spec_facts.ta_bindings;
        cross_module.spec_return_proofs = spec_return_proof::collect_proven_returns(
            hir,
            &cross_module.spec_abi_functions,
            &cross_module.type_aliases,
        );
        // P2 (T2/T3): every selector above ran exactly as on
        // main; the typed set is decided once, here, before any body or site
        // is lowered.
        signature_entry::select(
            hir,
            &mut cross_module,
            &func_synthetic_arguments,
            &llmod.target_triple,
        );
        signature_entry::select_methods(hir, &mut cross_module, &class_ids);

        // The descriptor is immutable rodata, not a GC object. Emit in HIR
        // order (never HashMap order) so cache/object bytes stay deterministic.
        for f in &hir.functions {
            let Some(plan) = cross_module.spec_abi_functions.get(&f.id) else {
                continue;
            };
            for guard in plan.guards.iter().flatten() {
                // (#8079) Scalar descriptors are decided inline by a
                // typed-abi leaf guard; no rodata blob is referenced.
                if param_guard::scalar_descriptor_rep(&guard.descriptor).is_some() {
                    continue;
                }
                llmod.add_named_string_constant(
                    &guard.descriptor_name,
                    guard.descriptor.len() + 1,
                    &param_guard::descriptor_llvm_literal(&guard.descriptor),
                );
            }
        }

        // #8175: recursion-participating specialized clones take LLVM's
        // `preserve_none` convention. Registered HERE — after the plan is
        // final and before any function body compiles — so every dispatch
        // tier (static, guarded/range-checked, the public trampoline's fast
        // arm, and the clone's own self-recursion) stamps the call-site
        // convention through the one `LlBlock::call` choke point, and the
        // clone's define/declare render it from the same registry. Spec
        // entries are `internal` and direct-call-only by construction
        // (`spec_abi_symbol_reachability`), so the convention cannot escape
        // the module. Gated to recursion because the boundary cost is real:
        // a normal-CC caller saves ~20 CSRs once per entry, which amortizes
        // under a recursive tree and pessimizes a cheap non-recursive callee
        // in a hot loop.
        if spec_abi::spec_preserve_none_enabled()
            && spec_abi::preserve_none_target_ok(&triple)
            && !cross_module.spec_abi_functions.is_empty()
        {
            let recursive = crate::collectors::collect_recursion_participants(hir);
            let mut preserve_none: Vec<String> = hir
                .functions
                .iter()
                .filter(|f| recursive.contains(&f.id))
                .filter_map(|f| {
                    let plan = cross_module.spec_abi_functions.get(&f.id)?;
                    let public = func_names.get(&f.id)?;
                    Some(spec_entry_name(public, plan))
                })
                .collect();
            preserve_none.sort_unstable();
            llmod.set_preserve_none_fns(preserve_none);
        }
    }

    progress.checkpoint("locals, closures, and module globals analysis");

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

    // Representation-selection Phase 2: emit full-body specialized entries
    // (`{public}$spec_...`, internal linkage) before the public bodies. Same
    // real `compile_function`, parameterized on the plan's rep tuple.
    for f in &hir.functions {
        let Some(plan) = cross_module.spec_abi_functions.get(&f.id).cloned() else {
            continue;
        };
        // P2: `F$sig` compiles F without the literal defaults its callers
        // apply (§4.1).
        let sig_body = plan
            .signature
            .is_some()
            .then(|| record_sig::sig_body(f, &plan.sig_defaults, plan.record.as_ref()))
            .flatten();
        let f = sig_body.as_ref().unwrap_or(f);
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
            &collected.closure_rest_params,
            &cross_module,
            None,
            Some(&plan),
            None,
        )
        .with_context(|| format!("lowering specialized entry for function '{}'", f.name))?;
    }

    progress.checkpoint("typed top-level function clones");

    // Lower each user function into the module. Generated single-file bundles
    // can spend minutes here; report real item progress instead of making the
    // 30-second heartbeat the only proof that lowering is advancing.
    let function_bodies_started = Instant::now();
    let function_bodies_progress_step = (hir.functions.len() / 20).max(1);
    // An all-boxed specialized wrapper compares its already-lowered clone
    // with its generic body. Keep that dependent phase on the owning module.
    callable_workers::lower(
        &mut llmod,
        &mut strings,
        &hir.functions,
        cross_module.spec_abi_functions.is_empty(),
        |llmod, strings, f| {
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
                llmod,
                f,
                &func_names,
                strings,
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
                &collected.closure_rest_params,
                &cross_module,
                typed_public_trampoline,
                None,
                None,
            )
            .with_context(|| format!("lowering function '{}'", f.name))?;
            Ok(())
        },
        |done| {
            if done == hir.functions.len() || done % function_bodies_progress_step == 0 {
                progress.items(
                    "top-level function bodies",
                    done,
                    hir.functions.len(),
                    function_bodies_started,
                );
            }
        },
    )?;

    // A native module phase must not be represented by an empty HIR function:
    // the inliner would erase its call before the backend emits the real work.
    if hir.async_init_function.is_some() {
        let id = entry_outline::max_func_id(hir) + 1;
        // The completion phase stands at the module construct, like the
        // resumable body it finishes.
        let finish = perry_hir::Function {
            byte_offset: hir.byte_offset,
            id,
            name: "async module completion".into(),
            type_params: Vec::new(),
            params: Vec::new(),
            return_type: perry_hir::types::Type::Any,
            body: Vec::new(),
            is_async: false,
            is_generator: false,
            is_strict: hir.init_is_strict,
            is_exported: false,
            captures: Vec::new(),
            decorators: Vec::new(),
            was_plain_async: false,
            was_unrolled: false,
            construct_only: false,
        };
        let mut finish_names = func_names.clone();
        finish_names.insert(id, format!("{module_prefix}__async_finish"));
        compile_function(
            &mut llmod,
            &finish,
            &finish_names,
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
            &collected.closure_rest_params,
            &cross_module,
            None,
            None,
            Some(hir),
        )?;
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
            // Publish the exported spelling through the same constructor used
            // by importers. The function registry owns the local body symbol.
            let alias_sym = crate::symbol_names::function(&module_prefix, &exported_name);
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
                perry_hir::Export::Named { local, exported }
                    // #11044: matches the broadened getter-emission gate in
                    // export_value_wrappers.rs: native modules and runtime
                    // Node submodules publish ordinary value getters. Reserve
                    // those symbols for the getter emitter instead of claiming
                    // them here with undefined-returning fallback stubs.
                    if !hir.imports.iter().any(|import| {
                        import.has_native_value_bindings()
                            && import.specifiers.iter().any(|specifier| {
                                matches!(
                                    specifier,
                                    perry_hir::ImportSpecifier::Named {
                                        local: import_local,
                                        ..
                                    } if import_local == local
                                )
                            })
                    }) =>
                {
                    Some(exported.clone())
                }
                _ => None,
            })
            .collect();
        for exported in stub_targets {
            let stub_sym = crate::symbol_names::function(&module_prefix, &exported);
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

    progress.checkpoint("top-level function bodies and export stubs");

    // ── End of compile_module prelude (data + initial emission). ──
    // The remainder (closures, methods, ctors, statics, function /
    // ExternFuncRef / export-rename / unknown-func / method
    // closure-call wrappers, namespace globals + extern declares,
    // entry-fn emission, string-pool init) lives in
    // `artifacts::emit_module_artifacts`. Behavior is unchanged —
    // see the doc on that fn for the split rationale.
    emit_module_artifacts(
        ModuleArtifactsCtx {
            progress: &progress,
            llmod: &mut llmod,
            target_triple: &triple,
            strings: &mut strings,
            hir,
            import_function_prefixes: &opts.import_function_prefixes,
            imported_classes: &opts.imported_classes,
            constructor_param_counts: &opts.constructor_param_counts,
            is_entry_module: opts.is_entry_module,
            non_entry_module_prefixes: &opts.non_entry_module_prefixes,
            output_type: &opts.output_type,
            realm_reach: opts.program_facts.realm_reach.as_deref(),
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
            closure_rest_params: &collected.closure_rest_params,
            closure_synthetic_arguments: &collected.closure_synthetic_arguments,
            closure_rest_and_arguments: &collected.closure_rest_and_arguments,
            closure_arities: &collected.closure_arities,
            closure_lengths: &collected.closure_lengths,
            closure_arrow_functions: &collected.closure_arrow_functions,
            trusted_box_closures: &trusted_box_closures,
            versioned_loop_callbacks: &versioned_loop_callbacks,
            closures: &closures,
            class_keys_init_data: &class_keys_init_data,
            class_header_image_inits: &class_header_image_inits,
            imported_class_stubs: &imported_class_stubs,
            cross_module: &cross_module,
        },
        agent_strings_tls,
    )?;

    // The graph's first prefix owns its only preparation callback. All launch
    // sites reference that symbol, including launches in other modules. With
    // no graph, only a local launcher emits a local-only callback. Neither path
    // evaluates module bodies; preparation precedes worker deserialization.
    if thread_literal_callback_prefix == &module_prefix
        && (!opts.thread_literal_module_prefixes.is_empty() || local_thread_use)
    {
        // Normalize only in the owner, never once per module. Preserve the
        // owner as the first entry; the remaining string-only calls are pure
        // preparation and have no module-evaluation ordering dependencies.
        let mut prefixes: Vec<_> = opts
            .thread_literal_module_prefixes
            .iter()
            .skip(1)
            .filter(|prefix| *prefix != &module_prefix)
            .cloned()
            .collect();
        prefixes.sort();
        prefixes.dedup();
        prefixes.insert(0, module_prefix.clone());
        for prefix in &prefixes {
            llmod.declare_function(
                &format!("__perry_prepare_agent_strings_{}", prefix),
                crate::types::VOID,
                &[],
            );
        }
        let callback = llmod.define_function(
            format!("__perry_prepare_thread_strings_{}", module_prefix),
            crate::types::VOID,
            vec![],
        );
        let blk = callback.create_block("entry");
        for prefix in &prefixes {
            blk.call_void(&format!("__perry_prepare_agent_strings_{}", prefix), &[]);
        }
        blk.ret_void();
    }

    // One `JsFunctionInfo` per body a function object runs (`crate::fn_info`),
    // after every function — and so every allocation site — exists.
    // Executables admit permanent ConstFn bodies unless explicitly disabled.
    // A dylib never advertises a permanent body, even with the knob set.
    let constfn_body_metadata = constfn_enabled;
    if constfn_body_metadata {
        // Omitted/dead literals must not leave body-info relocations behind.
        static_constfn::emit_final_entries(
            &mut llmod,
            &module_prefix,
            &static_shape_ids::module_final_seeds(),
        );
        // The seed table names every ConstFn body of every seed, including
        // shapes that joined the seed set through a guard rather than a
        // finalizer; each of those records must be emitted linkable too.
        for info in static_shape_ids::module_seed_constfn_infos() {
            llmod.request_static_seed_body(info.strip_suffix("$info").expect("body info suffix"));
        }
    }
    // Accessor capabilities are module-local and never registered as JS
    // functions. Drop unused bodies before codegen units promote internals.
    let accessor_bodies = cross_module
        .pshape_methods
        .keys()
        .filter(|(_, member)| member.starts_with("__get_") || member.starts_with("__set_"))
        .filter_map(|(receiver, member)| {
            let public = method_names.get(&(receiver.clone(), member.clone()))?;
            let setter = member.starts_with("__set_");
            let property = &member[6..];
            let (owner, _) = static_prototype::accessor_owner(
                &class_table,
                &class_ids,
                receiver,
                property,
                setter,
            )?;
            let body = if owner == *receiver {
                public.clone()
            } else {
                inherited_accessors::receiver_body_name(public, class_ids[receiver])
            };
            Some(crate::collectors::pshape_method_name(&body))
        })
        .collect();
    llmod.prune_private_capabilities(&accessor_bodies);
    llmod.emit_fn_infos(constfn_body_metadata);

    // Emit the buffer alias-scope metadata once per module, covering every
    // scope id allocated across compile_function / compile_closure /
    // compile_method / compile_static_method / compile_module_entry. Must
    // run AFTER all function compilation so the counter reflects the true
    // total — otherwise functions whose scope ids exceed the init
    // function's count emit `!alias.scope !N` references with no matching
    // metadata definition (issue #71).
    closed_targets::apply(&mut llmod);
    emit_buffer_alias_metadata(&mut llmod);
    llmod.number_loop_metadata();
    llmod
        .native_rep_records
        .extend(typed_clone_rejection_records);

    if let Some(census) = &reach_census {
        census.bind_namespace_getters(&hir.name, &module_prefix, &opts.namespace_entries);
        census.capture(&hir.name, &llmod)?;
    }

    progress.phase(1, "lowering complete; releasing HIR and analyses");
    // Return only emission inputs: every lowering analysis, borrowed callable,
    // CJS HIR copy, string-pool index and CompileOptions graph dies here.
    Ok(Some(LoweredModule {
        llmod,
        progress,
        module_name: hir.name.clone(),
        callable_count: module_callable_count(hir),
        interned_strings: strings.len(),
        opts: EmissionOptions {
            target: opts.target,
            emit_ir_only: opts.emit_ir_only,
            verify_native_regions: opts.verify_native_regions,
            output_type: opts.output_type,
        },
        module_prefix,
        triple,
    }))
}

mod emit_lowered;
use emit_lowered::{emit_lowered_module, EmissionOptions, LoweredModule};

#[cfg(test)]
mod memory_ownership_tests;

pub(crate) mod callable_reloc;
mod callable_workers;
