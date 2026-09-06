//! #9846: the segment-view for-of matcher, pinned against the HIR shape that
//! `--trace hir` actually produces for
//! `for (let {segment: O} of X.segment(q))`.
//!
//! The shape below was transcribed from a real `perry compile --trace hir`
//! dump, not invented — the campaign's rule about never hand-typing a constant
//! into emitted code applies to hand-typing an IR shape into a test just as
//! much ([[perry-emitted-constant-transcription]]). If the lowering moves,
//! `head_not_canonical` is what these tests report, which is the honest
//! failure and the one the real-bundle counter would also report.
//!
//! The distinction each test exists to pin: **a use of the segment STRING is
//! never a rejection** (it costs one materialisation, which is what the loop
//! pays today); only a use of the segment RECORD is.

use super::segview::{collect_segment_for_of_sites, SegViewVerdict};
use perry_hir::types::Type;
use perry_hir::{CatchClause, Expr, Stmt, UnaryOp};

const ITER: u32 = 5;
const RESULT: u32 = 7;
const RECORD: u32 = 11;
const SEG: u32 = 9;

fn pget(obj: Expr, prop: &str) -> Expr {
    Expr::PropertyGet {
        object: Box::new(obj),
        property: prop.to_string(),
        byte_offset: 0,
    }
}

fn call(callee: Expr, args: Vec<Expr>) -> Expr {
    Expr::Call {
        callee: Box::new(callee),
        args,
        type_args: vec![],
        byte_offset: 0,
    }
}

fn for_of_next() -> Expr {
    call(
        Expr::ExternFuncRef {
            name: "js_for_of_next".to_string(),
            param_types: vec![Type::Any],
            return_type: Type::Any,
        },
        vec![Expr::LocalGet(ITER)],
    )
}

fn let_(id: u32, name: &str, init: Expr) -> Stmt {
    Stmt::Let {
        id,
        name: name.to_string(),
        ty: Type::Any,
        mutable: false,
        init: Some(init),
    }
}

/// `let __arr_5 = GetIterator(rR_.segment(q))`.
fn iter_let() -> Stmt {
    let_(
        ITER,
        "__arr_5",
        Expr::GetIterator(Box::new(call(
            pget(Expr::LocalGet(0), "segment"),
            vec![Expr::LocalGet(3)],
        ))),
    )
}

/// The `For` with the canonical `js_for_of_next` head and the given body
/// after the destructuring lets.
fn for_stmt(destructure: Vec<Stmt>, body: Vec<Stmt>) -> Stmt {
    let mut all = destructure;
    // The real lowering wraps the user body in the iterator-close protocol.
    all.push(Stmt::Try {
        body,
        catch: Some(CatchClause {
            param: Some((12, "__forof_err_12".to_string())),
            body: vec![Stmt::Throw(Expr::LocalGet(12))],
        }),
        finally: None,
    });
    Stmt::For {
        init: Some(Box::new(let_(RESULT, "__result_7", for_of_next()))),
        condition: Some(Expr::Unary {
            op: UnaryOp::Not,
            operand: Box::new(pget(Expr::LocalGet(RESULT), "done")),
        }),
        update: Some(Expr::LocalSet(RESULT, Box::new(for_of_next()))),
        body: all,
    }
}

/// `let __destruct_11 = __result_7.value; let O = __destruct_11.segment;`
fn destructure_segment_only() -> Vec<Stmt> {
    vec![
        let_(
            RECORD,
            "__destruct_11",
            pget(Expr::LocalGet(RESULT), "value"),
        ),
        let_(SEG, "O", pget(Expr::LocalGet(RECORD), "segment")),
    ]
}

fn region(body: Vec<Stmt>) -> Vec<Stmt> {
    vec![iter_let(), for_stmt(destructure_segment_only(), body)]
}

/// cc's body, in shape: one `codePointAt` and one regex test on the segment.
fn cc_body() -> Vec<Stmt> {
    vec![
        let_(
            10,
            "w",
            call(
                pget(Expr::LocalGet(SEG), "codePointAt"),
                vec![Expr::Integer(0)],
            ),
        ),
        Stmt::If {
            condition: Expr::RegExpTest {
                regex: Box::new(Expr::LocalGet(1)),
                string: Box::new(Expr::LocalGet(SEG)),
            },
            then_branch: vec![Stmt::Continue],
            else_branch: None,
        },
    ]
}

#[test]
fn fires_on_the_cc_shape_and_names_the_two_view_entry_points() {
    let sites = collect_segment_for_of_sites(&region(cc_body()));
    assert_eq!(sites.len(), 1, "exactly one segment for-of site");
    let s = &sites[0];
    assert_eq!(
        s.verdict,
        SegViewVerdict::Fires,
        "the record's only uses are the head's own field reads"
    );
    assert!(
        s.two_arg_open,
        "the subject is the `X.segment(q)` call itself"
    );
    assert_eq!(s.record_keys, vec!["segment".to_string()]);
    assert_eq!(s.segment_id, Some(SEG));
    assert_eq!(s.segment_uses.code_point_at, 1);
    assert_eq!(s.segment_uses.regexp_test_static, 1);
    assert_eq!(
        s.segment_uses.materialise, 0,
        "every use of the segment string is view-answerable, so this site is v2-ready"
    );
    assert!(s.segment_uses.view_answerable_v2());
    assert!(
        !s.segment_uses.view_answerable_v1(),
        "the regex test is a v2 entry point; v1 must still materialise here"
    );
}

