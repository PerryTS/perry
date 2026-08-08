//! One test per root-lowering mechanic #7502 lists as having no native-roots
//! assertion. Each names the shadow-stack test it is the counterpart of.

use super::*;
use perry_hir::{BinaryOp, CompareOp};

/// `for (let i = 0; i < n; i++) { … }` with an OPAQUE bound.
///
/// The bound is the function's parameter on purpose: a constant-trip loop is
/// unrolled, and an unrolled loop has no back edge, so a claim about "the next
/// iteration's safepoint" would be a claim about a loop that no longer exists.
fn counted_loop(body: Vec<Stmt>) -> Stmt {
    Stmt::For {
        init: Some(Box::new(Stmt::Let {
            id: 90,
            name: "i".to_string(),
            ty: Type::Number,
            mutable: true,
            init: Some(Expr::Number(0.0)),
        })),
        condition: Some(Expr::Compare {
            op: CompareOp::Lt,
            left: Box::new(Expr::LocalGet(90)),
            right: Box::new(Expr::LocalGet(100)),
        }),
        update: Some(Expr::LocalSet(
            90,
            Box::new(Expr::Binary {
                op: BinaryOp::Add,
                left: Box::new(Expr::LocalGet(90)),
                right: Box::new(Expr::Number(1.0)),
            }),
        )),
        body,
    }
}

// ---------------------------------------------------------------------------
// 1. A pointer-typed local is a root at the safepoints it is live across
//    (shadow counterpart: `function_shadow_slots_clear_dead_values_and_skip_
//    numeric_roots`, first half)
// ---------------------------------------------------------------------------

/// The load-bearing claim of the whole backend, stated where the collector will
/// read it: a heap value held in a local across a later allocation is in that
/// allocation's live set, in the emitted map.
///
/// Asserted at all three vantages, because each can be right while the next is
/// wrong: codegen can ask for a root slot LLVM then declines to record, and
/// LLVM can record a statepoint whose roots the map encoder drops.
///
/// **Sabotage 1** — `function/precise_roots.rs`, the alloca-retype arm emits
/// `alloca double` instead of `alloca ptr addrspace(1)`: RED, `root_allocas`
/// 2 → 0. This one reddens 10 of the 14 tests in the module (all eight
/// mechanics and both pipeline self-tests), which is the point: nothing here
/// can pass without the retype.
///
/// **Sabotage 2** — the same arm's `.filter(|reg| roots.contains(reg))`
/// dropped, so every scalar alloca is retyped: RED.
#[test]
fn a_live_pointer_local_is_a_root_in_the_emitted_map() {
    for target in NATIVE_TARGETS {
        let name = "m1_live_local.ts";
        let module = probe_module(
            name,
            vec![
                let_stmt(1, "a", Expr::MapNew),
                // Allocates while `a` is still live.
                let_stmt(2, "b", Expr::MapNew),
                Stmt::Return(Some(Expr::LocalGet(1))),
            ],
        );
        let _pin = NativeRootsPin::native();
        let ir = native_ir(&module, target, false);
        let symbol = probe_symbol(name);
        let fn_ir = function_slice(&ir, &symbol);

        // (1) the request
        assert!(
            fn_ir.contains("gc \"statepoint-example\""),
            "[{target}] no GC strategy — RS4GC would skip this function \
             entirely:\n{fn_ir}"
        );
        assert_eq!(
            root_allocas(fn_ir),
            2,
            "[{target}] both heap locals must be `ptr addrspace(1)` root \
             slots:\n{fn_ir}"
        );

        // (2) the result
        let points = statepoints_of(&ir, target, &symbol);
        let allocs = points.at("js_map_alloc");
        assert_eq!(
            allocs.len(),
            2,
            "[{target}] one safepoint per allocation: {:?}",
            points.iter().collect::<Vec<_>>()
        );
        assert_eq!(
            allocs[0].live.len(),
            0,
            "[{target}] nothing is live yet at the first allocation: {:?}",
            allocs[0]
        );
        assert_eq!(
            allocs[1].live.len(),
            1,
            "[{target}] `a` is live across `b`'s allocation and must be in that \
             statepoint's live set — a value the collector cannot see here is \
             a stale pointer the moment the minor evacuates: {:?}",
            allocs[1]
        );

        // (3) what the collector reads
        assert_eq!(
            map_max_roots(&assembly_for(&ir, target), target, &symbol),
            1,
            "[{target}] the compact map must carry that root; a map that says \
             nothing lives here is indistinguishable at run time from no \
             rooting at all"
        );
    }
}

