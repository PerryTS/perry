//! #7469 — array locals whose GC element layout is known **at the allocation
//! site**, so the per-store pointer-mask bookkeeping can be declared once
//! instead of maintained per push.
//!
//! An `out = []` + `out.push(objectLiteral)` loop pays, on every iteration, a
//! `js_gc_note_slot_layout` call that walks into a thread-local hashmap to set
//! one bit of a per-array bitmap, plus a `js_array_note_numeric_write` call
//! that clears an already-clear flag. Worse, the `LAYOUT_SLOT_MASKS` entry it
//! creates makes the map non-empty for the rest of the program, so the
//! `is_empty()` fast-out in `layout_forget_object` — probed on **every** object
//! allocation — starts hashing instead. Both disappear if the array's element
//! layout is declared once, at allocation, as all-pointer.
//!
//! # What this collector proves
//!
//! For a local `id` it admits the declaration only when ALL of the following
//! hold over the whole region:
//!
//! 1. **Exactly one binding, and it is an array literal.** `id` is bound by a
//!    `Stmt::Let { init: Some(Expr::Array(elems)) }`, and every element of
//!    `elems` is an allocation site
//!    ([`crate::expr::expr_produces_fresh_heap_allocation`]); the empty
//!    literal — the shape this exists for — passes vacuously.
//! 2. **No other write to the binding.** No `LocalSet`, no `Update`. A rebind
//!    would point the local at an array the declaration never covered, and the
//!    elided stores would then be describing the wrong object.
//! 3. **Every store into it is a push of an allocation, and there is at least
//!    one.** Every `Expr::ArrayPush { array_id: id }` pushes a fresh
//!    allocation; no `Expr::ArrayPushSpread`, no `Expr::IndexSet` whose object
//!    is `LocalGet(id)` (an indexed store can jump past `length`, a different
//!    claim than "append"), no other in-place array mutation.
//!
//!    The "at least one" is not decoration. Deforestation
//!    (`perry-transform/src/deforest`) rewrites a producer's call site to
//!    `const all = []; f(args, all);` — an array literal binding whose stores
//!    all live in another region. Declaring THAT array would be a blind bet on
//!    a callee this collector cannot see, and if the callee pushes numbers the
//!    bet loses (the array is stuck on a conservative scan and has lost its
//!    raw-f64 layout). Requiring a proven push in the same region refuses it.
//! 4. **No closure capture, no box, no module global.** Those route stores
//!    through paths this region cannot see and cannot gate.
//!
//! # Shapes this deliberately does not reach
//!
//! The deforested PRODUCER is the mirror image of the case above: its
//! `const out = []` is gone (the accumulator arrives as the
//! `__deforest_out` parameter) and only the pushes remain, so there is no
//! allocation site in the region to declare at. Both halves of a deforested
//! producer/consumer pair are therefore out of scope; the analysis fires on
//! arrays built and kept inside one region.
//!
//! # What the proof is and is not responsible for
//!
//! It is NOT the soundness argument for the elided store. Declaring
//! all-pointer is conservative for the collector in the only direction that
//! matters (`GC_LAYOUT_ALL_POINTERS` visits `0..length`, exactly what
//! `GC_LAYOUT_UNKNOWN` visits; a non-pointer slot is re-validated and
//! rejected). What makes the elision sound is the **header test emitted at
//! every elided store** (`expr/array_push.rs`): the inline push happens only
//! when the array's `_reserved` word still reads
//! `SIDE_MASK | ALL_POINTERS` with both raw-f64 bits clear, and any push that
//! fails the test falls through to `js_array_push_f64`, which notes the slot
//! exactly as it always did. That is what covers a declaration the RUNTIME
//! later revokes — `rebuild_array_layout` (sort/splice) installs a precise
//! mask, `js_array_is_numeric_f64_layout` can re-publish a still-empty array
//! as `POINTER_FREE` — which no static proof over this region could see.
//!
//! This proof's job is therefore *profitability*: it picks arrays where the
//! declaration will stick, so the fast path is actually taken.
//!
//! That is also the standard the kill list below is written to. It enumerates
//! the write forms that reach an array local in lowered HIR — rebinds, keyed
//! stores (`IndexSet` / `IndexUpdate` / `PropertySet` / `PropertyUpdate` /
//! `PutValueSet`), and the in-place `Array*` mutators — and a form it MISSES
//! cannot produce a wrong layout: the missed store either carries its own note
//! (every un-elided store path does) or fails the header test and routes
//! through `js_array_push_f64`. The cost of a miss is that the array is
//! downgraded to a conservative scan earlier than predicted, which for an
//! all-pointer array is the same set of slots the precise mask would have
//! visited.