/// The distinction the whole design rests on: an unclassifiable use of the
/// segment STRING costs a materialisation, it does not reject the site.
#[test]
fn a_use_of_the_segment_string_is_a_materialisation_not_a_rejection() {
    let mut body = cc_body();
    body.push(Stmt::Expr(call(
        Expr::LocalGet(42),
        vec![Expr::LocalGet(SEG)],
    )));
    let sites = collect_segment_for_of_sites(&region(body));
    assert_eq!(sites[0].verdict, SegViewVerdict::Fires);
    assert_eq!(sites[0].segment_uses.materialise, 1);
    assert!(!sites[0].segment_uses.view_answerable_v2());
}

/// `recv.test(O)` with an opaque receiver — cc's `g54.default().test(O)` — is
/// classified apart from the statically-proven `RegExpTest`, because the
/// runtime declines it three-valued (§5 of the interface).
#[test]
fn an_opaque_test_receiver_is_counted_separately() {
    let body = vec![Stmt::Expr(call(
        pget(call(pget(Expr::LocalGet(54), "default"), vec![]), "test"),
        vec![Expr::LocalGet(SEG)],
    ))];
    let sites = collect_segment_for_of_sites(&region(body));
    assert_eq!(sites[0].segment_uses.regexp_test_dynamic, 1);
    assert_eq!(sites[0].segment_uses.materialise, 0);
}

/// A use of the RECORD outside the head is the one thing that rejects, and it
/// must be caught even when it hides inside a closure the walker only reaches
/// through `Expr::Closure`.
#[test]
fn a_record_use_inside_a_closure_rejects() {
    let mut body = cc_body();
    body.push(Stmt::Expr(Expr::Closure {
        func_id: 1,
        params: vec![],
        return_type: Type::Any,
        body: vec![Stmt::Return(Some(Expr::LocalGet(RECORD)))],
        captures: vec![RECORD],
        mutable_captures: vec![],
        captures_this: false,
        captures_new_target: false,
        enclosing_class: None,
        is_arrow: true,
        is_async: false,
        is_generator: false,
        is_strict: false,
    }));
    let sites = collect_segment_for_of_sites(&region(body));
    assert!(
        matches!(
            sites[0].verdict,
            SegViewVerdict::RecordEscapes {
                uses: 2,
                destructure_reads: 1
            }
        ),
        "expected record_escapes, got {:?}",
        sites[0].verdict
    );
}

/// `{segment: O, index: I}` does not escape the record, but `index` is a v2
/// symbol — the site must decline with its own reason rather than fire.
#[test]
fn a_second_destructured_field_declines_with_its_own_reason() {
    let destructure = vec![
        let_(
            RECORD,
            "__destruct_11",
            pget(Expr::LocalGet(RESULT), "value"),
        ),
        let_(SEG, "O", pget(Expr::LocalGet(RECORD), "segment")),
        let_(8, "I", pget(Expr::LocalGet(RECORD), "index")),
    ];
    let stmts = vec![iter_let(), for_stmt(destructure, cc_body())];
    let sites = collect_segment_for_of_sites(&stmts);
    match &sites[0].verdict {
        SegViewVerdict::RecordFieldsBeyondV1 { keys } => {
            assert_eq!(keys, &vec!["segment".to_string(), "index".to_string()])
        }
        other => panic!("expected record_fields_beyond_v1, got {other:?}"),
    }
}

/// A `for…of` lowered any other way (a collection view, an index arm) is not
/// this tier's shape and must say so rather than silently matching.
#[test]
fn a_non_canonical_head_declines_by_name() {
    let mut stmts = region(cc_body());
    if let Stmt::For { update, .. } = &mut stmts[1] {
        *update = Some(Expr::LocalSet(RESULT, Box::new(Expr::Undefined)));
    } else {
        panic!("shape");
    }
    let sites = collect_segment_for_of_sites(&stmts);
    assert_eq!(sites[0].verdict, SegViewVerdict::HeadNotCanonical);
}

/// A `for…of` over anything but an `X.segment(q)` call is not examined at
/// all — the denominator, not a rejection.
#[test]
fn an_unrelated_for_of_is_not_a_candidate() {
    let stmts = vec![
        let_(
            ITER,
            "__arr_5",
            Expr::GetIterator(Box::new(Expr::LocalGet(99))),
        ),
        for_stmt(destructure_segment_only(), cc_body()),
    ];
    assert!(collect_segment_for_of_sites(&stmts).is_empty());
}
