//! `collectors::loop_pushed_arrays`: an empty literal a later loop pushes into
//! gets the larger birth; one pushed once per iteration of its own loop, or
//! never in a loop, keeps the four-slot birth.
use super::loop_pushed_arrays;
use perry_hir::types::Type;
use perry_hir::{Expr, Module, ModuleInitKind, Stmt};

fn empty(id: u32) -> Stmt {
    Stmt::Let {
        id,
        name: format!("a{id}"),
        ty: Type::Array(Box::new(Type::Number)),
        mutable: false,
        init: Some(Expr::Array(Vec::new(), None)),
    }
}

fn push(id: u32) -> Stmt {
    Stmt::Expr(Expr::ArrayPush {
        array_id: id,
        value: Box::new(Expr::Number(1.0)),
        field_writeback: None,
    })
}

fn looped(body: Vec<Stmt>) -> Stmt {
    Stmt::While {
        condition: Expr::Bool(false),
        body,
    }
}

fn module(init: Vec<Stmt>) -> Module {
    let mut m = Module::new("loop_pushed_arrays.ts");
    m.init = init;
    m.init_kind = ModuleInitKind::Eager;
    m
}

#[test]
fn only_a_loop_entered_after_the_binding_counts() {
    let m = module(vec![
        // Filled by a later loop: the larger birth.
        empty(1),
        looped(vec![push(1)]),
        // A fresh array per iteration, pushed once: four slots.
        looped(vec![empty(2), push(2)]),
        // Pushed outside any loop: four slots.
        empty(3),
        push(3),
        // Declared in a loop, filled by a nested one: the larger birth.
        looped(vec![empty(4), looped(vec![push(4)])]),
    ]);
    let mut got: Vec<u32> = loop_pushed_arrays(&m).reserve.into_iter().collect();
    got.sort_unstable();
    assert_eq!(got, vec![1, 4]);
}

#[test]
fn the_larger_birth_reserves_sixteen_slots() {
    let m = module(vec![empty(1), looped(vec![push(1)]), empty(2), push(2)]);
    let ir = String::from_utf8(
        crate::compile_module(&m, crate::expr::class_field_barrier_tests::ir_opts()).unwrap(),
    )
    .unwrap();
    assert!(
        ir.contains("define linkonce_odr preserve_mostcc i64 @perry_birth_empty_array_16("),
        "{ir}"
    );
    assert!(
        ir.contains("define linkonce_odr preserve_mostcc i64 @perry_birth_empty_array("),
        "the once-pushed literal keeps the four-slot birth:\n{ir}"
    );
    // Length 0, capacity 16.
    assert!(
        ir.contains(&format!("store i64 {}, ptr %array", 16u64 << 32)),
        "{ir}"
    );
}

#[test]
fn owned_arrays_are_only_pushed_read_and_returned() {
    let get = |id: u32| Expr::LocalGet(id);
    let length = |id: u32| Expr::PropertyGet {
        object: Box::new(get(id)),
        property: "length".into(),
        byte_offset: 0,
        byte_end: 0,
    };
    let other = |id: u32| Stmt::Let {
        id,
        name: format!("o{id}"),
        ty: Type::Any,
        mutable: false,
        init: Some(Expr::Number(0.0)),
    };
    let m = module(vec![
        // Pushed, its length read and returned: owned.
        empty(1),
        looped(vec![push(1)]),
        Stmt::Expr(length(1)),
        // Aliased into another binding: not owned.
        empty(2),
        push(2),
        Stmt::Let {
            id: 20,
            name: "alias".into(),
            ty: Type::Any,
            mutable: false,
            init: Some(get(2)),
        },
        // Pushed as a value into another array: not owned.
        empty(3),
        Stmt::Expr(Expr::ArrayPush {
            array_id: 1,
            value: Box::new(get(3)),
            field_writeback: None,
        }),
        // Reassigned: not owned.
        empty(4),
        Stmt::Expr(Expr::LocalSet(4, Box::new(Expr::Array(Vec::new(), None)))),
        // Not bound to an array literal: not owned.
        other(5),
        push(5),
        Stmt::Return(Some(get(1))),
    ]);
    let mut got: Vec<u32> = loop_pushed_arrays(&m).owned.into_iter().collect();
    got.sort_unstable();
    assert_eq!(got, vec![1]);
}

#[test]
fn an_array_another_body_mentions_is_not_owned() {
    let mut m = module(vec![empty(1), looped(vec![push(1)]), empty(2), push(2)]);
    // A module function pushing into the module-level array 1.
    m.functions.push(perry_hir::Function {
        id: 0,
        name: "addTop".into(),
        type_params: Vec::new(),
        params: Vec::new(),
        return_type: Type::Any,
        body: vec![push(1)],
        is_async: false,
        is_generator: false,
        is_strict: true,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
        was_plain_async: false,
        was_unrolled: false,
    });
    let mut got: Vec<u32> = loop_pushed_arrays(&m).owned.into_iter().collect();
    got.sort_unstable();
    assert_eq!(got, vec![2]);
}

/// IR of `m` with the facts' list capacities `caps` (local id, slots).
fn ir_with_list_capacities(m: &Module, caps: &[(u32, u32)]) -> String {
    let mut elision = crate::codegen::ElisionFacts::default();
    elision.list_capacities.extend(caps.iter().copied());
    let mut opts = crate::expr::class_field_barrier_tests::ir_opts();
    opts.program_facts = crate::codegen::ProgramFactsView {
        elision: std::sync::Arc::new(elision),
        ..Default::default()
    };
    String::from_utf8(crate::compile_module(m, opts).unwrap()).unwrap()
}

/// A capacity from the facts (wpfacts `list_caps`) is the sized birth with
/// that many slots (rounded up to a multiple of four: the holes are stored
/// four at a time); it replaces the collector's sixteen, and a local the facts do
/// not name keeps what the collector gives it.
#[test]
fn a_list_capacity_from_the_facts_is_the_sized_birth() {
    let m = module(vec![
        empty(1),
        looped(vec![push(1)]),
        empty(2),
        push(2),
        empty(3),
        looped(vec![push(3)]),
    ]);
    let ir = ir_with_list_capacities(&m, &[(1, 40), (2, 7)]);
    assert!(
        ir.contains("define linkonce_odr preserve_mostcc i64 @perry_birth_empty_array_n("),
        "{ir}"
    );
    assert!(
        ir.contains("i64 @perry_birth_empty_array_n(ptr null, ptr %"),
        "the collecting arm passes no raw cell:\n{ir}"
    );
    for slots in [40, 8] {
        assert!(
            ir.contains(&format!(", i64 {slots})")),
            "a {slots}-slot birth:\n{ir}"
        );
    }
    // Local 3: no fact, the collector's birth.
    assert!(
        ir.contains("define linkonce_odr preserve_mostcc i64 @perry_birth_empty_array_16("),
        "{ir}"
    );
    // Without facts nothing is sized.
    let plain = ir_with_list_capacities(&m, &[]);
    assert!(!plain.contains("@perry_birth_empty_array_n("), "{plain}");
}