// ---------------------------------------------------------------------------
// 2. A dead value is not a root at the next safepoint
//    (shadow counterpart: the `js_shadow_slot_set(i32 0, i64 0)` clear in
//    `function_shadow_slots_clear_dead_values_and_skip_numeric_roots`)
// ---------------------------------------------------------------------------

/// The shadow stack clears a dead slot before the next allocation so the
/// collector stops tracing it. Native roots have no clear to emit: liveness is
/// computed over SSA values, so the value is simply absent from the statepoint.
/// That is a *property of the composition* (`mem2reg` then RS4GC), not
/// something either half guarantees alone, and nothing asserted it.
///
/// Stated differentially against a program that differs only in whether the
/// first local is read again. Without the control half, "zero roots" would be
/// satisfied by a lowering that roots nothing at all.
///
/// **Sabotage** — `function/precise_roots.rs`, a reload-and-use of every root
/// alloca spliced in before each `ret`, which is the explicit statepoint
/// bridge's old conservative CFG-union liveness reintroduced: RED, the dead
/// value's live set at the second allocation went 0 → 1
/// (`live: ["%rs4gc.s3"]`). This is the ONLY test of the seven that sabotage
/// reddens, so it is measuring its own subject and not a shared prerequisite.
#[test]
fn a_value_that_is_dead_at_a_safepoint_is_not_in_its_live_set() {
    for target in NATIVE_TARGETS {
        let _pin = NativeRootsPin::native();

        let dead_name = "m2_dead.ts";
        let dead = probe_module(
            dead_name,
            vec![
                let_stmt(1, "dead", Expr::MapNew),
                let_stmt(2, "live", Expr::MapNew),
                Stmt::Return(Some(Expr::LocalGet(2))),
            ],
        );
        let dead_ir = native_ir(&dead, target, false);
        let dead_sym = probe_symbol(dead_name);
        let dead_points = statepoints_of(&dead_ir, target, &dead_sym);
        let dead_allocs = dead_points.at("js_map_alloc");
        assert_eq!(dead_allocs.len(), 2, "[{target}] {dead_allocs:?}");
        assert_eq!(
            dead_allocs[1].live.len(),
            0,
            "[{target}] the first local is dead by the second allocation and \
             must not be traced: {:?}",
            dead_allocs[1]
        );

        let live_name = "m2_live.ts";
        let live = probe_module(
            live_name,
            vec![
                let_stmt(1, "kept", Expr::MapNew),
                let_stmt(2, "other", Expr::MapNew),
                Stmt::Return(Some(Expr::LocalGet(1))),
            ],
        );
        let live_ir = native_ir(&live, target, false);
        let live_sym = probe_symbol(live_name);
        let live_allocs = statepoints_of(&live_ir, target, &live_sym);
        let live_allocs = live_allocs.at("js_map_alloc");
        assert_eq!(
            live_allocs[1].live.len(),
            1,
            "[{target}] CONTROL: the same program with the first local read \
             afterwards must report it live — otherwise the zero above is an \
             inability to report roots, not an exclusion: {:?}",
            live_allocs[1]
        );

        // Same claim where it is consumed.
        assert_eq!(
            map_max_roots(&assembly_for(&dead_ir, target), target, &dead_sym),
            0,
            "[{target}] the emitted map must carry no root for a dead value"
        );
        assert_eq!(
            map_max_roots(&assembly_for(&live_ir, target), target, &live_sym),
            1,
            "[{target}] CONTROL: the emitted map does carry the live one"
        );
    }
}

