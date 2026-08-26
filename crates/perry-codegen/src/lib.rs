//! LLVM Code Generation for Perry
//!
//! Produces textual LLVM IR (`.ll`) from Perry's HIR, then shells out to
//! `clang -c` to build an object file linked against `libperry_runtime.a`.
//! This is Perry's sole native code generation backend (since v0.5.0).

pub mod block;
pub(crate) mod boxed_vars;
pub mod codegen;
pub(crate) mod collectors;
#[cfg(feature = "llvm-inprocess")]
pub(crate) mod dialect;
pub(crate) mod eh_mode;
pub mod expr;
pub mod ext_registry;
pub mod function;
pub(crate) mod gc_call_effects;
pub mod gc_map;
#[cfg(feature = "llvm-inprocess")]
pub mod inprocess;
pub mod inst;
pub mod linker;
pub(crate) mod loop_purity;
pub(crate) mod lower_array_method;
pub(crate) mod lower_call;
pub(crate) mod lower_conditional;
pub(crate) mod lower_string_concat;
pub(crate) mod lower_string_method;
pub mod module;
pub mod nanbox;
#[cfg(feature = "llvm-inprocess")]
pub mod native_emit;
/// Coverage for the native-roots (RS4GC statepoint) lowering that ships —
/// #7502. Test-only; see the module docs for what it asserts and why the
/// shadow-pinned suites are not a substitute.
///
/// Gated on `llvm-inprocess` as well as `test`: two of its three vantages run
/// the statepoint rewrite and emit assembly through that pipeline, so under
/// `--no-default-features` (the text path, kept for bisection) there is nothing
/// for it to assert against.
#[cfg(all(test, feature = "llvm-inprocess"))]
mod native_root_coverage;
pub(crate) mod native_value;
pub(crate) mod nm_install;
pub mod opt_report;
pub(crate) mod root_reload;
pub mod rooting;
pub mod runtime_decls;
pub mod statepoint_report;
pub(crate) mod stmt;
pub mod strings;
pub mod stubs;
pub mod target_layout;
/// The #6951 temp-root emission contract, asserted in the per-PR `cargo-test`
/// gate rather than in the nightly-only integration tier (#6988), and against
/// the pooled lowering #7487 actually emits rather than the FFI spelling it
/// replaced (#7503).
#[cfg(test)]
mod temp_root_coverage;
/// Test-support surface — compiled only under `cfg(test)` or the `testing`
/// cargo feature (which nothing but this crate's own `[dev-dependencies]`
/// enables). See the module docs for why it is a feature and not a
/// `#[doc(hidden)] pub`.
#[cfg(any(test, feature = "testing"))]
pub mod testing;
pub(crate) mod type_analysis;
pub(crate) mod type_analysis_class_fields;
pub(crate) mod type_analysis_facts;
pub(crate) mod type_analysis_net;
pub(crate) mod typed_shape;
pub mod types;

pub use codegen::{
    compile_module, resolve_target_triple, short_spread_method_capabilities, AppMetadata,
    CompileOptions, ExportedObjectLiteralCapability, FpContractMode, ImportedClass,
    ImportedObjectLiteral, ImportedObjectLiteralMethod, NamespaceEntry, NamespaceEntryKind,
    ObjectLiteralMethodCandidate, ShortSpreadMethodCandidate,
};
pub use collectors::CjsPreambleCensus;

/// Return the guarded proven-`this` method-clone capabilities a native module
/// may safely publish to importing codegen units. The first map contains all
/// eligible methods; the second is the profitable subset for class-ID dispatch
/// towers, whose extra receiver-shape recheck is not free.
pub fn exported_proven_this_method_capabilities(
    hir: &perry_hir::Module,
) -> (
    std::collections::HashMap<String, Vec<String>>,
    std::collections::HashMap<String, Vec<String>>,
) {
    collectors::exportable_proven_this_method_capabilities(hir)
}

/// Return immutable exported object-literal capabilities for the compile
/// driver to resolve through ESM aliases and re-exports.
pub fn exported_object_literal_method_capabilities(
    hir: &perry_hir::Module,
) -> std::collections::HashMap<String, ExportedObjectLiteralCapability> {
    collectors::exported_object_literal_capabilities(hir)
}

/// Return whether an instance method may publish the additive direct-call ABI
/// whose synthetic `arguments` slot carries only the actual argument count.
/// The compile driver uses the same producer-side proof when building import
/// metadata, so consumers never infer this capability from an incomplete
/// class stub.
pub fn method_supports_arguments_length_direct_abi(method: &perry_hir::Function) -> bool {
    codegen::arguments::method_supports_arguments_length_direct_abi(method)
}

/// The shadow-stack field offsets generated code bakes into its inline root
/// stores (#7088).
///
/// Exported so `perry`'s `shadow_layout_contract` test can compare them with
/// `perry-runtime`'s copy. `perry-codegen` does not depend on `perry-runtime`,
/// so nothing else can catch the two drifting apart — and drift is silent:
/// the emitted code would store live GC roots through the wrong offset rather
/// than fail to build.
pub mod expr_shadow_layout {
    pub use crate::expr::shadow_inline::{
        SHADOW_ENTRY_META_OFFSET, SHADOW_ENTRY_SHIFT, SHADOW_ENTRY_SIZE, SHADOW_SLOT_ACTIVE_BIT,
        SHADOW_STACK_HEADER_SLOTS, SHADOW_STATE_FRAME_TOP_OFFSET, SHADOW_STATE_LEN_OFFSET,
        SHADOW_STATE_PTR_OFFSET,
    };
}

