//! repsel #7480 / #5093: the **element-shape versioned loop clone** — the
//! first consumer of the per-array homogeneous element-shape invariant
//! (`perry-runtime/src/array/element_shape.rs`, #7496, matrix #7608).
//!
//! ## The shape, and why it costs what it costs
//!
//! ```text
//! for (let j = 0; j < n; j++) sum += keep[j].v;
//! ```
//!
//! `keep[j]` yields an untyped `JSValue`, so `.v` re-enters the field-read
//! diamond that repsel 3b deleted for proven locals. #7480 measured the pure
//! shape at 6.2× node and localized the cost precisely: not out-of-line guard
//! *calls*, but **stacked inline diamonds** — an element-read tier
//! (tag / handle-band / `GC_TYPE_ARRAY` / descriptor tests, a branch and a
//! phi) feeding a field-read precheck (a volatile gate load, tag and band
//! tests again, then seven dependent header loads), every iteration.
//!
//! Almost every one of those predicates is answered *once, for the whole
//! array*, by the element-shape invariant. This module hoists that one
//! question into a preheader and clones the body against the answer.
//!
//! ## Mid-loop revocation — the chosen mechanism, and its failure mode
//!
//! The invariant is construction-maintained and self-healing, but a store
//! inside the loop body (or inside anything the body calls) can revoke it
//! mid-iteration, and a specialized body reading a revoked array is a
//! **miscompile**, not a slow path.
//!
//! Of the three options in the design space — restrict the body, re-check per
//! back-edge, or deopt on runtime invalidation — this ships the **first**:
//! *the clone is admitted only for bodies that provably cannot revoke*. That
//! is enforced twice, at two different levels, and the second enforcement is
//! the load-bearing one:
//!
//! 1. **By shape (the matcher).** The body must be a single
//!    `acc = <pure numeric>` statement over tracked `arr[i].field` reads,
//!    numeric locals, literals and pure arithmetic / `Math` — or, since
//!    #7771, that statement preceded by exactly one `const r = arr[i]`
//!    binding whose only uses are tracked `r.field` reads (#7766: the shape
//!    the `for…of` desugar emits, and the form a parameter array reaches the
//!    clone through — the binding is virtual in the fast clone: its `Let`
//!    emits nothing and the reads lower through the fact), or, since #10123,
//!    by exactly one `const d = i % m` binding whose only use is as the
//!    element index (equally virtual: its `Let` emits one `srem`). No store of
//!    any kind, no call, no closure, no `await`, no update other than the
//!    counter's.
//! 2. **By construction (the lowering).** After the fast clone is emitted,
//!    every one of its blocks is scanned for a GC-unsafe call
//!    (`LlBlock::contains_gc_unsafe_call`). If ANY call survived — because
//!    some lowering path we did not predict emitted one — the deref block
//!    branches *unconditionally* to the slow clone and the fast blocks are
//!    left as unreachable code. A clone whose call-freeness is unproven is
//!    never entered.
//!
//! Call-freeness is exactly the right property, because **every** way to
//! revoke the invariant is a runtime call:
//!
//! | revocation | funnel | is a call |
//! |---|---|---|
//! | `arr[i] = x` (different class / non-object / hole) | `gc::layout_note_slot` → `note_element_store` | yes |
//! | `arr.push` / `pop` / `shift` / `splice` / `arr.length = n` | length change ⇒ `verified_len` mismatch on next query | yes |
//! | `delete arr[i]` | `TAG_HOLE` store through the same funnel | yes |
//! | `Object.defineProperty(arr, i, …)` | `OBJ_FLAG_ARRAY_DESCRIPTORS` ⇒ `array_admits_element_proof` | yes |
//! | prototype surgery on the element class | `invalidate_all_element_shapes` | yes |
//! | a GC moving the array | `layout_transfer` → `transfer_element_shape` | needs an allocation |
//!
//! Codegen's *inline* element store is the one path that can skip the note,
//! and it does so only when the array is statically proven numeric and
//! pointer-free — which an element-shape array (whose slots are NaN-boxed
//! pointers) can never be. It is also excluded by the matcher anyway, which
//! admits no stores at all.
//!
//! **Failure mode of this choice:** it is conservative, never unsound. A loop
//! that writes anything, calls anything, or reads a field the analysis cannot
//! type simply does not get a clone and runs exactly as it does today. The
//! risk that remains is a *silent loss of the optimization* — a lowering
//! change that starts emitting a call inside a body the matcher still admits
//! would make the whole clone dead code with no test failing. That is why
//! `element_shape_loop_tests.rs` asserts the fast blocks appear in the emitted
//! IR AND that the fast clone contains no `call` at all: the IR census is the
//! regression gate for the optimization, and the call-free scan is the
//! regression gate for correctness.
//!
//! The rejected options, for the record. A per-back-edge re-check (one load
//! and compare of the header bit plus the shape id) is cheap, but it does not
//! actually discharge the hazard: the bit can be revoked *between* the check
//! and the read within one iteration, and the residual per-element facts (see
//! `expr::element_shape_guard`) would still be needed. Guard-at-entry plus
//! runtime invalidation deopt needs an on-stack-replacement mechanism Perry
//! does not have.
//!
//! ## The SHAPE-keyed arm (#10123)
//!
//! Everything above keys on a compile-time CLASS, which excluded the one array
//! shape record-processing code is actually written against: `JSON.parse`'d
//! objects are `class_id == 0` with an ordinary birth ShapeId, and the class
//! resolver has nothing to resolve for a `rows: any` receiver. The clone
//! therefore never fired for a parsed record array — the measured case it
//! exists for.
//!
//! The second arm proves the same thing about a different identity: the
//! preheader asks `js_array_ensure_element_shape_ordinary` for the exact
//! ordinary ShapeId every element carries, then asks
//! `js_shape_ordinary_inline_slot_for_key` where each tracked property sits in
//! THAT shape. Both answers are loop-invariant values, so the clone's read is
//! still one bare offset load.
//!
//! **The revocation argument is unchanged**, because it never mentioned
//! classes: every funnel in the table above retires a shape-keyed proof
//! exactly as it retires a class-keyed one (`note_element_store` compares the
//! record, which for a class-0 proof is the exact ShapeId; a length change
//! fails `verified_len`; prototype surgery bumps the generation). Call-free is
//! still the whole admission test.
//!
//! Two things ARE different, and both are narrowings:
//!
//! * the per-element residual drops `GC_OBJ_TYPED_LAYOUT_INTACT`. A parsed
//!   record never has that bit — `object/json_construction.rs` finishes every
//!   record with `layout_init_pointer_free` or `layout_mark_unknown`, and both
//!   clear it — so keeping it would have side-exited on the FIRST element of
//!   every loop while every IR-census assertion still passed. What the bit
//!   bought was "this slot holds a raw `double`"; the shape arm buys the same
//!   claim per read, from the value, with the Number-tag test
//!   `emit_js_value_is_number` and a side exit to the slow clone.
//! * the index grammar widens to `arr[k]` and `const d = j % m; arr[d]` — the
//!   `repeat` and `sequential` shapes of real access code. Each carries its
//!   own preheader bounds obligation (`length > k`, `m <= length`), so the
//!   clone still pays no per-read bounds test, and the matcher admits exactly
//!   ONE index form per loop so a fact can never be consulted for a spelling
//!   whose obligation was not discharged.
//!
//! ## Extension plan (write-up for #5093 / #7480)
//!
//! The clone still pays a residual per-element check — the exact ShapeId, the
//! per-object descriptor flag and (class arm) the typed-layout intact bit —
//! because the array-level invariant deliberately does not cover them. Folding
//! them into `element_identity_of_bits` would make the reads bare, but it
//! needs an invalidation surface for `delete elem.f`, `defineProperty(elem)`
//! and typed-layout downgrade that does not exist today; #7496 kept the
//! maintenance matrix small precisely by not opening that surface. That is the
//! natural next slice, and it should land the way #7496 did: invariant first,
//! matrix second, consumer third.

use anyhow::Result;
use perry_hir::Stmt;

use super::loops::{
    emit_js_value_is_number, local_bound_is_loop_invariant, local_has_readable_slot,
    loop_counter_bounds_are_safe, loop_counter_entry_i32_range_is_safe, lower_for_after_init,
    lower_for_after_init_with_i32_bound, CLASS_FIELD_LOOP_CLASS_DENYLIST,
    CLASS_FIELD_LOOP_PROP_DENYLIST,
};
use crate::expr::{lower_expr, FnCtx};
use crate::types::{DOUBLE, I1, I32, I64};

/// Loop bound: a literal, a loop-invariant local / module global that is
/// materialized to i32 once in the preheader, or the tracked array's own
/// `length`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ElementShapeLoopBound {
    Constant(i64),
    Local(u32),
    /// `j < arr.length` where `arr` is the array the body reads. The guard
    /// already loads that word, so this arm costs nothing to materialize —
    /// see [`crate::expr::element_shape_guard::ElementShapeLoopTripCount`].
    ///
    /// Carries the receiver id because the bound is matched before the body
    /// names the array; the caller cross-checks the two.
    ArrayLength(u32),
}

