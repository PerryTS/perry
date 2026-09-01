//! Tests for the receiver-region model and its active descriptor table (#9254).
//!
//! Two jobs. The first half pins the boundary algebra — in particular the
//! unwind rule, which no fact table expresses today and which is therefore the
//! part with no shipping implementation to check against. The active-table
//! tests cover phase 2's first consumer. The second half is the equivalence
//! lint: the model held against `loop_purity::loop_may_allocate`, a shipping
//! audited predicate, on a shared battery.
//!
//! The battery is the load-bearing artifact. Three unsoundnesses in the first
//! draft of the model survived review and died here.

use super::receiver_regions::*;
use perry_hir::types::Type;
use perry_hir::{BinaryOp, CatchClause, CompareOp, Expr, Stmt, UnaryOp, UpdateOp};

// ---------------------------------------------------------------------------
// Shared fixtures. `stub_inert` mirrors `loop_purity`'s own test stub exactly,
// so a battery entry means the same thing to both predicates.
// ---------------------------------------------------------------------------

/// A local the real `expr_is_inert_primitive` would prove a non-pointer
/// primitive.
const NUM: u32 = 1;
const NUM2: u32 = 2;
/// A local it would refuse: `any`-typed / shadow-slotted / a module global —
/// one that can hold an object with a user-defined `valueOf`.
const OBJ: u32 = 9;

fn stub_inert(e: &Expr) -> bool {
    match e {
        Expr::Undefined | Expr::Null | Expr::Bool(_) | Expr::Number(_) | Expr::Integer(_) => true,
        Expr::LocalGet(id) | Expr::Update { id, .. } => *id == NUM || *id == NUM2,
        Expr::Unary { operand, .. } => stub_inert(operand),
        Expr::Compare { left, right, .. } | Expr::Binary { left, right, .. } => {
            stub_inert(left) && stub_inert(right)
        }
        _ => false,
    }
}

fn enders(body: &[Stmt]) -> Vec<RegionEnder> {
    region_enders_in_stmts(body, &[], &stub_inert)
}

fn has(body: &[Stmt], e: RegionEnder) -> bool {
    enders(body).contains(&e)
}

fn num(id: u32) -> Expr {
    Expr::LocalGet(id)
}

fn call(args: Vec<Expr>) -> Expr {
    Expr::Call {
        callee: Box::new(Expr::Undefined),
        args,
        type_args: vec![],
        byte_offset: 0,
    }
}

fn add(left: Expr, right: Expr) -> Expr {
    Expr::Binary {
        op: BinaryOp::Add,
        left: Box::new(left),
        right: Box::new(right),
    }
}

fn lt(left: Expr, right: Expr) -> Expr {
    Expr::Compare {
        op: CompareOp::Lt,
        left: Box::new(left),
        right: Box::new(right),
    }
}

fn closure(body: Vec<Stmt>) -> Expr {
    Expr::Closure {
        func_id: 0,
        params: vec![],
        return_type: Type::Any,
        body,
        captures: vec![],
        mutable_captures: vec![],
        captures_this: false,
        captures_new_target: false,
        enclosing_class: None,
        is_arrow: false,
        is_async: false,
        is_generator: false,
        is_strict: false,
    }
}

/// A descriptor in the shape of one of the real tables.
fn desc(
    table: &'static str,
    claim: ReceiverClaim,
    boundary: FactBoundary,
    excludes_try: bool,
) -> ReceiverDescriptor {
    ReceiverDescriptor {
        table,
        receiver: OBJ,
        claim,
        boundary,
        excludes_try,
    }
}

/// `cached_lengths` / `bounded_index_pairs`: a value, not an address.
fn cached_length_desc() -> ReceiverDescriptor {
    desc(
        "cached_lengths",
        ReceiverClaim::ScalarRelation,
        FactBoundary::DynamicExtent,
        false,
    )
}

/// `receiver_descriptors`: an address, reloaded at the poll, inside a region
/// the packed matcher keeps free of `Stmt::Try`.
fn poll_refreshed_receiver_desc() -> ReceiverDescriptor {
    desc(
        "receiver_descriptors",
        ReceiverClaim::Address,
        FactBoundary::PollRefresh,
        true,
    )
}

/// `buffer_view_slots`: an address, function-lifetime, degraded in place, and
/// with no region that excludes `Stmt::Try`.
fn buffer_view_desc() -> ReceiverDescriptor {
    desc(
        "buffer_view_slots",
        ReceiverClaim::Address,
        FactBoundary::InPlaceDegradation,
        false,
    )
}

