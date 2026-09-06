//! #9843: the segment-view for-of matcher, pinned against the HIR shape that
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

/// The bundle regression. `O.codePointAt(0)` survives as a generic
/// `Call(PropertyGet…)` when the receiver's type is unknown — which is what a
/// TypeScript probe produces — but perry's JS pipeline folds it into
/// `Expr::StringCodePointAt`. The first version of the classifier matched only
/// the former, so on `cli_2.1.112.js` the loop that is 60-85 % of claude-code's
/// CPU reported `code_point_at=0, materialise=1`.
///
/// The reconciliation against the sound counter is why that read as a blind
/// spot rather than as a correct answer, and this test is why it cannot come
/// back.
#[test]
fn the_folded_code_point_at_node_is_classified_like_the_generic_call() {
    let body = vec![let_(
        10,
        "w",
        Expr::StringCodePointAt {
            string: Box::new(Expr::LocalGet(SEG)),
            index: Box::new(Expr::Integer(0)),
        },
    )];
    let sites = collect_segment_for_of_sites(&region(body));
    assert_eq!(sites[0].verdict, SegViewVerdict::Fires);
    assert_eq!(
        sites[0].segment_uses.code_point_at, 1,
        "the folded node must count as a code_point_at use, not a materialisation"
    );
    assert_eq!(
        sites[0].segment_uses.materialise, 0,
        "nothing is left over for the sound counter to book conservatively"
    );
    assert!(
        sites[0].segment_uses.view_answerable_v1(),
        "a loop whose only use is the folded codePointAt is answerable by v1 alone"
    );
}

// ── the lowering ───────────────────────────────────────────────────────────

use super::segview::segview_rewrite_module;

fn module_with(stmts: Vec<Stmt>) -> perry_hir::Module {
    let mut m = perry_hir::Module::new("t");
    m.init = stmts;
    m
}

fn render(m: &perry_hir::Module) -> String {
    format!("{:?}", m.init)
}

/// The shape the lowering must emit, pinned on the parts that carry meaning.
#[test]
fn the_rewrite_elides_the_record_and_keeps_a_spec_path() {
    let mut m = module_with(region(cc_body()));
    assert_eq!(segview_rewrite_module(&mut m), 1);
    let out = render(&m);

    assert!(
        out.contains("js_segments_view_open"),
        "the two-argument open must be emitted: {out}"
    );
    assert!(
        out.contains("js_segments_view_next"),
        "the in-loop advance must be emitted"
    );
    assert!(
        out.contains("js_segments_view_segment"),
        "v1 materialises the segment once per step"
    );
    assert!(
        out.contains("js_for_of_next"),
        "the spec path must survive for the decline case"
    );
    assert!(
        !out.contains("__destruct_"),
        "the record binding is what this removes; it must be gone: {out}"
    );
}

/// The receiver and the input appear on BOTH arms, so they must be evaluated
/// once and read from locals — not re-evaluated in the decline arm. A receiver
/// with a side effect would otherwise run twice.
#[test]
fn the_receiver_and_input_are_hoisted_exactly_once() {
    let mut m = module_with(region(cc_body()));
    assert_eq!(segview_rewrite_module(&mut m), 1);
    let out = render(&m);
    assert!(out.contains("__segview_recv"), "receiver hoisted");
    assert!(out.contains("__segview_input"), "input hoisted");
    // `LocalGet(0)` was the receiver and `LocalGet(3)` the input in `region`.
    // After the rewrite each must appear exactly ONCE — in its hoist.
    assert_eq!(
        out.matches("LocalGet(0)").count(),
        1,
        "the receiver is evaluated once, not on both arms: {out}"
    );
    assert_eq!(
        out.matches("LocalGet(3)").count(),
        1,
        "the input is evaluated once, not on both arms: {out}"
    );
}

/// The `.segment` property get must stay inside the decline arm, so a receiver
/// whose `segment` is an accessor runs it exactly once, in its original
/// position, and never on the accepted path.
#[test]
fn the_segment_property_get_stays_on_the_decline_arm_only() {
    let mut m = module_with(region(cc_body()));
    segview_rewrite_module(&mut m);
    let out = render(&m);
    assert_eq!(
        out.matches("property: \"segment\"").count(),
        2,
        "exactly two: the decline arm's `recv.segment(inp)` and the decline \
         arm's `R.value.segment` — never on the accepted path: {out}"
    );
}