// ---------------------------------------------------------------------------
// 3. A numeric local reserves no root
//    (shadow counterpart: `js_shadow_frame_enter(i32 2)` for three locals)
// ---------------------------------------------------------------------------

/// A local that cannot hold a collectable value must not become a root — the
/// #6997 lesson, restated for the lowering that ships. Rooting a number costs a
/// map entry per safepoint it is live across, and the collector then traces an
/// integer as a pointer.
///
/// The negative direction is asserted on PRE-`opt` IR, which is the only
/// vantage where "codegen never asked for a root" and "LLVM removed one" are
/// still distinguishable — and against a heap-valued twin that differs in
/// exactly one expression, so a lowering that stopped rooting anything fails
/// the control.
///
/// **Sabotage** — `function/precise_roots.rs`, retyping every `alloca double`
/// rather than only the bound roots: RED, the numeric program's root slots went
/// 1 → 2 while the heap control stayed at 2, collapsing the difference the
/// mechanic is about.
#[test]
fn a_numeric_local_reserves_no_root_and_a_heap_one_does() {
    for target in NATIVE_TARGETS {
        let _pin = NativeRootsPin::native();

        // Identical programs but for the type of `x`, which is live across the
        // allocation in both.
        let numeric_name = "m3_numeric.ts";
        let numeric = probe_module(
            numeric_name,
            vec![
                let_stmt(1, "x", Expr::Number(42.0)),
                let_stmt(2, "b", Expr::MapNew),
                Stmt::Return(Some(Expr::LocalGet(1))),
            ],
        );
        let numeric_ir = native_ir(&numeric, target, false);
        let numeric_sym = probe_symbol(numeric_name);
        let numeric_fn = function_slice(&numeric_ir, &numeric_sym);

        let heap_name = "m3_heap.ts";
        let heap = probe_module(
            heap_name,
            vec![
                let_stmt(1, "x", Expr::MapNew),
                let_stmt(2, "b", Expr::MapNew),
                Stmt::Return(Some(Expr::LocalGet(1))),
            ],
        );
        let heap_ir = native_ir(&heap, target, false);
        let heap_sym = probe_symbol(heap_name);
        let heap_fn = function_slice(&heap_ir, &heap_sym);

        assert_eq!(
            (root_allocas(numeric_fn), root_allocas(heap_fn)),
            (1, 2),
            "[{target}] two locals, of which only the heap one may reserve a \
             root slot. Stated as a pair so a lowering that stopped rooting \
             ANYTHING fails the second half instead of passing the first.\
             \nnumeric:\n{numeric_fn}\nheap:\n{heap_fn}"
        );
        assert!(
            scalar_allocas(numeric_fn) > scalar_allocas(heap_fn),
            "[{target}] the numeric local must still get a plain scalar slot: \
             {} vs {}",
            scalar_allocas(numeric_fn),
            scalar_allocas(heap_fn)
        );

        // The same claim where it costs: the numeric value is live across the
        // allocation and must still not appear in its live set.
        let numeric_alloc = statepoints_of(&numeric_ir, target, &numeric_sym);
        let numeric_alloc = numeric_alloc.at("js_map_alloc");
        assert_eq!(
            numeric_alloc[0].live.len(),
            0,
            "[{target}] a number live across an allocation must not be traced: \
             {:?}",
            numeric_alloc[0]
        );
        let heap_alloc = statepoints_of(&heap_ir, target, &heap_sym);
        let heap_alloc = heap_alloc.at("js_map_alloc");
        assert_eq!(
            heap_alloc[1].live.len(),
            1,
            "[{target}] CONTROL: the heap twin's value IS traced across the \
             same allocation: {:?}",
            heap_alloc[1]
        );
    }
}

