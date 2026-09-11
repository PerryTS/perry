//! #10048: emitting an earlier branch does not initialize a sibling branch.
use perry_hir::types::Type;
use perry_hir::{Expr, Function, Module, Param, Stmt};

fn branch(tdz: bool) -> Vec<Stmt> {
    vec![
        if tdz {
            Stmt::PreallocateTdzBoxes(vec![101])
        } else {
            Stmt::PreallocateBoxes(vec![101])
        },
        Stmt::Let {
            id: 101,
            name: "callback".into(),
            ty: Type::Any,
            mutable: true,
            init: Some(Expr::Integer(40)),
        },
        Stmt::Return(Some(Expr::LocalGet(101))),
    ]
}

fn assert_each_continuation_allocates(tdz: bool) {
    let mut module = Module::new("prealloc_continuation.ts");
    module.functions.push(Function {
        id: 1,
        name: "resume".into(),
        type_params: Vec::new(),
        params: vec![Param {
            id: 100,
            name: "state".into(),
            ty: Type::Any,
            default: None,
            decorators: Vec::new(),
            is_rest: false,
            arguments_object: None,
        }],
        return_type: Type::Any,
        body: vec![Stmt::If {
            condition: Expr::LocalGet(100),
            then_branch: branch(tdz),
            else_branch: Some(branch(tdz)),
        }],
        is_async: false,
        is_generator: false,
        is_strict: true,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
        was_plain_async: false,
        was_unrolled: false,
    });
    let ir = String::from_utf8(
        crate::compile_module(&module, super::prealloc_module_global_tests::ir_opts()).unwrap(),
    )
    .unwrap();
    let allocations: Vec<_> = ir
        .lines()
        .filter(|line| line.contains("call i64 @js_box_alloc_bits("))
        .collect();
    // Both mutually exclusive copies initialize the same lexical binding.
    // The pre-fix ctx.locals check only emitted the first allocation.
    assert_eq!(
        allocations.len(),
        2,
        "each continuation must allocate:\n{ir}"
    );
    let seed = if tdz {
        crate::nanbox::TAG_TDZ_I64
    } else {
        crate::nanbox::TAG_UNDEFINED_I64
    };
    let mut slots = Vec::new();
    for allocation in allocations {
        assert!(
            allocation.contains(seed),
            "wrong initial cell value: {allocation}"
        );
        let result = allocation.trim().split(" = ").next().unwrap();
        let store = ir
            .lines()
            .find(|line| {
                line.trim()
                    .starts_with(&format!("store i64 {result}, ptr "))
            })
            .expect("each allocated box must initialize its pointer slot");
        slots.push(store.trim().split(", ptr ").nth(1).unwrap());
    }
    assert_eq!(
        slots[0], slots[1],
        "continuations must share the lexical slot"
    );
    assert!(
        ir.contains(&format!(
            "store i64 {}, ptr {}",
            crate::nanbox::TAG_UNDEFINED_I64,
            slots[0]
        )),
        "bypassed declarations still need the entry sentinel"
    );
}

#[test]
fn each_plain_continuation_allocates_its_cell() {
    assert_each_continuation_allocates(false);
}

#[test]
fn each_tdz_continuation_allocates_its_cell() {
    assert_each_continuation_allocates(true);
}
