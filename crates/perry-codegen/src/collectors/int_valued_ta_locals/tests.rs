use super::*;
use perry_hir::{BinaryOp, Expr, Param, Stmt, UpdateOp};

fn let_stmt(id: u32, ty: HirType, init: Option<Expr>) -> Stmt {
    Stmt::Let {
        id,
        name: format!("v{id}"),
        ty,
        mutable: true,
        init,
    }
}

fn set(id: u32, rhs: Expr) -> Stmt {
    Stmt::Expr(Expr::LocalSet(id, Box::new(rhs)))
}

fn bin(op: BinaryOp, l: Expr, r: Expr) -> Expr {
    Expr::Binary {
        op,
        left: Box::new(l),
        right: Box::new(r),
    }
}

fn xor(l: Expr, r: Expr) -> Expr {
    bin(BinaryOp::BitXor, l, r)
}

/// `S[idx]` — an element read on local `arr_id`.
fn idx_get(arr_id: u32, idx: Expr) -> Expr {
    Expr::IndexGet {
        object: Box::new(Expr::LocalGet(arr_id)),
        index: Box::new(idx),
    }
}

/// Params: `arr: Int32Array` (id 0), `off: number` (id 1) unless overridden.
fn int32_array_param(id: u32) -> Param {
    Param {
        id,
        name: format!("p{id}"),
        ty: HirType::Named("Int32Array".to_string()),
        default: None,
        decorators: vec![],
        is_rest: false,
        arguments_object: None,
    }
}

fn number_param(id: u32) -> Param {
    Param {
        id,
        name: format!("p{id}"),
        ty: HirType::Number,
        default: None,
        decorators: vec![],
        is_rest: false,
        arguments_object: None,
    }
}

fn run(stmts: &[Stmt], params: &[Param]) -> HashSet<u32> {
    collect_int_valued_ta_locals(stmts, params, &HashMap::new(), &HashMap::new())
}

#[test]
fn bcrypt_feistel_accumulator_is_eligible() {
    // arr:Int32Array (0), off:number (1)
    //   let l = arr[off];        // int-TA read init (SEED, possibly OOB)
    //   l = l ^ 0x12345678;      // bitwise
    //   arr[off] = l;            // int-TA store value (coercing)
    let params = [int32_array_param(0), number_param(1)];
    let stmts = vec![
        let_stmt(9, HirType::Any, Some(idx_get(0, Expr::LocalGet(1)))),
        set(9, xor(Expr::LocalGet(9), Expr::Integer(0x1234_5678))),
        Stmt::Expr(Expr::PutValueSet {
            target: Box::new(Expr::LocalGet(0)),
            key: Box::new(Expr::LocalGet(1)),
            value: Box::new(Expr::LocalGet(9)),
            receiver: Box::new(Expr::LocalGet(0)),
            strict: false,
        }),
    ];
    let got = run(&stmts, &params);
    assert!(got.contains(&9), "feistel accumulator missing: {got:?}");
}

#[test]
fn additive_write_excludes_local() {
    // `n = arr[i]; n = n + arr[j];` — the additive write can overflow i32, so
    // `n` must NOT be admitted (mirrors `_encipher`'s `n`).
    let params = [int32_array_param(0), number_param(1)];
    let stmts = vec![
        let_stmt(8, HirType::Number, Some(Expr::Integer(0))),
        set(8, idx_get(0, Expr::LocalGet(1))),
        set(
            8,
            bin(
                BinaryOp::Add,
                Expr::LocalGet(8),
                idx_get(0, Expr::LocalGet(1)),
            ),
        ),
    ];
    let got = run(&stmts, &params);
    assert!(
        !got.contains(&8),
        "additive local wrongly admitted: {got:?}"
    );
}

#[test]
fn console_log_style_observation_excludes_local() {
    // `let x = arr[off]; f(x);` — `x` observed as a call argument (where
    // undefined-vs-integer is distinguishable) must be excluded.
    let params = [int32_array_param(0), number_param(1)];
    let stmts = vec![
        let_stmt(5, HirType::Any, Some(idx_get(0, Expr::LocalGet(1)))),
        Stmt::Expr(Expr::Call {
            callee: Box::new(Expr::FuncRef(42)),
            args: vec![Expr::LocalGet(5)],
            type_args: vec![],
            byte_offset: 0,
        }),
    ];
    let got = run(&stmts, &params);
    assert!(
        !got.contains(&5),
        "call-arg-observed local wrongly admitted: {got:?}"
    );
}

