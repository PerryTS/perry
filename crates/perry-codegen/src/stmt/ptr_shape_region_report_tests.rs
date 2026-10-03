//! The P8 handoff consumes class provenance only when a real static-region
//! access uses it. A learned region must never be counted as that consumption.

use super::*;
use crate::opt_report::{test_support::Session, Analysis, Outcome};

fn fixture() -> Module {
    let mut m = method_calls_module(Expr::Integer(200), Vec::new(), false);
    // A loop-local receiver forces the body-region handoff that stole the
    // original census fixtures. The self-reading store prevents scalarization.
    let mut receiver = m.init.remove(0);
    if let Stmt::Let {
        init: Some(Expr::New { args, .. }),
        ..
    } = &mut receiver
    {
        *args = vec![Expr::Integer(0)];
    }
    let Stmt::For { body, .. } = &mut m.init[0] else {
        panic!("fixture loop")
    };
    body.insert(0, receiver);
    // A compound update alongside the put-value store: the admitted region
    // owns its receivers' STORES, so the generic copy's update of this
    // receiver is where the region authority refuses the unguarded route.
    body.push(Stmt::Expr(Expr::PropertyUpdate {
        object: Box::new(Expr::LocalGet(1)),
        property: "value".into(),
        op: BinaryOp::Add,
        prefix: false,
        strict: false,
    }));
    m
}

fn static_options(m: &Module) -> CompileOptions {
    let births = crate::module_birth_shapes(m, ir_opts()).unwrap();
    let mut opts = ir_opts();
    opts.static_shape_ids = crate::assign_static_shape_ids(births.iter().map(|b| &b.shape))
        .into_iter()
        .collect();
    assert!(
        !opts.static_shape_ids.is_empty(),
        "fixture needs a static supplier"
    );
    opts
}

#[test]
fn selected_class_provenance_is_consumed_by_real_shape_guarded_region_accesses() {
    let m = fixture();
    let opts = static_options(&m);
    let session = Session::start();
    let ir = String::from_utf8(compile_module(&m, opts).unwrap()).unwrap();
    let entries = session.entries();
    assert!(
        entries.iter().any(|e| e.analysis == Analysis::PtrShape
            && e.outcome == Outcome::Selected
            && e.local_id == Some(1)),
        "fixture must actually select its receiver: {entries:?}"
    );
    assert!(
        ir.contains("rloop.guard.static") && ir.contains("rloop.fast"),
        "a reported access must retain the live shape guard: {ir}"
    );
    for site in ["ptr_shape_region_get", "ptr_shape_region_set"] {
        assert!(
            entries.iter().any(|e| e.analysis == Analysis::PtrShape
                && e.outcome == Outcome::Consumed
                && e.local_id == Some(1)
                && e.site.as_deref() == Some(site)),
            "missing {site}: {entries:?}"
        );
    }
    assert!(
        entries.iter().any(|e| e.outcome == Outcome::Unconsumed
            && e.rule.as_deref() == Some(crate::expr::PTR_SHAPE_REGION_AUTHORITY)),
        "the generic copy's unguarded-route refusal must be named: {entries:?}"
    );
}

#[test]
fn removing_static_supplier_does_not_claim_ptr_shape_consumption_for_learned_accesses() {
    let session = Session::start();
    // Sabotage precisely the class-fact consumer's prerequisite, leaving the
    // same selected receiver and real region accesses alive.
    let ir = emit(&fixture());
    let entries = session.entries();
    assert!(entries.iter().any(|e| e.analysis == Analysis::PtrShape
        && e.outcome == Outcome::Selected
        && e.local_id == Some(1)));
    assert!(ir.contains("rloop.fast") && ir.contains("@js_region_loop_prime("));
    assert!(
        !entries.iter().any(|e| e
            .site
            .as_deref()
            .is_some_and(|s| s == "ptr_shape_region_get" || s == "ptr_shape_region_set")),
        "learned words do not consume static class provenance: {entries:?}"
    );
    assert!(entries.iter().any(|e| e.outcome == Outcome::Unconsumed
        && e.rule.as_deref() == Some(crate::expr::PTR_SHAPE_REGION_AUTHORITY)));
}

