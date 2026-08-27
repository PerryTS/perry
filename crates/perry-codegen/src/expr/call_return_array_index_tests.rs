use crate::{compile_module, CompileOptions};
use perry_hir::types::Type;
use perry_hir::{Class, Expr, Function, Module, Param, Stmt};

fn param(id: u32, name: &str, ty: Type) -> Param {
    Param {
        id,
        name: name.to_string(),
        ty,
        default: None,
        decorators: Vec::new(),
        is_rest: false,
        arguments_object: None,
    }
}

fn function(
    id: u32,
    name: &str,
    params: Vec<Param>,
    return_type: Type,
    body: Vec<Stmt>,
) -> Function {
    Function {
        id,
        name: name.to_string(),
        type_params: Vec::new(),
        params,
        return_type,
        body,
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

fn call_get_data(selector: i64) -> Expr {
    Expr::Call {
        callee: Box::new(Expr::PropertyGet {
            object: Box::new(Expr::This),
            property: "getData".to_string(),
            byte_offset: 0,
        }),
        args: vec![Expr::Integer(selector)],
        type_args: Vec::new(),
        byte_offset: 0,
    }
}

fn store_class(receiver_selector: i64) -> Class {
    let get_data = function(
        2,
        "getData",
        vec![param(3, "selector", Type::Number)],
        Type::Array(Box::new(Type::Any)),
        vec![Stmt::Return(Some(Expr::Array(vec![Expr::Number(0.0)])))],
    );
    let write = function(
        3,
        "write",
        vec![
            param(1, "index", Type::Number),
            param(2, "value", Type::Any),
        ],
        Type::Void,
        vec![Stmt::Expr(Expr::PutValueSet {
            target: Box::new(call_get_data(0)),
            key: Box::new(Expr::LocalGet(1)),
            value: Box::new(Expr::LocalGet(2)),
            receiver: Box::new(call_get_data(receiver_selector)),
            strict: true,
        })],
    );
    Class {
        id: 1,
        name: "Store".to_string(),
        type_params: Vec::new(),
        extends: None,
        extends_name: None,
        native_extends: None,
        extends_expr: None,
        heritage_lexically_shadowed: false,
        fields: Vec::new(),
        constructor: None,
        methods: vec![get_data, write],
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
    }
}

fn compile_store_ir(receiver_selector: i64) -> String {
    let mut module = Module::new("call_return_array_put_value.ts");
    module.classes.push(store_class(receiver_selector));
    let bytes = compile_module(
        &module,
        CompileOptions {
            emit_ir_only: true,
            ..Default::default()
        },
    )
    .expect("call-returned array store compiles");
    String::from_utf8(bytes).expect("LLVM IR is UTF-8")
}

fn write_method_ir(ir: &str) -> &str {
    // Index-specialized methods publish a small guard wrapper and retain the
    // original semantics in `$generic`; assertions about assignment lowering
    // belong to that body rather than the wrapper.
    let generic = "@perry_method_call_return_array_put_value_ts__Store__write$generic(";
    let public = "@perry_method_call_return_array_put_value_ts__Store__write(";
    let start = ir
        .match_indices("define ")
        .map(|(start, _)| start)
        .find(|start| {
            let line_end = ir[*start..]
                .find('\n')
                .map(|len| *start + len)
                .unwrap_or(ir.len());
            let signature = &ir[*start..line_end];
            signature.contains(generic) || signature.contains(public)
        })
        .expect("write method body is present in IR");
    let method_and_rest = &ir[start..];
    let end = method_and_rest
        .find("\n}\n")
        .expect("write method has a closing brace");
    &method_and_rest[..end + 3]
}

#[test]
fn same_call_returned_array_uses_array_index_store_and_evaluates_receiver_once() {
    let ir = compile_store_ir(0);
    let write_ir = write_method_ir(&ir);

    assert!(
        write_ir.contains("call i64 @js_typed_feedback_array_set_index_or_string("),
        "a call with an Array return type must use the array-index semantic fallback:\n{write_ir}"
    );
    assert!(
        !write_ir.contains("call double @js_put_value_set_dyn_ic("),
        "the proven array receiver must not enter the generic Proxy-compatible PutValue ladder:\n{write_ir}"
    );
    assert_eq!(
        write_ir
            .matches(
            "call double @perry_method_call_return_array_put_value_ts__Store__getData("
        )
        .count(),
        1,
        "the syntactically duplicated target/receiver call represents one evaluated assignment base"
    );
}

#[test]
fn distinct_call_receiver_stays_on_explicit_receiver_put_value_path() {
    let ir = compile_store_ir(1);
    let write_ir = write_method_ir(&ir);

    assert!(
        !write_ir.contains("call i64 @js_typed_feedback_array_set_index_or_string("),
        "a receiver that differs from the target must not use same-receiver array lowering:\n{write_ir}"
    );
    assert!(
        write_ir.contains("call double @js_put_value_set("),
        "the distinct receiver must be passed to the generic PutValue helper:\n{write_ir}"
    );
    assert_eq!(
        write_ir
            .matches("call double @perry_method_call_return_array_put_value_ts__Store__getData(")
            .count(),
        2,
        "target and distinct receiver calls are independently evaluated"
    );
}