/// One row of the native-module dispatch table, projected to just
/// the manifest-relevant fields (module / method / has_receiver /
/// class_filter / arg-kind summary / return-kind summary). Exposed so
/// `perry-api-manifest`'s consistency test can walk the dispatch table
/// and assert every row has a counterpart entry in `API_MANIFEST` —
/// drift between the two would otherwise let an unimplemented-API
/// check (#463) miss a real implementation.
///
/// Arg / return *kinds* are reported as opaque strings (`"NA_STR"`,
/// `"NR_PTR"`, ...) so the consistency test can compare against the
/// manifest's `params` / `returns` types without `perry-api-manifest`
/// having to depend on `perry-codegen`'s internal enums (#512).
pub struct NativeMethodRef {
    /// Module specifier (e.g. `"crypto"`, `"mysql2/promise"`).
    pub module: &'static str,
    /// True for instance methods (`db.query(...)`); false for
    /// receiver-less calls (`crypto.randomUUID()`).
    pub has_receiver: bool,
    /// Method name on the module.
    pub method: &'static str,
    /// Optional class filter. `Some("Pool")` matches only entries
    /// constructed via that class.
    pub class_filter: Option<&'static str>,
    /// Per-arg coercion kinds, in declaration order. Each element is
    /// one of `"NA_F64"`, `"NA_STR"`, `"NA_PTR"`, `"NA_JSV"`,
    /// `"NA_VARARGS"`, `"NA_JSON"`. Used by `perry-api-manifest`'s
    /// param-count drift test (#512).
    pub arg_kinds: &'static [&'static str],
    /// Return-kind tag. One of `"NR_PTR"`, `"NR_PROMISE"`, `"NR_STR"`,
    /// `"NR_BIGINT"`, `"NR_F64"`, `"NR_I32"`, `"NR_VOID"`.
    pub ret_kind: &'static str,
}

/// Walk every entry in the native-module dispatch table.
/// `perry-api-manifest`'s consistency test consumes this to verify
/// the manifest is in sync with the dispatch table. Stable iteration
/// order — declaration order in `lower_call.rs::NATIVE_MODULE_TABLE`.
pub fn iter_native_method_signatures() -> impl Iterator<Item = NativeMethodRef> {
    lower_call::iter_native_module_table().map(
        |(module, has_receiver, method, class_filter, arg_kinds, ret_kind)| NativeMethodRef {
            module,
            has_receiver,
            method,
            class_filter,
            arg_kinds,
            ret_kind,
        },
    )
}

/// #7139 template-change canary: does `hir` carry a `Ptr<Shape>` §5.2 module
/// barrier (`collectors::ModuleDispatchFacts::shape_barrier_sites`)?
///
/// Public **solely** so the `perry` crate's `cjs_wrap` tests can assert that
/// the CommonJS preamble they emit is still recognised as module scaffolding
/// by `collectors::cjs_scaffolding` — the recogniser is here, the template is
/// there, and only `perry` can see both. That coupling is otherwise silent:
/// a template edit would not break anything, it would just quietly re-arm the
/// barrier for 100 % of CommonJS modules and evaporate the #7139 win with no
/// symptom. Same shape as [`iter_native_method_signatures`], which exists for
/// `perry-api-manifest`'s consistency test.
///
/// Not part of any codegen contract; nothing in the compile pipeline calls it.
pub fn module_has_ptr_shape_barrier(hir: &perry_hir::Module) -> bool {
    collectors::collect_module_dispatch_facts(hir).has_shape_barrier_sites()
}

/// #7170 R2 whole-program pre-pass: exported native function name -> fresh
/// anonymous-record class returned by that function's final HIR.
///
/// Public only for the `perry` compile driver, which resolves these source
/// facts through imports/re-exports before modules are code-generated in
/// parallel. Consumers should treat absence as "no proof".
pub fn module_exported_return_shapes(
    hir: &perry_hir::Module,
) -> std::collections::HashMap<String, String> {
    collectors::collect_exported_return_shapes(hir)
}

/// #7152 template-change canary: what the `Ptr<Shape>` report suppresses in
/// `hir` as Perry's own `cjs_wrap` scaffolding.
///
/// Public for exactly the reason [`module_has_ptr_shape_barrier`] is, and with
/// the same failure mode: rename `__cjs_module`, drop the
/// `var module = __cjs_module` alias, or change the `{ exports: {} }` literal,
/// and `collectors::cjs_scaffolding`'s recogniser stops firing. Nothing breaks
/// — the report just goes back to attributing Perry's scaffolding to the
/// user's code, in every CommonJS module, with no symptom.
///
/// Not part of any codegen contract; nothing in the compile pipeline calls it.
pub fn cjs_preamble_census(hir: &perry_hir::Module) -> CjsPreambleCensus {
    collectors::cjs_preamble_census(hir)
}