#[test]
fn bare_index_observation_excludes_local() {
    // `let x = arr[off]; let y = arr[x];` — `x` used as a *bare* array index is
    // NOT a ToInt32-coercing context (`arr[undefined]` != `arr[0]`), so exclude.
    let params = [int32_array_param(0), number_param(1)];
    let stmts = vec![
        let_stmt(5, HirType::Any, Some(idx_get(0, Expr::LocalGet(1)))),
        let_stmt(6, HirType::Any, Some(idx_get(0, Expr::LocalGet(5)))),
    ];
    let got = run(&stmts, &params);
    assert!(
        !got.contains(&5),
        "bare-index-observed local wrongly admitted: {got:?}"
    );
}

#[test]
fn return_observation_excludes_local() {
    // `let x = arr[off]; return x;` — a bare return observes undefined-vs-int.
    let params = [int32_array_param(0), number_param(1)];
    let stmts = vec![
        let_stmt(5, HirType::Any, Some(idx_get(0, Expr::LocalGet(1)))),
        Stmt::Return(Some(Expr::LocalGet(5))),
    ];
    let got = run(&stmts, &params);
    assert!(
        !got.contains(&5),
        "returned local wrongly admitted: {got:?}"
    );
}

#[test]
fn returned_bitwise_result_keeps_local_eligible() {
    // `let x = arr[off]; x = x ^ 7; return (x & 0xff);` — `x` itself is only
    // read in bitwise ops; the *result* of a bitwise op (always defined) is
    // returned, so `x` stays eligible.
    let params = [int32_array_param(0), number_param(1)];
    let stmts = vec![
        let_stmt(5, HirType::Any, Some(idx_get(0, Expr::LocalGet(1)))),
        set(5, xor(Expr::LocalGet(5), Expr::Integer(7))),
        Stmt::Return(Some(bin(
            BinaryOp::BitAnd,
            Expr::LocalGet(5),
            Expr::Integer(0xff),
        ))),
    ];
    let got = run(&stmts, &params);
    assert!(
        got.contains(&5),
        "bitwise-only local wrongly excluded: {got:?}"
    );
}

#[test]
fn plain_array_store_value_excludes_local() {
    // Storing into a plain (non-typed) array does NOT coerce to ToInt32, so a
    // candidate used as such a store value is excluded. Here `arr2` is an
    // untyped local (no declared int-TA type), so `arr2[i] = x` is non-coercing.
    let params = [int32_array_param(0), number_param(1)];
    let stmts = vec![
        // arr2: untyped local (id 7), holds some object/array
        let_stmt(7, HirType::Any, Some(Expr::Integer(0))),
        let_stmt(5, HirType::Any, Some(idx_get(0, Expr::LocalGet(1)))),
        Stmt::Expr(Expr::IndexSet {
            object: Box::new(Expr::LocalGet(7)),
            index: Box::new(Expr::LocalGet(1)),
            value: Box::new(Expr::LocalGet(5)),
        }),
    ];
    let got = run(&stmts, &params);
    assert!(
        !got.contains(&5),
        "plain-array store value wrongly admitted: {got:?}"
    );
}

#[test]
fn update_target_excluded() {
    // `let x = arr[off]; x++;` — `++` is not modeled as a safe write.
    let params = [int32_array_param(0), number_param(1)];
    let stmts = vec![
        let_stmt(5, HirType::Any, Some(idx_get(0, Expr::LocalGet(1)))),
        Stmt::Expr(Expr::Update {
            id: 5,
            op: UpdateOp::Increment,
            prefix: false,
        }),
    ];
    let got = run(&stmts, &params);
    assert!(!got.contains(&5), "++ target wrongly admitted: {got:?}");
}