// ---------------------------------------------------------------------------
// 5. The entry module's roots begin after the init prelude
//    (shadow counterpart: `entry_module_top_level_shadow_frame_starts_after_
//    init_prelude`)
// ---------------------------------------------------------------------------

/// `main` runs an init prelude — `js_gc_init`, then the module's string table —
/// before any user code. The shadow lowering expresses "no root before that" by
/// pushing the frame after the prelude. Native roots have no frame, so the
/// property has to be stated directly: **no safepoint at or before `js_gc_init`
/// may carry a live GC value**, because there is no collector yet to relocate
/// it and no heap it could have come from.
///
/// The non-vacuity half matters more than usual here: "no live roots before
/// `js_gc_init`" is trivially true of a `main` with no roots anywhere, which is
/// exactly what a broken entry lowering produces. So the test also requires a
/// non-empty live set to appear later in the same function.
///
/// **Sabotage 1** — `codegen/entry.rs`, the `js_gc_init` call deleted from
/// `main`: RED ("entry `main` must initialize the GC"). Proves the anchor is
/// real rather than assumed.
///
/// **Sabotage 2** — the module's `__perry_init_strings_*` call moved from the
/// prelude to a pre-return call, so it runs after user code: RED, "a root is
/// live before `__perry_init_strings_*` (#32 vs #4)". Both reddened only this
/// test.
#[test]
fn no_entry_module_root_is_live_before_the_gc_is_initialized() {
    for target in NATIVE_TARGETS {
        let _pin = NativeRootsPin::native();
        let module = entry_module(
            "m5_entry.ts",
            vec![
                let_stmt(1, "a", Expr::MapNew),
                let_stmt(2, "b", Expr::MapNew),
                console_log(vec![Expr::LocalGet(1), Expr::LocalGet(2)]),
            ],
        );
        let ir = native_ir(&module, target, true);
        let points = statepoints_of(&ir, target, "main");
        let callees = || points.iter().map(|sp| &sp.callee).collect::<Vec<_>>();

        let gc_init = points
            .iter()
            .position(|sp| sp.callee == "js_gc_init")
            .unwrap_or_else(|| {
                panic!(
                    "[{target}] entry `main` must initialize the GC: {:?}",
                    callees()
                )
            });
        let strings_init = points
            .iter()
            .position(|sp| sp.callee.starts_with("__perry_init_strings_"))
            .unwrap_or_else(|| {
                panic!(
                    "[{target}] entry `main` must run the module string table \
                     before user code: {:?}",
                    callees()
                )
            });
        // NON-VACUITY: this is the assertion that makes the two orderings below
        // mean something. "Nothing is rooted before the prelude" is trivially
        // true of a `main` that roots nothing anywhere — which is what a broken
        // entry lowering produces.
        let first_rooted = points
            .iter()
            .position(|sp| !sp.live.is_empty())
            .unwrap_or_else(|| {
                panic!(
                    "[{target}] no safepoint anywhere in `main` carries a live \
                     root, so an ordering claim about them is vacuous: {:?}",
                    points.iter().collect::<Vec<_>>()
                )
            });

        assert!(
            gc_init < first_rooted,
            "[{target}] safepoint #{first_rooted} (`{}`) carries a live GC \
             value at or before `js_gc_init` (#{gc_init}) — before there is a \
             collector to relocate it or a heap it could have come from: {:?}",
            points.iter().nth(first_rooted).unwrap().callee,
            callees()
        );
        assert!(
            strings_init < first_rooted,
            "[{target}] a root is live before `__perry_init_strings_*` \
             (#{strings_init} vs #{first_rooted}) — the shadow lowering states \
             this by pushing its frame after the prelude; native roots have no \
             frame, so it has to be stated here: {:?}",
            callees()
        );
    }
}

// ---------------------------------------------------------------------------
// 6. A loop body's roots do not survive into the next iteration
//    (shadow counterpart: `loop_body_shadow_slots_are_cleared_each_iteration`)
// ---------------------------------------------------------------------------

