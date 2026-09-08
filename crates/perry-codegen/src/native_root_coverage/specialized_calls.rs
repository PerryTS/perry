//! #9782: a raw typed-array argument still needs its boxed owner alive in
//! the caller while the specialized callee executes.

use super::*;

#[test]
fn a_last_use_typed_array_is_live_across_the_specialized_call() {
    let mut module = bare_module("last_use_typed_array.ts");
    module.functions.push(Function {
        id: 1,
        name: "consume".to_string(),
        type_params: Vec::new(),
        params: vec![Param {
            id: 10,
            name: "array".to_string(),
            ty: Type::Any,
            default: None,
            decorators: Vec::new(),
            is_rest: false,
            arguments_object: None,
        }],
        return_type: Type::Number,
        body: vec![
            Stmt::Expr(Expr::MapNew),
            Stmt::Return(Some(Expr::IndexGet {
                object: Box::new(Expr::LocalGet(10)),
                index: Box::new(Expr::Integer(0)),
            })),
        ],
        is_async: false,
        is_generator: false,
        is_strict: true,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
        was_plain_async: false,
        was_unrolled: false,
    });
    module.init = vec![
        let_stmt(
            20,
            "owner",
            Expr::TypedArrayNew {
                kind: perry_hir::TYPED_ARRAY_KIND_INT32,
                arg: Some(Box::new(Expr::Integer(256))),
            },
        ),
        Stmt::Expr(Expr::Call {
            callee: Box::new(Expr::FuncRef(1)),
            args: vec![Expr::LocalGet(20)],
            type_args: Vec::new(),
            byte_offset: 0,
        }),
    ];

    for target in NATIVE_TARGETS {
        let ir = native_ir(&module, target, true);
        let prefix = format!("perry_fn_last_use_typed_array_ts__consume${}", "spec_");
        let specialized = ir
            .lines()
            .filter(|line| line.starts_with("define "))
            .filter_map(|line| line.split_once('@')?.1.split_once('(').map(|p| p.0))
            .find(|name| name.starts_with(&prefix))
            .expect("fixture must select a raw typed-array entry");
        let points = statepoints_of(&ir, target, "main");
        for point in points.at(specialized) {
            assert!(
                !point.live.is_empty(),
                "[{target}] raw argument lost its owner: {point:?}"
            );
        }

        // Move the lifetime use (and its reloads) above the raw call. Merely
        // deleting the asm leaves dead post-call SSA uses that RS4GC still
        // considers live before the later dead-code elimination pass.
        let mut lines: Vec<_> = ir.lines().collect();
        let call = lines
            .iter()
            .position(|line| line.contains(&format!("call double @{specialized}(")))
            .expect("fixture must call its specialized entry");
        let end = lines
            .iter()
            .enumerate()
            .skip(call + 1)
            .find_map(|(i, line)| {
                line.contains("call void asm sideeffect \"\", \"r\"(i64")
                    .then_some(i)
            })
            .expect("post-call lifetime use must exist");
        let raw_call = lines.remove(call);
        lines.insert(end, raw_call);
        let broken = lines.join("\n");
        let broken_points = statepoints_of(&broken, target, "main");
        for point in broken_points.at(specialized) {
            assert!(
                point.live.is_empty(),
                "[{target}] control still roots the owner: {point:?}"
            );
        }
    }
}