/// `packed_f64_loop_facts`: a representation claim, scope-id bounded, inside a
/// matcher that rejects `Stmt::Try`.
fn packed_f64_desc() -> ReceiverDescriptor {
    desc(
        "packed_f64_loop_facts",
        ReceiverClaim::Representation,
        FactBoundary::ScopeId,
        true,
    )
}

const ALL_ENDERS: [RegionEnder; 6] = [
    RegionEnder::CollectingCall,
    RegionEnder::AllocatingOperation,
    RegionEnder::Coercion,
    RegionEnder::Suspension,
    RegionEnder::UnwindEdge,
    RegionEnder::BackEdgePoll,
];

// ---------------------------------------------------------------------------
// The boundary algebra.
// ---------------------------------------------------------------------------

/// A moving collection changes an object's ADDRESS, never its length. This is
/// the reason `cached_lengths` needs no safepoint logic at all, and it has to
/// fall out of the model rather than be special-cased per table.
#[test]
fn a_scalar_relation_survives_every_relocation_point() {
    let d = cached_length_desc();
    for e in ALL_ENDERS {
        assert!(
            boundary_admits(&d, e).is_ok(),
            "a length/bounds VALUE must survive {e:?} — relocation moves the object, not the number"
        );
    }
    assert!(violations_for(&d, &ALL_ENDERS).is_empty());
}

/// The active receiver-descriptor contract: the cache is reloaded on the
/// armed arm of the poll, which is the only collection point a call-free
/// clone has.
#[test]
fn a_cached_address_survives_the_poll_only_with_a_refresh_recipe() {
    assert!(
        boundary_admits(&poll_refreshed_receiver_desc(), RegionEnder::BackEdgePoll).is_ok(),
        "PollRefresh is written for exactly this ender"
    );

    // The same claim under any other boundary mechanism is a stale pointer.
    for boundary in [
        FactBoundary::ScopeId,
        FactBoundary::DynamicExtent,
        FactBoundary::InPlaceDegradation,
        FactBoundary::Never,
    ] {
        let d = desc("hypothetical", ReceiverClaim::Address, boundary, true);
        let v = boundary_admits(&d, RegionEnder::BackEdgePoll)
            .expect_err("a cached address with no refresh recipe cannot cross a poll");
        assert_eq!(v.ender, RegionEnder::BackEdgePoll);
    }
}

/// Phase 2's first production consumer keeps the box, masked handle and
/// authoritative source together, and runs the boundary algebra before the
/// poll lowering can consume their refresh recipe.
#[test]
fn active_descriptor_table_owns_lookup_refresh_and_dynamic_extent() {
    let mut table = ReceiverDescriptorTable::default();
    assert!(table.materialize_poll_refreshed_address(
        OBJ,
        "%box.root".into(),
        "%base.handle".into(),
        "%source.root".into(),
        true,
    ));
    assert_eq!(table.rooted_box_slot(OBJ), Some("%box.root"));
    assert_eq!(table.base_handle_slot(OBJ), Some("%base.handle"));

    // An inner clone reuses the outer materialisation rather than installing
    // an entry it would remove while the outer clone is still active.
    assert!(!table.materialize_poll_refreshed_address(
        OBJ,
        "%inner.box".into(),
        "%inner.handle".into(),
        "%inner.source".into(),
        true,
    ));
    assert!(table.materialize_poll_refreshed_address(
        OBJ + 1,
        "%other.box".into(),
        "%other.handle".into(),
        "%other.source".into(),
        true,
    ));

    assert_eq!(
        table
            .poll_refreshes()
            .expect("PollRefresh must admit BackEdgePoll"),
        vec![
            ReceiverPollRefresh {
                rooted_box_slot: "%box.root".into(),
                base_handle_slot: "%base.handle".into(),
                source_root: "%source.root".into(),
            },
            ReceiverPollRefresh {
                rooted_box_slot: "%other.box".into(),
                base_handle_slot: "%other.handle".into(),
                source_root: "%other.source".into(),
            },
        ]
    );
    assert!(table.dematerialize(OBJ + 1));
    assert_eq!(table.rooted_box_slot(OBJ), Some("%box.root"));
    assert!(table.dematerialize(OBJ));
    assert_eq!(table.rooted_box_slot(OBJ), None);
    assert!(table.poll_refreshes().unwrap().is_empty());
    assert!(!table.dematerialize(OBJ));
}