/// #10123: which identity the clone keys the elements on.
#[derive(Debug)]
enum ElementShapeIdentity {
    /// The original arm. A compile-time class supplies the id the preheader
    /// compares against, the canonical keys global the expected ShapeId is
    /// loaded from, and a packed slot index per property.
    Class {
        class_name: String,
        expected_class_id: u32,
        keys_global_name: String,
        /// property name -> packed slot index.
        packed_fields: std::collections::BTreeMap<String, u32>,
        /// The native-region E1--E5 proof already establishes every element's
        /// exact class for this array's whole lifetime. When true, the
        /// preheader need not rebuild the weaker runtime invariant by scanning
        /// the array.
        statically_class_proven: bool,
        /// The same contained group proof also established that every requested
        /// field remains a raw-f64 slot, so per-object residual checks are
        /// redundant inside the call-free clone.
        statically_layout_proven: bool,
    },
    /// #10123: an `any`-typed array of plain objects — canonically the result
    /// of `JSON.parse`. There is no class and no compile-time layout; the
    /// preheader asks the runtime for the exact ordinary ShapeId every element
    /// carries and for each tracked property's inline slot in that shape, and
    /// the per-element residual adds a Number-tag test on the loaded word.
    Shape,
}

/// #10123: the index spelling the whole body uses.
///
/// One form per loop, deliberately: each carries its own preheader bounds
/// obligation (`expr::element_shape_guard::ElementShapeIndexBound`), so a body
/// mixing `arr[j]` with `arr[7]` would need both discharged and both matched
/// at every read. The matcher declines such a body instead.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MatchedIndex {
    Counter,
    Constant(i64),
    DerivedMod {
        local_id: u32,
        modulus_id: u32,
    },
    /// #10185: `c = (a*c + b) % m; … arr[c]`, optionally spelled through a
    /// `const index = c` alias. The recurrence itself lives on
    /// [`ElementShapeVersionedLoop::carried`] — this arm records only the
    /// spellings the subscript may take. See `stmt/element_shape_carried.rs`.
    Carried {
        carried_id: u32,
        alias_id: Option<u32>,
    },
}

impl MatchedIndex {
    /// Mirror of [`crate::expr::ElementShapeIndex::needs_counter_i32_slot`],
    /// asked before the fact exists.
    fn needs_counter_i32_slot(self) -> bool {
        !matches!(
            self,
            MatchedIndex::Constant(_) | MatchedIndex::Carried { .. }
        )
    }
}

#[derive(Debug)]
struct ElementShapeVersionedLoop {
    counter_id: u32,
    bound: ElementShapeLoopBound,
    array_id: u32,
    identity: ElementShapeIdentity,
    /// Tracked property names, in both arms. The class arm additionally
    /// carries a packed slot index per name inside its `identity`.
    props: std::collections::BTreeSet<String>,
    index: MatchedIndex,
    /// #7771: the body's `const r = arr[counter]` binding in the two-statement
    /// form; `None` for the original single-statement accumulator body.
    element_binding: Option<u32>,
    accumulator_id: u32,
    /// #10185: the matched recurrence, for the [`MatchedIndex::Carried`] form.
    carried: Option<super::element_shape_carried::MatchedCarried>,
    /// #10185: the body the FAST clone lowers, when it is not the source body.
    ///
    /// Two rewrites, both of which exist to put every side exit an iteration
    /// can take BEFORE any of its stores:
    ///
    /// * K accumulator statements fold into one, so a tag test that fails on
    ///   the third read cannot leave the first two already applied to `acc`
    ///   when the slow clone re-runs the iteration;
    /// * a carried recurrence gains a trailing commit statement, so the real
    ///   binding is published once the iteration is past every exit.
    ///
    /// The SLOW clone always lowers the original `body`.
    fast_body: Option<Vec<Stmt>>,
}

/// The locals the pure-expression walk reasons about.
#[derive(Clone, Copy)]
struct PureExprScope {
    counter_id: u32,
    accumulator_id: u32,
    element_binding: Option<u32>,
    /// #10123: `(d, m)` for a body whose first statement is
    /// `const d = counter % m`.
    derived: Option<(u32, u32)>,
    /// #10185: `(carried, alias)` for a body whose first statement advances a
    /// loop-carried index. Both are excluded from bare reads for the same
    /// reason `derived` is — inside the clone the real slot is one iteration
    /// behind until the trailing commit.
    carried: Option<(u32, Option<u32>)>,
}

/// What the walk collects.
#[derive(Default)]
struct PureExprFacts {
    array: Option<u32>,
    props: std::collections::BTreeSet<String>,
    index: Option<MatchedIndex>,
}

/// Effect-free expression walk for the element-shape loop.
///
/// Admits exactly: tracked `arr[<index>].prop` reads on ONE array, numeric
/// locals, numeric literals, and pure arithmetic / `Math` (libm intrinsics
/// cannot trigger a GC). Everything else bails the whole match — a catch-all
/// that silently accepted an unknown expression would be the #6377 shape, and
/// worse here, because an unadmitted expression is what would smuggle a call
/// into a body the revocation argument assumes is call-free.
fn element_shape_loop_pure_expr_collect(
    ctx: &FnCtx<'_>,
    expr: &perry_hir::Expr,
    scope: &PureExprScope,
    out: &mut PureExprFacts,
) -> bool {
    use perry_hir::Expr;
    match expr {
        Expr::PropertyGet {
            object, property, ..
        } => match object.as_ref() {
            Expr::IndexGet { object, index } => {
                let Expr::LocalGet(arr_id) = object.as_ref() else {
                    return false;
                };
                if *arr_id == scope.counter_id {
                    return false;
                }
                // #10123: the admitted index spellings. `Counter` is the
                // original one, and the two additions are the shapes real
                // record-access loops are written in (`rows[7]`, and
                // `const d = i % n; rows[d]`). An offset index (`arr[j + 1]`)
                // is still out: it would need a bounds obligation of its own.
                let Some(form) = match_index_form(index.as_ref(), scope) else {
                    return false;
                };
                match out.index {
                    Some(seen) if seen == form => {}
                    Some(_) => return false,
                    None => out.index = Some(form),
                }
                match out.array {
                    Some(a) if a == *arr_id => {}
                    Some(_) => return false, // one array per loop
                    None => out.array = Some(*arr_id),
                }
                out.props.insert(property.clone());
                true
            }
            // #7771: `r.field` through the body's `const r = arr[counter]`
            // binding is the same tracked read spelled through the Let the
            // body match admitted; the binding already pins (array, counter),
            // so only the property is left to record.
            Expr::LocalGet(recv_id) if scope.element_binding == Some(*recv_id) => {
                out.props.insert(property.clone());
                true
            }
            // #10185: `arr[<index>].prop.length` — a STRING field's JS length.
            // Admitted as its own form rather than as "a read of `prop`
            // followed by a generic `.length`", because the generic one is a
            // property diamond ending in a runtime call, and a call inside this
            // clone deletes it. The emitted read tag-tests the loaded word for
            // both string representations and side-exits otherwise
            // (`expr::element_shape_reads`).
            Expr::PropertyGet { .. } if property == "length" => {
                element_shape_loop_pure_expr_collect(ctx, object, scope, out)
            }
            _ => false,
        },
        // #10185: `arr[<index>].prop ? A : B` with constant arms. JS truthiness
        // of an arbitrary value is a runtime question the clone is not allowed
        // to guess, so the emitted read admits ONLY the two boolean singletons
        // and side-exits on everything else. Anything but a tracked element
        // read in the condition, or a non-constant arm, declines.
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            matches!(condition.as_ref(), Expr::PropertyGet { .. })
                && crate::expr::element_shape_reads::integer_arm(then_expr).is_some()
                && crate::expr::element_shape_reads::integer_arm(else_expr).is_some()
                && element_shape_loop_pure_expr_collect(ctx, condition, scope, out)
        }
        // A bare read of the array, the counter, the element binding or the
        // derived index as a VALUE could flow it into arbitrary lowering; only
        // scalar reads the analysis proves numeric are admitted. The element
        // binding and the derived index are excluded EXPLICITLY rather than via
        // the numeric test: both are bound by a `Let` the fast clone does not
        // lower generically, so a bare read would hand out a reference nothing
        // bound (#7771), and betting that exclusion on a type predicate is the
        // #6377 shape this walk's docs warn about.
        Expr::LocalGet(id) => {
            scope.element_binding != Some(*id)
                && scope.derived.map(|(d, _)| d) != Some(*id)
                // #10185: the carried index and its alias are virtual too —
                // their real slots are one iteration behind inside the clone,
                // so a bare read would hand out a stale value.
                && scope.carried.is_none_or(|(c, alias)| *id != c && alias != Some(*id))
                && out.array.is_none_or(|a| a != *id)
                && (*id == scope.accumulator_id || crate::type_analysis::is_numeric_expr(ctx, expr))
        }
        Expr::Number(_) | Expr::Integer(_) => true,
        // NOTE (#7480 step 3): deliberately NOT gated on
        // `is_numeric_expr(ctx, expr)`. `BinaryOp` is arithmetic/bitwise only
        // (no `in`/`instanceof`), so the sole hazard the whole-expression test
        // covered was `+` on a possibly-string operand — and every leaf this
        // walk admits is numeric by the time the match is ACCEPTED: numeric
        // locals and literals by their own arms, and tracked `arr[j].field`
        // reads because the caller rejects the whole loop unless every
        // collected property is a declared raw-f64 candidate on the resolved
        // element class (class arm) or the emitted read tag-tests the loaded
        // word and side-exits when it is not a Number (#10123's shape arm).
        //
        // The gate had to go for the object-literal kernel: at match time no
        // fact is installed yet, so `is_numeric_expr` cannot see through
        // `keep[j].v` (its `PropertyGet` arm resolves the owner through
        // `receiver_class_name`, which by design does not type an
        // object-literal element). Keeping it would have declined #7480's own
        // kernel before the class resolver was ever consulted.
        Expr::Binary { left, right, .. } => {
            element_shape_loop_pure_expr_collect(ctx, left, scope, out)
                && element_shape_loop_pure_expr_collect(ctx, right, scope, out)
        }
        Expr::NumberCoerce(operand) => {
            element_shape_loop_pure_expr_collect(ctx, operand, scope, out)
        }
        Expr::MathImul(left, right) | Expr::MathPow(left, right) => {
            element_shape_loop_pure_expr_collect(ctx, left, scope, out)
                && element_shape_loop_pure_expr_collect(ctx, right, scope, out)
        }
        Expr::MathMin(values) | Expr::MathMax(values) => values
            .iter()
            .all(|e| element_shape_loop_pure_expr_collect(ctx, e, scope, out)),
        Expr::MathAbs(value)
        | Expr::MathSqrt(value)
        | Expr::MathFloor(value)
        | Expr::MathCeil(value)
        | Expr::MathRound(value)
        | Expr::MathTrunc(value)
        | Expr::MathSign(value)
        | Expr::MathF16round(value) => element_shape_loop_pure_expr_collect(ctx, value, scope, out),
        _ => false,
    }
}

