//! Guarded omitted-argument/false-field indexed method versioning.

use crate::{compile_module, CompileOptions};
use perry_hir::types::Type;
use perry_hir::{Class, ClassField, CompareOp, Expr, Function, Module, Param, Stmt};

const ROWS: u32 = 30;
const INDEX: u32 = 31;
const DEFER: u32 = 32;

fn param(id: u32, name: &str) -> Param {
    Param {
        id,
        name: name.to_string(),
        ty: Type::Any,
        default: None,
        decorators: Vec::new(),
        is_rest: false,
        arguments_object: None,
    }
}

fn default_get() -> Expr {
    Expr::PropertyGet {
        object: Box::new(Expr::This),
        property: "DEFAULT_DEFER".to_string(),
        byte_offset: 0,
    }
}

fn candidate_method() -> Function {
    let mut defer = param(DEFER, "defer");
    defer.default = Some(default_get());
    Function {
        id: 40,
        name: "update".to_string(),
        type_params: Vec::new(),
        params: vec![param(ROWS, "rows"), param(INDEX, "index"), defer],
        return_type: Type::Any,
        body: vec![
            Stmt::If {
                condition: Expr::Compare {
                    op: CompareOp::Eq,
                    left: Box::new(Expr::LocalGet(DEFER)),
                    right: Box::new(Expr::Undefined),
                },
                then_branch: vec![Stmt::Expr(Expr::LocalSet(DEFER, Box::new(default_get())))],
                else_branch: None,
            },
            // Nominates the existing nonnegative-index family.
            Stmt::Expr(Expr::IndexGet {
                object: Box::new(Expr::LocalGet(ROWS)),
                index: Box::new(Expr::LocalGet(INDEX)),
            }),
            Stmt::If {
                condition: Expr::LocalGet(DEFER),
                then_branch: vec![Stmt::Return(Some(Expr::Integer(1)))],
                else_branch: Some(vec![Stmt::Return(Some(Expr::Integer(2)))]),
            },
        ],
        is_async: false,
        is_generator: false,
        is_strict: true,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
        was_plain_async: false,
        was_unrolled: false,
    }
}

fn fixture(method: Function) -> Module {
    let class = Class {
        id: 41,
        name: "Store".to_string(),
        type_params: Vec::new(),
        extends: None,
        extends_name: None,
        native_extends: None,
        extends_expr: None,
        heritage_lexically_shadowed: false,
        fields: vec![ClassField {
            name: "DEFAULT_DEFER".to_string(),
            key_expr: None,
            ty: Type::Any,
            init: Some(Expr::Bool(false)),
            is_private: false,
            is_readonly: false,
            decorators: Vec::new(),
        }],
        constructor: None,
        methods: vec![method],
        getters: Vec::new(),
        setters: Vec::new(),
        static_accessor_names: Vec::new(),
        static_accessor_fn_ids: Vec::new(),
        computed_members: Vec::new(),
        static_fields: Vec::new(),
        static_methods: Vec::new(),
        decorators: Vec::new(),
        is_exported: false,
        aliases: Vec::new(),
        is_nested: false,
        alloc_width_hint: 0,
        specialized_from: None,
    };
    let mut module = Module::new("guarded_falsy_default_method.ts");
    module.classes = vec![class];
    module
}

fn emit(method: Function) -> String {
    let opts = CompileOptions {
        emit_ir_only: true,
        output_type: "executable".to_string(),
        ..Default::default()
    };
    String::from_utf8(compile_module(&fixture(method), opts).expect("fixture compiles"))
        .expect("LLVM IR is UTF-8")
}

fn function_body<'a>(ir: &'a str, marker: &str) -> &'a str {
    let start = ir
        .match_indices("define ")
        .find(|(index, _)| {
            let end = ir[*index..]
                .find('\n')
                .map(|offset| index + offset)
                .unwrap_or(ir.len());
            ir[*index..end].contains(marker)
        })
        .map(|(index, _)| index)
        .unwrap_or_else(|| panic!("missing function containing {marker}:\n{ir}"));
    let end = ir[start..]
        .find("\n}")
        .map(|offset| start + offset)
        .expect("function terminator");
    &ir[start..end]
}

#[test]
fn wrapper_proves_live_false_field_and_clone_erases_default_and_branch() {
    let ir = emit(candidate_method());
    let base = "perry_method_guarded_falsy_default_method_ts__Store__update";
    let index = format!("{base}$idx_u31_{INDEX}");
    let specialized = format!("{index}$default_false2");
    let wrapper = function_body(&ir, &format!("@{base}("));
    let ordinary = function_body(&ir, &format!("@{index}("));
    let false_default = function_body(&ir, &format!("@{specialized}("));

    assert!(wrapper.contains(&crate::nanbox::TAG_UNDEFINED_I64.to_string()));
    assert!(wrapper.contains(&crate::nanbox::TAG_FALSE_I64.to_string()));
    assert!(wrapper.contains("load i32, ptr @perry_class_shape_id_"));
    assert!(wrapper.contains(&format!("@{specialized}(")));
    assert!(wrapper.contains(&format!("@{index}(")));
    assert!(ordinary.contains("@js_is_truthy("), "{ordinary}");
    assert!(
        !false_default.contains("@js_is_truthy("),
        "the guarded clone retained the known-false condition:\n{false_default}"
    );
    assert!(
        !false_default.contains("class_field_get"),
        "the guarded clone reevaluated its already-proved default field:\n{false_default}"
    );
}

#[test]
fn arbitrary_parameter_use_rejects_the_clone() {
    let mut method = candidate_method();
    method.body.push(Stmt::Return(Some(Expr::LocalGet(DEFER))));
    let ir = emit(method);
    assert!(
        !ir.contains("$default_false"),
        "a parameter whose actual false value remains observable was specialized"
    );
}