/// A cached address dies at a call no matter how the table scopes itself —
/// scoping is not a substitute for the region being call-free.
#[test]
fn a_cached_address_dies_at_a_collecting_call_under_every_boundary() {
    for boundary in [
        FactBoundary::ScopeId,
        FactBoundary::DynamicExtent,
        FactBoundary::InPlaceDegradation,
        FactBoundary::PollRefresh,
        FactBoundary::Never,
    ] {
        let d = desc("hypothetical", ReceiverClaim::Address, boundary, true);
        assert!(
            boundary_admits(&d, RegionEnder::CollectingCall).is_err(),
            "{boundary:?} must not license a cached address across a call"
        );
    }
}

/// THE phase-1 finding. `buffer_view_slots` caches a raw data pointer, is
/// function-lifetime, and is never removed — only downgraded in place. Nothing
/// structurally stops an entry registered before a `try` from being consulted
/// inside the `catch` handler, and `lower_try` clears no fact table.
///
/// It is sound in the shipped compiler for a reason outside the model (typed
/// and buffer storage is marked non-movable and never relocates). The model
/// flags it anyway, and that is correct behaviour for phase 1: the tier is
/// relying on a property of the storage kind that its own boundary mechanism
/// does not state. When phase 2 gives descriptors a non-movable-storage
/// attribute this becomes a clean pass; until then a flag is the honest answer.
#[test]
fn an_address_claim_with_no_try_exclusion_is_flagged_on_the_unwind_edge() {
    let v = boundary_admits(&buffer_view_desc(), RegionEnder::UnwindEdge)
        .expect_err("function-lifetime address claim reaches the catch handler");
    assert_eq!(v.table, "buffer_view_slots");
    assert_eq!(v.ender, RegionEnder::UnwindEdge);

    // A tier that DOES exclude `Try` from its region is not flagged: the
    // packed matcher rejects `Stmt::Try` outright, so no handler can observe
    // its cache.
    assert!(boundary_admits(&poll_refreshed_receiver_desc(), RegionEnder::UnwindEdge).is_ok());
}

/// An unwind edge diverts control; it does not fall through. That is why a
/// read-only cache is unharmed by it while a loop-carried accumulator written
/// back at region exit is not — the unwind edge skips the exit block. #9185.
#[test]
fn the_unwind_edge_is_the_only_non_fallthrough_ender() {
    for e in ALL_ENDERS {
        assert_eq!(
            e.is_fallthrough(),
            e != RegionEnder::UnwindEdge,
            "{e:?} misclassified"
        );
    }
}

/// A representation claim survives relocation but not arbitrary user code:
/// the object stays where the poll left it, but a callee can convert its
/// storage.
#[test]
fn a_representation_claim_survives_the_poll_but_not_user_code() {
    let d = packed_f64_desc();
    assert!(boundary_admits(&d, RegionEnder::BackEdgePoll).is_ok());
    for e in [
        RegionEnder::CollectingCall,
        RegionEnder::AllocatingOperation,
        RegionEnder::Coercion,
        RegionEnder::Suspension,
        RegionEnder::Unmodelled,
    ] {
        assert!(
            boundary_admits(&d, e).is_err(),
            "{e:?} can reach user code that changes the receiver's storage kind"
        );
    }
}

/// The catch-all must behave as a relocation point, or adding an HIR variant
/// silently widens every region.
#[test]
fn an_unmodelled_expression_is_a_relocation_point() {
    for d in [poll_refreshed_receiver_desc(), packed_f64_desc()] {
        assert!(boundary_admits(&d, RegionEnder::Unmodelled).is_err());
    }
    // ...but it still does not disturb a value claim.
    assert!(boundary_admits(&cached_length_desc(), RegionEnder::Unmodelled).is_ok());
}

// ---------------------------------------------------------------------------
// The walker.
// ---------------------------------------------------------------------------

#[test]
fn constants_reads_and_non_coercing_operators_are_not_enders() {
    let body = vec![
        Stmt::Expr(Expr::Number(1.0)),
        Stmt::Expr(num(NUM)),
        // `===` never coerces, so it stays open to operands of any type.
        Stmt::Expr(Expr::Compare {
            op: CompareOp::Eq,
            left: Box::new(num(OBJ)),
            right: Box::new(Expr::Null),
        }),
        // `!x` is ToBoolean; `typeof x` reads a tag.
        Stmt::Expr(Expr::Unary {
            op: UnaryOp::Not,
            operand: Box::new(num(OBJ)),
        }),
        Stmt::Expr(Expr::TypeOf(Box::new(num(OBJ)))),
    ];
    assert!(enders(&body).is_empty(), "got {:?}", enders(&body));
}