/// Classify one `arr[<index>]` subscript. `None` declines the whole match.
fn match_index_form(index: &perry_hir::Expr, scope: &PureExprScope) -> Option<MatchedIndex> {
    use perry_hir::Expr;
    match index {
        Expr::LocalGet(id) if *id == scope.counter_id => Some(MatchedIndex::Counter),
        Expr::LocalGet(id) => match scope.derived {
            Some((derived_id, modulus_id)) if derived_id == *id => Some(MatchedIndex::DerivedMod {
                local_id: derived_id,
                modulus_id,
            }),
            // #10185: `arr[c]` and `const index = c; arr[index]` are the same
            // subscript; the alias is virtual and carries no obligation of its
            // own.
            _ => match scope.carried {
                Some((carried_id, alias_id)) if carried_id == *id || alias_id == Some(*id) => {
                    Some(MatchedIndex::Carried {
                        carried_id,
                        alias_id,
                    })
                }
                _ => None,
            },
        },
        // `0..=i32::MAX`, so the preheader's `length > k` compare is an i32
        // one and the emitted index needs no conversion.
        Expr::Integer(k) if (0..=i64::from(i32::MAX)).contains(k) => {
            Some(MatchedIndex::Constant(*k))
        }
        _ => None,
    }
}

/// Resolve the class every element of `array_id` must have for the clone to
/// fire.
///
/// Two sources, tried in order:
///
/// 1. `receiver_class_name` on the `IndexGet`, i.e. a declared *named* element
///    type (`keep: Node[]`) — the same path Perry already uses to resolve
///    `items[2].display()`.
/// 2. #7480 step 3: an **object-literal element type** (`keep: {v: number}[]`),
///    resolved to the `__AnonShape_<hash>` class the literals actually
///    allocate ([`anon_shape_class_for_element_type`]). This is #7480's own
///    kernel and the whole measured gap: 408 ms against node's 12 on
///    200k × 50 before this resolved, 12 ms after, where the named-class arm
///    was already 13 ms.
///
/// Neither has to be *right*: the preheader compares the class id the runtime
/// invariant reports against this one, so a wrong answer costs the clone,
/// never correctness. The annotation stays a hint, never layout.
fn element_class_name(ctx: &FnCtx<'_>, array_id: u32, _counter_id: u32) -> Option<String> {
    if let perry_hir::types::Type::Named(named) =
        resolve_type_alias(ctx, declared_array_element_type_hint(ctx, array_id)?)
    {
        // Only if it names a REAL class. `type Node = {v: number}` makes the
        // element type `Named("Node")`, and the receiver resolver reports
        // "Node" for it — a name no `ctx.classes` entry answers to, because the
        // literals allocate an `__AnonShape_…`. Returning it unconditionally
        // shadowed arm 2 for every alias-typed array, which is how the second
        // half of `churn_read`'s miss survived #7669: the anon-shape resolver
        // landed and was then never consulted for the shape it was written for.
        if ctx.classes.contains_key(named) {
            return Some(named.clone());
        }
    }
    anon_shape_class_for_element_type(ctx, array_id)
}

/// Follow `type A = B; type B = {…}` to the object type an alias spells.
///
/// Bounded rather than cycle-detected: `type A = A` is not expressible in a
/// well-formed program, but codegen must not hang on a malformed one, and no
/// real alias chain is deep. Running out of budget declines the clone.
fn resolve_type_alias<'t>(
    ctx: &'t FnCtx<'_>,
    ty: &'t perry_hir::types::Type,
) -> &'t perry_hir::types::Type {
    let mut current = ty;
    for _ in 0..8 {
        let perry_hir::types::Type::Named(name) = current else {
            return current;
        };
        let Some(next) = ctx.type_aliases.get(name) else {
            return current;
        };
        current = next;
    }
    current
}

/// Content-addressed synthetic class every closed-shape object literal lowers
/// to (`perry-hir/src/lower/context.rs::mint_anon_shape_class`).
const ANON_SHAPE_PREFIX: &str = "__AnonShape_";

/// Erased element metadata used only to choose the class-id candidate for the
/// versioned clone. The preheader validates that candidate against the live
/// array invariant before the clone is reachable.
fn declared_array_element_type_hint<'a>(
    ctx: &'a FnCtx<'_>,
    array_id: u32,
) -> Option<&'a perry_hir::types::Type> {
    use perry_hir::types::Type as HirType;

    match resolve_type_alias(ctx, ctx.local_type_hint(&array_id)?) {
        HirType::Array(elem) => Some(elem.as_ref()),
        HirType::Generic { base, type_args } if base == "Array" && type_args.len() == 1 => {
            Some(&type_args[0])
        }
        _ => None,
    }
}

/// #7480 step 3: resolve `keep: {v: number, w: number}[]` to the
/// `__AnonShape_<hash>` class its literals allocate.
///
/// **Why not widen `receiver_class_name`.** That is the #6377 blast radius
/// #7612 deliberately refused — every consumer of the receiver-class resolver
/// would start seeing a class for an `Object`-typed read, un-gating latent
/// fast paths this change never measured. The resolver therefore lives here,
/// in the matcher, and the fast clone is made self-contained instead: its
/// field read carries its own `class_name` + packed slot index on
/// `ElementShapeLoopFact`, and the three predicates that would otherwise have
/// re-derived the class from the receiver
/// (`lower_raw_f64_class_field_get_for_number_context`, `is_numeric_expr`,
/// `lower_arithmetic_operand`'s routing test) consult that fact instead. All
/// three are scoped to the fast clone, where the guard has already proven the
/// element's class *and* — via the residual check's
/// `GC_OBJ_TYPED_LAYOUT_INTACT` bit — that the slot really holds a raw double.
///
/// **Why the hash cannot be recomputed.** `mint_anon_shape_class` keys the
/// FNV hash on the literal's *inferred value* types (`{v: 1}` tags `i`, not
/// `n`), while the annotation says `number`. So the class is found by matching
/// the declared property order against the module's anon shapes, not by
/// recomputing the name.
///
/// Ambiguity declines rather than guesses: two anon shapes can share a field
/// name list (`{v: n, w: n}` vs `{v: s, w: s}`), so candidates are narrowed by
/// field-type compatibility and a still-ambiguous set returns `None`. That
/// keeps the answer independent of `ctx.classes` iteration order, which is a
/// `HashMap`'s.
fn anon_shape_class_for_element_type(ctx: &FnCtx<'_>, array_id: u32) -> Option<String> {
    use perry_hir::types::Type as HirType;

    // The annotation selects a candidate versioned clone.  The clone's
    // preheader validates the receiver kind, array head, shape, and key token
    // before any representation-specific access, and falls back on failure.
    let elem = declared_array_element_type_hint(ctx, array_id)?;
    // `type Node = {v: number; w: number}` — the annotation names the shape one
    // indirection away. Both levels are resolved (`type Row = Node[]` too).
    let HirType::Object(obj) = resolve_type_alias(ctx, elem) else {
        return None;
    };
    // Only a CLOSED shape names a layout: an index signature, a method
    // signature (which `property_order` does not record) or an optional
    // property all mean the runtime object may not have exactly these slots.
    if obj.index_signature.is_some() {
        return None;
    }
    let order = obj.property_order.as_ref()?;
    if order.is_empty() || order.len() != obj.properties.len() {
        return None;
    }
    if obj.properties.values().any(|p| p.optional) {
        return None;
    }

    let candidates: Vec<&str> = ctx
        .classes
        .iter()
        .filter(|(name, class)| {
            name.starts_with(ANON_SHAPE_PREFIX)
                // The clone's packed slot indices describe ONE class's own
                // fields; an inherited layout or a computed key would not be
                // self-describing. Anon shapes never have either, so this is
                // a belt-and-braces check that keeps the invariant local.
                && class.extends_name.is_none()
                && class.computed_members.is_empty()
                && class.fields.len() == order.len()
                && class
                    .fields
                    .iter()
                    .zip(order)
                    .all(|(f, want)| f.key_expr.is_none() && f.name == *want)
                // Field types are checked for EVERY candidate, not only to
                // break a tie: "the declared shape and the class agree" should
                // mean the same thing whether or not a second shape happens to
                // share the field names, otherwise a one-candidate module and a
                // two-candidate one apply different rules to the same pair.
                && class.fields.iter().all(|f| {
                    obj.properties
                        .get(&f.name)
                        .is_some_and(|p| anon_shape_field_type_is_compatible(&p.ty, &f.ty))
                })
        })
        .map(|(name, _)| name.as_str())
        .collect();
    match candidates.as_slice() {
        [only] => Some((*only).to_string()),
        _ => None,
    }
}

