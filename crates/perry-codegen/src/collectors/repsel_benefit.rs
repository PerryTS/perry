//! Profitability of routing a class-id dispatch-tower case to a
//! proven-receiver clone (#7142).
//!
//! The canonical-i32 profitability term that used to live here (#7128, the
//! `15_mandelbrot` refusal) is gone (representation RFC §10.8, P5): whether a
//! local is worth an i32 slot is the whole-program facts' int demand
//! (`codegen/facts_rep.rs`), a property of the value and not of one body.

use std::collections::HashSet;

use perry_hir::{Expr, Function};

/// #7142: how many guarded `this.<declared field>` sites a proven-receiver
/// clone must delete before routing a class-id dispatch-tower case to it pays
/// for the inline shape re-check the route adds.
///
/// The re-check (`expr::class_field_inline_guard::emit_proven_shape_recheck`)
/// is one instance of the same header check the per-access inline guard emits
/// at every `this.field` site inside the public body. So the trade at a tower
/// case is *N in-body checks for 1 at the call site*, and the break-even is
/// exactly `N == 1`: at one field site the route swaps a check for a check,
/// adds a second call site (code size) and a branch, and is strictly worse
/// whenever that single site sits behind a conditional the call does not always
/// reach. At two it deletes one whole check plus its cold guard-call block, and
/// the margin grows with every further site.
///
/// This is a *count*, not a cost model: it has no target-specific term, so it
/// says the same thing on AArch64 and x86-64 (contrast the fusion term in
/// #7146, which does not). It deliberately under-counts — see
/// [`proven_receiver_clone_benefit_sites`].
const TOWER_ROUTE_MIN_FIELD_SITES: u32 = 2;

/// #7142: how many guarded `this.<declared chain field>` sites the
/// proven-receiver clone of `method` deletes.
///
/// Counts the HIR forms the clone lowers guard-free — `PropertyGet`,
/// `PropertySet`, `PropertyUpdate` and `PutValueSet` on `this` — restricted to
/// fields the class chain actually declares, because only those get a fixed
/// slot; anything else keeps its by-name lowering in the clone too.
///
/// Deliberately an UNDER-count in two directions, both of which push toward
/// refusing a route rather than taking one:
///
/// * a site inside a loop is counted once, though it is paid per iteration;
/// A direct `this.other()` call counts when `other` also has a proven-receiver
/// clone: the caller clone routes that nested boundary directly, deleting the
/// public body's method guard in addition to its own field sites.
fn proven_receiver_clone_benefit_sites(
    method: &Function,
    chain_fields: &HashSet<String>,
    pshape_methods: &HashSet<String>,
) -> u32 {
    let mut sites = 0u32;
    super::scalar_method_dispatch::for_each_expr_in_stmts(&method.body, &mut |e| {
        let named = match e {
            Expr::PropertyGet {
                object, property, ..
            }
            | Expr::PropertySet {
                object, property, ..
            }
            | Expr::PropertyUpdate {
                object, property, ..
            } => matches!(object.as_ref(), Expr::This) && chain_fields.contains(property.as_str()),
            Expr::PutValueSet { target, key, .. } => {
                matches!(target.as_ref(), Expr::This)
                    && matches!(key.as_ref(), Expr::String(k) if chain_fields.contains(k.as_str()))
            }
            Expr::Call { callee, .. } => matches!(
                callee.as_ref(),
                Expr::PropertyGet { object, property, .. }
                    if matches!(object.as_ref(), Expr::This)
                        && pshape_methods.contains(property.as_str())
            ),
            _ => false,
        };
        if named {
            sites += 1;
        }
    });
    sites
}

/// #7142: may a class-id dispatch-tower case route to `method`'s
/// proven-receiver clone?
///
/// A pure PROFITABILITY verdict — refusing is always sound (the case keeps
/// calling the public body, which is correct for any receiver), and the
/// soundness half lives entirely in the emitted keys check. See
/// [`TOWER_ROUTE_MIN_FIELD_SITES`] for the break-even argument.
pub(crate) fn tower_route_profitable(
    method: &Function,
    chain_fields: &HashSet<String>,
    pshape_methods: &HashSet<String>,
) -> bool {
    proven_receiver_clone_benefit_sites(method, chain_fields, pshape_methods)
        >= TOWER_ROUTE_MIN_FIELD_SITES
}

#[cfg(test)]
#[path = "repsel_benefit/tests.rs"]
mod tests;