#[test]
fn typed_array_element_access_is_not_an_ender() {
    // A fixed-layout numeric load, and a store into a buffer that never grows.
    let body = vec![
        Stmt::Expr(Expr::Uint8ArrayGet {
            array: Box::new(num(OBJ)),
            index: Box::new(num(NUM)),
        }),
        Stmt::Expr(Expr::BufferIndexSet {
            buffer: Box::new(num(OBJ)),
            index: Box::new(num(NUM)),
            value: Box::new(Expr::Number(0.0)),
        }),
    ];
    assert!(enders(&body).is_empty(), "got {:?}", enders(&body));
}

/// `is_inert` is consulted on the WHOLE coercing node, not per operand — the
/// #6975 hole. `a < b` over two plain locals recurses clean while the
/// comparison itself can dispatch to a user `valueOf`.
#[test]
fn coercion_is_an_ender_exactly_when_the_node_is_not_proven_inert() {
    let proven = vec![Stmt::Expr(add(num(NUM), num(NUM2)))];
    assert!(
        enders(&proven).is_empty(),
        "proven-primitive operands: no ender"
    );

    let unproven = vec![Stmt::Expr(add(num(NUM), num(OBJ)))];
    assert_eq!(enders(&unproven), vec![RegionEnder::Coercion]);

    // `x++` on an unproven local coerces too.
    let update = vec![Stmt::Expr(Expr::Update {
        id: OBJ,
        op: UpdateOp::Increment,
        prefix: false,
    })];
    assert_eq!(enders(&update), vec![RegionEnder::Coercion]);

    // Re-run the passing shape under a predicate that proves nothing, so the
    // clean answer above is attributable to the operand proof and not to the
    // shape being unreachable.
    let nothing_inert = region_enders_in_stmts(&proven, &[], &|_| false);
    assert_eq!(nothing_inert, vec![RegionEnder::Coercion]);
}

/// Deliberate divergence from `collectors::safepoint_sites`, which does not
/// count reads. Its consumer wants a spill estimate and over-counting reads
/// would over-spill a read-heavy loop. A region model has the opposite
/// obligation: a `PropertyGet` can reach an accessor, an `IndexGet` a proxy
/// trap, and missing either licenses a stale cached address.
#[test]
fn generic_property_and_index_reads_are_enders() {
    let get = vec![Stmt::Expr(Expr::PropertyGet {
        object: Box::new(num(OBJ)),
        property: "x".to_string(),
        byte_offset: 0,
    })];
    assert_eq!(get.len(), 1);
    assert!(has(&get, RegionEnder::CollectingCall));

    let idx = vec![Stmt::Expr(Expr::IndexGet {
        object: Box::new(num(OBJ)),
        index: Box::new(num(NUM)),
    })];
    assert!(has(&idx, RegionEnder::CollectingCall));
}

/// Allocating a closure allocates. The first draft of the model let this
/// through its `_ => None` catch-all.
#[test]
fn closure_allocation_is_an_ender_and_its_body_is_not_descended_into() {
    let body = vec![Stmt::Expr(closure(vec![Stmt::Expr(call(vec![]))]))];
    let got = enders(&body);
    // The closure allocation itself, and nothing from its body: a nested
    // closure is its own frame with its own regions.
    assert_eq!(got, vec![RegionEnder::AllocatingOperation], "got {got:?}");
}

#[test]
fn expression_enders_follow_execution_order() {
    let body = vec![Stmt::Expr(call(vec![closure(vec![])]))];
    assert_eq!(
        enders(&body),
        vec![
            RegionEnder::AllocatingOperation,
            RegionEnder::CollectingCall,
        ],
        "the argument closure is allocated before the parent call executes"
    );
}