/// Candidate filter for [`anon_shape_class_for_element_type`]: is a
/// synthesized anon-shape field type (inferred from the literal's VALUES)
/// consistent with the declared property type (an annotation)?
///
/// Deliberately coarse, because the two sides are not the same kind of fact.
/// `Number`/`Int32` are one bucket (`{v: 1}` infers `Int32` for a
/// `number`-declared property), as are `String`/`StringLiteral`, and an
/// `any`/`unknown` on EITHER side rules nothing out: a declared `any` names no
/// layout, and an inferred `Any` just means the lowering could not type that
/// literal's value expression (`{v: i, w: f()}`), which is not evidence of
/// disagreement. Making that arm one-sided would have silently declined the
/// very common `{v: <typed>, w: <untyped call>}` shape.
///
/// Being wrong here costs the clone and never correctness — the preheader
/// still compares the class id the runtime invariant reports.
fn anon_shape_field_type_is_compatible(
    declared: &perry_hir::types::Type,
    actual: &perry_hir::types::Type,
) -> bool {
    use perry_hir::types::Type as T;
    match (declared, actual) {
        (T::Any | T::Unknown, _) | (_, T::Any | T::Unknown) => true,
        (T::Number | T::Int32, T::Number | T::Int32) => true,
        (T::String | T::StringLiteral(_), T::String | T::StringLiteral(_)) => true,
        (d, a) => d == a,
    }
}

/// Lower one of the fast clone's VIRTUAL body bindings, or report that `id` is
/// not one. Called from `stmt/let_stmt.rs` before the generic `Let` lowering.
///
/// Two shapes, both admitted by the matcher and neither lowered generically:
///
/// * **#7771's element binding** (`const r = arr[j]`) emits NOTHING. The
///   matcher admitted the body only because every use of `r` is a tracked
///   `r.field` read, and each of those lowers through
///   `element_shape_loop_fact_for_property_get` to a bare element load.
///   Lowering the generic `IndexGet` would put a runtime-call diamond inside
///   the clone, fail its call-free admission scan, and DELETE the clone rather
///   than slow it (#7690's lesson).
/// * **#10123's derived index** (`const d = j % m`) emits one `srem i32`. `%`
///   on two possibly-untyped operands is a runtime call, with the same
///   consequence. The preheader already proved `m` is an integral
///   `1..=i32::MAX` and materialized it as an i32, and the counter is a
///   non-negative i32, so the whole statement is one instruction.
///
/// Both are sound for the same four reasons: nothing reads the binding bare
/// inside the clone (the matcher's walk excludes both explicitly), the clone
/// is call-free so no GC observes the slot mid-loop, `const` scoping means
/// nothing after the loop can read it, and a residual-check side exit re-runs
/// the current iteration in the slow clone, whose OWN `Let` binds the real
/// slot before any use. The facts are popped before the slow clone lowers, so
/// this cannot fire there.
///
/// Answering `false` for want of the counter's i32 slot cannot happen — the
/// matcher requires one for the derived-index form — and would only cost the
/// clone, never correctness.
pub(super) fn lower_virtual_clone_binding(ctx: &mut FnCtx<'_>, id: u32) -> Result<bool> {
    enum VirtualBinding {
        /// #7771 `const r = arr[j]` and #10185 `const index = c`: pure aliases,
        /// no instruction of their own.
        Alias,
        /// #10123 `const d = j % m`: one `srem i32`.
        DerivedMod {
            counter_id: u32,
            modulus_i32: String,
            slot: String,
        },
    }

    let Some((scope_id, binding)) = ctx.element_shape_loop_facts.iter().rev().find_map(|fact| {
        if fact.element_binding == Some(id) {
            return Some((fact.scope_id, VirtualBinding::Alias));
        }
        match &fact.index {
            crate::expr::ElementShapeIndex::DerivedMod {
                local_id,
                modulus_i32,
                slot,
            } if *local_id == id => Some((
                fact.scope_id,
                VirtualBinding::DerivedMod {
                    counter_id: fact.index_local_id,
                    modulus_i32: modulus_i32.clone(),
                    slot: slot.clone(),
                },
            )),
            crate::expr::ElementShapeIndex::Carried(carried) if carried.alias_id == Some(id) => {
                Some((fact.scope_id, VirtualBinding::Alias))
            }
            _ => None,
        }
    }) else {
        return Ok(false);
    };

    match binding {
        VirtualBinding::Alias => {}
        VirtualBinding::DerivedMod {
            counter_id,
            modulus_i32,
            slot,
        } => {
            let Some(counter_slot) = ctx.i32_counter_slots.get(&counter_id).cloned() else {
                return Ok(false);
            };
            let blk = ctx.block();
            let counter = blk.load(I32, &counter_slot);
            let derived = blk.srem(I32, &counter, &modulus_i32);
            blk.store(I32, &derived, &slot);
        }
    }
    // #10185: this iteration's index is now in its slot, so this is where the
    // shared element deref belongs — for the ONE binding that owns it.
    emit_element_prefetch_for(ctx, scope_id, id);
    Ok(true)
}

/// #10185: emit the once-per-iteration element prologue, if `site_local` is the
/// statement the fact designated to own it.
///
/// Every read in the clone loads the handle this parks, so the designated
/// statement MUST run before any of them — which is why the matcher only ever
/// designates the body's leading virtual statement, and why a fact carrying a
/// prefetch whose index cannot be materialized is a compiler bug rather than a
/// missed optimization (the reads would load an uninitialized alloca). The
/// matcher checks `needs_counter_i32_slot` before the fact is built, so the
/// index is always available here.
pub(super) fn emit_element_prefetch_for(ctx: &mut FnCtx<'_>, scope_id: u32, site_local: u32) {
    let Some(fact) = ctx
        .element_shape_loop_facts
        .iter()
        .find(|fact| fact.scope_id == scope_id)
        .cloned()
    else {
        return;
    };
    let Some(prefetch) = fact.elem_prefetch.clone() else {
        return;
    };
    if prefetch.site_local_id != site_local {
        return;
    }
    let idx_i32 = crate::expr::element_shape_guard::emit_element_shape_index(ctx, &fact)
        .expect("a prefetching fact always has a materializable index");
    crate::expr::element_shape_guard::emit_element_shape_prefetch(
        ctx,
        &fact,
        &idx_i32,
        &prefetch.handle_slot,
    );
}

/// Is `array_id` an untyped receiver — the shape-keyed arm's entry condition?
///
/// The class resolver having declined is not on its own enough: a `Node[]`
/// whose class the module does not define, or a shape the anon-shape resolver
/// found ambiguous, are both "declined" and both name a layout this arm would
/// be guessing about. Requiring the declared type to be absent / `any` /
/// `unknown` keeps #10123 to exactly the receivers that carry no layout claim
/// at all, which is what `JSON.parse` hands back.
fn array_is_untyped(ctx: &FnCtx<'_>, array_id: u32) -> bool {
    use perry_hir::types::Type;
    matches!(
        ctx.local_type_hint(&array_id)
            .map(|ty| resolve_type_alias(ctx, ty)),
        None | Some(Type::Any) | Some(Type::Unknown)
    )
}

/// A modulus local for #10123's `const d = counter % m` index.
///
/// `m` must be readable and provably unchanged for the loop's duration: the
/// preheader materializes it ONCE and the clone's `srem` uses that value for
/// every iteration, so a body that could rewrite `m` would derive indices
/// against a stale bound. `local_bound_is_loop_invariant` answers exactly
/// that (it looks for WRITES, so the `const d = counter % m` read itself is
/// not a mutation).
fn modulus_local_is_admissible(
    ctx: &FnCtx<'_>,
    modulus_id: u32,
    condition: &perry_hir::Expr,
    update: Option<&perry_hir::Expr>,
    body: &[Stmt],
) -> bool {
    !ctx.boxed_vars.contains(&modulus_id)
        && !ctx.closure_captures.contains_key(&modulus_id)
        && (local_has_readable_slot(ctx, modulus_id)
            || ctx.module_globals.contains_key(&modulus_id))
        && local_bound_is_loop_invariant(condition, update, body, modulus_id)
}

