//! `--opt-report` (#6952) end-to-end tests for the `Ptr<Shape>` collector.
//!
//! Split out of `ptr_shape.rs` to stay under the 2000-line CI gate; declared
//! there with `#[path]` so it remains a child module and can reach the
//! collector's private items through `use super::*`.

//! End-to-end tests for the `--opt-report` instrumentation (#6952).
//!
//! These run the real collector over hand-built HIR and assert BOTH
//! halves of the contract:
//!
//! 1. the report names the right value with the right rule, and
//! 2. **the collector's returned facts are unchanged** — the recording
//!    must be observational. Assertion (2) is what stops a future edit
//!    from "fixing" a report line by changing the proof.

use super::*;
use crate::opt_report::{test_support::Session, Outcome, Position};
use perry_hir::types::Type;
use perry_hir::{Class, ClassField, Expr, Stmt};

fn field(name: &str) -> ClassField {
    ClassField {
        name: name.to_string(),
        key_expr: None,
        ty: Type::Number,
        init: None,
        is_private: false,
        is_readonly: false,
        decorators: Vec::new(),
    }
}

fn class_with_fields(name: &str, fields: &[&str]) -> Class {
    Class {
        id: 0,
        name: name.to_string(),
        type_params: Vec::new(),
        extends: None,
        extends_name: None,
        native_extends: None,
        extends_expr: None,
        heritage_lexically_shadowed: false,
        fields: fields.iter().map(|f| field(f)).collect(),
        constructor: None,
        methods: Vec::new(),
        getters: Vec::new(),
        setters: Vec::new(),
        static_fields: Vec::new(),
        static_methods: Vec::new(),
        computed_members: Vec::new(),
        decorators: Vec::new(),
        is_exported: false,
        aliases: Vec::new(),
        is_nested: false,
        alloc_width_hint: 0,
        static_accessor_names: Vec::new(),
        static_accessor_fn_ids: Vec::new(),
    }
}

fn new_c() -> Expr {
    Expr::New {
        class_name: "C".to_string(),
        args: Vec::new(),
        type_args: Vec::new(),
        byte_offset: 0,
        cap_args_appended: 0,
    }
}

fn let_c(id: u32, name: &str) -> Stmt {
    Stmt::Let {
        id,
        name: name.to_string(),
        ty: Type::Any,
        mutable: false,
        init: Some(new_c()),
    }
}

/// `o.x = 1` — a declared-field store, which rule 2 permits.
fn store_x(id: u32) -> Stmt {
    Stmt::Expr(Expr::PropertySet {
        object: Box::new(Expr::LocalGet(id)),
        property: "x".to_string(),
        value: Box::new(Expr::Number(1.0)),
    })
}

/// A barrier-free fact set. `ModuleDispatchFacts::default()` is
/// deliberately fail-safe (every barrier ON), so using it here would make
/// every test in this module vacuously assert the rule-5 kill.
fn clean_dispatch() -> ModuleDispatchFacts {
    super::super::collect_module_dispatch_facts(&perry_hir::Module::new("clean"))
}

fn run(stmts: &[Stmt], classes: &HashMap<String, &Class>) -> HashMap<u32, PtrShapeLocal> {
    collect_shape_proven_ptr_locals(
        stmts,
        &HashSet::new(),
        &HashMap::new(),
        classes,
        &clean_dispatch(),
        &HashSet::new(),
        &crate::collectors::ptr_shape_elements::ElementShapeFacts::default(),
    )
}