#[test]
fn loop_enders_follow_body_then_update_order() {
    let body_call = Stmt::Expr(call(vec![]));
    let update_alloc = Expr::Array(vec![]);
    let for_loop = vec![Stmt::For {
        init: None,
        condition: None,
        update: Some(update_alloc.clone()),
        body: vec![body_call.clone()],
    }];
    assert_eq!(
        enders(&for_loop),
        vec![
            RegionEnder::CollectingCall,
            RegionEnder::AllocatingOperation,
            RegionEnder::BackEdgePoll,
        ]
    );

    let do_while = vec![Stmt::DoWhile {
        body: vec![body_call],
        condition: update_alloc,
    }];
    assert_eq!(
        enders(&do_while),
        vec![
            RegionEnder::CollectingCall,
            RegionEnder::AllocatingOperation,
            RegionEnder::BackEdgePoll,
        ]
    );
}

#[test]
fn throw_and_try_contribute_an_unwind_edge() {
    let thrown = vec![Stmt::Throw(Expr::Number(1.0))];
    assert!(has(&thrown, RegionEnder::UnwindEdge));

    let tried = vec![Stmt::Try {
        body: vec![Stmt::Expr(num(NUM))],
        catch: Some(CatchClause {
            param: None,
            body: vec![Stmt::Expr(num(NUM))],
        }),
        finally: None,
    }];
    assert!(
        has(&tried, RegionEnder::UnwindEdge),
        "every statement in a try body may divert to the handler"
    );
}

#[test]
fn every_loop_contributes_a_back_edge_poll_including_a_nested_one() {
    let inner = Stmt::While {
        condition: Expr::Bool(true),
        body: vec![Stmt::Expr(num(NUM))],
    };
    let outer = vec![Stmt::While {
        condition: Expr::Bool(true),
        body: vec![inner],
    }];
    let polls = enders(&outer)
        .iter()
        .filter(|e| **e == RegionEnder::BackEdgePoll)
        .count();
    assert_eq!(
        polls, 2,
        "an inner loop's poll is a relocation point for the enclosing region too — \
         which is why the armed-poll refresh reloads every active receiver cache, \
         not just the innermost scope's"
    );
}

// ---------------------------------------------------------------------------
// The equivalence lint.
// ---------------------------------------------------------------------------

/// Bodies spanning both answers, shared with `loop_purity`'s vocabulary.
fn battery() -> Vec<(&'static str, Vec<Stmt>)> {
    vec![
        ("empty", vec![]),
        ("constant", vec![Stmt::Expr(Expr::Number(1.0))]),
        ("local read", vec![Stmt::Expr(num(NUM))]),
        (
            "proven-primitive arithmetic",
            vec![Stmt::Expr(add(num(NUM), num(NUM2)))],
        ),
        (
            "proven-primitive comparison",
            vec![Stmt::Expr(lt(num(NUM), num(NUM2)))],
        ),
        (
            "typed-array copy",
            vec![Stmt::Expr(Expr::Uint8ArraySet {
                array: Box::new(num(OBJ)),
                index: Box::new(num(NUM)),
                value: Box::new(Expr::Uint8ArrayGet {
                    array: Box::new(num(OBJ)),
                    index: Box::new(num(NUM2)),
                }),
            })],
        ),
        (
            "strict equality on an object",
            vec![Stmt::Expr(Expr::Compare {
                op: CompareOp::Eq,
                left: Box::new(num(OBJ)),
                right: Box::new(Expr::Null),
            })],
        ),
        ("a call", vec![Stmt::Expr(call(vec![]))]),
        (
            "coercion over an unproven operand",
            vec![Stmt::Expr(add(num(NUM), num(OBJ)))],
        ),
        (
            "object literal",
            vec![Stmt::Expr(Expr::Object(vec![(
                "k".to_string(),
                Expr::Number(1.0),
            )]))],
        ),
        ("array literal", vec![Stmt::Expr(Expr::Array(vec![]))]),
        ("closure allocation", vec![Stmt::Expr(closure(vec![]))]),
        (
            "generic property read",
            vec![Stmt::Expr(Expr::PropertyGet {
                object: Box::new(num(OBJ)),
                property: "x".to_string(),
                byte_offset: 0,
            })],
        ),
        (
            "generic index read",
            vec![Stmt::Expr(Expr::IndexGet {
                object: Box::new(num(OBJ)),
                index: Box::new(num(NUM)),
            })],
        ),
        (
            "generic index write",
            vec![Stmt::Expr(Expr::IndexSet {
                object: Box::new(num(OBJ)),
                index: Box::new(num(NUM)),
                value: Box::new(Expr::Number(1.0)),
            })],
        ),
        (
            "conditional over clean operands",
            vec![Stmt::Expr(Expr::Conditional {
                condition: Box::new(Expr::Bool(true)),
                then_expr: Box::new(num(NUM)),
                else_expr: Box::new(num(NUM2)),
            })],
        ),
        (
            "if with a call in one arm",
            vec![Stmt::If {
                condition: Expr::Bool(true),
                then_branch: vec![Stmt::Expr(call(vec![]))],
                else_branch: Some(vec![Stmt::Expr(num(NUM))]),
            }],
        ),
        (
            "nested loop, clean body",
            vec![Stmt::While {
                condition: Expr::Bool(true),
                body: vec![Stmt::Expr(add(num(NUM), num(NUM2)))],
            }],
        ),
    ]
}