/// Match `for (let j = k0; j < B; j++) acc = <pure over arr[<index>].field>`.
///
/// The single-statement, store-free body is the revocation argument (see the
/// module docs) AND the side-exit protocol: the residual per-element check
/// fires before the accumulator's `LocalSet` commits, so resuming the current
/// iteration in the slow clone cannot double-apply anything.
fn match_element_shape_versioned_loop(
    ctx: &FnCtx<'_>,
    init: Option<&Stmt>,
    condition: Option<&perry_hir::Expr>,
    update: Option<&perry_hir::Expr>,
    body: &[Stmt],
) -> Option<ElementShapeVersionedLoop> {
    use perry_hir::{BinaryOp, CompareOp, Expr, UpdateOp};

    // Oversized modules full-outline the class-field diamonds for code size;
    // a clone that re-inlines them there would fight that decision.
    if crate::codegen::full_outline_ic_enabled() {
        return None;
    }
    if !ctx.pending_labels.is_empty() {
        return None;
    }

    let (counter_id, start) = match init? {
        Stmt::Let {
            id,
            init: Some(init_expr),
            ..
        } => {
            let start = match init_expr {
                Expr::Integer(n) => *n,
                Expr::Number(n) if n.is_finite() && n.fract() == 0.0 => *n as i64,
                _ => return None,
            };
            (*id, start)
        }
        _ => return None,
    };
    if !(0..=i64::from(i32::MAX)).contains(&start) {
        return None;
    }

    let (op, left, right) = match condition? {
        Expr::Compare { op, left, right } => (*op, left.as_ref(), right.as_ref()),
        _ => return None,
    };
    if !matches!(op, CompareOp::Lt) || !matches!(left, Expr::LocalGet(id) if *id == counter_id) {
        return None;
    }
    let bound = match right {
        Expr::Integer(k) if (0..=i64::from(i32::MAX)).contains(k) => {
            ElementShapeLoopBound::Constant(*k)
        }
        // `j < arr.length` — the idiom every one of #7480's own kernels is
        // written in, and the reason the clone had not moved `churn_read` at
        // all: `keep.length` is a `PropertyGet`, so the bound match declined
        // before the class resolver was ever reached. The receiver is checked
        // against the body's array below; an unrelated array's length would
        // need its own invariance argument and is not admitted.
        Expr::PropertyGet {
            object, property, ..
        } if property == "length" => match object.as_ref() {
            Expr::LocalGet(recv_id) if *recv_id != counter_id => {
                ElementShapeLoopBound::ArrayLength(*recv_id)
            }
            _ => return None,
        },
        Expr::LocalGet(bound_id) if *bound_id != counter_id => {
            if ctx.boxed_vars.contains(bound_id) {
                return None;
            }
            if !local_has_readable_slot(ctx, *bound_id)
                && !ctx.module_globals.contains_key(bound_id)
            {
                return None;
            }
            if !local_bound_is_loop_invariant(condition?, update, body, *bound_id) {
                return None;
            }
            ElementShapeLoopBound::Local(*bound_id)
        }
        _ => return None,
    };

    if !matches!(
        update?,
        Expr::Update {
            id,
            op: UpdateOp::Increment,
            ..
        } if *id == counter_id
    ) {
        return None;
    }
    if !local_has_readable_slot(ctx, counter_id)
        || ctx.boxed_vars.contains(&counter_id)
        || !ctx.integer_locals.contains(&counter_id)
        || !loop_counter_bounds_are_safe(ctx, counter_id, update, body)
        || !loop_counter_entry_i32_range_is_safe(init, counter_id)
    {
        return None;
    }

    // Store-free body, in one of four admitted shapes (see the module docs):
    //
    //   1. `acc = <pure numeric over arr[<index>].field>` — the original
    //      single statement;
    //   2. `const r = arr[j]; acc = <pure numeric over r.field>` — #7771's
    //      element-binding form, the shape real read loops are written in;
    //   3. `const d = j % m; acc = <pure numeric over arr[d].field>` —
    //      #10123's derived-index form, the shape every sequential pass over a
    //      parsed record array is written in;
    //   4. #10185's loop-carried form, `c = (a*c + b) % m;` optionally followed
    //      by `const index = c;`.
    //
    // In 2, 3 and 4 the leading statement is VIRTUAL inside the fast clone: it
    // emits nothing (form 2), one `srem i32` (form 3) or the i64 recurrence
    // (form 4) rather than the generic lowering (`stmt/let_stmt.rs`,
    // `stmt/element_shape_carried.rs`), so the revocation argument (no store,
    // no call in the clone) is unchanged. The bindings are `const`-only,
    // deliberately: a `var` binding is function-scoped and observable after the
    // loop, where the skipped `Let` would leave the slot holding its pre-loop
    // value. Form 4's `c` is the one mutable exception, and it pays for it with
    // a write-back (see `element_shape_carried`).
    //
    // #10185 also admits K >= 1 accumulator statements instead of exactly one,
    // which is what `sum += rows[i].id; sum += rows[i].name.length; …` needs.
    // NOTHING else is admitted.
    let mut element_binding: Option<(u32, u32)> = None;
    let mut derived: Option<(u32, u32)> = None;
    let mut carried: Option<super::element_shape_carried::MatchedCarried> = None;
    let mut carried_alias: Option<u32> = None;
    let mut rest: &[Stmt] = body;

    // (4) The recurrence, which must be the body's FIRST statement: the index
    // it produces is read by every statement after it, and a read BEFORE it
    // would see a value the preheader bounded but the commit protocol does not
    // describe.
    if let Some(first) = rest.first() {
        if let Some(matched) = super::element_shape_carried::match_carried_update(
            ctx, first, counter_id, condition?, update, body,
        ) {
            carried = Some(matched);
            rest = &rest[1..];
        }
    }

    // (2)/(3)/(4-alias) The leading virtual binding.
    if let Some(Stmt::Let {
        id,
        mutable: false,
        init: Some(binding_init),
        ..
    }) = rest.first()
    {
        // The binding must be a plain, loop-owned const local in every form. A
        // boxed or captured binding lives in a cell the clone's replacement
        // `Let` would leave stale for an observer outside the clone; a
        // module-global id is not a body-scoped binding at all.
        if *id == counter_id
            || ctx.boxed_vars.contains(id)
            || ctx.module_globals.contains_key(id)
            || ctx.closure_captures.contains_key(id)
        {
            return None;
        }
        match binding_init {
            Expr::IndexGet { object, index } if carried.is_none() => {
                let (Expr::LocalGet(arr_id), Expr::LocalGet(idx_id)) =
                    (object.as_ref(), index.as_ref())
                else {
                    return None;
                };
                // Same receiver/index discipline as the walk's IndexGet
                // arm: the fetch must be `arr[counter]` exactly.
                if *idx_id != counter_id || *arr_id == counter_id || *id == *arr_id {
                    return None;
                }
                element_binding = Some((*id, *arr_id));
            }
            // #10123. `%` on two locals, the left one the counter. The
            // modulus is validated below (it must be readable and
            // loop-invariant), and the RANGE obligation — `1 <= m` so the
            // `srem` cannot divide by zero, `m <= length` so every derived
            // index is in bounds — is discharged in the preheader, which
            // is also where a non-number / fractional `m` sends the loop
            // to the slow clone.
            Expr::Binary {
                op: BinaryOp::Mod,
                left,
                right,
            } if carried.is_none() => {
                let (Expr::LocalGet(num_id), Expr::LocalGet(modulus_id)) =
                    (left.as_ref(), right.as_ref())
                else {
                    return None;
                };
                if *num_id != counter_id || *modulus_id == counter_id || *id == *modulus_id {
                    return None;
                }
                if !modulus_local_is_admissible(ctx, *modulus_id, condition?, update, body) {
                    return None;
                }
                derived = Some((*id, *modulus_id));
            }
            // #10185: `const index = c` after the recurrence. A pure alias, so
            // it emits nothing at all — the subscript resolves to the carried
            // slot either way.
            Expr::LocalGet(aliased) => {
                let matched = carried.as_ref()?;
                if *aliased != matched.carried_id || *id == matched.modulus_id {
                    return None;
                }
                carried_alias = Some(*id);
            }
            _ => return None,
        }
        rest = &rest[1..];
    }

    // (1) One or more accumulator statements over the SAME local.
    let mut acc_id: Option<&u32> = None;
    let mut values: Vec<&perry_hir::Expr> = Vec::new();
    for stmt in rest {
        let Stmt::Expr(Expr::LocalSet(id, value)) = stmt else {
            return None;
        };
        match acc_id {
            None => acc_id = Some(id),
            Some(seen) if seen == id => {}
            Some(_) => return None,
        }
        values.push(value.as_ref());
    }
    let acc_id = acc_id?;
    if *acc_id == counter_id
        || !ctx.locals.contains_key(acc_id)
        || ctx.boxed_vars.contains(acc_id)
        || ctx.module_globals.contains_key(acc_id)
        || carried.is_some_and(|c| c.carried_id == *acc_id || c.modulus_id == *acc_id)
    {
        return None;
    }
    // #10185: statements 2..K are folded into statement 1 for the fast clone
    // (`acc = a; acc = acc + b` ≡ `acc = (a) + b`), so that the WHOLE iteration
    // commits once, after every side exit it can take. Each of them must
    // therefore be `acc <op> <pure>` with `acc` as the left operand — the shape
    // every compound assignment lowers to — and the right operand must not read
    // `acc`, or substituting it would make it see the pre-statement value.
    for value in &values[1..] {
        let Expr::Binary { left, right, .. } = value else {
            return None;
        };
        if !matches!(left.as_ref(), Expr::LocalGet(id) if id == acc_id) {
            return None;
        }
        if super::element_shape_carried::expr_reads_local(right, *acc_id) {
            return None;
        }
    }

    // The binding form pins the array before the walk runs, so a body mixing
    // `r.field` with `other[j].field` is declined by the walk's one-array rule.
    let scope = PureExprScope {
        counter_id,
        accumulator_id: *acc_id,
        element_binding: element_binding.map(|(id, _)| id),
        derived,
        carried: carried.map(|c| (c.carried_id, carried_alias)),
    };
    if scope.element_binding == Some(*acc_id)
        || derived.map(|(d, _)| d) == Some(*acc_id)
        || carried_alias == Some(*acc_id)
    {
        return None;
    }
    let mut facts = PureExprFacts {
        array: element_binding.map(|(_, arr_id)| arr_id),
        ..PureExprFacts::default()
    };
    if !values
        .iter()
        .all(|value| element_shape_loop_pure_expr_collect(ctx, value, &scope, &mut facts))
    {
        return None;
    }
    let array_id = facts.array?;
    if facts.props.is_empty() || array_id == *acc_id || array_id == counter_id {
        return None;
    }
    // The element-binding form carries no subscript at the reads, so its index
    // form comes from the binding rather than from the walk.
    let index = match element_binding {
        Some(_) => MatchedIndex::Counter,
        None => facts.index?,
    };
    if let MatchedIndex::DerivedMod {
        local_id,
        modulus_id,
    } = index
    {
        if modulus_id == array_id || modulus_id == *acc_id || local_id == array_id {
            return None;
        }
    }
    // #10185: a matched recurrence whose value nothing subscripts with would
    // still have its `LocalSet` lowered generically — a `frem` libcall inside
    // the clone, which deletes it. Decline instead, so the loop keeps whatever
    // lowering it has today.
    match (carried.as_ref(), index) {
        (Some(matched), MatchedIndex::Carried { .. }) => {
            if matched.carried_id == array_id
                || matched.modulus_id == array_id
                || carried_alias == Some(array_id)
            {
                return None;
            }
        }
        // A recurrence the subscripts do not use, or a carried subscript with
        // no recurrence (which the walk's scope makes unreachable).
        (Some(_), _) | (None, MatchedIndex::Carried { .. }) => return None,
        (None, _) => {}
    }
    match bound {
        ElementShapeLoopBound::Local(bound_id) => {
            if bound_id == array_id
                || bound_id == *acc_id
                || Some(bound_id) == scope.element_binding
                || derived.map(|(d, _)| d) == Some(bound_id)
                // The bound is materialized ONCE in the preheader, so a bound
                // the recurrence rewrites every iteration would be stale. The
                // carried local is also not readable bare inside the clone.
                || carried.is_some_and(|c| c.carried_id == bound_id)
                || carried_alias == Some(bound_id)
            {
                return None;
            }
        }
        // The guard's `length` load answers for the array the guard branded.
        // `for (j = 0; j < other.length; j++) acc += keep[j].v` reads a
        // DIFFERENT array's length, and the preheader proves nothing about how
        // the two relate — that is an out-of-range read, not a slow clone.
        ElementShapeLoopBound::ArrayLength(recv_id) => {
            if recv_id != array_id {
                return None;
            }
        }
        ElementShapeLoopBound::Constant(_) => {}
    }

    // The array must be loop-invariant and directly addressable — no boxing,
    // no POD / scalar-replacement alias (those take other lowering paths).
    if ctx.boxed_vars.contains(&array_id)
        || ctx.pod_records.contains_key(&array_id)
        || ctx.scalar_replaced.contains_key(&array_id)
        || ctx.scalar_replaced_arrays.contains_key(&array_id)
        || ctx.array_row_aliases.contains_key(&array_id)
    {
        return None;
    }
    if !ctx.locals.contains_key(&array_id) && !ctx.module_globals.contains_key(&array_id) {
        return None;
    }
    // #7480: the preheader must be able to write the growth-forwarding-repaired
    // head BACK into the binding (see
    // `expr::element_shape_guard::emit_element_shape_loop_preheader_check`
    // step 2). A closure-captured array lives in a capture cell that a plain
    // slot store would not update, so the two views could disagree; decline
    // rather than repair only half of them.
    if ctx.closure_captures.contains_key(&array_id) {
        return None;
    }
    if !local_bound_is_loop_invariant(condition?, update, body, array_id) {
        return None;
    }

    let identity = match element_class_name(ctx, array_id, counter_id) {
        Some(class_name) => {
            // The class arm keeps its ORIGINAL grammar. Its bounds argument is
            // "the trip count covers every index", which only `Counter`
            // satisfies, and widening it here would change the emitted code
            // for receivers #10123 never measured.
            if index != MatchedIndex::Counter {
                return None;
            }
            match_class_identity(ctx, array_id, &class_name, &facts.props)?
        }
        // #10123. A receiver that declares no element type at all is the
        // `JSON.parse` case: nothing is known statically, so everything is
        // asked of the runtime in the preheader.
        None if array_is_untyped(ctx, array_id) => ElementShapeIdentity::Shape,
        None => return None,
    };

    // The property denylist, which is arm-specific (#10185).
    //
    // The CLASS arm's list exists because its read bakes in a compile-time
    // packed slot index while the surrounding lowering may route the name
    // somewhere else entirely (`length` header loads, `errors` runtime calls,
    // accessor-ish names) — a name collision there costs the call-free
    // guarantee.
    //
    // The SHAPE arm bakes in nothing: the preheader asks the runtime for the
    // inline slot of that exact key in that exact ordinary ShapeId and declines
    // on `-1`, and the per-element residual pins `obj_type == GC_TYPE_OBJECT`
    // with no per-object descriptors. Every receiver kind whose builtin branch
    // could answer a name differently — a function's `name`, an array's
    // `length`, a Map's `size`, an AggregateError's `errors` — fails that
    // residual, and for a plain record an own data property SHADOWS the
    // prototype name it collides with, which is exactly what the inline slot
    // holds. The whole list would otherwise have cost the benchmark its
    // `fields` shape outright, for a field literally called `name`.
    //
    // `__proto__` stays denied, and not because of JavaScript: node gives
    // `JSON.parse('{"__proto__":1}')` an OWN `__proto__` data property and
    // reads `1` back from it, so the inline slot would be right. It is denied
    // because Perry's generic property path may special-case the name ahead of
    // own-property lookup, and the clone must agree with the clone-of, not with
    // the spec, wherever those two differ.
    const SHAPE_PROP_DENYLIST: &[&str] = &["__proto__"];
    let denylist = match identity {
        ElementShapeIdentity::Class { .. } => CLASS_FIELD_LOOP_PROP_DENYLIST,
        ElementShapeIdentity::Shape => SHAPE_PROP_DENYLIST,
    };
    for prop in &facts.props {
        if denylist.contains(&prop.as_str()) {
            return None;
        }
    }

    // The declared accumulator type is only a candidate: the lowering
    // validates the accumulator's current NaN-box tag in the preheader before
    // installing the numeric fact for the fast clone. The class arm keeps its
    // original Number/Int32 requirement; the shape arm also admits an
    // untyped accumulator, because `let sum = 0; sum += rows[i].id` over an
    // `any` array is the shape this optimization exists for and HIR widens
    // `sum` to `Any` exactly when the element read is untyped. The preheader
    // tag check is what makes that safe, and it is emitted either way.
    let accumulator_hint_ok = match ctx.local_type_hint(acc_id) {
        Some(perry_hir::types::Type::Number | perry_hir::types::Type::Int32) => true,
        None | Some(perry_hir::types::Type::Any | perry_hir::types::Type::Unknown) => {
            matches!(identity, ElementShapeIdentity::Shape)
        }
        _ => false,
    };
    if !accumulator_hint_ok {
        return None;
    }

    // #10185: the fast clone's body, when the two rewrites apply. `lead` is
    // whatever statements the walk consumed ahead of the accumulator run (the
    // recurrence and/or the virtual binding), kept verbatim.
    let lead = &body[..body.len() - rest.len()];
    let fast_body = (values.len() > 1 || carried.is_some()).then(|| {
        let mut stmts: Vec<Stmt> = lead.to_vec();
        // `acc = v1; acc = acc <op> p2; acc = acc <op> p3`
        //   ≡ `acc = ((v1) <op> p2) <op> p3`
        // — the same operations on the same operands in the same order, which
        // is what keeps the folded float arithmetic bit-identical.
        let mut folded = values[0].clone();
        for value in &values[1..] {
            let Expr::Binary { op, right, .. } = value else {
                unreachable!("the accumulator fold validated every statement's shape");
            };
            folded = Expr::Binary {
                op: *op,
                left: Box::new(folded),
                right: right.clone(),
            };
        }
        stmts.push(Stmt::Expr(Expr::LocalSet(*acc_id, Box::new(folded))));
        if let Some(matched) = carried.as_ref() {
            // The commit marker (`stmt/element_shape_carried.rs`). It is the
            // LAST statement, so it runs only once the iteration is past every
            // exit it could take.
            stmts.push(Stmt::Expr(Expr::LocalSet(
                matched.carried_id,
                Box::new(Expr::LocalGet(matched.carried_id)),
            )));
        }
        stmts
    });

    Some(ElementShapeVersionedLoop {
        counter_id,
        bound,
        array_id,
        identity,
        props: facts.props,
        index,
        element_binding: scope.element_binding,
        accumulator_id: *acc_id,
        carried,
        fast_body,
    })
}

