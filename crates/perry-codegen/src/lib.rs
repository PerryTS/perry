//! LLVM Code Generation for Perry
//!
//! Produces textual LLVM IR (`.ll`) from Perry's HIR, then shells out to
//! `clang -c` to build an object file linked against `libperry_runtime.a`.
//! This is Perry's sole native code generation backend (since v0.5.0).

pub mod block;
pub(crate) mod boxed_vars;
pub mod codegen;
pub(crate) mod collectors;
pub(crate) mod expr;
pub mod ext_registry;
pub mod function;
pub mod linker;
pub(crate) mod loop_purity;
pub(crate) mod lower_array_method;
pub(crate) mod lower_call;
pub(crate) mod lower_conditional;
pub(crate) mod lower_string_method;
pub mod module;
pub mod nanbox;
pub(crate) mod native_value;
pub(crate) mod nm_install;
pub mod runtime_decls;
pub(crate) mod setjmp_abi;
pub(crate) mod stmt;
pub mod strings;
pub mod stubs;
pub mod target_layout;
pub(crate) mod type_analysis;
pub(crate) mod type_analysis_class_fields;
pub(crate) mod type_analysis_facts;
pub(crate) mod type_analysis_net;
pub(crate) mod typed_shape;
pub mod types;
pub(crate) mod volatile_setjmp;

pub use codegen::{
    compile_module, resolve_target_triple, AppMetadata, CompileOptions, FpContractMode,
    ImportedClass, NamespaceEntry, NamespaceEntryKind,
};

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
