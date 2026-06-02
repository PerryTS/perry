//! HIR (High-level Intermediate Representation) definitions
//!
//! The HIR is a typed, lowered representation of TypeScript that is
//! easier to compile to native code than the raw AST.
//!
//! This module was a single ~3,000-line file until it was split into the
//! topical submodules below. External crates pattern-match on every
//! variant of the public enums, so the surface is preserved verbatim and
//! re-exported here with explicit names — `use perry_hir::ir::*;` and
//! `use perry_hir::ir::Foo;` continue to work unchanged.

mod constants;
mod decl;
mod expr;
mod module;
mod ops;
mod stmt;
mod widget;

// ---- constants.rs ----
pub use constants::{
    clear_allow_dynamic_stdlib_packages, clear_compile_packages_override,
    clear_current_module_source, clear_precompile_state, current_module_has_allow_dynamic_at,
    current_module_line_at, determine_module_kind, dynamic_stdlib_allowed_for_package,
    is_native_module, is_native_module_with_externals, is_node_builtin_module,
    package_name_for_source_path, precompile_capture_enabled, precompile_result_at,
    refuse_dynamic_stdlib_dispatch_enabled, requires_stdlib, set_allow_dynamic_stdlib_packages,
    set_compile_packages_override, set_current_module_source, set_precompile_capture,
    set_precompile_results, set_refuse_dynamic_stdlib_dispatch, typed_array_kind_for_name, ClassId,
    EnumId, InterfaceId, ModuleInitKind, ModuleKind, PosixCredentialKind, TypeAliasId,
    NATIVE_MODULES, TYPED_ARRAY_KIND_BIGINT64, TYPED_ARRAY_KIND_BIGUINT64,
    TYPED_ARRAY_KIND_FLOAT16, TYPED_ARRAY_KIND_FLOAT32, TYPED_ARRAY_KIND_FLOAT64,
    TYPED_ARRAY_KIND_INT16, TYPED_ARRAY_KIND_INT32, TYPED_ARRAY_KIND_INT8, TYPED_ARRAY_KIND_UINT16,
    TYPED_ARRAY_KIND_UINT32, TYPED_ARRAY_KIND_UINT8, TYPED_ARRAY_KIND_UINT8_CLAMPED,
};

// ---- module.rs ----
pub use module::Module;

// ---- widget.rs ----
pub use widget::{
    GaugeStyle, WidgetConditionOp, WidgetConfigParam, WidgetConfigParamType, WidgetDecl,
    WidgetFieldType, WidgetFont, WidgetFormatArg, WidgetFormatCall, WidgetFormatExpr,
    WidgetModifier, WidgetNode, WidgetPlaceholderValue, WidgetStackKind, WidgetTemplatePart,
    WidgetTextContent,
};

// ---- decl.rs ----
pub use decl::{
    Class, ClassField, Decorator, Enum, EnumMember, EnumValue, Export, Function, Global, Import,
    ImportSpecifier, Interface, InterfaceMethod, InterfaceProperty, Param, TypeAlias,
};

// ---- stmt.rs ----
pub use stmt::{CatchClause, Stmt, SwitchCase};

// ---- expr.rs ----
pub use expr::{BoxedPrimitiveKind, Expr, PathWin32Method, ProcessStdinLifecycleMethod};

// ---- ops.rs ----
pub use ops::{ArrayElement, BinaryOp, CallArg, CompareOp, LogicalOp, UnaryOp, UpdateOp};