/// Resolve the class arm's compile-time facts, or decline.
fn match_class_identity(
    ctx: &FnCtx<'_>,
    array_id: u32,
    class_name: &str,
    props: &std::collections::BTreeSet<String>,
) -> Option<ElementShapeIdentity> {
    if CLASS_FIELD_LOOP_CLASS_DENYLIST.contains(&class_name) {
        return None;
    }
    let class = ctx.classes.get(class_name)?;
    if !class.computed_members.is_empty() {
        return None;
    }
    // An `extends`-ing element class would put the field's slot behind an
    // inherited layout the packed index does not describe on its own; and a
    // native base (`extends Array`) is exactly the #7573/#7603 hazard. Decline.
    if class.extends_name.is_some() {
        return None;
    }
    let expected_class_id = *ctx.class_ids.get(class_name)?;
    let keys_global_name = ctx.class_keys_globals.get(class_name)?.clone();
    let statically_class_proven = ctx
        .native_facts
        .exact_element_class(array_id)
        .is_some_and(|proven| proven == class_name);

    let mut packed_fields = std::collections::BTreeMap::new();
    for prop in props {
        // Accessors route through synthesized __get_/__set_ methods before the
        // class-field diamond; mirror that dispatch gate exactly.
        if ctx
            .methods
            .contains_key(&(class_name.to_string(), format!("__get_{prop}")))
            || ctx
                .methods
                .contains_key(&(class_name.to_string(), format!("__set_{prop}")))
        {
            return None;
        }
        let field_index = crate::type_analysis::class_field_global_index(ctx, class_name, prop)?;
        let raw_f64 = crate::type_analysis::class_field_declared_type(ctx, class_name, prop)
            .as_ref()
            .is_some_and(crate::typed_shape::type_is_raw_f64_candidate);
        if !raw_f64 {
            return None;
        }
        packed_fields.insert(prop.clone(), field_index);
    }
    let statically_layout_proven = statically_class_proven
        && ctx
            .native_facts
            .exact_numeric_element_fields(array_id)
            .is_some_and(|proven| packed_fields.keys().all(|field| proven.contains(field)));

    Some(ElementShapeIdentity::Class {
        class_name: class_name.to_string(),
        expected_class_id,
        keys_global_name,
        packed_fields,
        statically_class_proven,
        statically_layout_proven,
    })
}