/// The phase-1 contract: **if the model finds no relocation point other than a
/// back-edge poll, `loop_may_allocate` must also have proven the body
/// alloc-free.**
///
/// The converse is not asserted — `loop_may_allocate` answers `true` for any
/// statement it does not model, which is imprecision, not a collection point.
///
/// This assertion is why the model is written as an allowlist with an
/// `Unmodelled` catch-all rather than as an enumeration of enders. The
/// enumeration version passed every hand-written test above and failed here on
/// three entries: `generic property read`, `generic index read`, and
/// `closure allocation`.
#[test]
fn model_is_no_weaker_than_loop_purity_across_the_battery() {
    let weaker: Vec<&str> = battery()
        .into_iter()
        .filter(|(_, body)| !model_is_no_weaker_than_loop_purity(body, &[], &stub_inert))
        .map(|(name, _)| name)
        .collect();

    assert!(
        weaker.is_empty(),
        "MODEL IS WEAKER THAN loop_may_allocate on {weaker:?}: the model found no \
         relocation point in these bodies, but loop_purity did not prove them \
         alloc-free. Either the model is missing an ender (unsound — it would \
         license believing a cached address across a real collection point) or \
         loop_purity is imprecise there and the exemption belongs in this test \
         with an argument."
    );
}

/// The battery has to contain entries on both sides, or the implication above
/// is vacuously true and proves nothing.
#[test]
fn the_battery_exercises_both_answers() {
    let mut clean = 0;
    let mut dirty = 0;
    for (_, body) in battery() {
        if region_enders_in_stmts(&body, &[], &stub_inert)
            .iter()
            .any(|e| !matches!(e, RegionEnder::BackEdgePoll))
        {
            dirty += 1;
        } else {
            clean += 1;
        }
    }
    assert!(
        clean >= 6,
        "only {clean} clean entries — implication is near-vacuous"
    );
    assert!(dirty >= 6, "only {dirty} dirty entries");
}

// ---------------------------------------------------------------------------
// The fact-table inventory.
//
// Every receiver-keyed fact table on `FnCtx`, transcribed from the code with
// its declaration site. This is the model's contact with reality: the four
// fixtures above are hand-picked, and a model that only agrees with its own
// examples proves nothing.
//
// The `unwind_safe_by` column is the one that matters. Read down it and the
// #9254 thesis is visible without argument: nine tables, six different reasons
// none of which is a boundary the table itself states.
// ---------------------------------------------------------------------------

struct TableRow {
    table: &'static str,
    claim: ReceiverClaim,
    boundary: FactBoundary,
    /// Whether the tier structurally keeps `Stmt::Try` out of the extent in
    /// which the fact is live — by an explicit match arm, by a body shape that
    /// cannot contain one, or by a verified call-free clone.
    excludes_try: bool,
    /// How unwind safety is actually obtained today, in the code's own terms.
    unwind_safe_by: &'static str,
}

