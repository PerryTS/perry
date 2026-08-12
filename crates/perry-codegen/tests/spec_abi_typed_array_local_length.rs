use perry_codegen::{compile_module, CompileOptions};
use perry_hir::types::Type;
use perry_hir::{Expr, Function, Module, Param, Stmt, TYPED_ARRAY_KIND_FLOAT64};

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

fn function_ir_section<'a>(ir: &'a str, symbol: &str) -> &'a str {
    let needle = format!("@{symbol}(");
    let mut search_start = 0;
    let start = loop {
        let Some(relative) = ir[search_start..].find(&needle) else {
            panic!("function `{symbol}` definition not found in IR:\n{ir}");
        };
        let symbol_pos = search_start + relative;
        let line_start = ir[..symbol_pos].rfind('\n').map_or(0, |index| index + 1);
        if ir[line_start..symbol_pos]
            .trim_start()
            .starts_with("define ")
        {
            break line_start;
        }
        search_start = symbol_pos + needle.len();
    };
    let rest = &ir[start..];
    let end = rest.find("\n}\n").map_or(rest.len(), |index| index + 3);
    &rest[..end]
}

#[test]
fn const_literal_length_local_routes_float64array_helper_to_raw_specialized_entry() {
    let mut module = Module::new("spec_abi_typed_array_local_length.ts");
    module.functions.push(Function {
        id: 7,
        name: "fill".to_string(),
        type_params: Vec::new(),
        params: vec![param(100, "values"), param(101, "nodes")],
        return_type: Type::Number,
        body: vec![
            Stmt::Let {
                id: 102,
                name: "index".to_string(),
                ty: Type::Number,
                mutable: false,
                init: Some(Expr::Binary {
                    op: perry_hir::BinaryOp::Sub,
                    left: Box::new(Expr::LocalGet(101)),
                    right: Box::new(Expr::Integer(10_000)),
                }),
            },
            Stmt::Expr(Expr::PutValueSet {
                target: Box::new(Expr::LocalGet(100)),
                key: Box::new(Expr::LocalGet(102)),
                value: Box::new(Expr::Number(1.25)),
                receiver: Box::new(Expr::LocalGet(100)),
                strict: false,
            }),
            Stmt::Return(Some(Expr::Number(1.25))),
        ],
        is_async: false,
        is_generator: false,
        is_strict: false,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
        was_plain_async: false,
        was_unrolled: false,
    });
    module.init = vec![
        Stmt::Let {
            id: 1,
            name: "nodes".to_string(),
            ty: Type::Number,
            mutable: false,
            init: Some(Expr::Integer(10_000)),
        },
        Stmt::Let {
            id: 2,
            name: "values".to_string(),
            ty: Type::Named("Float64Array".to_string()),
            mutable: false,
            init: Some(Expr::TypedArrayNew {
                kind: TYPED_ARRAY_KIND_FLOAT64,
                arg: Some(Box::new(Expr::LocalGet(1))),
            }),
        },
        Stmt::Expr(Expr::Call {
            callee: Box::new(Expr::FuncRef(7)),
            args: vec![Expr::LocalGet(2), Expr::LocalGet(1)],
            type_args: Vec::new(),
            byte_offset: 0,
        }),
    ];

    let ir = String::from_utf8(
        compile_module(
            &module,
            CompileOptions {
                emit_ir_only: true,
                ..CompileOptions::default()
            },
        )
        .unwrap(),
    )
    .unwrap();
    let symbol = "perry_fn_spec_abi_typed_array_local_length_ts__fill$spec_ta7x10000_i32";
    let specialized = function_ir_section(&ir, symbol);

    assert!(
        specialized.contains("i32 %arg101"),
        "the proven integer local must cross the specialized ABI as raw i32:\n{specialized}"
    );
    assert!(
        specialized.contains("store double"),
        "specialized helper must emit a raw Float64Array store:\n{specialized}"
    );
    assert!(
        !specialized.contains("js_typed_array_index_set_dynamic"),
        "specialized helper must not retain the boxed dynamic store:\n{specialized}"
    );
    assert!(
        ir.contains(&format!("call double @{symbol}(")),
        "the proven call site must call the specialized entry directly:\n{ir}"
    );
}
