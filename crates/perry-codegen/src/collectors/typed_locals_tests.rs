use super::*;
use perry_hir::types::Type as HirType;

fn let_(id: u32, init: Option<Expr>) -> Stmt {
    Stmt::Let {
        id,
        name: format!("v{id}"),
        ty: HirType::Any,
        mutable: true,
        init,
    }
}

fn set(id: u32, v: Expr) -> Stmt {
    Stmt::Expr(Expr::LocalSet(id, Box::new(v)))
}

fn get(id: u32) -> Expr {
    Expr::LocalGet(id)
}

fn method(recv: Expr, name: &str, args: Vec<Expr>) -> Expr {
    Expr::Call {
        byte_end: 0,
        callee: Box::new(Expr::PropertyGet {
            byte_end: 0,
            object: Box::new(recv),
            property: name.into(),
            byte_offset: 0,
        }),
        args,
        type_args: Vec::new(),
        byte_offset: 0,
    }
}

fn typeof_is(id: u32, lit: &str, eq: bool) -> Expr {
    Expr::Compare {
        op: if eq { CompareOp::Eq } else { CompareOp::Ne },
        left: Box::new(Expr::TypeOf(Box::new(get(id)))),
        right: Box::new(Expr::String(lit.into())),
    }
}

fn bin(op: BinaryOp, l: Expr, r: Expr) -> Expr {
    Expr::Binary {
        op,
        left: Box::new(l),
        right: Box::new(r),
    }
}

fn opaque_call() -> Expr {
    Expr::Call {
        byte_end: 0,
        callee: Box::new(get(99)),
        args: vec![get(1)],
        type_args: Vec::new(),
        byte_offset: 0,
    }
}

/// qs `utils.encode`: `var string = str; if (typeof str === 'symbol')
/// string = <opaque>; else if (typeof str !== 'string') string = <opaque>;
/// var seg = string.slice(0, 2); var c = seg.charCodeAt(0);
/// c = 0x10000 + (c & 0x3FF); use(c)`.
fn encode_body() -> Vec<Stmt> {
    vec![
        let_(2, Some(get(1))),
        Stmt::If {
            condition: typeof_is(1, "symbol", true),
            then_branch: vec![set(2, opaque_call())],
            else_branch: Some(vec![Stmt::If {
                condition: typeof_is(1, "string", false),
                then_branch: vec![set(2, opaque_call())],
                else_branch: None,
            }]),
        },
        let_(
            3,
            Some(method(
                get(2),
                "slice",
                vec![Expr::Integer(0), Expr::Integer(2)],
            )),
        ),
        let_(
            4,
            Some(method(get(3), "charCodeAt", vec![Expr::Integer(0)])),
        ),
        set(
            4,
            bin(
                BinaryOp::Add,
                Expr::Integer(0x10000),
                bin(BinaryOp::BitAnd, get(4), Expr::Integer(0x3FF)),
            ),
        ),
        Stmt::Return(Some(get(4))),
    ]
}

fn run(body: &[Stmt], proofs: &[(u32, HirType)]) -> TypedLocals {
    let proofs: HashMap<u32, HirType> = proofs.iter().cloned().collect();
    collect_typed_locals(body, &proofs, &HashSet::new(), &HashMap::new())
}

#[test]
fn a_proven_string_param_decides_typeof_branches_and_types_the_chain() {
    let t = run(&encode_body(), &[(1, HirType::String)]);
    assert_eq!(t.strings, HashSet::from([2, 3]));
    assert_eq!(t.numbers, HashSet::from([4]));
}

#[test]
fn without_the_entry_proof_nothing_downstream_is_proven() {
    let t = run(&encode_body(), &[]);
    assert!(t.strings.is_empty(), "{t:?}");
    assert!(t.numbers.is_empty(), "{t:?}");
}