/// A value allocated inside a loop body and dead at the back edge must not be a
/// root at the next iteration's allocation. On the shadow stack that needs an
/// emitted clear; here it needs the per-back-edge live sets to be per-iteration
/// rather than a union over the loop.
///
/// The differential control is a second outer local that IS live across the
/// loop: the same in-loop safepoint then reports two roots, which proves the
/// one-root answer below is an exclusion and not a ceiling.
///
/// **Sabotage** — `function/precise_roots.rs`, every root reloaded at the top
/// of each block and used before its terminator, so a root spans every block it
/// is defined before: RED, the in-loop live set went 1 → 2
/// (`live: ["%.0", "%r16.0"]`).
///
/// Worth recording what did NOT work, because it says something about the
/// mechanic: the weaker mechanic-2 sabotage (keep-alive at `ret` only) leaves
/// this test GREEN, and LLVM is right about that — the previous iteration's
/// value is genuinely dead at the next iteration's allocation, since the phi
/// that carries it to the return is redefined in the body. Only a lowering that
/// keeps a root live across the back edge itself can break this row.
#[test]
fn a_loop_iterations_dead_root_is_not_live_at_the_next_iteration() {
    for target in NATIVE_TARGETS {
        let _pin = NativeRootsPin::native();

        let subject_name = "m6_loop.ts";
        let subject = probe_module(
            subject_name,
            vec![
                let_stmt(1, "acc", Expr::MapNew),
                counted_loop(vec![let_stmt(2, "tmp", Expr::MapNew)]),
                Stmt::Return(Some(Expr::LocalGet(1))),
            ],
        );
        let subject_ir = native_ir(&subject, target, false);
        let subject_sym = probe_symbol(subject_name);
        let subject_points = statepoints_of(&subject_ir, target, &subject_sym);
        let subject_allocs = subject_points.at("js_map_alloc");
        assert_eq!(
            subject_allocs.len(),
            2,
            "[{target}] the loop must not have been unrolled or its body \
             sunk — one allocation outside, one inside: {subject_allocs:?}"
        );
        assert_eq!(
            subject_allocs[1].live.len(),
            1,
            "[{target}] only `acc` may be live at the in-loop allocation. A \
             second root here is the previous iteration's `tmp` surviving the \
             back edge: {:?}",
            subject_allocs[1]
        );

        let control_name = "m6_loop_control.ts";
        let control = probe_module(
            control_name,
            vec![
                let_stmt(1, "acc", Expr::MapNew),
                let_stmt(3, "acc2", Expr::MapNew),
                counted_loop(vec![let_stmt(2, "tmp", Expr::MapNew)]),
                console_log(vec![Expr::LocalGet(3)]),
                Stmt::Return(Some(Expr::LocalGet(1))),
            ],
        );
        let control_ir = native_ir(&control, target, false);
        let control_sym = probe_symbol(control_name);
        let control_points = statepoints_of(&control_ir, target, &control_sym);
        let control_allocs = control_points.at("js_map_alloc");
        assert_eq!(
            control_allocs.last().unwrap().live.len(),
            2,
            "[{target}] CONTROL: two outer locals live across the loop must \
             both be roots at the in-loop allocation — so the count above is \
             not a cap: {:?}",
            control_allocs.last().unwrap()
        );
    }
}

// ---------------------------------------------------------------------------
// 9. Every reserved root reaches the native lowering (#7184's shape)
//    (shadow counterpart: `duplicate_var_declarations_keep_every_slot_inside_
//    the_frame`)
// ---------------------------------------------------------------------------