use std::collections::{HashMap, HashSet};

use perry_hir::{Expr, Stmt};

/// Locals eligible for an at-allocation all-pointer element-layout
/// declaration. See the module docs for the four admission terms.
pub(crate) fn collect_all_pointer_array_locals(
    stmts: &[Stmt],
    boxed_vars: &HashSet<u32>,
    module_globals: &HashMap<u32, String>,
) -> HashSet<u32> {
    let mut candidates: HashSet<u32> = HashSet::new();
    walk_stmts(stmts, &mut |stmt| {
        if let Stmt::Let {
            id,
            init: Some(Expr::Array(elements)),
            ..
        } = stmt
        {
            if boxed_vars.contains(id) || module_globals.contains_key(id) {
                return;
            }
            if elements
                .iter()
                .all(crate::expr::expr_produces_fresh_heap_allocation)
            {
                candidates.insert(*id);
            }
        }
    });
    if candidates.is_empty() {
        return candidates;
    }

    let mut killed: HashSet<u32> = HashSet::new();
    let mut pushed: HashSet<u32> = HashSet::new();
    super::scalar_method_dispatch::for_each_expr_in_stmts(stmts, &mut |expr| match expr {
        // Rebinding the local points it at an array the declaration never
        // covered.
        Expr::LocalSet(id, _) => {
            killed.insert(*id);
        }
        Expr::Update { id, .. } => {
            killed.insert(*id);
        }
        // A keyed store is not an append: `a[10] = x` on a length-2 array is a
        // different layout claim than the push protocol's
        // `slot_index == length`, and `a.length = 0` is not a store at all.
        // `PutValueSet` is the form a lowered `a[i] = x` actually takes when
        // the receiver is not statically an array-typed local.
        Expr::IndexSet { object, .. }
        | Expr::IndexUpdate { object, .. }
        | Expr::PropertySet { object, .. }
        | Expr::PropertyUpdate { object, .. } => {
            if let Expr::LocalGet(id) = object.as_ref() {
                killed.insert(*id);
            }
        }
        Expr::PutValueSet {
            target, receiver, ..
        } => {
            for expr in [target, receiver] {
                if let Expr::LocalGet(id) = expr.as_ref() {
                    killed.insert(*id);
                }
            }
        }
        Expr::ArraySort { array, .. } => {
            // `sort` routes through `rebuild_array_layout`, which installs a
            // PRECISE mask in place of the declaration.
            if let Expr::LocalGet(id) = array.as_ref() {
                killed.insert(*id);
            }
        }
        // Every other in-place array mutation reaches slots that are not the
        // append position (`unshift`/`splice`/`copyWithin` shift or replace,
        // `pop`/`shift` shrink), and lowers through runtime helpers this
        // region cannot gate. None of them is unsound under a declaration —
        // the header test still fronts every elided store — but each makes
        // the declaration unlikely to survive, so they are refused.
        Expr::ArrayPushSpread { array_id, .. }
        | Expr::ArrayUnshift { array_id, .. }
        | Expr::ArraySplice { array_id, .. }
        | Expr::ArrayCopyWithin { array_id, .. } => {
            killed.insert(*array_id);
        }
        Expr::ArrayPop(array_id) | Expr::ArrayShift(array_id) => {
            killed.insert(*array_id);
        }
        Expr::ArrayPush { array_id, value } => {
            if crate::expr::expr_produces_fresh_heap_allocation(value) {
                pushed.insert(*array_id);
            } else {
                killed.insert(*array_id);
            }
        }
        // A captured array is stored into from a region this collector is not
        // analysing, through a lowering path that cannot carry the gate.
        Expr::Closure { captures, .. } => {
            killed.extend(captures.iter().copied());
        }
        _ => {}
    });

    candidates.retain(|id| !killed.contains(id) && pushed.contains(id));
    candidates
}