/// A contained local is promoted AND reported as a win; an escaping one
/// is denied AND reported with rule 2 naming the return position.
///
/// #7034 §4 narrowed what "the return position" means: a BARE `return o` is
/// now exempt (`ptr_shape.rs` rule 2, and
/// `ptr_shape_returns_tests::returned_local_is_promoted`). The escaping local
/// here therefore returns the object through a CONDITIONAL, which is what the
/// rule still denies and still reports as a return escape.
#[test]
fn contained_local_wins_and_returned_local_is_denied_with_its_rule() {
    let c = class_with_fields("C", &["x"]);
    let mut classes = HashMap::new();
    classes.insert("C".to_string(), &c);

    let stmts = vec![
        let_c(1, "contained"),
        store_x(1),
        let_c(2, "escaped"),
        store_x(2),
        Stmt::Return(Some(Expr::Conditional {
            condition: Box::new(Expr::Bool(true)),
            then_expr: Box::new(Expr::LocalGet(2)),
            else_expr: Box::new(Expr::Undefined),
        })),
    ];

    let session = Session::start();
    let facts = run(&stmts, &classes);
    let entries = session.entries();

    // (2) The proof itself is unchanged by the instrumentation.
    assert!(
        facts.contains_key(&1),
        "the contained local must still be promoted"
    );
    assert!(
        !facts.contains_key(&2),
        "the returned local must still be denied"
    );

    // (1) And the report says so, with the rule.
    let win = entries
        .iter()
        .find(|e| e.name == "contained")
        .expect("the promoted local must appear in the report");
    assert_eq!(win.outcome, Outcome::Selected);
    assert_eq!(win.rep, "Ptr<Shape>");
    assert_eq!(win.position, Position::Local);

    let miss = entries
        .iter()
        .find(|e| e.name == "escaped")
        .expect("the denied local must appear in the report");
    assert_eq!(miss.outcome, Outcome::Denied);
    assert_eq!(miss.rep, "Boxed");
    assert_eq!(miss.rule.as_deref(), Some("rule 2 (containment)"));
    assert!(
        miss.reason.as_deref().unwrap_or("").contains("returned"),
        "the reason must name the RETURN escape, not a generic one: {:?}",
        miss.reason
    );
}

/// The escape kind must be discriminated, not collapsed into one bucket.
/// A closure capture and a call argument are different fixes.
#[test]
fn escape_kinds_are_discriminated() {
    let c = class_with_fields("C", &["x"]);
    let mut classes = HashMap::new();
    classes.insert("C".to_string(), &c);

    let captured = vec![
        let_c(1, "captured"),
        Stmt::Expr(Expr::Closure {
            func_id: 99,
            params: Vec::new(),
            return_type: Type::Any,
            body: vec![store_x(1)],
            captures: vec![1],
            mutable_captures: Vec::new(),
            captures_this: false,
            captures_new_target: false,
            enclosing_class: None,
            is_arrow: true,
            is_async: false,
            is_generator: false,
            is_strict: false,
        }),
    ];
    let session = Session::start();
    let facts = run(&captured, &classes);
    let entries = session.entries();
    assert!(!facts.contains_key(&1), "a captured local must be denied");
    let e = entries.iter().find(|e| e.name == "captured").unwrap();
    assert!(
        e.reason.as_deref().unwrap_or("").contains("captured"),
        "closure capture must be reported as such: {:?}",
        e.reason
    );
    drop(session);

    let passed = vec![
        let_c(2, "passed"),
        Stmt::Expr(Expr::Call {
            callee: Box::new(Expr::LocalGet(50)),
            args: vec![Expr::LocalGet(2)],
            type_args: Vec::new(),
            byte_offset: 0,
        }),
    ];
    let session = Session::start();
    let facts = run(&passed, &classes);
    let entries = session.entries();
    assert!(
        !facts.contains_key(&2),
        "a local passed as an argument must be denied"
    );
    let e = entries.iter().find(|e| e.name == "passed").unwrap();
    assert!(
        e.reason
            .as_deref()
            .unwrap_or("")
            .contains("passed as a call argument"),
        "a call-argument escape must be reported as such, not as a bare \
         reference: {:?}",
        e.reason
    );
}