#[test]
fn a_number_proof_keeps_the_typeof_number_branch() {
    // Under `typeof p === 'number'`, `typeof p !== 'string'` is TRUE: the
    // opaque write stays live and `string` is not proven.
    let t = run(&encode_body(), &[(1, HirType::Number)]);
    assert!(!t.strings.contains(&2), "{t:?}");
}

#[test]
fn a_read_before_the_first_write_is_not_proven() {
    // if (k) use(n + 1); var n = 5; n = n * 2;
    let body = vec![
        Stmt::If {
            condition: get(7),
            then_branch: vec![Stmt::Expr(bin(BinaryOp::Add, get(5), Expr::Integer(1)))],
            else_branch: None,
        },
        let_(5, Some(Expr::Integer(5))),
        set(5, bin(BinaryOp::Mul, get(5), Expr::Integer(2))),
    ];
    let t = run(&body, &[]);
    assert!(!t.numbers.contains(&5), "{t:?}");
}

#[test]
fn a_loop_write_does_not_dominate_a_read_after_the_loop() {
    let body = vec![
        Stmt::While {
            condition: get(7),
            body: vec![let_(5, Some(Expr::String("x".into())))],
        },
        Stmt::Return(Some(get(5))),
    ];
    assert!(run(&body, &[]).strings.is_empty());
    // ... but a read later in the same iteration is dominated.
    let body = vec![Stmt::While {
        condition: get(7),
        body: vec![
            let_(5, Some(Expr::String("x".into()))),
            Stmt::Expr(method(get(5), "charCodeAt", vec![Expr::Integer(0)])),
        ],
    }];
    assert_eq!(run(&body, &[]).strings, HashSet::from([5]));
}

#[test]
fn an_increment_turns_a_string_into_a_number() {
    let body = vec![
        let_(5, Some(Expr::String("1".into()))),
        Stmt::Expr(Expr::Update {
            id: 5,
            op: perry_hir::UpdateOp::Increment,
            prefix: false,
        }),
        Stmt::Return(Some(get(5))),
    ];
    let t = run(&body, &[]);
    assert!(t.strings.is_empty() && t.numbers.is_empty(), "{t:?}");
}

#[test]
fn concat_with_a_string_is_a_string_and_unknown_operands_stay_unknown() {
    let body = vec![
        let_(5, Some(Expr::String(String::new()))),
        set(5, bin(BinaryOp::Add, get(5), opaque_call())),
        let_(6, Some(opaque_call())),
        set(6, bin(BinaryOp::Add, get(6), Expr::Integer(1))),
        Stmt::Return(Some(bin(BinaryOp::Add, get(5), get(6)))),
    ];
    let t = run(&body, &[]);
    assert_eq!(t.strings, HashSet::from([5]));
    assert!(t.numbers.is_empty(), "{t:?}");
}

#[test]
fn a_closure_capturing_before_the_write_disqualifies_the_local() {
    let clo = Expr::Closure {
        func_id: 50,
        params: Vec::new(),
        return_type: HirType::Any,
        body: vec![Stmt::Return(Some(get(5)))],
        captures: vec![5],
        mutable_captures: Vec::new(),
        captures_this: false,
        captures_new_target: false,
        enclosing_class: None,
        is_arrow: false,
        is_async: false,
        is_generator: false,
        is_strict: true,
    };
    let body = vec![
        let_(6, Some(clo)),
        let_(5, Some(Expr::String("x".into()))),
        Stmt::Return(Some(get(5))),
    ];
    assert!(run(&body, &[]).strings.is_empty());
}

