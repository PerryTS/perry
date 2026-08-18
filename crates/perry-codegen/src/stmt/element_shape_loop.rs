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
//!    emits nothing and the reads lower through the fact). No store of any
//!    kind, no call, no closure, no `await`, no update other than the
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
//! ## Extension plan (write-up for #5093 / #7480)
//!
//! The clone still pays a residual per-element check — `keys_array` identity,
//! `field_count`, the per-object descriptor flag and the typed-layout intact
//! bit — because the array-level invariant deliberately does not cover them.
//! Folding them into `element_class_of_bits` would make the reads bare, but it
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
use crate::types::{DOUBLE, I1, I32};

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

#[derive(Debug)]
struct ElementShapeVersionedLoop {
    counter_id: u32,
    bound: ElementShapeLoopBound,
    array_id: u32,
    class_name: String,
    expected_class_id: u32,
    keys_global_name: String,
    /// The native-region E1--E5 proof already establishes every element's
    /// exact class for this array's whole lifetime. When true, the preheader
    /// need not rebuild the weaker runtime invariant by scanning the array.
    statically_proven: bool,
    /// property name -> packed slot index.
    fields: std::collections::BTreeMap<String, u32>,
    /// #7771: the body's `const r = arr[counter]` binding in the two-statement
    /// form; `None` for the original single-statement accumulator body.
    element_binding: Option<u32>,
    accumulator_id: u32,
}