/// Rule 5 kills the whole module. The report must still enumerate what
/// *would* have been considered — otherwise a barrier-carrying module
/// produces a blank report and looks like it has no object code at all.
#[test]
fn module_barrier_still_enumerates_the_candidates_it_killed() {
    let c = class_with_fields("C", &["x"]);
    let mut classes = HashMap::new();
    classes.insert("C".to_string(), &c);
    let stmts = vec![let_c(1, "victim"), store_x(1)];

    // A module with a §5.2 barrier: `delete o.x` anywhere.
    let mut barrier_module = perry_hir::Module::new("barrier");
    barrier_module.init = vec![Stmt::Expr(Expr::Delete(Box::new(Expr::PropertyGet {
        object: Box::new(Expr::LocalGet(7)),
        property: "x".to_string(),
        byte_offset: 0,
    })))];
    let dispatch = super::super::collect_module_dispatch_facts(&barrier_module);
    assert!(
        dispatch.has_shape_barrier_sites(),
        "fixture must actually trip the barrier, or this test is vacuous"
    );

    let session = Session::start();
    let facts = collect_shape_proven_ptr_locals(
        &stmts,
        &HashSet::new(),
        &HashMap::new(),
        &classes,
        &dispatch,
        &HashSet::new(),
        &crate::collectors::ptr_shape_elements::ElementShapeFacts::default(),
    );
    let entries = session.entries();

    assert!(facts.is_empty(), "the barrier must still kill promotion");
    let e = entries
        .iter()
        .find(|e| e.name == "victim")
        .expect("the killed candidate must still be named in the report");
    assert_eq!(e.rule.as_deref(), Some("rule 5 (module-wide barrier)"));
}

/// The `.map(x => ({...}))` idiom: an allocation never bound to a local.
/// Rule 1 can never see it, so it must be reported as an allocation site
/// rather than silently omitted.
#[test]
fn unbound_allocation_sites_are_reported() {
    let c = class_with_fields("C", &["x"]);
    let mut classes = HashMap::new();
    classes.insert("C".to_string(), &c);
    let stmts = vec![Stmt::Return(Some(new_c()))];

    let session = Session::start();
    let facts = run(&stmts, &classes);
    let entries = session.entries();

    assert!(facts.is_empty());
    let e = entries
        .iter()
        .find(|e| e.position == Position::AllocSite)
        .expect("an unbound `new` must be reported as an allocation site");
    assert_eq!(e.rule.as_deref(), Some("rule 1 (provenance)"));
    assert_eq!(e.name, "new C(...)");
    assert!(
        e.detail.as_deref().unwrap_or("").contains("return"),
        "the allocation position must be named: {:?}",
        e.detail
    );
}

/// Nothing is recorded when the report is off — the zero-cost claim.
#[test]
fn nothing_is_recorded_when_the_report_is_off() {
    let c = class_with_fields("C", &["x"]);
    let mut classes = HashMap::new();
    classes.insert("C".to_string(), &c);
    // A container return, not a bare one: #7034 §4 exempts `return o`, and
    // this test's subject is the report gate, not the proof.
    let stmts = vec![
        let_c(1, "escaped"),
        Stmt::Return(Some(Expr::Array(vec![Expr::LocalGet(1)]))),
    ];

    // Take the same lock the enabled sessions use — otherwise a
    // concurrently-running enabled test would make this one flaky, since
    // the gate and the sink are both process-global.
    let session = Session::start_disabled();
    assert!(!crate::opt_report::enabled());
    let facts = run(&stmts, &classes);
    assert!(facts.is_empty(), "the returned facts must be unchanged");
    assert!(
        session.entries().is_empty(),
        "the sink must stay empty when the report is off"
    );
}

// ── #7170 R0: what the alloc-site buckets MEAN ─────────────────────────────
//
// Three defects, measured on 197 dependency modules, each of which made a
// scheduler-facing number say something other than what it looked like. The
// tests below are grouped by defect and each one is written so that reverting
// its arm in the compiler takes it — and only it — red.

fn anon_shape(args: Vec<Expr>) -> Expr {
    Expr::New {
        class_name: "__AnonShape_1f2e".to_string(),
        args,
        type_args: Vec::new(),
        byte_offset: 0,
        cap_args_appended: 0,
    }
}

fn alloc_rows(entries: &[crate::opt_report::Entry]) -> Vec<&crate::opt_report::Entry> {
    entries
        .iter()
        .filter(|e| e.position == Position::AllocSite)
        .collect()
}