#[test]
fn shape_barrier_removes_proof_attribution_but_keeps_guarded_type_hint_route() {
    let mut m = fixture();
    // A separate object's delete trips rule 5 without disturbing this loop's
    // shape supplier. The same guard/bare instructions must not acquire a
    // Ptr<Shape> consumption row when no such proof was selected.
    m.init
        .push(Stmt::Expr(Expr::Delete(Box::new(Expr::PropertyGet {
            object: Box::new(Expr::New {
                class_name: "Counter".into(),
                args: vec![Expr::Integer(0)],
                type_args: Vec::new(),
                byte_offset: 0,
                cap_args_appended: 0,
            }),
            property: "value".into(),
            byte_offset: 0,
        }))));
    let opts = static_options(&m);
    let session = Session::start();
    let ir = String::from_utf8(compile_module(&m, opts).unwrap()).unwrap();
    let entries = session.entries();
    assert!(ir.contains("rloop.guard.static") && ir.contains("rloop.fast"));
    assert!(!entries.iter().any(|e| e.analysis == Analysis::PtrShape
        && e.outcome == Outcome::Selected
        && e.local_id == Some(1)));
    assert!(
        !entries.iter().any(|e| e
            .site
            .as_deref()
            .is_some_and(|s| s == "ptr_shape_region_get" || s == "ptr_shape_region_set")),
        "a type hint is not a consumed containment proof: {entries:?}"
    );
}

#[test]
fn region_consumption_reporting_off_emits_identical_ir_and_no_entries() {
    let m = fixture();
    let opts = static_options(&m);
    let enabled = {
        let session = Session::start();
        let ir = compile_module(&m, opts.clone()).unwrap();
        assert!(session
            .entries()
            .iter()
            .any(|e| e.site.as_deref() == Some("ptr_shape_region_get")));
        ir
    };
    let session = Session::start_disabled();
    let disabled = compile_module(&m, opts).unwrap();
    assert!(session.entries().is_empty());
    assert_eq!(
        enabled, disabled,
        "reporting must not change the emitted program"
    );
}

#[test]
fn straight_line_numeric_load_and_update_remain_live() {
    let mut m = method_calls_module(Expr::Integer(200), Vec::new(), false);
    m.init.pop();
    if let Stmt::Let {
        init: Some(Expr::New { args, .. }),
        ..
    } = &mut m.init[0]
    {
        *args = vec![Expr::Integer(0)];
    }
    m.init.push(bump_stmt(1, false));
    m.init.push(Stmt::Expr(Expr::PropertyUpdate {
        object: Box::new(Expr::LocalGet(1)),
        property: "value".into(),
        op: BinaryOp::Add,
        prefix: false,
        strict: false,
    }));
    m.init.push(Stmt::Expr(Expr::Binary {
        op: BinaryOp::Mul,
        left: Box::new(Expr::PropertyGet {
            object: Box::new(Expr::LocalGet(1)),
            property: "value".into(),
            byte_offset: 0,
        }),
        right: Box::new(Expr::Integer(2)),
    }));
    let session = Session::start();
    let ir = emit(&m);
    let entries = session.entries();
    assert!(!ir.contains("rloop.fast"));
    for site in [
        "class_field_get_number.shape_proven_load",
        "ptr_shape_update",
    ] {
        assert!(
            entries.iter().any(|e| e.outcome == Outcome::Consumed
                && e.local_id == Some(1)
                && e.site.as_deref() == Some(site)),
            "surviving site {site} must have independent coverage: {entries:?}"
        );
    }
    assert!(!entries
        .iter()
        .any(|e| e.rule.as_deref() == Some(crate::expr::PTR_SHAPE_REGION_AUTHORITY)));
}
