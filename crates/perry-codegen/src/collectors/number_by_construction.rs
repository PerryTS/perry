//! #8105: locals that hold a JS **Number by construction**, exposed to
//! `type_analysis::is_numeric_expr`.
//!
//! ## The gap this closes
//!
//! Every numeric proof `is_numeric_expr` had for a bare `LocalGet` was either
//! an integer-range fact (`integer_locals`, `unsigned_i32_locals`,
//! `int_valued_i64_locals`) or `FnCtx::stable_local_type_proof`, which answers
//! `None` the moment the local is written a second time. A plain fractional
//! accumulator therefore had **no** numeric proof at all:
//!
//! ```ignore
//! let x = 0.0;
//! let y = 0.0;
//! while (x * x + y * y <= 4.0 && iter < MAX_ITER) {
//!     const xtemp = x * x - y * y + cx;
//!     y = 2.0 * x * y + cy;
//!     x = xtemp;
//! }
//! ```
//!
//! `x` and `y` are reassigned, so `expr/binary.rs`'s "both operands are
//! statically primitive" test failed and **every multiply** bailed to the
//! BigInt-aware `js_dynamic_mul` — six opaque calls per iteration of
//! `benchmarks/suite/15_mandelbrot.ts`'s inner loop. `+` and `-` stayed inline
//! because a `Binary { op: Mul, .. }` operand IS numeric by the recursive rule,
//! which is why the symptom was "only the multiplies escape".
//!
//! ## The proof
//!
//! [`collect_number_by_construction_locals`] is the existing
//! `collectors/ptr_shape_numeric.rs` locals fixpoint (#7770), which is already
//! trusted for a strictly harder claim: it licenses a bare `load double` on a
//! proven numeric FIELD with no coercion and no value check. It admits a local
//! only when its `let` initialiser **and every later write** is an expression
//! the spec guarantees evaluates to a Number, judged structurally — declared
//! types are never evidence (Perry does not enforce annotations, #7773).
//!
//! A JS Number's Perry representation IS its raw double (numbers carry no
//! NaN-box tag), so "the value is a Number" and "the f64 in this slot is a
//! canonical double" are the same statement. That is exactly what the numeric
//! fast path needs for both of its steps: skipping the dynamic helper, and
//! skipping the residual `js_number_coerce` in
//! `binary.rs::operand_needs_residual_coerce`.
//!
//! ## Why the candidate set is fail-closed
//!
//! Candidates are the ids with a `Stmt::Let` in the scanned body, so a
//! PARAMETER is never admitted (its incoming value is unconstrained, and a
//! read can precede the first assignment), and neither is a local captured
//! from an enclosing scope (its writes live outside this walk). Closure-boxed
//! locals and module globals are excluded outright. A `let x;` with no
//! initialiser is `undefined`, which is not a Number, and drops the local.

use std::collections::{HashMap, HashSet};

use perry_hir::Stmt;

/// `PERRY_NUMBER_BY_CONSTRUCTION` gate. Enabled by default; `=0`/`off`/`false`
/// empties the fact, reverting every reassigned numeric accumulator to the
/// BigInt-aware dynamic-helper routing (pre-#8105 behaviour). Kept as an env
/// flag for A/B bisection, consistent with the sibling codegen fast paths
/// (`PERRY_INLINE_NONBIGINT_BITWISE`, `PERRY_PTR_SHAPE_LOCALS`); both the
/// build-level probe and the object cache key it, so a warm cache cannot serve
/// an object built under the other setting.
pub(crate) fn enabled() -> bool {
    !matches!(
        std::env::var("PERRY_NUMBER_BY_CONSTRUCTION").as_deref(),
        Ok("0") | Ok("off") | Ok("false")
    )
}

/// Locals whose every write is number-producing by construction.
///
/// Deliberately independent of `PERRY_PTR_SHAPE_LOCALS`: the fact is about
/// locals, not about shape promotion, so it must not appear and disappear with
/// the repsel gate. The `const_local_inits` chase the `Ptr<Shape>` pass feeds
/// the same fixpoint is a strictly additional edge — every single-`Let` const
/// it resolves is itself a candidate here, so the fixpoint reaches the same
/// verdict without it.
pub(crate) fn collect_number_by_construction_locals(
    stmts: &[Stmt],
    boxed_vars: &HashSet<u32>,
    module_globals: &HashMap<u32, String>,
    not_bigint_locals: &HashSet<u32>,
) -> HashSet<u32> {
    if !enabled() {
        return HashSet::new();
    }
    super::ptr_shape::collect_numeric_by_construction_locals_for_type_analysis(
        stmts,
        boxed_vars,
        module_globals,
        not_bigint_locals,
        &HashMap::new(),
    )
}