#[test]
fn hoisted_var_seeds_are_not_writes_but_reads_must_follow_a_real_write() {
    // var s, c; (HIR: `Let s = undefined` seeds) s = p.slice(1); c =
    // s.charCodeAt(0); return c;
    let body = vec![
        let_(5, Some(Expr::Undefined)),
        let_(6, None),
        set(5, method(get(1), "slice", vec![Expr::Integer(1)])),
        set(6, method(get(5), "charCodeAt", vec![Expr::Integer(0)])),
        Stmt::Return(Some(get(6))),
    ];
    let t = run(&body, &[(1, HirType::String)]);
    assert_eq!(t.strings, HashSet::from([5]));
    assert_eq!(t.numbers, HashSet::from([6]));
    // A read between the seed and the first write sees `undefined`.
    let mut early = body.clone();
    early.insert(2, Stmt::Expr(get(5)));
    let t = run(&early, &[(1, HirType::String)]);
    assert!(!t.strings.contains(&5), "{t:?}");
    assert!(!t.numbers.contains(&6), "{t:?}");
}

#[test]
fn a_literal_array_local_stays_an_array_through_in_place_pushes() {
    let body = vec![
        let_(5, Some(Expr::Undefined)),
        set(5, Expr::Array(Vec::new(), None)),
        Stmt::Expr(Expr::ArrayPush {
            array_id: 5,
            value: Box::new(Expr::String("x".into())),
            field_writeback: None,
        }),
        Stmt::Return(Some(Expr::PropertyGet {
            byte_end: 0,
            object: Box::new(get(5)),
            property: "length".into(),
            byte_offset: 0,
        })),
    ];
    let t = run(&body, &[]);
    assert_eq!(t.arrays, HashSet::from([5]));
    // Another write kind drops it.
    let mut mixed = body.clone();
    mixed.insert(2, set(5, Expr::String("s".into())));
    assert!(run(&mixed, &[]).arrays.is_empty());
}

fn closure(func_id: u32, body: Vec<Stmt>) -> Expr {
    Expr::Closure {
        func_id,
        params: Vec::new(),
        return_type: HirType::Any,
        body,
        captures: Vec::new(),
        mutable_captures: Vec::new(),
        captures_this: false,
        captures_new_target: false,
        enclosing_class: None,
        is_arrow: false,
        is_async: false,
        is_generator: false,
        is_strict: true,
    }
}

fn iife(func_id: u32, body: Vec<Stmt>) -> Expr {
    Expr::Call {
        byte_end: 0,
        callee: Box::new(closure(func_id, body)),
        args: Vec::new(),
        type_args: Vec::new(),
        byte_offset: 0,
    }
}

#[test]
fn an_iife_that_always_returns_its_array_local_types_the_binding() {
    // var hexTable = (function () { var a = []; …; return a; })();
    let table = iife(
        60,
        vec![
            let_(61, Some(Expr::Undefined)),
            set(61, Expr::Array(Vec::new(), None)),
            Stmt::Return(Some(get(61))),
        ],
    );
    let body = vec![
        let_(5, Some(Expr::Undefined)),
        set(5, table),
        Stmt::Return(Some(get(5))),
    ];
    assert_eq!(run(&body, &[]).arrays, HashSet::from([5]));
    // A body that can fall off its end may return `undefined`.
    let maybe = iife(
        62,
        vec![Stmt::If {
            condition: get(7),
            then_branch: vec![Stmt::Return(Some(Expr::Array(Vec::new(), None)))],
            else_branch: None,
        }],
    );
    let body = vec![let_(5, Some(maybe)), Stmt::Return(Some(get(5)))];
    assert!(run(&body, &[]).arrays.is_empty());
}

fn arr_any() -> HirType {
    HirType::Array(Box::new(HirType::Any))
}

