//! #8103: a proven local array's direct inline callback inherits the exact
//! element shape in the separately emitted closure function.

use crate::compile_module;
use perry_hir::types::Type;
use perry_hir::{Class, ClassField, Expr, Module, ModuleInitKind, Param, Stmt};

const ARRAY_ID: u32 = 1;
const ELEMENT_ID: u32 = 2;
const INDEX_ID: u32 = 3;
const SOURCE_ID: u32 = 4;
const CLOSURE_ID: u32 = 99;

fn row_class() -> Class {
    Class {
        id: 1,
        name: "Row".to_string(),
        type_params: Vec::new(),
        extends: None,
        extends_name: None,
        native_extends: None,
        extends_expr: None,
        heritage_lexically_shadowed: false,
        fields: vec![ClassField {
            name: "value".to_string(),
            key_expr: None,
            ty: Type::Number,
            init: None,
            is_private: false,
            is_readonly: false,
            decorators: Vec::new(),
        }],
        constructor: None,
        methods: Vec::new(),
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

fn module_with_callback(declare_source_array_param: bool) -> Module {
    let mut callback_params = vec![
        param(ELEMENT_ID, "row", Type::Named("Row".to_string())),
        param(INDEX_ID, "index", Type::Number),
    ];
    if declare_source_array_param {
        callback_params.push(param(
            SOURCE_ID,
            "source",
            Type::Array(Box::new(Type::Named("Row".to_string()))),
        ));
    }

    let callback = Expr::Closure {
        func_id: CLOSURE_ID,
        params: callback_params,
        return_type: Type::Number,
        body: vec![Stmt::Return(Some(Expr::PropertyGet {
            object: Box::new(Expr::LocalGet(ELEMENT_ID)),
            property: "value".to_string(),
            byte_offset: 0,
        }))],
        captures: Vec::new(),
        mutable_captures: Vec::new(),
        captures_this: false,
        captures_new_target: false,
        enclosing_class: None,
        is_arrow: true,
        is_async: false,
        is_generator: false,
        is_strict: false,
    };

    let mut module = Module::new("array_callback_shape.ts");
    module.classes = vec![row_class()];
    module.init = vec![
        Stmt::Let {
            id: ARRAY_ID,
            name: "rows".to_string(),
            ty: Type::Array(Box::new(Type::Named("Row".to_string()))),
            mutable: false,
            init: Some(Expr::Array(Vec::new())),
        },
        Stmt::Expr(Expr::ArrayPush {
            array_id: ARRAY_ID,
            value: Box::new(Expr::New {
                class_name: "Row".to_string(),
                args: Vec::new(),
                type_args: Vec::new(),
                byte_offset: 0,
                cap_args_appended: 0,
            }),
        }),
        Stmt::Expr(Expr::ArrayForEach {
            array: Box::new(Expr::LocalGet(ARRAY_ID)),
            callback: Box::new(callback),
        }),
    ];
    module.init_kind = ModuleInitKind::Eager;
    module
}

fn emit(module: &Module) -> String {
    String::from_utf8(
        compile_module(module, super::class_field_barrier_tests::ir_opts())
            .expect("callback-shape fixture compiles"),
    )
    .expect("LLVM IR is UTF-8")
}

fn callback_body(ir: &str) -> &str {
    let start = ir
        .lines()
        .find(|line| {
            line.starts_with("define ")
                && line.contains("perry_closure_")
                && line.contains(&format!("__{CLOSURE_ID}("))
        })
        .and_then(|line| ir.find(line))
        .expect("closure definition is emitted");
    let tail = &ir[start..];
    let end = tail.find("\n}\n").expect("closure definition terminates") + 3;
    &tail[..end]
}

#[test]
fn inline_array_callback_field_read_has_no_shape_guard() {
    let ir = emit(&module_with_callback(false));
    let callback = callback_body(&ir);
    assert!(
        !callback.contains("js_typed_feedback_class_field_get_guard")
            && !callback.contains("js_object_get_field_by_name_f64"),
        "the proven element parameter must use a direct fixed-offset load:\n{callback}"
    );
    assert!(
        callback.contains("getelementptr"),
        "the test must observe a real direct field load:\n{callback}"
    );
}

#[test]
fn callback_source_array_alias_keeps_the_shape_guard() {
    let ir = emit(&module_with_callback(true));
    let callback = callback_body(&ir);
    assert!(
        callback.contains("js_typed_feedback_class_field_get_guard")
            || callback.contains("js_object_get_field_by_name_f64"),
        "declaring the source-array argument must deny the cross-boundary fact:\n{callback}"
    );
}

fn module_with_some_callback(captures_this: bool) -> Module {
    let callback = Expr::Closure {
        func_id: CLOSURE_ID,
        params: vec![param(ELEMENT_ID, "row", Type::Named("Row".to_string()))],
        return_type: Type::Boolean,
        body: vec![Stmt::Return(Some(Expr::Bool(true)))],
        captures: Vec::new(),
        mutable_captures: Vec::new(),
        captures_this,
        captures_new_target: false,
        enclosing_class: None,
        is_arrow: true,
        is_async: false,
        is_generator: false,
        is_strict: false,
    };
    let mut module = Module::new("array_some_captureless.ts");
    module.classes = vec![row_class()];
    module.init = vec![
        Stmt::Let {
            id: ARRAY_ID,
            name: "rows".to_string(),
            ty: Type::Array(Box::new(Type::Named("Row".to_string()))),
            mutable: false,
            init: Some(Expr::Array(Vec::new())),
        },
        Stmt::Expr(Expr::ArraySome {
            array: Box::new(Expr::LocalGet(ARRAY_ID)),
            callback: Box::new(callback),
        }),
    ];
    module.init_kind = ModuleInitKind::Eager;
    module
}

#[test]
fn captureless_inline_some_passes_the_callback_body_directly() {
    let ir = emit(&module_with_some_callback(false));
    assert!(
        ir.contains("call double @js_array_some_captureless")
            && ir.contains("ptr @perry_closure_array_some_captureless_ts__99"),
        "a captureless inline arrow should pass its body symbol directly:\n{ir}"
    );
    // The admitted receiver runs the loop inline: the arrow's body is a direct
    // call (a null closure, then as many of element/index/receiver as it
    // declares — one here), a hole skips, a `true` result exits without a
    // truthiness call, and the runtime helper above is only the fallback.
    assert!(
        ir.contains("some.inline.loop")
            && ir.contains(
                "call double @perry_closure_array_some_captureless_ts__99(i64 0, double "
            )
            && ir.contains("call i64 @js_array_live_head(")
            && ir.contains("call i32 @js_is_truthy("),
        "the captureless some loop should run inline with the direct body call:\n{ir}"
    );
    assert!(
        !ir.contains("call i64 @js_closure_alloc_singleton")
            && !ir.contains("call double @js_array_some("),
        "the direct some path must not materialize or dynamically dispatch a closure:\n{ir}"
    );
}

#[test]
fn lexical_this_some_callback_keeps_the_closure_path() {
    let ir = emit(&module_with_some_callback(true));
    assert!(
        !ir.contains("call double @js_array_some_captureless")
            && ir.contains("call double @js_array_some("),
        "a callback with lexical-this state must retain its real closure environment:\n{ir}"
    );
}