#[test]
fn non_int_kind_typed_array_read_not_seeded() {
    // Float64Array element reads are not int-kind; a local seeded only from one
    // is not a candidate (no int-TA-read write → never seeded).
    let mut binding_types = HashMap::new();
    binding_types.insert(0u32, HirType::Named("Float64Array".to_string()));
    let params = [number_param(1)];
    let stmts = vec![
        let_stmt(5, HirType::Any, Some(idx_get(0, Expr::LocalGet(1)))),
        set(5, xor(Expr::LocalGet(5), Expr::Integer(3))),
    ];
    let got = collect_int_valued_ta_locals(&stmts, &params, &binding_types, &HashMap::new());
    assert!(
        !got.contains(&5),
        "float64 read wrongly seeded a candidate: {got:?}"
    );
}

/// Masked read on a constant-length receiver: `S[x >>> 24]`-style, provably
/// in-bounds (window ⊂ [0, len)).
fn masked_read(arr_id: u32, src: Expr, shift: i64) -> Expr {
    idx_get(arr_id, bin(BinaryOp::UShr, src, Expr::Integer(shift)))
}

#[test]
fn wrap_i32_additive_chain_with_proven_operands_is_admitted() {
    // The `_encipher` `n` shape: straight-line additive chain whose TA-read
    // operands carry static windows within a KNOWN constant length.
    //   var n;              (Let n = undefined — hoisted var seed)
    //   n = S[l >>> 24];    (possibly-OOB whole-write: seed rule)
    //   n = n + S[l >>> 24];   (additive: operand window [0,255] < 1024)
    //   n = n ^ S[l >>> 24];
    //   l = l ^ n;          (n observed only in bitwise)
    let params = [int32_array_param(0), number_param(1)];
    let mut lens = HashMap::new();
    lens.insert(0u32, 1024i64);
    let stmts = vec![
        let_stmt(9, HirType::Any, Some(Expr::Undefined)),
        let_stmt(8, HirType::Any, Some(idx_get(0, Expr::LocalGet(1)))),
        set(9, masked_read(0, Expr::LocalGet(8), 24)),
        set(
            9,
            bin(
                BinaryOp::Add,
                Expr::LocalGet(9),
                masked_read(0, Expr::LocalGet(8), 24),
            ),
        ),
        set(
            9,
            xor(Expr::LocalGet(9), masked_read(0, Expr::LocalGet(8), 24)),
        ),
        set(8, xor(Expr::LocalGet(8), Expr::LocalGet(9))),
    ];
    let got = collect_int_valued_ta_locals(&stmts, &params, &HashMap::new(), &lens);
    assert!(
        got.contains(&9),
        "wrap-i32 additive accumulator wrongly excluded: {got:?}"
    );
    assert!(got.contains(&8), "feistel half wrongly excluded: {got:?}");
}

#[test]
fn wrap_i32_additive_inside_loop_is_rejected() {
    // The same additive write INSIDE a loop: unbounded accumulation makes the
    // true f64 value exceed 2^53 (rounding), diverging from the wrapped image.
    let params = [int32_array_param(0), number_param(1)];
    let mut lens = HashMap::new();
    lens.insert(0u32, 1024i64);
    let stmts = vec![
        let_stmt(9, HirType::Any, Some(idx_get(0, Expr::LocalGet(1)))),
        Stmt::While {
            condition: Expr::Bool(true),
            body: vec![set(
                9,
                bin(
                    BinaryOp::Add,
                    Expr::LocalGet(9),
                    masked_read(0, Expr::Integer(0x7fff_ffff), 24),
                ),
            )],
        },
    ];
    let got = collect_int_valued_ta_locals(&stmts, &params, &HashMap::new(), &lens);
    assert!(
        !got.contains(&9),
        "loop-carried additive chain wrongly admitted: {got:?}"
    );
}

#[test]
fn wrap_i32_candidate_as_bare_index_is_rejected() {
    // A wrapped image used as a BARE index could alias a different element
    // than the true out-of-range value — index positions stay disqualifying.
    let params = [int32_array_param(0), number_param(1)];
    let mut lens = HashMap::new();
    lens.insert(0u32, 1024i64);
    let stmts = vec![
        let_stmt(9, HirType::Any, Some(idx_get(0, Expr::LocalGet(1)))),
        set(
            9,
            bin(
                BinaryOp::Add,
                Expr::LocalGet(9),
                masked_read(0, Expr::LocalGet(1), 24),
            ),
        ),
        let_stmt(10, HirType::Any, Some(idx_get(0, Expr::LocalGet(9)))),
    ];
    let got = collect_int_valued_ta_locals(&stmts, &params, &HashMap::new(), &lens);
    assert!(
        !got.contains(&9),
        "bare-index wrap-i32 candidate wrongly admitted: {got:?}"
    );
}