/// Materialize a NaN-boxed local as an i32 in `[min, max]`, taking the slow
/// clone on a non-number, out-of-range or fractional value.
///
/// Leaves `ctx.current_block` on a fresh block dominated by all three checks,
/// and returns the i32 SSA value. Call-free by construction — the whole point
/// is that the clone's hot path reads an i32 it can trust.
fn materialize_loop_i32(
    ctx: &mut FnCtx<'_>,
    local_id: u32,
    min: i32,
    max: i32,
    slow_label: &str,
    label_prefix: &str,
) -> Result<String> {
    let value = lower_expr(ctx, &perry_hir::Expr::LocalGet(local_id))?;
    let is_number = emit_js_value_is_number(ctx, &value);
    let range_idx = ctx.new_block(&format!("{label_prefix}.range"));
    let convert_idx = ctx.new_block(&format!("{label_prefix}.convert"));
    let done_idx = ctx.new_block(&format!("{label_prefix}.ok"));
    let range_label = ctx.block_label(range_idx);
    let convert_label = ctx.block_label(convert_idx);
    let done_label = ctx.block_label(done_idx);
    ctx.block().cond_br(&is_number, &range_label, slow_label);

    ctx.current_block = range_idx;
    let ge_min = {
        let min_literal = format!("{:.1}", f64::from(min));
        ctx.block().fcmp("oge", &value, &min_literal)
    };
    let le_max = {
        let max_literal = format!("{:.1}", f64::from(max));
        ctx.block().fcmp("ole", &value, &max_literal)
    };
    let in_range = ctx.block().and(I1, &ge_min, &le_max);
    ctx.block().cond_br(&in_range, &convert_label, slow_label);

    ctx.current_block = convert_idx;
    let as_i32 = ctx.block().fptosi(DOUBLE, &value, I32);
    let roundtrip = ctx.block().sitofp(I32, &as_i32, DOUBLE);
    let is_integral = ctx.block().fcmp("oeq", &roundtrip, &value);
    ctx.block().cond_br(&is_integral, &done_label, slow_label);

    ctx.current_block = done_idx;
    Ok(as_i32)
}