/// Defect 2 (#7170 §5.1). A closed-shape object literal lowers to
/// `new __AnonShape_N(v0, …)` whose constructor arguments ARE its property
/// values, so `{a: {b: 1}}` filed its inner literal under
/// `constructor argument` — the label for a genuine `new C(arg)`. On the
/// dependency corpus the two are 24.5% and 2.0% of sites: one bucket was 92%
/// the other thing.
#[test]
fn a_nested_object_literal_is_not_filed_as_a_constructor_argument() {
    let classes = HashMap::new();
    let stmts = vec![Stmt::Let {
        id: 1,
        name: "outer".to_string(),
        ty: Type::Any,
        mutable: false,
        init: Some(anon_shape(vec![anon_shape(Vec::new())])),
    }];

    let session = Session::start();
    let _ = run(&stmts, &classes);
    let entries = session.entries();

    let rows = alloc_rows(&entries);
    assert_eq!(rows.len(), 1, "the inner literal is the only unbound site");
    assert_eq!(
        rows[0].alloc_context.as_deref(),
        Some("object literal property value"),
        "a property value of an anonymous shape is not a constructor argument"
    );
}

/// The other direction: a genuine `new C(arg)` argument must KEEP the
/// `constructor argument` bucket. Without this the split is satisfiable by a
/// rule that simply renames everything.
#[test]
fn a_real_constructor_argument_keeps_its_own_bucket() {
    let c = class_with_fields("C", &["x"]);
    let boxc = class_with_fields("Box", &["v"]);
    let mut classes = HashMap::new();
    classes.insert("C".to_string(), &c);
    classes.insert("Box".to_string(), &boxc);
    let stmts = vec![Stmt::Let {
        id: 1,
        name: "b".to_string(),
        ty: Type::Any,
        mutable: false,
        init: Some(Expr::New {
            class_name: "Box".to_string(),
            args: vec![new_c()],
            type_args: Vec::new(),
            byte_offset: 0,
            cap_args_appended: 0,
        }),
    }];

    let session = Session::start();
    let _ = run(&stmts, &classes);
    let entries = session.entries();

    let rows = alloc_rows(&entries);
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].alloc_context.as_deref(),
        Some("constructor argument"),
        "`new Box(new C())` really is a constructor argument"
    );
}

/// Defect 3 (#7170 §5.2). `deny_alloc_site` fires unconditionally at the top
/// of the collector, before any seeding, so a `return new C(...)` in a
/// function that ALREADY carries a return-shape fact (#7107) was counted as a
/// rule-1 miss. It is not one: the class reaches every
/// `const r = producer(...)` call site.
#[test]
fn a_return_in_a_return_shape_producer_is_reported_as_served() {
    let c = class_with_fields("C", &["x"]);
    let mut classes = HashMap::new();
    classes.insert("C".to_string(), &c);
    let stmts = vec![Stmt::Return(Some(new_c()))];

    let session = Session::start();
    let _guard = crate::opt_report::enter_function_region("mk", true);
    let _ = run(&stmts, &classes);
    drop(_guard);
    let entries = session.entries();

    let rows = alloc_rows(&entries);
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].rule.as_deref(),
        Some("rule 1 (provenance) — already served by return-shape"),
        "a served return must leave the rule-1 bucket, not merely annotate it"
    );
    assert_eq!(rows[0].tier, Some(crate::opt_report::Tier::Served));
}

/// The same site in a function WITHOUT a return-shape fact stays an ordinary
/// rule-1 denial. This is the anti-vacuity half: a classifier that answers
/// "served" unconditionally passes the test above and fails this one.
#[test]
fn a_return_in_a_plain_function_is_still_a_rule_1_denial() {
    let c = class_with_fields("C", &["x"]);
    let mut classes = HashMap::new();
    classes.insert("C".to_string(), &c);
    let stmts = vec![Stmt::Return(Some(new_c()))];

    let session = Session::start();
    let _guard = crate::opt_report::enter_function_region("mk", false);
    let _ = run(&stmts, &classes);
    drop(_guard);
    let entries = session.entries();

    let rows = alloc_rows(&entries);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].rule.as_deref(), Some("rule 1 (provenance)"));
    assert_eq!(
        rows[0].tier,
        Some(crate::opt_report::Tier::CompilerLimitation)
    );
}