/// Statement-level descent. `for_each_expr_in_stmts` visits expressions, not
/// `Stmt::Let` ids, so the binding scan needs its own walk. It descends into
/// every nested statement position; a `Let` inside a closure body belongs to
/// that closure's own region (which collects its own facts) and its id is
/// distinct, so seeing it here is harmless — the kill walk above descends into
/// closures too, and every use of such an id is inside the closure body it is
/// scoped to.
/// #7598 — the subset of [`collect_all_pointer_array_locals`]-admitted locals
/// whose pushed objects should be born TENURED in old-gen.
///
/// The all-pointer terms already prove the accumulator *shape* (one binding,
/// never rebound, every store a push of a fresh allocation, no captures /
/// boxes / globals). What they do not prove is *cohort lifetime*, and that is
/// the entire pretenuring bet: an object born old that dies young sits in
/// old-gen until a full reclaim. The discriminator is loop position:
///
/// - the `let` must sit at **loop depth 0** — a per-iteration accumulator
///   (`for (…) { const keep = []; … keep.push(x) … }`) dies every iteration,
///   and pretenuring it floods old-gen with garbage at allocation rate;
/// - every push must sit at **depth ≥ 1** — the cohort accumulates across
///   iterations, so it is live for the remainder of the loop by construction.
///
/// This is deliberately NOT a proof the array outlives the function; a
/// depth-0 accumulator that is dropped at function exit still pretenures, and
/// its cohort is then reclaimed by the proportional-band full cycles (#7596)
/// instead of dying free in the nursery. That trade is measured, not assumed —
/// see the adversarial arm in the PR.
pub(crate) fn collect_pretenure_accumulator_locals(
    stmts: &[Stmt],
    all_pointer_admitted: &HashSet<u32>,
) -> HashSet<u32> {
    if all_pointer_admitted.is_empty() {
        return HashSet::new();
    }
    let mut decl_depth: HashMap<u32, u32> = HashMap::new();
    let mut push_depths: HashMap<u32, Vec<u32>> = HashMap::new();
    scan_depths(stmts, 0, &mut decl_depth, &mut push_depths);
    all_pointer_admitted
        .iter()
        .copied()
        .filter(|id| {
            decl_depth.get(id) == Some(&0)
                && push_depths
                    .get(id)
                    .is_some_and(|ds| !ds.is_empty() && ds.iter().all(|&d| d >= 1))
        })
        .collect()
}

