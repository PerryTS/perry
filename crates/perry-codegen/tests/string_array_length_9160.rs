use perry_codegen::{compile_module, CompileOptions};
use perry_hir::types::Type;
use perry_hir::{BinaryOp, CompareOp, Expr, Function, Module, Param, Stmt, UpdateOp};

const STRINGS: u32 = 10;
const TOTAL: u32 = 11;
const COUNTER: u32 = 12;

fn string_array_param() -> Param {
    Param {
        id: STRINGS,
        name: "strings".to_string(),
        ty: Type::Array(Box::new(Type::String)),
        default: None,
        decorators: Vec::new(),
        is_rest: false,
        arguments_object: None,
    }
}

fn sum_lengths() -> Function {
    let indexed_string = Expr::IndexGet {
        object: Box::new(Expr::LocalGet(STRINGS)),
        index: Box::new(Expr::Binary {
            op: BinaryOp::BitAnd,
            left: Box::new(Expr::LocalGet(COUNTER)),
            right: Box::new(Expr::Integer(3)),
        }),
    };
    Function {
        id: 1,
        name: "sumLengths".to_string(),
        type_params: Vec::new(),
        params: vec![string_array_param()],
        return_type: Type::Number,
        body: vec![
            Stmt::Let {
                id: TOTAL,
                name: "total".to_string(),
                ty: Type::Number,
                mutable: true,
                init: Some(Expr::Integer(0)),
            },
            Stmt::For {
                init: Some(Box::new(Stmt::Let {
                    id: COUNTER,
                    name: "i".to_string(),
                    ty: Type::Number,
                    mutable: true,
                    init: Some(Expr::Integer(0)),
                })),
                condition: Some(Expr::Compare {
                    op: CompareOp::Lt,
                    left: Box::new(Expr::LocalGet(COUNTER)),
                    right: Box::new(Expr::Integer(1000)),
                }),
                update: Some(Expr::Update {
                    id: COUNTER,
                    op: UpdateOp::Increment,
                    prefix: false,
                }),
                body: vec![Stmt::Expr(Expr::LocalSet(
                    TOTAL,
                    Box::new(Expr::Binary {
                        op: BinaryOp::Add,
                        left: Box::new(Expr::LocalGet(TOTAL)),
                        right: Box::new(Expr::PropertyGet {
                            object: Box::new(indexed_string),
                            property: "length".to_string(),
                            byte_offset: 0,
                        }),
                    }),
                ))],
            },
            Stmt::Return(Some(Expr::LocalGet(TOTAL))),
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

fn compile_ir() -> String {
    let mut module = Module::new("string_array_length_9160.ts");
    module.functions.push(sum_lengths());
    module.init.push(Stmt::Expr(Expr::Call {
        callee: Box::new(Expr::FuncRef(1)),
        args: vec![Expr::Array(vec![
            Expr::String("a".to_string()),
            Expr::String("bb".to_string()),
            Expr::String("ccc".to_string()),
            Expr::String("dddddddddddddddd".to_string()),
        ])],
        type_args: Vec::new(),
        byte_offset: 0,
    }));
    String::from_utf8(
        compile_module(
            &module,
            CompileOptions {
                emit_ir_only: true,
                ..CompileOptions::default()
            },
        )
        .expect("module compiles"),
    )
    .expect("LLVM IR is UTF-8")
}

#[test]
fn masked_string_length_loop_has_call_free_element_and_length_fast_path() {
    let ir = compile_ir();
    assert!(
        ir.contains("call i32 @js_string_array_range_loop_guard"),
        "each emitted function clone must validate the full string window before its fast loop:\n{ir}"
    );
    let fast_start = ir
        .find("for.string_length_fast.body")
        .expect("fast loop body");
    let slow_start = ir[fast_start..]
        .find("for.string_length_slow.cond")
        .map(|offset| fast_start + offset)
        .expect("semantic fallback loop");
    let fast = &ir[fast_start..slow_start];
    assert!(
        fast.contains("strlen.sso") && fast.contains("strlen.heap") && fast.contains("fadd double"),
        "the fast clone must load the boxed slot and select SSO/heap length inline:\n{fast}"
    );
    for helper in [
        "js_value_length_property_f64",
        "js_typed_feedback_array_get_f64",
        "js_dyn_index_get",
    ] {
        assert!(
            !fast.contains(helper),
            "the guarded fast clone must not call `{helper}`:\n{fast}"
        );
    }
    assert!(
        ir[slow_start..].contains("js_value_length_property_f64"),
        "guard failure must retain ordinary property semantics:\n{}",
        &ir[slow_start..]
    );
}