fn inventory() -> Vec<TableRow> {
    vec![
        TableRow {
            table: "cached_lengths",
            claim: ReceiverClaim::ScalarRelation,
            boundary: FactBoundary::DynamicExtent,
            excludes_try: false,
            unwind_safe_by: "a length is a value; relocation moves the object, not the number",
        },
        TableRow {
            table: "bounded_index_pairs",
            claim: ReceiverClaim::ScalarRelation,
            boundary: FactBoundary::ScopeId,
            excludes_try: false,
            unwind_safe_by: "arithmetic relation; admission walkers descend into Try",
        },
        TableRow {
            table: "bounded_buffer_index_pairs",
            claim: ReceiverClaim::ScalarRelation,
            boundary: FactBoundary::ScopeId,
            excludes_try: false,
            unwind_safe_by: "arithmetic relation over local ids carrying an explicit \
                             BoundsProof; admission walkers descend into Try",
        },
        TableRow {
            table: "guarded_buffer_index_pairs",
            claim: ReceiverClaim::ScalarRelation,
            boundary: FactBoundary::ScopeId,
            excludes_try: false,
            unwind_safe_by: "arithmetic relation carried by a dominating guard",
        },
        TableRow {
            table: "int_range_facts",
            claim: ReceiverClaim::ScalarRelation,
            boundary: FactBoundary::ScopeId,
            excludes_try: false,
            unwind_safe_by: "producer rejects every writer (stmts_mutate_local walks Try); \
                             lexical-order invalidation precedes catch lowering",
        },
        TableRow {
            table: "packed_f64_loop_facts",
            claim: ReceiverClaim::Representation,
            boundary: FactBoundary::ScopeId,
            excludes_try: true,
            unwind_safe_by: "matcher rejects Stmt::Try outright (stmt/loops.rs); Throw admitted \
                             only with the #9185 accumulator flush at the throw site",
        },
        TableRow {
            table: "masked_window_array_facts",
            claim: ReceiverClaim::Representation,
            boundary: FactBoundary::ScopeId,
            excludes_try: true,
            unwind_safe_by: "region admits only scalar statements; ctx.try_depth gates \
                             privatization",
        },
        TableRow {
            table: "string_window_array_facts",
            claim: ReceiverClaim::Representation,
            boundary: FactBoundary::ScopeId,
            excludes_try: true,
            unwind_safe_by: "body must be a single LocalSet, so no handler can exist in extent",
        },
        TableRow {
            table: "class_field_loop_facts",
            claim: ReceiverClaim::Address,
            boundary: FactBoundary::ScopeId,
            excludes_try: true,
            unwind_safe_by: "single-statement body plus a post-hoc contains_gc_unsafe_call scan \
                             that discards the clone if any call was emitted",
        },
        TableRow {
            table: "element_shape_loop_facts",
            claim: ReceiverClaim::Address,
            boundary: FactBoundary::ScopeId,
            excludes_try: true,
            unwind_safe_by: "same double lock as class_field_loop_facts",
        },
        TableRow {
            table: "receiver_descriptors",
            claim: ReceiverClaim::Address,
            boundary: FactBoundary::PollRefresh,
            excludes_try: true,
            unwind_safe_by: "read-only copy whose authority stays in the source root; matcher \
                             forbids receiver reassignment",
        },
        TableRow {
            table: "versioned_indexed_loop_facts",
            claim: ReceiverClaim::Address,
            boundary: FactBoundary::DynamicExtent,
            excludes_try: true,
            unwind_safe_by: "the ONE explicit try_depth == 0 gate in codegen, plus per-iteration \
                             reload from rooted slots in Fingerprints mode",
        },
        TableRow {
            table: "stable_packed_loop_facts",
            claim: ReceiverClaim::Address,
            boundary: FactBoundary::DynamicExtent,
            // stmt_flags has a `_ => {}` catch-all, so Stmt::Try is invisible
            // to the admission scan and the body tail is unconstrained.
            excludes_try: false,
            unwind_safe_by: "EMERGENT: call-free post-scan (ordinary mode) or a dirty bit stored \
                             before every call/invoke (capture/nested modes)",
        },
        TableRow {
            table: "buffer_view_slots",
            claim: ReceiverClaim::Address,
            boundary: FactBoundary::InPlaceDegradation,
            excludes_try: false,
            unwind_safe_by: "storage kind is non-movable (GC_TYPE_TYPED_ARRAY/BUFFER); the fact \
                             is immutable, not bounded",
        },
        TableRow {
            table: "buffer_data_slots",
            claim: ReceiverClaim::Address,
            boundary: FactBoundary::Never,
            excludes_try: false,
            unwind_safe_by: "never-reassigned binding plus non-movable storage; nothing can make \
                             the fact false",
        },
        TableRow {
            table: "class_keys_slots",
            claim: ReceiverClaim::Address,
            boundary: FactBoundary::Never,
            excludes_try: false,
            unwind_safe_by: "shadow-slot-bound root the collector rewrites in place; every use \
                             reloads per site",
        },
    ]
}

