//! #10477: `x instanceof F` where `F` is an IMPORTED binding.
//!
//! HIR cannot tell an imported function constructor from an imported class —
//! both are `ExternFuncRef` bindings — so it hands every imported RHS to
//! codegen as a dynamic `ty_expr` and codegen decides. Both directions are
//! asserted here, because each one silently degrades in a way no other test
//! sees:
//!
//! * a compiled-source import with no class id must reach
//!   `js_instanceof_dynamic`, which resolves the constructor value and walks
//!   the prototype chain. Before #10477 it folded to `js_instanceof(v, 0)` —
//!   a well-formed call that always answers `false`;
//! * an imported CLASS must keep the static `js_instanceof(v, <class id>)`
//!   check. Routing it through the dynamic helper would still be correct, so
//!   only an IR census catches the regression: the class fast path would just
//!   get slower.

use crate::{compile_module, CompileOptions, ImportedClass};
use perry_hir::types::Type;
use perry_hir::{Expr, Module, Stmt};

const DYNAMIC_CALL: &str = "call double @js_instanceof_dynamic(";
const STATIC_CALL: &str = "call double @js_instanceof(";

fn imported_ref(name: &str) -> Expr {
    Expr::ExternFuncRef {
        name: name.to_string(),
        param_types: Vec::new(),
        return_type: Type::Any,
    }
}

/// `{} instanceof <name>` with the imported binding's value attached, exactly
/// as `lower_expr/arm_bin.rs` lowers a bare imported identifier RHS.
fn instanceof_imported(name: &str) -> Module {
    let mut module = Module::new("instanceof_imported.ts");
    module.init = vec![Stmt::Expr(Expr::InstanceOf {
        expr: Box::new(Expr::Object(Vec::new())),
        ty: name.to_string(),
        ty_expr: Some(Box::new(imported_ref(name))),
    })];
    module
}

fn imported_class(name: &str, class_id: u32) -> ImportedClass {
    ImportedClass {
        name: name.to_string(),
        local_alias: None,
        namespace: None,
        source_prefix: "lib_ts".to_string(),
        constructor_param_count: 0,
        has_own_constructor: true,
        constructor_has_rest: false,
        has_instance_fields: false,
        method_names: Vec::new(),
        proven_this_method_names: Vec::new(),
        proven_this_tower_method_names: Vec::new(),
        method_return_types: Vec::new(),
        method_param_counts: Vec::new(),
        method_has_rest: Vec::new(),
        method_has_synthetic_arguments: Vec::new(),
        method_arguments_length_only: Vec::new(),
        static_field_names: Vec::new(),
        static_method_names: Vec::new(),
        static_method_return_types: Vec::new(),
        static_method_param_counts: Vec::new(),
        static_method_has_rest: Vec::new(),
        static_method_has_user_rest: Vec::new(),
        static_method_has_synthetic_arguments: Vec::new(),
        getter_names: Vec::new(),
        getter_return_types: Vec::new(),
        setter_names: Vec::new(),
        parent_name: None,
        field_names: Vec::new(),
        field_types: Vec::new(),
        source_class_id: Some(class_id),
        return_shape_imports: Vec::new(),
        object_literal: None,
    }
}

fn compile(module: &Module, opts: CompileOptions) -> String {
    String::from_utf8(compile_module(module, opts).expect("instanceof module compiles"))
        .expect("LLVM IR is UTF-8")
}

#[test]
fn imported_function_constructor_rhs_resolves_the_constructor_value() {
    let mut opts = CompileOptions {
        emit_ir_only: true,
        ..Default::default()
    };
    opts.import_function_prefixes
        .insert("Plain".to_string(), "lib_ts".to_string());
    let ir = compile(&instanceof_imported("Plain"), opts);

    assert!(
        ir.contains(DYNAMIC_CALL),
        "an imported function constructor must resolve its value and walk the \
         prototype chain:\n{ir}"
    );
    assert!(
        !ir.contains(STATIC_CALL),
        "the class-id check has no id for a function import — it folds to \
         `false` (#10477):\n{ir}"
    );
}

#[test]
fn imported_class_rhs_keeps_the_static_class_id_check() {
    let mut opts = CompileOptions {
        emit_ir_only: true,
        ..Default::default()
    };
    opts.import_function_prefixes
        .insert("Klass".to_string(), "lib_ts".to_string());
    opts.imported_classes.push(imported_class("Klass", 7701));
    let ir = compile(&instanceof_imported("Klass"), opts);

    assert!(
        ir.contains(STATIC_CALL),
        "an imported class must keep the static class-id check:\n{ir}"
    );
    assert!(
        !ir.contains(DYNAMIC_CALL),
        "the dynamic helper would only unpack the same class id back out:\n{ir}"
    );
}

#[test]
fn unresolved_import_rhs_keeps_its_reserved_builtin_id() {
    // Not a compiled source module (nothing in `import_function_prefixes`):
    // the binding's value form is a placeholder, so the reserved-id mapping
    // stays — `js_instanceof_dynamic` on a placeholder would throw instead of
    // answering.
    let ir = compile(
        &instanceof_imported("Error"),
        CompileOptions {
            emit_ir_only: true,
            ..Default::default()
        },
    );

    assert!(
        ir.contains(STATIC_CALL),
        "an unresolved import must keep the static reserved-id check:\n{ir}"
    );
    assert!(
        !ir.contains(DYNAMIC_CALL),
        "nothing resolves this binding to a constructor value:\n{ir}"
    );
}