/// Depth-attributed scan: bindings and pushes recorded with their enclosing
/// real-loop count. Expressions directly attached to a statement (conditions,
/// initializers, the statement expression itself) are scanned deeply at that
/// statement's depth — a push nested inside a larger expression still counts,
/// at the depth of the statement carrying it. `for_each_expr` descends into
/// closure bodies too; a push on the same id from inside a closure records the
/// enclosing statement's depth, which is harmless — a captured id was already
/// refused by the all-pointer capture kill.
fn scan_depths(
    stmts: &[Stmt],
    depth: u32,
    decl_depth: &mut HashMap<u32, u32>,
    push_depths: &mut HashMap<u32, Vec<u32>>,
) {
    let mut record = |expr: &Expr, at: u32, push_depths: &mut HashMap<u32, Vec<u32>>| {
        super::scalar_method_dispatch::for_each_expr(expr, &mut |e| {
            if let Expr::ArrayPush { array_id, .. } = e {
                push_depths.entry(*array_id).or_default().push(at);
            }
        });
    };
    for s in stmts {
        match s {
            Stmt::Let { id, init, .. } => {
                if let Some(Expr::Array(_)) = init {
                    // First binding wins; a rebind was refused upstream.
                    decl_depth.entry(*id).or_insert(depth);
                }
                if let Some(init) = init {
                    record(init, depth, push_depths);
                }
            }
            Stmt::Expr(expr) | Stmt::Throw(expr) | Stmt::Return(Some(expr)) => {
                record(expr, depth, push_depths);
            }
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                record(condition, depth, push_depths);
                scan_depths(then_branch, depth, decl_depth, push_depths);
                if let Some(eb) = else_branch {
                    scan_depths(eb, depth, decl_depth, push_depths);
                }
            }
            Stmt::While { condition, body } | Stmt::DoWhile { condition, body } => {
                record(condition, depth + 1, push_depths);
                scan_depths(body, depth + 1, decl_depth, push_depths);
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(init) = init {
                    // The initializer runs once, outside the iteration.
                    scan_depths(
                        std::slice::from_ref(init.as_ref()),
                        depth,
                        decl_depth,
                        push_depths,
                    );
                }
                if let Some(condition) = condition {
                    record(condition, depth + 1, push_depths);
                }
                if let Some(update) = update {
                    record(update, depth + 1, push_depths);
                }
                scan_depths(body, depth + 1, decl_depth, push_depths);
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                scan_depths(body, depth, decl_depth, push_depths);
                if let Some(c) = catch {
                    scan_depths(&c.body, depth, decl_depth, push_depths);
                }
                if let Some(fin) = finally {
                    scan_depths(fin, depth, decl_depth, push_depths);
                }
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                record(discriminant, depth, push_depths);
                for c in cases {
                    if let Some(test) = &c.test {
                        record(test, depth, push_depths);
                    }
                    scan_depths(&c.body, depth, decl_depth, push_depths);
                }
            }
            Stmt::Labeled { body, .. } => {
                scan_depths(
                    std::slice::from_ref(body.as_ref()),
                    depth,
                    decl_depth,
                    push_depths,
                );
            }
            _ => {}
        }
    }
}