/// Effect-free expression walk for the element-shape loop.
///
/// Admits exactly: tracked `arr[counter].prop` reads on ONE array, numeric
/// locals, numeric literals, and pure arithmetic / `Math` (libm intrinsics
/// cannot trigger a GC). Everything else bails the whole match — a catch-all
/// that silently accepted an unknown expression would be the #6377 shape, and
/// worse here, because an unadmitted expression is what would smuggle a call
/// into a body the revocation argument assumes is call-free.
fn element_shape_loop_pure_expr_collect(
    ctx: &FnCtx<'_>,
    expr: &perry_hir::Expr,
    counter_id: u32,
    accumulator_id: u32,
    element_binding: Option<u32>,
    array: &mut Option<u32>,
    props: &mut std::collections::BTreeSet<String>,
) -> bool {
    use perry_hir::Expr;
    match expr {
        Expr::PropertyGet {
            object, property, ..
        } => match object.as_ref() {
            Expr::IndexGet { object, index } => {
                let (Expr::LocalGet(arr_id), Expr::LocalGet(idx_id)) =
                    (object.as_ref(), index.as_ref())
                else {
                    return false;
                };
                // The index must be the loop counter itself. An offset index
                // (`arr[j + 1]`) would need the preheader's `length >= bound`
                // check widened; deliberately out of the first slice.
                if *idx_id != counter_id || *arr_id == counter_id {
                    return false;
                }
                match array {
                    Some(a) if *a == *arr_id => {}
                    Some(_) => return false, // one array per loop
                    None => *array = Some(*arr_id),
                }
                props.insert(property.clone());
                true
            }
            // #7771: `r.field` through the body's `const r = arr[counter]`
            // binding is the same tracked read spelled through the Let the
            // body match admitted; the binding already pins (array, counter),
            // so only the property is left to record.
            Expr::LocalGet(recv_id) if element_binding == Some(*recv_id) => {
                props.insert(property.clone());
                true
            }
            _ => false,
        },
        // A bare read of the array, the counter, or the element binding as a
        // VALUE could flow it into arbitrary lowering; only scalar reads the
        // analysis proves numeric are admitted. The element binding is
        // excluded EXPLICITLY rather than via the numeric test: a bare `r`
        // would hand out a reference the clone's skipped `Let` never bound
        // (#7771), and betting that exclusion on a type predicate is the
        // #6377 shape this walk's docs warn about.
        Expr::LocalGet(id) => {
            element_binding != Some(*id)
                && array.is_none_or(|a| a != *id)
                && (*id == accumulator_id || crate::type_analysis::is_numeric_expr(ctx, expr))
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
        // element class.
        //
        // The gate had to go for the object-literal kernel: at match time no
        // fact is installed yet, so `is_numeric_expr` cannot see through
        // `keep[j].v` (its `PropertyGet` arm resolves the owner through
        // `receiver_class_name`, which by design does not type an
        // object-literal element). Keeping it would have declined #7480's own
        // kernel before the class resolver was ever consulted.
        Expr::Binary { left, right, .. } => {
            element_shape_loop_pure_expr_collect(
                ctx,
                left,
                counter_id,
                accumulator_id,
                element_binding,
                array,
                props,
            ) && element_shape_loop_pure_expr_collect(
                ctx,
                right,
                counter_id,
                accumulator_id,
                element_binding,
                array,
                props,
            )
        }
        Expr::NumberCoerce(operand) => element_shape_loop_pure_expr_collect(
            ctx,
            operand,
            counter_id,
            accumulator_id,
            element_binding,
            array,
            props,
        ),
        Expr::MathImul(left, right) | Expr::MathPow(left, right) => {
            element_shape_loop_pure_expr_collect(
                ctx,
                left,
                counter_id,
                accumulator_id,
                element_binding,
                array,
                props,
            ) && element_shape_loop_pure_expr_collect(
                ctx,
                right,
                counter_id,
                accumulator_id,
                element_binding,
                array,
                props,
            )
        }
        Expr::MathMin(values) | Expr::MathMax(values) => values.iter().all(|e| {
            element_shape_loop_pure_expr_collect(
                ctx,
                e,
                counter_id,
                accumulator_id,
                element_binding,
                array,
                props,
            )
        }),
        Expr::MathAbs(value)
        | Expr::MathSqrt(value)
        | Expr::MathFloor(value)
        | Expr::MathCeil(value)
        | Expr::MathRound(value)
        | Expr::MathTrunc(value)
        | Expr::MathSign(value)
        | Expr::MathF16round(value) => element_shape_loop_pure_expr_collect(
            ctx,
            value,
            counter_id,
            accumulator_id,
            element_binding,
            array,
            props,
        ),
        _ => false,
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

/// Match `for (let j = k0; j < B; j++) acc = <pure over arr[j].field>`.
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
    use perry_hir::{CompareOp, Expr, UpdateOp};

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

    // Store-free body, in one of two admitted shapes (see the module docs):
    //
    //   1. `acc = <pure numeric over arr[j].field>` — the original single
    //      statement;
    //   2. `const r = arr[j]; acc = <pure numeric over r.field>` — #7771's
    //      element-binding form, the shape real read loops are written in.
    //      The binding is VIRTUAL inside the fast clone: its `Let` emits
    //      nothing (`stmt/let_stmt.rs`) and every `r.field` lowers through
    //      the fact, so the revocation argument (no store, no call in the
    //      clone) is unchanged. `const`-only, deliberately: a `var` binding
    //      is function-scoped and observable after the loop, where the
    //      skipped `Let` would leave the slot holding its pre-loop value.
    //
    // NOTHING else is admitted.
    let (element_binding, acc_id, value) = match body {
        [Stmt::Expr(Expr::LocalSet(acc_id, value))] => (None, acc_id, value),
        [Stmt::Let {
            id,
            mutable: false,
            init: Some(Expr::IndexGet { object, index }),
            ..
        }, Stmt::Expr(Expr::LocalSet(acc_id, value))] => {
            let (Expr::LocalGet(arr_id), Expr::LocalGet(idx_id)) =
                (object.as_ref(), index.as_ref())
            else {
                return None;
            };
            // Same receiver/index discipline as the walk's IndexGet arm: the
            // fetch must be `arr[counter]` exactly.
            if *idx_id != counter_id || *arr_id == counter_id || *id == counter_id {
                return None;
            }
            // The binding must be a plain, loop-owned const local. A boxed or
            // captured binding lives in a cell the skipped `Let` would leave
            // stale for an observer outside the clone; a module-global id is
            // not a body-scoped binding at all.
            if *id == *arr_id
                || ctx.boxed_vars.contains(id)
                || ctx.module_globals.contains_key(id)
                || ctx.closure_captures.contains_key(id)
            {
                return None;
            }
            (Some((*id, *arr_id)), acc_id, value)
        }
        _ => return None,
    };
    if *acc_id == counter_id
        || !ctx.locals.contains_key(acc_id)
        || ctx.boxed_vars.contains(acc_id)
        || ctx.module_globals.contains_key(acc_id)
        // The declared type is only a candidate. The lowering validates the
        // accumulator's current NaN-box tag in the preheader before installing
        // the numeric fact for the fast clone.
        || !matches!(ctx.local_type_hint(acc_id), Some(perry_hir::types::Type::Number | perry_hir::types::Type::Int32))
    {
        return None;
    }
    // The binding form pins the array before the walk runs, so a body mixing
    // `r.field` with `other[j].field` is declined by the walk's one-array rule.
    let mut array: Option<u32> = element_binding.map(|(_, arr_id)| arr_id);
    let element_binding = element_binding.map(|(id, _)| id);
    if element_binding == Some(*acc_id) {
        return None;
    }
    let mut props: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    if !element_shape_loop_pure_expr_collect(
        ctx,
        value,
        counter_id,
        *acc_id,
        element_binding,
        &mut array,
        &mut props,
    ) {
        return None;
    }
    let array_id = array?;
    if props.is_empty() || array_id == *acc_id || array_id == counter_id {
        return None;
    }
    match bound {
        ElementShapeLoopBound::Local(bound_id) => {
            if bound_id == array_id || bound_id == *acc_id || Some(bound_id) == element_binding {
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
    // step 2b). A closure-captured array lives in a capture cell that a plain
    // slot store would not update, so the two views could disagree; decline
    // rather than repair only half of them.
    if ctx.closure_captures.contains_key(&array_id) {
        return None;
    }
    if !local_bound_is_loop_invariant(condition?, update, body, array_id) {
        return None;
    }

    let class_name = element_class_name(ctx, array_id, counter_id)?;
    if CLASS_FIELD_LOOP_CLASS_DENYLIST.contains(&class_name.as_str()) {
        return None;
    }
    let class = ctx.classes.get(&class_name)?;
    if !class.computed_members.is_empty() {
        return None;
    }
    // An `extends`-ing element class would put the field's slot behind an
    // inherited layout the packed index does not describe on its own; and a
    // native base (`extends Array`) is exactly the #7573/#7603 hazard. Decline.
    if class.extends_name.is_some() {
        return None;
    }
    let expected_class_id = *ctx.class_ids.get(&class_name)?;
    let keys_global_name = ctx.class_keys_globals.get(&class_name)?.clone();
    let statically_proven = ctx
        .native_facts
        .exact_element_class(array_id)
        .is_some_and(|proven| proven == class_name);

    let mut fields = std::collections::BTreeMap::new();
    for prop in props {
        if CLASS_FIELD_LOOP_PROP_DENYLIST.contains(&prop.as_str()) {
            return None;
        }
        // Accessors route through synthesized __get_/__set_ methods before the
        // class-field diamond; mirror that dispatch gate exactly.
        if ctx
            .methods
            .contains_key(&(class_name.clone(), format!("__get_{prop}")))
            || ctx
                .methods
                .contains_key(&(class_name.clone(), format!("__set_{prop}")))
        {
            return None;
        }
        let field_index = crate::type_analysis::class_field_global_index(ctx, &class_name, &prop)?;
        let raw_f64 = crate::type_analysis::class_field_declared_type(ctx, &class_name, &prop)
            .as_ref()
            .is_some_and(crate::typed_shape::type_is_raw_f64_candidate);
        if !raw_f64 {
            return None;
        }
        fields.insert(prop, field_index);
    }

    Some(ElementShapeVersionedLoop {
        counter_id,
        bound,
        array_id,
        class_name,
        expected_class_id,
        keys_global_name,
        statically_proven,
        fields,
        element_binding,
        accumulator_id: *acc_id,
    })
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
    // The fast clone reads the counter through its canonical i32 slot; without
    // one it would win nothing (and the element GEP would need an fptosi).
    if !ctx.i32_counter_slots.contains_key(&matched.counter_id) {
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
        ElementShapeLoopBound::Local(bound_id) => Some({
            let bound_d = lower_expr(ctx, &perry_hir::Expr::LocalGet(bound_id))?;
            let is_number = emit_js_value_is_number(ctx, &bound_d);
            let range_idx = ctx.new_block("element_shape.loop.bound.range");
            let convert_idx = ctx.new_block("element_shape.loop.bound.convert");
            let check_idx = ctx.new_block("element_shape.loop.shape_check");
            let range_label = ctx.block_label(range_idx);
            let convert_label = ctx.block_label(convert_idx);
            let check_label = ctx.block_label(check_idx);
            ctx.block()
                .cond_br(&is_number, &range_label, &slow_pre_label);

            ctx.current_block = range_idx;
            let ge_zero = ctx.block().fcmp("oge", &bound_d, "0.0");
            let le_max = {
                let max_literal = format!("{:.1}", i32::MAX as f64);
                ctx.block().fcmp("ole", &bound_d, &max_literal)
            };
            let in_range = ctx.block().and(I1, &ge_zero, &le_max);
            ctx.block()
                .cond_br(&in_range, &convert_label, &slow_pre_label);

            ctx.current_block = convert_idx;
            let bound_i32 = ctx.block().fptosi(DOUBLE, &bound_d, I32);
            let roundtrip = ctx.block().sitofp(I32, &bound_i32, DOUBLE);
            let is_integral = ctx.block().fcmp("oeq", &roundtrip, &bound_d);
            ctx.block()
                .cond_br(&is_integral, &check_label, &slow_pre_label);

            ctx.current_block = check_idx;
            bound_i32
        }),
    };

    let trip_count = match &materialized_bound {
        Some(bound) => {
            crate::expr::element_shape_guard::ElementShapeLoopTripCount::Bound(bound.as_str())
        }
        None => crate::expr::element_shape_guard::ElementShapeLoopTripCount::ArrayLength,
    };
    let expected_class_id_str = matched.expected_class_id.to_string();
    let (elements_base, expected_shape_id, shape_ok, bound_i32) =
        crate::expr::element_shape_guard::emit_element_shape_loop_preheader_check(
            ctx,
            matched.array_id,
            &expected_class_id_str,
            &matched.keys_global_name,
            trip_count,
            &slow_pre_label,
            matched.statically_proven,
        )?;
    let accumulator = lower_expr(ctx, &perry_hir::Expr::LocalGet(matched.accumulator_id))?;
    let accumulator_is_number = emit_js_value_is_number(ctx, &accumulator);
    let fast_path_ok = ctx.block().and(I1, &shape_ok, &accumulator_is_number);
    // Deliberately unterminated: it branches into the fast clone only after
    // the clone is PROVEN call-free below.
    let deref_idx = ctx.current_block;

    let scope_id = ctx.next_loop_proof_scope_id();
    let fast_scan_start = ctx.func.num_blocks();
    ctx.current_block = fast_pre_idx;
    ctx.element_shape_loop_facts
        .push(crate::expr::ElementShapeLoopFact {
            array_local_id: matched.array_id,
            index_local_id: matched.counter_id,
            scope_id,
            class_name: matched.class_name.clone(),
            elements_base,
            expected_shape_id,
            side_exit_label: slow_pre_label.clone(),
            fields: matched.fields.clone(),
            element_binding: matched.element_binding,
            numeric_accumulator: matched.accumulator_id,
        });
    let lowered = lower_for_after_init_with_i32_bound(
        ctx,
        init,
        condition,
        update,
        body,
        "for.element_shape_fast",
        Some((matched.counter_id, bound_i32)),
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
        crate::opt_report::select(
            crate::opt_report::Position::Local,
            &name,
            local_id,
            crate::opt_report::Analysis::PtrShape,
            "Ptr<Shape>",
            1,
            Some(format!(
                "element-shape loop clone ({}): class {}, {} tracked field(s); \
                 element reads in this loop lower to offset loads behind the preheader guard",
                if matched.statically_proven {
                    "statically proven"
                } else {
                    "runtime-guarded"
                },
                matched.class_name,
                matched.fields.len()
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