#[test]
fn outer_proofs_meet_capture_and_definite_facts() {
    let mut definite = crate::collectors::DefiniteInit::default();
    definite.kinds.insert(7, arr_any());
    definite.kinds.insert(8, arr_any());
    // 8 is declared by body 3: body 3 gets no definite fact for it.
    definite
        .declaring
        .insert(8, [Some(3)].into_iter().collect());
    let captures: HashMap<u32, HirType> = [
        (7, HirType::Array(Box::new(HirType::String))),
        (9, HirType::String),
    ]
    .into_iter()
    .collect();
    let seed = outer_proofs(Some(&captures), &definite, Some(3));
    // Comparable facts: the narrower one wins.
    assert_eq!(
        seed.get(&7),
        Some(&HirType::Array(Box::new(HirType::String)))
    );
    assert_eq!(seed.get(&9), Some(&HirType::String));
    assert!(!seed.contains_key(&8));
    // Another body sees 8; equal facts stand.
    let seed = outer_proofs(None, &definite, Some(4));
    assert_eq!(seed.get(&7), Some(&arr_any()));
    assert_eq!(seed.get(&8), Some(&arr_any()));
}

#[test]
fn incomparable_outer_facts_do_not_meet() {
    assert_eq!(meet(&HirType::String, &arr_any()), None);
    assert_eq!(meet(&arr_any(), &arr_any()), Some(arr_any()));
}

#[test]
fn outer_seed_fills_only_unknown_oracle_slots_and_never_numbers() {
    let outer: HashMap<u32, HirType> = [(1, arr_any()), (2, arr_any()), (3, HirType::Number)]
        .into_iter()
        .collect();
    let mut local_types: HashMap<u32, HirType> =
        [(1, HirType::Any), (2, HirType::Named("Foo".into()))]
            .into_iter()
            .collect();
    seed_local_types(&outer, &mut local_types);
    assert_eq!(local_types.get(&1), Some(&arr_any()));
    assert_eq!(local_types.get(&2), Some(&HirType::Named("Foo".into())));
    assert!(!local_types.contains_key(&3));
    assert!(!outer_only(&outer).contains_key(&3));
}

#[test]
fn a_char_code_of_a_proven_string_and_a_bounded_combination_is_a_code_unit() {
    let t = run(&encode_body(), &[(1, HirType::String)]);
    assert_eq!(t.code_units, HashSet::from([4]), "{t:?}");
    assert!(run(&encode_body(), &[]).code_units.is_empty());
}

#[test]
fn a_write_that_can_leave_int32_is_not_a_code_unit() {
    // var c = s.charCodeAt(0); c = c / 2;   (a Number, not int32-or-NaN)
    let body = vec![
        let_(5, Some(Expr::String("ab".into()))),
        let_(
            6,
            Some(method(get(5), "charCodeAt", vec![Expr::Integer(0)])),
        ),
        set(6, bin(BinaryOp::Div, get(6), Expr::Integer(2))),
        Stmt::Return(Some(get(6))),
    ];
    let t = run(&body, &[]);
    assert!(t.numbers.contains(&6) && t.code_units.is_empty(), "{t:?}");
    // c++ can step out of int32 too.
    let body = vec![
        let_(5, Some(Expr::String("ab".into()))),
        let_(
            6,
            Some(method(get(5), "charCodeAt", vec![Expr::Integer(0)])),
        ),
        Stmt::Expr(Expr::Update {
            id: 6,
            op: perry_hir::UpdateOp::Increment,
            prefix: false,
        }),
        Stmt::Return(Some(get(6))),
    ];
    let t = run(&body, &[]);
    assert!(t.numbers.contains(&6) && t.code_units.is_empty(), "{t:?}");
}

#[test]
fn a_copy_of_a_code_unit_is_a_code_unit_and_an_unknown_receiver_is_not() {
    let body = vec![
        let_(5, Some(Expr::String("ab".into()))),
        let_(6, Some(method(get(5), "charCodeAt", vec![get(7)]))),
        let_(8, Some(get(6))),
        let_(
            9,
            Some(method(get(7), "charCodeAt", vec![Expr::Integer(0)])),
        ),
        Stmt::Return(Some(bin(BinaryOp::Add, get(8), get(9)))),
    ];
    let t = run(&body, &[]);
    assert_eq!(t.code_units, HashSet::from([6, 8]), "{t:?}");
}