#[test]
fn wrap_i32_additive_operand_with_possibly_undefined_value_is_rejected() {
    // The operand's LAST write before the additive step is a possibly-OOB
    // read (dynamic index): its true value can be `undefined`, and
    // `undefined + 1` is NaN (→ ToInt32 0) while the wrapped image would be
    // `0 + 1 == 1` — the flow leg must reject the additive write even though
    // the operand IS a pool member.
    let params = [int32_array_param(0), number_param(1)];
    let mut lens = HashMap::new();
    lens.insert(0u32, 1024i64);
    let stmts = vec![
        // let m = S[off];        (possibly OOB — seeds m, but NOT numberish)
        let_stmt(9, HirType::Any, Some(idx_get(0, Expr::LocalGet(1)))),
        // let n = m + S[0 >>> 24];  (additive with non-numberish operand m)
        let_stmt(
            10,
            HirType::Any,
            Some(bin(
                BinaryOp::Add,
                Expr::LocalGet(9),
                masked_read(0, Expr::Integer(0), 24),
            )),
        ),
        set(10, xor(Expr::LocalGet(10), Expr::Integer(1))),
        set(9, xor(Expr::LocalGet(9), Expr::Integer(1))),
    ];
    let got = collect_int_valued_ta_locals(&stmts, &params, &HashMap::new(), &lens);
    assert!(
        !got.contains(&10),
        "additive over possibly-undefined operand wrongly admitted: {got:?}"
    );
    // m is disqualified too: once n's additive write is inadmissible, n is no
    // longer a candidate, so m's read inside `m + S[...]` is an ordinary
    // NON-coercing observation (rule 2) — its true `undefined` would be
    // distinguishable there.
    assert!(
        !got.contains(&9),
        "operand read in a non-candidate additive position must disqualify: {got:?}"
    );
}

#[test]
fn wrap_i32_additive_operand_reset_in_bounds_is_admitted() {
    // Same shape, but the operand is RESET by an in-bounds-proven read before
    // the additive step (the enc_real `n = S[l >>> 24]; n += S[...]` flow) —
    // the flow leg must keep it.
    let params = [int32_array_param(0), number_param(1)];
    let mut lens = HashMap::new();
    lens.insert(0u32, 1024i64);
    let stmts = vec![
        let_stmt(9, HirType::Any, Some(Expr::Undefined)),
        // reset: in-bounds-proven (window [0,255] < 1024)
        set(9, masked_read(0, Expr::LocalGet(1), 24)),
        set(
            9,
            bin(
                BinaryOp::Add,
                Expr::LocalGet(9),
                masked_read(0, Expr::LocalGet(1), 24),
            ),
        ),
        set(9, xor(Expr::LocalGet(9), Expr::Integer(1))),
    ];
    let got = collect_int_valued_ta_locals(&stmts, &params, &HashMap::new(), &lens);
    assert!(
        got.contains(&9),
        "in-bounds-reset additive accumulator wrongly rejected: {got:?}"
    );
}

#[test]
fn wrap_i32_additive_without_length_proof_is_rejected() {
    // No known constant length for the receiver → the TA-read ADD operand
    // could be an OOB `undefined` (true value NaN-poisons the chain, wrapped
    // image would not) — inadmissible.
    let params = [int32_array_param(0), number_param(1)];
    let stmts = vec![
        let_stmt(9, HirType::Any, Some(idx_get(0, Expr::LocalGet(1)))),
        set(
            9,
            bin(
                BinaryOp::Add,
                Expr::LocalGet(9),
                masked_read(0, Expr::LocalGet(1), 24),
            ),
        ),
        set(9, xor(Expr::LocalGet(9), Expr::Integer(1))),
    ];
    let got = collect_int_valued_ta_locals(&stmts, &params, &HashMap::new(), &HashMap::new());
    assert!(
        !got.contains(&9),
        "length-unproven additive operand wrongly admitted: {got:?}"
    );
}