/// A site that does not fire must be left byte-identical.
#[test]
fn a_declining_site_is_not_rewritten() {
    let destructure = vec![
        let_(
            RECORD,
            "__destruct_11",
            pget(Expr::LocalGet(RESULT), "value"),
        ),
        let_(SEG, "O", pget(Expr::LocalGet(RECORD), "segment")),
        let_(8, "I", pget(Expr::LocalGet(RECORD), "index")),
    ];
    let mut m = module_with(vec![iter_let(), for_stmt(destructure, cc_body())]);
    let before = render(&m);
    assert_eq!(segview_rewrite_module(&mut m), 0);
    assert_eq!(before, render(&m), "a declining site must be untouched");
}

/// v2: on a site where every use of the segment is view-answerable, nothing is
/// materialised on the accepted path. `N$6` is exactly this shape — one
/// `codePointAt` and two opaque-receiver `.test()` calls.
#[test]
fn v2_answers_every_use_from_the_view_and_materialises_nothing() {
    let mut m = module_with(region(cc_body()));
    assert_eq!(segview_rewrite_module(&mut m), 1);
    let out = render(&m);

    assert!(
        out.contains("js_segments_view_code_point_at"),
        "the codePointAt use must be answered from the cursor: {out}"
    );
    assert!(
        out.contains("js_segments_view_regexp_test"),
        "the regex test must be answered from the cursor"
    );
    assert!(
        out.contains("__segview_test_recv"),
        "the opaque receiver must be hoisted so it is evaluated exactly once"
    );

    // The decisive property, checked on the tree rather than on its Debug
    // rendering: the segment binding's ACCEPTED arm must be `Undefined`. If it
    // were `_segment(cursor)` the loop would still allocate a substring per
    // grapheme and v2 would buy nothing — and a string-contains assertion
    // would not have caught it, because `_segment` legitimately appears inside
    // the regexp_test decline arm.
    let seg_bind_accept_is_undefined = m.init.iter().any(|s| match s {
        Stmt::For { body, .. } => matches!(
            body.first(),
            Some(Stmt::Let {
                init: Some(Expr::Conditional { then_expr, .. }),
                ..
            }) if matches!(then_expr.as_ref(), Expr::Undefined)
        ),
        _ => false,
    });
    assert!(
        seg_bind_accept_is_undefined,
        "the segment must NOT be materialised on the accepted path: {out}"
    );
}

/// The receiver of `.test(O)` is `g54.default()` in cc — an opaque call that
/// must run exactly once per evaluation. It is bound to a temporary and the
/// fallback arm reuses the temporary rather than re-evaluating it.
#[test]
fn v2_evaluates_an_opaque_test_receiver_exactly_once() {
    let body = vec![Stmt::Expr(call(
        pget(call(pget(Expr::LocalGet(54), "default"), vec![]), "test"),
        vec![Expr::LocalGet(SEG)],
    ))];
    let mut m = module_with(region(body));
    assert_eq!(segview_rewrite_module(&mut m), 1);
    let out = render(&m);
    assert_eq!(
        out.matches("property: \"default\"").count(),
        2,
        "once in the view arm's hoist and once in the decline arm's original — \
         never twice within one arm: {out}"
    );
}

/// A site with a use the classifier cannot answer keeps v1: materialise once
/// and leave the body alone. Paying the per-use guards on top of a
/// materialisation that happens anyway would be strictly worse.
#[test]
fn a_site_with_an_unanswerable_use_stays_on_v1() {
    let mut body = cc_body();
    body.push(Stmt::Expr(call(
        Expr::LocalGet(42),
        vec![Expr::LocalGet(SEG)],
    )));
    let mut m = module_with(region(body));
    assert_eq!(segview_rewrite_module(&mut m), 1);
    let out = render(&m);
    assert!(
        out.contains("js_segments_view_segment"),
        "v1 binds the segment by materialising it once"
    );
    assert!(
        !out.contains("js_segments_view_code_point_at"),
        "v1 does not rewrite uses: {out}"
    );
}