/// #7170 §6, as a test rather than a comment: **91.6% of dependency-JS
/// allocation sites are in closure regions, and none of them is served.**
/// `collect_return_shape_functions` issues facts only for `hir.functions`
/// entries and the caller-side seed only fires on a bare `Expr::FuncRef`
/// callee, which a closure call never is. A classifier that keyed servedness
/// on the syntax of the returns alone — rather than on the fact — would mark
/// this whole population served and delete the wall it is supposed to measure.
#[test]
fn a_closure_region_never_reports_a_served_return() {
    let c = class_with_fields("C", &["x"]);
    let mut classes = HashMap::new();
    classes.insert("C".to_string(), &c);
    let stmts = vec![Stmt::Return(Some(new_c()))];

    let session = Session::start();
    let _guard = crate::opt_report::enter_closure("mk", 7);
    let _ = run(&stmts, &classes);
    drop(_guard);
    let entries = session.entries();

    let rows = alloc_rows(&entries);
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].rule.as_deref(),
        Some("rule 1 (provenance)"),
        "a closure body cannot carry a return-shape fact"
    );
}

/// Only the RETURN position is served. `return f({a: 1})` puts the literal in
/// a call-argument position of a function that is not a producer at all, and
/// even inside a real producer a nested argument is not one of the returns.
#[test]
fn only_the_return_position_is_marked_served() {
    let c = class_with_fields("C", &["x"]);
    let mut classes = HashMap::new();
    classes.insert("C".to_string(), &c);
    // `return new C(new C())` — outer is the return, inner is an argument.
    let stmts = vec![Stmt::Return(Some(Expr::New {
        class_name: "C".to_string(),
        args: vec![new_c()],
        type_args: Vec::new(),
        byte_offset: 0,
        cap_args_appended: 0,
    }))];

    let session = Session::start();
    let _guard = crate::opt_report::enter_function_region("mk", true);
    let _ = run(&stmts, &classes);
    drop(_guard);
    let entries = session.entries();

    let rows = alloc_rows(&entries);
    assert_eq!(rows.len(), 2, "outer return plus inner argument");
    let served: Vec<_> = rows
        .iter()
        .filter(|e| e.tier == Some(crate::opt_report::Tier::Served))
        .collect();
    assert_eq!(served.len(), 1, "exactly the return position is served");
    assert_eq!(served[0].alloc_context.as_deref(), Some("return"));
}

/// Defect 1 (PR #7171 §3). Every anonymous literal renders as
/// `object literal { ... }` and carries `byte_offset: 0`, so
/// `(module, function, name, position, analysis, outcome, rule)` was the same
/// tuple for every unbound literal in a function and all but one vanished.
/// PR #7171 measured the consequence directly: suppressing 379 scaffolding
/// rows REVEALED 62 real user rows the dedup had been hiding.
#[test]
fn two_unbound_literals_in_one_function_are_two_rows() {
    let classes = HashMap::new();
    let stmts = vec![
        Stmt::Expr(anon_shape(Vec::new())),
        Stmt::Expr(anon_shape(Vec::new())),
    ];

    let session = Session::start();
    let _ = run(&stmts, &classes);
    let entries = session.entries();

    let rows = alloc_rows(&entries);
    assert_eq!(
        rows.len(),
        2,
        "two syntactic allocations are two rows, not one: {rows:#?}"
    );
    let mut ordinals: Vec<_> = rows.iter().filter_map(|e| e.alloc_ordinal).collect();
    ordinals.sort_unstable();
    assert_eq!(ordinals, vec![0, 1]);
}

/// ...and the de-duplication that motivated the key still works: a function
/// lowered twice (a boxed entry plus a typed clone) walks the same body and
/// produces the same ordinals, so its rows still collapse — and the count of
/// what collapsed is reported rather than left implicit.
#[test]
fn a_region_lowered_twice_still_collapses_and_says_how_much() {
    let classes = HashMap::new();
    let stmts = vec![
        Stmt::Expr(anon_shape(Vec::new())),
        Stmt::Expr(anon_shape(Vec::new())),
    ];

    let session = Session::start();
    let _guard = crate::opt_report::enter_function_region("twice", false);
    let _ = run(&stmts, &classes);
    let _ = run(&stmts, &classes);
    drop(_guard);
    let entries = session.entries();

    assert_eq!(
        alloc_rows(&entries).len(),
        2,
        "the second lowering collapses"
    );
    assert_eq!(
        crate::opt_report::masked_by_dedup(),
        2,
        "and the report says how many rows it folded away"
    );
}