/// The inventory, run through the model.
///
/// A flag here is NOT a bug report. It says: *this table's stated boundary
/// does not by itself license its claim across an unwind edge* — the safety
/// comes from somewhere the boundary mechanism cannot express, recorded in
/// `unwind_safe_by`. That gap is the thing #9254 proposes to close, and
/// pinning the exact set is how phase 2 proves it closed one.
#[test]
fn the_inventory_flags_exactly_the_tables_whose_unwind_safety_is_external() {
    let flagged: Vec<&str> = inventory()
        .iter()
        .filter(|row| {
            let d = ReceiverDescriptor {
                table: row.table,
                receiver: OBJ,
                claim: row.claim,
                boundary: row.boundary,
                excludes_try: row.excludes_try,
            };
            boundary_admits(&d, RegionEnder::UnwindEdge).is_err()
        })
        .map(|row| row.table)
        .collect();

    let with_reasons: Vec<String> = inventory()
        .iter()
        .filter(|row| flagged.contains(&row.table))
        .map(|row| format!("{} <- {}", row.table, row.unwind_safe_by))
        .collect();

    assert_eq!(
        flagged,
        vec![
            // Emergent, per the survey: `stmt_flags` has a `_ => {}` arm, so
            // `Stmt::Try` is invisible to admission and the body tail is
            // unconstrained. Safety rests on a post-hoc call-free scan in one
            // mode and a before-call dirty bit in the others.
            "stable_packed_loop_facts",
            // Immutable-fact tables: a cached pointer into storage the GC
            // marks non-movable, or a root the collector rewrites in place.
            // Sound, but for a reason the boundary vocabulary cannot state —
            // which is exactly why phase 2 needs a non-movable-storage
            // attribute on the descriptor.
            "buffer_view_slots",
            "buffer_data_slots",
            "class_keys_slots",
        ],
        "the set of tables relying on out-of-band unwind safety changed; if a tier \
         gained or lost a structural guarantee, update the row AND its unwind_safe_by \
         note with the code that changed.\nCurrent reasons:\n  {}",
        with_reasons.join("\n  ")
    );
}

/// Every row must say how its unwind safety is obtained, and a flagged row's
/// note is the whole point of the flag — it names the mechanism that lives
/// outside the boundary vocabulary. An empty note would make the inventory a
/// list of names instead of an argument.
#[test]
fn every_inventory_row_explains_how_its_unwind_safety_is_obtained() {
    for row in inventory() {
        assert!(
            !row.unwind_safe_by.trim().is_empty(),
            "{} has no unwind_safe_by note",
            row.table
        );
        // A claim that names no mechanism cannot be checked against the code
        // later, which is how these notes go stale without anyone noticing.
        assert!(
            row.unwind_safe_by.len() > 20,
            "{}'s note is too terse to check against the code: {:?}",
            row.table,
            row.unwind_safe_by
        );
    }
}

/// Every scalar-relation table must be clean under every ender — if one is
/// ever flagged, the model has confused a value with an address.
#[test]
fn no_scalar_relation_table_is_ever_flagged() {
    for row in inventory()
        .iter()
        .filter(|r| r.claim == ReceiverClaim::ScalarRelation)
    {
        let d = ReceiverDescriptor {
            table: row.table,
            receiver: OBJ,
            claim: row.claim,
            boundary: row.boundary,
            excludes_try: row.excludes_try,
        };
        assert!(
            violations_for(&d, &ALL_ENDERS).is_empty(),
            "{} is a value claim and must survive every relocation point",
            row.table
        );
    }
}

/// The inventory has to cover the declarations that exist. A table added to
/// `FnCtx` without a row here is a table the model has never seen.
#[test]
fn the_inventory_covers_every_claim_kind_and_every_boundary_mechanism() {
    let rows = inventory();
    for claim in [
        ReceiverClaim::ScalarRelation,
        ReceiverClaim::Representation,
        ReceiverClaim::Address,
    ] {
        assert!(
            rows.iter().any(|r| r.claim == claim),
            "no inventory row exercises {claim:?}"
        );
    }
    for boundary in [
        FactBoundary::ScopeId,
        FactBoundary::DynamicExtent,
        FactBoundary::InPlaceDegradation,
        FactBoundary::PollRefresh,
        FactBoundary::Never,
    ] {
        assert!(
            rows.iter().any(|r| r.boundary == boundary),
            "no inventory row exercises {boundary:?}"
        );
    }
    assert_eq!(
        rows.len(),
        16,
        "inventory size changed — see FnCtx declarations"
    );
}