fn walk_stmts<'a>(stmts: &'a [Stmt], f: &mut impl FnMut(&'a Stmt)) {
    for s in stmts {
        f(s);
        match s {
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                walk_stmts(then_branch, f);
                if let Some(eb) = else_branch {
                    walk_stmts(eb, f);
                }
            }
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => walk_stmts(body, f),
            Stmt::For { init, body, .. } => {
                if let Some(init) = init {
                    walk_stmts(std::slice::from_ref(init.as_ref()), f);
                }
                walk_stmts(body, f);
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                walk_stmts(body, f);
                if let Some(c) = catch {
                    walk_stmts(&c.body, f);
                }
                if let Some(fin) = finally {
                    walk_stmts(fin, f);
                }
            }
            Stmt::Switch { cases, .. } => {
                for c in cases {
                    walk_stmts(&c.body, f);
                }
            }
            Stmt::Labeled { body, .. } => walk_stmts(std::slice::from_ref(body.as_ref()), f),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use perry_hir::types::Type;

    fn let_array(id: u32, elements: Vec<Expr>) -> Stmt {
        Stmt::Let {
            id,
            name: format!("a{id}"),
            ty: Type::Any,
            mutable: true,
            init: Some(Expr::Array(elements)),
        }
    }

    fn push(id: u32, value: Expr) -> Stmt {
        Stmt::Expr(Expr::ArrayPush {
            array_id: id,
            value: Box::new(value),
        })
    }

    fn object_literal() -> Expr {
        Expr::Object(vec![("v".to_string(), Expr::Integer(1))])
    }

    fn collect(stmts: &[Stmt]) -> HashSet<u32> {
        collect_all_pointer_array_locals(stmts, &HashSet::new(), &HashMap::new())
    }

    #[test]
    fn empty_literal_plus_object_pushes_is_admitted() {
        let stmts = vec![let_array(1, vec![]), push(1, object_literal())];
        assert!(collect(&stmts).contains(&1));
    }

    #[test]
    fn literal_of_object_elements_is_admitted() {
        let stmts = vec![
            let_array(1, vec![object_literal(), object_literal()]),
            push(1, object_literal()),
        ];
        assert!(collect(&stmts).contains(&1));
    }

    #[test]
    fn a_numeric_element_in_the_literal_is_refused() {
        let stmts = vec![
            let_array(1, vec![object_literal(), Expr::Integer(3)]),
            push(1, object_literal()),
        ];
        assert!(!collect(&stmts).contains(&1));
    }

    /// HIR rewrites a closed-shape object literal into
    /// `New { class_name: "__AnonShape_<hash>" }` before codegen runs, so the
    /// `new` form IS the object-literal push loop this analysis targets.
    #[test]
    fn a_new_expression_push_is_admitted() {
        let stmts = vec![
            let_array(1, vec![]),
            push(
                1,
                Expr::New {
                    class_name: "__AnonShape_deadbeef".to_string(),
                    args: vec![],
                    type_args: vec![],
                    byte_offset: 0,
                    cap_args_appended: 0,
                },
            ),
        ];
        assert!(collect(&stmts).contains(&1));
    }

    /// The deforested CONSUMER shape: an array literal binding whose stores all
    /// live in a callee. Declaring it would be a bet on a region this collector
    /// cannot see.
    #[test]
    fn a_literal_with_no_push_in_this_region_is_refused() {
        let stmts = vec![
            let_array(1, vec![]),
            Stmt::Expr(Expr::Call {
                callee: Box::new(Expr::FuncRef(7)),
                args: vec![Expr::LocalGet(1)],
                type_args: vec![],
                byte_offset: 0,
            }),
        ];
        assert!(!collect(&stmts).contains(&1));
    }

    #[test]
    fn one_unproven_push_refuses_the_whole_local() {
        let stmts = vec![
            let_array(1, vec![]),
            push(1, object_literal()),
            push(1, Expr::Integer(7)),
        ];
        assert!(!collect(&stmts).contains(&1));
    }

    #[test]
    fn a_rebind_refuses_the_local() {
        let stmts = vec![
            let_array(1, vec![]),
            push(1, object_literal()),
            Stmt::Expr(Expr::LocalSet(1, Box::new(Expr::Array(vec![])))),
        ];
        assert!(!collect(&stmts).contains(&1));
    }

    #[test]
    fn an_indexed_store_refuses_the_local() {
        let stmts = vec![
            let_array(1, vec![]),
            push(1, object_literal()),
            Stmt::Expr(Expr::IndexSet {
                object: Box::new(Expr::LocalGet(1)),
                index: Box::new(Expr::Integer(4)),
                value: Box::new(object_literal()),
            }),
        ];
        assert!(!collect(&stmts).contains(&1));
    }

    /// `out[0] = {...}` on an array-typed local lowers to `PutValueSet`, not
    /// `IndexSet` — the form the kill list originally missed, caught by the
    /// probe in the #7469 PR.
    #[test]
    fn a_put_value_store_refuses_the_local() {
        let stmts = vec![
            let_array(1, vec![]),
            push(1, object_literal()),
            Stmt::Expr(Expr::PutValueSet {
                target: Box::new(Expr::LocalGet(1)),
                key: Box::new(Expr::Integer(0)),
                value: Box::new(object_literal()),
                receiver: Box::new(Expr::LocalGet(1)),
                strict: false,
            }),
        ];
        assert!(!collect(&stmts).contains(&1));
    }

    #[test]
    fn a_length_write_refuses_the_local() {
        let stmts = vec![
            let_array(1, vec![]),
            push(1, object_literal()),
            Stmt::Expr(Expr::PropertySet {
                object: Box::new(Expr::LocalGet(1)),
                property: "length".to_string(),
                value: Box::new(Expr::Integer(0)),
            }),
        ];
        assert!(!collect(&stmts).contains(&1));
    }

    #[test]
    fn a_spread_push_refuses_the_local() {
        let stmts = vec![
            let_array(1, vec![]),
            push(1, object_literal()),
            Stmt::Expr(Expr::ArrayPushSpread {
                array_id: 1,
                source: Box::new(Expr::LocalGet(2)),
            }),
        ];
        assert!(!collect(&stmts).contains(&1));
    }

    #[test]
    fn a_boxed_or_module_global_binding_is_refused() {
        let stmts = vec![let_array(1, vec![]), push(1, object_literal())];
        let boxed: HashSet<u32> = [1].into_iter().collect();
        assert!(!collect_all_pointer_array_locals(&stmts, &boxed, &HashMap::new()).contains(&1));
        let globals: HashMap<u32, String> = [(1, "g".to_string())].into_iter().collect();
        assert!(!collect_all_pointer_array_locals(&stmts, &HashSet::new(), &globals).contains(&1));
    }

    /// Pushes inside a nested closure lower through a path that carries no
    /// header gate, so a captured array must not be declared.
    #[test]
    fn a_captured_local_is_refused() {
        let stmts = vec![
            let_array(1, vec![]),
            push(1, object_literal()),
            Stmt::Expr(Expr::Closure {
                func_id: 9,
                params: vec![],
                return_type: Type::Any,
                body: vec![push(1, object_literal())],
                captures: vec![1],
                mutable_captures: vec![],
                captures_this: false,
                captures_new_target: false,
                enclosing_class: None,
                is_arrow: true,
                is_async: false,
                is_generator: false,
                is_strict: false,
            }),
        ];
        assert!(!collect(&stmts).contains(&1));
    }

    // ---- #7598 pretenure-accumulator loop-position terms -------------------

    fn while_loop(body: Vec<Stmt>) -> Stmt {
        Stmt::While {
            condition: Expr::Bool(true),
            body,
        }
    }

    fn collect_pretenure(stmts: &[Stmt]) -> HashSet<u32> {
        let admitted = collect(stmts);
        collect_pretenure_accumulator_locals(stmts, &admitted)
    }

    /// The target shape: `const out = []` outside every loop, pushes inside.
    #[test]
    fn pretenure_admits_the_outer_accumulator_inner_push_shape() {
        let stmts = vec![
            let_array(1, vec![]),
            while_loop(vec![push(1, object_literal())]),
        ];
        assert!(collect_pretenure(&stmts).contains(&1));
    }

    /// The per-iteration accumulator dies every iteration; pretenuring it
    /// would flood old-gen with garbage at allocation rate. This is
    /// push_bench's exact shape and MUST stay refused.
    #[test]
    fn pretenure_refuses_a_loop_local_accumulator() {
        let stmts = vec![while_loop(vec![
            let_array(1, vec![]),
            push(1, object_literal()),
        ])];
        assert!(!collect_pretenure(&stmts).contains(&1));
    }

    /// A one-shot push outside any loop has no cohort to speak of.
    #[test]
    fn pretenure_refuses_pushes_outside_loops() {
        let stmts = vec![let_array(1, vec![]), push(1, object_literal())];
        assert!(!collect_pretenure(&stmts).contains(&1));
    }

    /// One depth-0 push alongside loop pushes: refused — every push must be
    /// inside a loop for the cohort claim to hold.
    #[test]
    fn pretenure_refuses_mixed_depth_pushes() {
        let stmts = vec![
            let_array(1, vec![]),
            push(1, object_literal()),
            while_loop(vec![push(1, object_literal())]),
        ];
        assert!(!collect_pretenure(&stmts).contains(&1));
    }

    /// A push nested inside a larger expression still counts, at the depth of
    /// the statement carrying it — the depth scan is not statement-position
    /// only.
    #[test]
    fn pretenure_sees_a_push_nested_in_an_expression() {
        let stmts = vec![
            let_array(1, vec![]),
            while_loop(vec![Stmt::Expr(Expr::BooleanCoerce(Box::new(
                Expr::ArrayPush {
                    array_id: 1,
                    value: Box::new(object_literal()),
                },
            )))]),
        ];
        assert!(collect_pretenure(&stmts).contains(&1));
    }

    /// The all-pointer terms remain a prerequisite: a local they refuse
    /// (rebind) is never pretenured, whatever its loop position.
    #[test]
    fn pretenure_requires_all_pointer_admission() {
        let stmts = vec![
            let_array(1, vec![]),
            Stmt::Expr(Expr::LocalSet(1, Box::new(Expr::Array(vec![])))),
            while_loop(vec![push(1, object_literal())]),
        ];
        assert!(!collect_pretenure(&stmts).contains(&1));
    }
}