/// #7502's table marks this row `n/a` — "no frame bound exists; the #7184 shape
/// is unrepresentable". **That is half right, and the half it gets wrong is the
/// dangerous half.**
///
/// The frame bound is indeed gone. But `lower_precise_roots_to_native_stack`
/// sizes its root vector by `slot_count` and collects roots with
/// `roots.get_mut(idx)`, so a bind whose index is `>= slot_count` is dropped by
/// an `Option` returning `None` — exactly the shape of the runtime bounds check
/// that made #7184 silent, one layer up. A dropped index means the alloca is
/// never added to `root_ptrs`, is never retyped to `ptr addrspace(1)`, and is
/// therefore not a root at all. The IR still looks like rooted code.
///
/// So the mechanic survives the change of lowering and needs an assertion. This
/// is the shadow suite's duplicate-`var` program (two `Stmt::Let`s sharing a
/// `LocalId`, plus a trailing local — the arrangement that used to burn a slot
/// index per declaration while the frame was sized by map cardinality),
/// asserted natively: both pointer locals must end up as root slots.
///
/// **Sabotage** — `function/precise_roots.rs`, `roots` sized
/// `slot_count.saturating_sub(1)` so the last reserved index falls outside the
/// vector, i.e. `slot_count` under-counted by one: RED, the map's largest live
/// set went 2 → 1. One root vanished; the IR compiled, verified and emitted an
/// otherwise identical function.
#[test]
fn a_deduplicated_slot_index_still_reaches_the_native_root_set() {
    for target in NATIVE_TARGETS {
        let _pin = NativeRootsPin::native();
        let name = "m9_dup_var.ts";
        let module = probe_module(
            name,
            vec![
                let_stmt(1, "dup", Expr::MapNew),
                // Same LocalId, second declaration site.
                let_stmt(1, "dup", Expr::MapNew),
                let_stmt(2, "later", Expr::MapNew),
                // Keeps BOTH live across one allocation, so the map has to
                // report two roots at a single safepoint.
                Stmt::Return(Some(Expr::Array(vec![
                    Expr::LocalGet(1),
                    Expr::LocalGet(2),
                ]))),
            ],
        );
        let ir = native_ir(&module, target, false);
        let symbol = probe_symbol(name);

        // Both locals are live across the returned array's allocation, so the
        // collector must find TWO roots at that safepoint. A slot index that
        // fell outside `roots` costs exactly one of them, silently.
        assert_eq!(
            map_max_roots(&assembly_for(&ir, target), target, &symbol),
            2,
            "[{target}] both pointer locals are live across the array \
             allocation and must both be in the map. A dropped slot index does \
             not warn, does not fail to compile and does not change the shape \
             of the IR — it just removes a root:\n{}",
            function_slice(&ir, &symbol)
        );
    }
}

// ---------------------------------------------------------------------------
// 7 / 8. Scalar-replaced slots (#6968 / #6997)
//    (shadow counterparts: `scalar_replaced_object_field_holding_a_heap_value_
//    is_bound` and `numeric_only_scalar_replaced_object_emits_no_rooting`)
// ---------------------------------------------------------------------------