/// Lower the matched loop as a guarded fast clone plus the unchanged generic
/// body, modeled on `lower_class_field_versioned_for`.
///
/// SAFETY (miscompile class — see the module docs): between the preheader's
/// post-guard re-derivation of the elements base pointer and the end of the
/// fast clone, NO call may be emitted. The matcher enforces this by shape and
/// the scan below enforces it by construction; call-free ⇒ allocation-free ⇒
/// no GC ⇒ the array cannot move, and none of the revocation funnels can run.
pub(super) fn lower_element_shape_versioned_for(
    ctx: &mut FnCtx<'_>,
    init: Option<&Stmt>,
    condition: Option<&perry_hir::Expr>,
    update: Option<&perry_hir::Expr>,
    body: &[Stmt],
) -> Result<bool> {
    let Some(matched) = match_element_shape_versioned_loop(ctx, init, condition, update, body)
    else {
        return Ok(false);
    };
    // A counter- or modulo-derived index reads the counter through its
    // canonical i32 slot; without one the element GEP would need an fptosi and
    // the clone would win nothing. A CONSTANT index never reads the counter
    // (#10123), and its loops are exactly the ones `stmt/let_stmt.rs` mints no
    // i32 slot for — the counter is neither index-used nor i32-bounded — so
    // demanding one there would decline the shape this arm was written for.
    if matched.index.needs_counter_i32_slot()
        && !ctx.i32_counter_slots.contains_key(&matched.counter_id)
    {
        return Ok(false);
    }

    let fast_pre_idx = ctx.new_block("element_shape.loop.fast.preheader");
    let slow_pre_idx = ctx.new_block("element_shape.loop.slow.preheader");
    let merge_idx = ctx.new_block("element_shape.loop.merge");
    let fast_pre_label = ctx.block_label(fast_pre_idx);
    let slow_pre_label = ctx.block_label(slow_pre_idx);
    let merge_label = ctx.block_label(merge_idx);

    // One-time i32 materialization of the bound. A non-number / NaN /
    // fractional / out-of-range bound keeps full JS trip-count semantics in
    // the slow clone. `arr.length` materializes inside the guard instead — it
    // is the word the guard already loads — so it contributes nothing here.
    let materialized_bound: Option<String> = match matched.bound {
        ElementShapeLoopBound::ArrayLength(_) => None,
        ElementShapeLoopBound::Constant(k) => Some(k.to_string()),
        ElementShapeLoopBound::Local(bound_id) => Some(materialize_loop_i32(
            ctx,
            bound_id,
            0,
            i32::MAX,
            &slow_pre_label,
            "element_shape.loop.bound",
        )?),
    };

    // #10123: the same materialization for a derived index's modulus, with a
    // floor of 1 — `srem` by zero is undefined behaviour, and `x % 0` is NaN
    // in JS, so a zero modulus is a slow-clone case rather than something the
    // clone may compute.
    //
    // #10185's carried recurrence takes the same obligation for the same
    // reason, and one more of its own: its ENTRY value must be a non-negative
    // integral i32, because the whole i64 recurrence bound
    // (`a * i32::MAX + b < 2^53`, and a non-negative dividend for `srem` to
    // agree with JS `%`) is stated over that range. A fractional, negative or
    // non-number `cursor` at loop entry is a slow-clone case.
    let modulus_i32: Option<String> = match (&matched.index, &matched.carried) {
        (MatchedIndex::DerivedMod { modulus_id, .. }, _) => Some(materialize_loop_i32(
            ctx,
            *modulus_id,
            1,
            i32::MAX,
            &slow_pre_label,
            "element_shape.loop.modulus",
        )?),
        (MatchedIndex::Carried { .. }, Some(carried)) => Some(materialize_loop_i32(
            ctx,
            carried.modulus_id,
            1,
            i32::MAX,
            &slow_pre_label,
            "element_shape.loop.modulus",
        )?),
        _ => None,
    };
    let carried_slot: Option<String> = match &matched.carried {
        Some(carried) => {
            let entry = materialize_loop_i32(
                ctx,
                carried.carried_id,
                0,
                i32::MAX,
                &slow_pre_label,
                "element_shape.loop.carried",
            )?;
            // Entry-block alloca: LLVM lowers a non-entry `alloca` as a real
            // stack bump with no restore, and this one is written every
            // iteration.
            let slot = ctx.func.alloca_entry(I32);
            ctx.block().store(I32, &entry, &slot);
            Some(slot)
        }
        None => None,
    };

    let trip_count = match &materialized_bound {
        Some(bound) => {
            crate::expr::element_shape_guard::ElementShapeLoopTripCount::Bound(bound.as_str())
        }
        None => crate::expr::element_shape_guard::ElementShapeLoopTripCount::ArrayLength,
    };
    let index_bound = match (&matched.index, &modulus_i32) {
        (MatchedIndex::Counter, _) => {
            crate::expr::element_shape_guard::ElementShapeIndexBound::FromTripCount
        }
        (MatchedIndex::Constant(k), _) => {
            crate::expr::element_shape_guard::ElementShapeIndexBound::Constant(*k)
        }
        // #10185: the recurrence's result is `srem` of a non-negative dividend
        // by `m`, so it lands in `[0, m)` exactly like the derived index and
        // takes exactly the same preheader obligation, `m <= length`.
        (MatchedIndex::DerivedMod { .. } | MatchedIndex::Carried { .. }, Some(modulus)) => {
            crate::expr::element_shape_guard::ElementShapeIndexBound::Modulus(modulus.as_str())
        }
        (MatchedIndex::DerivedMod { .. } | MatchedIndex::Carried { .. }, None) => {
            unreachable!("a modulo-derived index always materializes its modulus")
        }
    };
    let expected_class_id_str = match &matched.identity {
        ElementShapeIdentity::Class {
            expected_class_id, ..
        } => expected_class_id.to_string(),
        ElementShapeIdentity::Shape => String::new(),
    };
    let guard_kind = match &matched.identity {
        ElementShapeIdentity::Class {
            keys_global_name, ..
        } => crate::expr::element_shape_guard::ElementShapeGuardKind::Class {
            expected_class_id: expected_class_id_str.as_str(),
            keys_global_name: keys_global_name.as_str(),
        },
        ElementShapeIdentity::Shape => {
            crate::expr::element_shape_guard::ElementShapeGuardKind::Shape {
                properties: &matched.props,
            }
        }
    };
    let statically_proven = matches!(
        matched.identity,
        ElementShapeIdentity::Class {
            statically_class_proven: true,
            ..
        }
    );
    let guard = crate::expr::element_shape_guard::emit_element_shape_loop_preheader_check(
        ctx,
        matched.array_id,
        guard_kind,
        trip_count,
        index_bound,
        &slow_pre_label,
        statically_proven,
    )?;
    let accumulator = lower_expr(ctx, &perry_hir::Expr::LocalGet(matched.accumulator_id))?;
    let accumulator_is_number = emit_js_value_is_number(ctx, &accumulator);
    let fast_path_ok = ctx.block().and(I1, &guard.shape_ok, &accumulator_is_number);
    // Deliberately unterminated: it branches into the fast clone only after
    // the clone is PROVEN call-free below.
    let deref_idx = ctx.current_block;

    let (shape_keyed, statically_layout_proven, fields, report_class) = match &matched.identity {
        ElementShapeIdentity::Class {
            class_name,
            packed_fields,
            statically_layout_proven,
            ..
        } => (
            false,
            *statically_layout_proven,
            packed_fields
                .iter()
                .map(|(prop, index)| {
                    (
                        prop.clone(),
                        crate::expr::ElementShapeFieldSlot::Packed(*index),
                    )
                })
                .collect(),
            class_name.clone(),
        ),
        ElementShapeIdentity::Shape => (
            true,
            false,
            guard
                .field_slots
                .iter()
                .map(|(prop, slot)| {
                    (
                        prop.clone(),
                        crate::expr::ElementShapeFieldSlot::Runtime(slot.clone()),
                    )
                })
                .collect(),
            "<runtime shape>".to_string(),
        ),
    };
    let fact_index = match matched.index {
        MatchedIndex::Counter => crate::expr::ElementShapeIndex::Counter,
        MatchedIndex::Constant(k) => crate::expr::ElementShapeIndex::Constant(k),
        MatchedIndex::DerivedMod { local_id, .. } => crate::expr::ElementShapeIndex::DerivedMod {
            local_id,
            modulus_i32: modulus_i32
                .clone()
                .expect("a derived index always materializes its modulus"),
            // Entry-block alloca: LLVM lowers a non-entry `alloca` as a real
            // stack bump with no restore, and this one is written once per
            // iteration.
            slot: ctx.func.alloca_entry(I32),
        },
        MatchedIndex::Carried {
            carried_id,
            alias_id,
        } => {
            let carried = matched
                .carried
                .as_ref()
                .expect("a carried index always matches a recurrence");
            crate::expr::ElementShapeIndex::Carried(Box::new(crate::expr::CarriedIndex {
                local_id: carried_id,
                alias_id,
                coeff_a: carried.coeff_a,
                coeff_b: carried.coeff_b,
                modulus_i32: modulus_i32
                    .clone()
                    .expect("a carried index always materializes its modulus"),
                slot: carried_slot
                    .clone()
                    .expect("a carried index always materializes its entry value"),
                commit_slot: ctx
                    .locals
                    .get(&carried_id)
                    .cloned()
                    .expect("the matcher required a directly-addressable local"),
            }))
        }
    };

    // #10185: the shared once-per-iteration element deref, hung off the body's
    // leading virtual statement. Shape-keyed only: the class-keyed arm's reads
    // are raw doubles behind a `GC_OBJ_TYPED_LAYOUT_INTACT` residual whose
    // emitted IR #7480's census pins, and nothing in #10185 measures it.
    let elem_prefetch = shape_keyed
        .then(|| {
            let site_local_id = match (&fact_index, matched.element_binding) {
                (_, Some(binding)) => Some(binding),
                (crate::expr::ElementShapeIndex::DerivedMod { local_id, .. }, _) => Some(*local_id),
                // With an alias the prologue belongs to the alias `Let`, which
                // runs after the recurrence; without one it belongs to the
                // recurrence itself. Exactly one statement owns it either way.
                (crate::expr::ElementShapeIndex::Carried(carried), _) => {
                    Some(carried.alias_id.unwrap_or(carried.local_id))
                }
                _ => None,
            }?;
            Some(crate::expr::ElementPrefetch {
                site_local_id,
                handle_slot: ctx.func.alloca_entry(I64),
            })
        })
        .flatten();

    let scope_id = ctx.next_loop_proof_scope_id();
    let fast_scan_start = ctx.func.num_blocks();
    ctx.current_block = fast_pre_idx;
    ctx.element_shape_loop_facts
        .push(crate::expr::ElementShapeLoopFact {
            array_local_id: matched.array_id,
            index_local_id: matched.counter_id,
            index: fact_index,
            elem_prefetch,
            shape_keyed,
            scope_id,
            class_name: report_class,
            elements_base: guard.elements_base,
            expected_shape_id: guard.expected_shape_id,
            side_exit_label: slow_pre_label.clone(),
            statically_layout_proven,
            fields,
            synthesized_body: matched.fast_body.is_some(),
            element_binding: matched.element_binding,
            numeric_accumulator: matched.accumulator_id,
        });
    let lowered = lower_for_after_init_with_i32_bound(
        ctx,
        init,
        condition,
        update,
        matched.fast_body.as_deref().unwrap_or(body),
        "for.element_shape_fast",
        Some((matched.counter_id, guard.bound_i32)),
    );
    ctx.element_shape_loop_facts
        .retain(|fact| fact.scope_id != scope_id);
    lowered?;
    if !ctx.block().is_terminated() {
        ctx.block().br(&merge_label);
    }
    let fast_scan_end = ctx.func.num_blocks();

    // Compile-time verification of the revocation argument. See the module
    // docs: call-free is precisely "no funnel that can revoke the invariant,
    // and no allocation that can move the array, runs while the clone does".
    let fast_clone_call_free = !ctx.func.blocks()[fast_pre_idx].contains_gc_unsafe_call()
        && (fast_scan_start..fast_scan_end)
            .all(|idx| !ctx.func.blocks()[idx].contains_gc_unsafe_call());
    ctx.current_block = deref_idx;
    if fast_clone_call_free {
        ctx.block()
            .cond_br(&fast_path_ok, &fast_pre_label, &slow_pre_label);
    } else {
        ctx.block().br(&slow_pre_label);
    }

    // `--opt-report` (#7766): the clone is a runtime-guarded `Ptr<Shape>`
    // selection for the reads it serves — the report must say so, or the
    // parameter-array case reads as an unserved rule-1 wall (the exact
    // mis-reading #7766 was filed on). Recorded ONLY when the deref block
    // branches INTO the fast clone: an emitted-but-deleted clone selects
    // nothing ("a gate must assert its subject was live").
    if fast_clone_call_free && crate::opt_report::enabled() {
        let (name, local_id) = match matched.element_binding {
            Some(id) => (
                ctx.local_id_to_name
                    .get(&id)
                    .cloned()
                    .unwrap_or_else(|| format!("<local {id}>")),
                Some(id),
            ),
            None => (
                ctx.local_id_to_name
                    .get(&matched.array_id)
                    .map(|n| format!("elements of `{n}`"))
                    .unwrap_or_else(|| format!("elements of <local {}>", matched.array_id)),
                None,
            ),
        };
        let (mode, described) = match &matched.identity {
            ElementShapeIdentity::Class {
                class_name,
                statically_class_proven,
                statically_layout_proven,
                ..
            } => (
                if *statically_layout_proven {
                    "statically layout-proven"
                } else if *statically_class_proven {
                    "statically proven"
                } else {
                    "runtime-guarded"
                },
                format!("class {class_name}"),
            ),
            ElementShapeIdentity::Shape => (
                "runtime-guarded",
                "runtime ordinary shape (class-less records)".to_string(),
            ),
        };
        crate::opt_report::select(
            crate::opt_report::Position::Local,
            &name,
            local_id,
            crate::opt_report::Analysis::PtrShape,
            "Ptr<Shape>",
            1,
            Some(format!(
                "element-shape loop clone ({mode}): {described}, {} tracked field(s); \
                 element reads in this loop lower to offset loads behind the preheader guard",
                matched.props.len()
            )),
        );
    }

    ctx.current_block = slow_pre_idx;
    lower_for_after_init(ctx, init, condition, update, body, "for.element_shape_slow")?;
    if !ctx.block().is_terminated() {
        ctx.block().br(&merge_label);
    }

    ctx.current_block = merge_idx;
    Ok(true)
}
