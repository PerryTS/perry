//! #10051: copies of a lexical scope must each execute their TDZ allocation.
use perry_hir::{types::Type, Expr, Function, Module, Stmt};

fn emit(body: Vec<Stmt>) -> String {
    let mut module = Module::new("tdz_paths.ts");
    module.functions.push(Function {
        id: 1,
        name: "test".into(),
        type_params: Vec::new(),
        params: Vec::new(),
        return_type: Type::Any,
        body,
        is_async: false,
        is_generator: false,
        is_strict: true,
        is_exported: true,
        captures: Vec::new(),
        decorators: Vec::new(),
        was_plain_async: false,
        was_unrolled: false,
    });
    String::from_utf8(
        crate::compile_module(&module, super::prealloc_module_global_tests::ir_opts()).unwrap(),
    )
    .unwrap()
}

fn allocated_slots(ir: &str, seed: &str) -> Vec<String> {
    ir.lines()
        .filter_map(|line| {
            if !line.contains(&format!("call i64 @js_box_alloc_bits(i64 {seed})")) {
                return None;
            }
            let value = line.trim().split(" = ").next().unwrap();
            let prefix = format!("store i64 {value}, ptr ");
            Some(
                ir.lines()
                    .find_map(|store| store.trim().strip_prefix(&prefix).map(str::to_string))
                    .expect("each allocated box is stored"),
            )
        })
        .collect()
}

#[test]
fn tdz_finally_allocates_on_normal_and_exception_paths() {
    let ir = emit(vec![Stmt::Try {
        body: vec![Stmt::Expr(Expr::Call {
            callee: Box::new(Expr::LocalGet(99)),
            args: Vec::new(),
            type_args: Vec::new(),
            byte_offset: 0,
        })],
        catch: None,
        finally: Some(vec![
            Stmt::PreallocateTdzBoxes(vec![10]),
            Stmt::Let {
                id: 10,
                name: "value".into(),
                ty: Type::Any,
                mutable: true,
                init: Some(Expr::Integer(42)),
            },
        ]),
    }]);
    let slots = allocated_slots(&ir, crate::nanbox::TAG_TDZ_I64);
    assert_eq!(
        slots.len(),
        2,
        "both finally paths need fresh TDZ cells:\n{ir}"
    );
    assert_eq!(slots[0], slots[1], "path copies share one stack slot");
    assert!(
        ir.contains(&format!(
            "store i64 {}, ptr {}",
            crate::nanbox::TAG_UNDEFINED_I64,
            slots[0]
        )),
        "slot must be entry-initialized"
    );
}

#[test]
fn ordinary_preallocation_still_preserves_an_existing_cell() {
    let ir = emit(vec![
        Stmt::PreallocateBoxes(vec![10]),
        Stmt::PreallocateBoxes(vec![10]),
    ]);
    assert_eq!(
        allocated_slots(&ir, crate::nanbox::TAG_UNDEFINED_I64).len(),
        1,
        "ordinary function-scoped cells must not be freshened:\n{ir}"
    );
}
