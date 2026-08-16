//! cargo-test-visible coverage for the declaration-based string proof in
//! `string_value_is_runtime_guaranteed` and the `type X = { … }` arm of
//! `static_type_of`.
//!
//! `"lit" + r.field` where `field` is DECLARED `string` must lower to the
//! static concat, not to `js_dynamic_string_or_number_add` — that helper opens
//! a `RuntimeHandleScope`, roots four operands and runs `ToPrimitive` on both
//! sides to rediscover what the declaration already stated. A field the
//! declaration does NOT describe must keep the dynamic helper.

use crate::{compile_module, CompileOptions};
use perry_hir::types::{ObjectType, PropertyInfo, Type};
use perry_hir::{BinaryOp, Expr, Function, Module, ModuleInitKind, Param};
use std::collections::HashMap;

fn rec_alias() -> HashMap<String, Type> {
    let mut properties = HashMap::new();
    properties.insert(
        "kind".to_string(),
        PropertyInfo {
            ty: Type::String,
            optional: false,
            readonly: false,
        },
    );
    properties.insert(
        "amount".to_string(),
        PropertyInfo {
            ty: Type::Number,
            optional: false,
            readonly: false,
        },
    );
    let mut aliases = HashMap::new();
    aliases.insert(
        "Rec".to_string(),
        Type::Object(ObjectType {
            name: Some("Rec".to_string()),
            properties,
            property_order: None,
            index_signature: None,
        }),
    );
    aliases
}

/// `function probe(r: Rec): string { return "t:" + r.<property>; }`
fn concat_probe_ir(property: &str) -> String {
    let module = Module {
        name: "alias_string_concat.ts".to_string(),
        imports: Vec::new(),
        exports: Vec::new(),
        classes: Vec::new(),
        interfaces: Vec::new(),
        type_aliases: Vec::new(),
        enums: Vec::new(),
        globals: Vec::new(),
        functions: vec![Function {
            id: 1,
            name: "probe".to_string(),
            type_params: Vec::new(),
            params: vec![Param {
                id: 1,
                name: "r".to_string(),
                ty: Type::Named("Rec".to_string()),
                default: None,
                decorators: Vec::new(),
                is_rest: false,
                arguments_object: None,
            }],
            return_type: Type::String,
            body: vec![perry_hir::Stmt::Return(Some(Expr::Binary {
                op: BinaryOp::Add,
                left: Box::new(Expr::String("t:".to_string())),
                right: Box::new(Expr::PropertyGet {
                    object: Box::new(Expr::LocalGet(1)),
                    property: property.to_string(),
                    byte_offset: 0,
                }),
            }))],
            is_async: false,
            is_generator: false,
            is_strict: false,
            is_exported: false,
            captures: Vec::new(),
            decorators: Vec::new(),
            was_plain_async: false,
            was_unrolled: false,
        }],
        script_global_functions: Vec::new(),
        references_global_this: false,
        annexb_global_undefined_names: Vec::new(),
        init: Vec::new(),
        exported_native_instances: Vec::new(),
        exported_func_return_native_instances: Vec::new(),
        exported_objects: Vec::new(),
        exported_functions: Vec::new(),
        widgets: Vec::new(),
        uses_fetch: false,
        uses_webassembly: false,
        extern_funcs: Vec::new(),
        init_was_unrolled: false,
        has_top_level_await: false,
        init_kind: ModuleInitKind::Eager,
        async_step_closures: std::collections::HashSet::new(),
        closure_display_names: HashMap::new(),
        class_display_names: HashMap::new(),
        closure_source_text: HashMap::new(),
        async_generator_funcs: std::collections::HashSet::new(),
        local_source_spans: std::collections::HashMap::new(),
        gen_param_prologue_len: HashMap::new(),
    };
    let opts = CompileOptions {
        emit_ir_only: true,
        output_type: "executable".to_string(),
        type_aliases: rec_alias(),
        ..Default::default()
    };
    String::from_utf8(compile_module(&module, opts).unwrap()).expect("LLVM IR should be UTF-8")
}

#[test]
fn alias_declared_string_field_takes_the_static_concat() {
    let ir = concat_probe_ir("kind");
    assert!(
        !ir.contains("call double @js_dynamic_string_or_number_add"),
        "`\"t:\" + r.kind` with `kind` declared `string` on the object-type \
         alias must not re-derive the operand types at runtime:\n{ir}"
    );
    assert!(
        ir.contains("@js_string_concat"),
        "it must lower to a string concat instead:\n{ir}"
    );
}

#[test]
fn alias_declared_number_field_is_deliberately_not_routed() {
    // The alias resolution added here is consumed by the STRING side only.
    // `"t:" + r.amount` lands in the one-sided arm, which keeps the strict
    // `string_value_is_runtime_guaranteed` on the left and asks `is_numeric_expr` about
    // the right — and `is_numeric_expr`'s `PropertyGet` arm answers from
    // `ctx.classes` alone, on purpose: a `true` there means "this lowers to a
    // REAL double", and the guarded class-field diamond's cold arm can hand
    // back a NaN-boxed value that `fadd` would propagate rather than add
    // (#7831). Widening the numeric side needs that PR's guarded lowering,
    // not this one's runtime delegation, so it stays dynamic here.
    let ir = concat_probe_ir("amount");
    assert!(
        ir.contains("call double @js_dynamic_string_or_number_add"),
        "the numeric side must NOT be widened by this change:\n{ir}"
    );
}

#[test]
fn field_absent_from_the_alias_keeps_the_dynamic_add() {
    // The proof is the DECLARATION, not the receiver's shape. A property the
    // alias says nothing about is still type-unknown and must keep the
    // spec-complete helper.
    let ir = concat_probe_ir("undeclared");
    assert!(
        ir.contains("call double @js_dynamic_string_or_number_add"),
        "an undeclared property proves nothing and must stay dynamic:\n{ir}"
    );
}