/// Scalar replacement deletes an object literal and keeps one entry-block
/// alloca per field. Those allocas belong to no HIR local, so the pre-lowering
/// pointer analysis cannot see them — which is how #6968 shipped a field
/// holding a heap value with no root at all.
///
/// Stated against a structurally identical numeric-only literal: same local,
/// same field count, same reads. The difference must be exactly one root slot
/// and a non-empty map.
///
/// **Sabotage** — `expr/scalar_slot_root.rs`, `root_scalar_replaced_slot`'s
/// `root_entry_alloca` call removed, which is #6968 reintroduced exactly: RED,
/// "heap literal has 3, numeric control 3" — the difference vanished.
#[test]
fn a_scalar_replaced_field_holding_a_heap_value_is_a_native_root() {
    for target in NATIVE_TARGETS {
        let _pin = NativeRootsPin::native();

        let heap = entry_module(
            "m7_scalar_heap.ts",
            vec![
                let_stmt(
                    1,
                    "o",
                    Expr::Object(vec![
                        ("a".to_string(), heap_value()),
                        ("b".to_string(), Expr::Number(2.0)),
                    ]),
                ),
                console_log(vec![field_get(1, "a"), field_get(1, "b")]),
            ],
        );
        let heap_ir = native_ir(&heap, target, true);
        let heap_main = function_slice(&heap_ir, "main");

        let numeric = entry_module(
            "m7_scalar_numeric.ts",
            vec![
                let_stmt(
                    1,
                    "o",
                    Expr::Object(vec![
                        ("a".to_string(), Expr::Number(1.0)),
                        ("b".to_string(), Expr::Number(2.0)),
                    ]),
                ),
                console_log(vec![field_get(1, "a"), field_get(1, "b")]),
            ],
        );
        let numeric_ir = native_ir(&numeric, target, true);
        let numeric_main = function_slice(&numeric_ir, "main");

        assert!(
            root_allocas(heap_main) > root_allocas(numeric_main),
            "[{target}] the pointer-capable scalar-replaced field must take a \
             root slot the pointer analysis could not have predicted: heap \
             literal has {}, numeric control {}",
            root_allocas(heap_main),
            root_allocas(numeric_main)
        );
        assert!(
            map_max_roots(&assembly_for(&heap_ir, target), target, "main") > 0,
            "[{target}] and the collector must be able to find it"
        );
        assert_eq!(
            map_max_roots(&assembly_for(&numeric_ir, target), target, "main"),
            0,
            "[{target}] CONTROL: the numeric-only twin puts nothing in the map, \
             so the root above is attributable to the heap field rather than to \
             anything else `main` contains"
        );
    }
}

/// The other side of the same gate (#6997): a literal whose every field is a
/// number must pay nothing.
///
/// This is the assertion shape that was passing vacuously before #7502 — the
/// shadow-pinned original counted `js_shadow_slot_bind` calls, of which the
/// native lowering emits zero for every program. Here it is a claim about
/// `ptr addrspace(1)` allocas and about the emitted map, and it is paired with
/// the heap-valued literal in the same test so that a lowering which roots
/// nothing cannot satisfy it.
///
/// **Sabotage** — `expr/scalar_slot_root.rs`, the
/// `expr_is_known_non_pointer_shadow_value` early-out removed so every
/// scalar-replaced field reserves a slot: RED, the numeric-only literal's map
/// went **0 → 2 roots**. That number is the whole answer to "is this negative
/// assertion vacuous under its lowering": it is not.
#[test]
fn a_numeric_only_scalar_replaced_literal_pays_no_native_rooting() {
    for target in NATIVE_TARGETS {
        let _pin = NativeRootsPin::native();

        let numeric = entry_module(
            "m8_numeric_only.ts",
            vec![
                let_stmt(
                    1,
                    "p",
                    Expr::Object(vec![
                        ("x".to_string(), Expr::Number(1.0)),
                        ("y".to_string(), Expr::Number(2.0)),
                    ]),
                ),
                console_log(vec![field_get(1, "x"), field_get(1, "y")]),
            ],
        );
        let numeric_ir = native_ir(&numeric, target, true);
        let numeric_asm = assembly_for(&numeric_ir, target);
        assert_eq!(
            map_max_roots(&numeric_asm, target, "main"),
            0,
            "[{target}] a scalar-replaced literal with only numeric fields must \
             not put anything in the GC map"
        );

        // NON-VACUITY, in the same test: swap one field for a heap value and
        // the same measurement must move.
        let heap = entry_module(
            "m8_one_heap_field.ts",
            vec![
                let_stmt(
                    1,
                    "p",
                    Expr::Object(vec![
                        ("x".to_string(), heap_value()),
                        ("y".to_string(), Expr::Number(2.0)),
                    ]),
                ),
                console_log(vec![field_get(1, "x"), field_get(1, "y")]),
            ],
        );
        let heap_ir = native_ir(&heap, target, true);
        assert!(
            map_max_roots(&assembly_for(&heap_ir, target), target, "main") > 0,
            "[{target}] CONTROL: one heap-valued field must produce a root, or \
             the zero above is measuring nothing"
        );
    }
}
