//! Module-scope bindings that hold one Number for the whole program (N6).
//!
//! `const EP = 1 << 17` is a module binding whose node holds only the Number
//! primitive, no token, and the point interval `[131072, 131072]`
//! (`bindings::number_range`): every value the binding ever holds is that
//! integer Number (an interval never contains `-0`). So every read that
//! yields a value yields it, and codegen emits the constant in place of the
//! load of the binding's cell (`ElisionFacts::const_numbers`).
//!
//! Only a `const` is exported. Its reads are covered by the dead zone: a
//! read that can run before the declarator carries a TDZ check
//! (`perry_hir::tdz_check`, placed by `lower::module_tdz`), a separate
//! expression evaluated before the read that throws while the binding is
//! uninitialized, so the read itself yields the binding's value or is never
//! reached. The HIR does not tell a `let` from a `var` (both are `mutable`),
//! and a `var` read before its initializer yields an `undefined` the
//! binding's node does not hold, so neither is exported. The fact is still
//! about values: a `const` whose initializer the intervals do not pin
//! (`const N = Math.random() < 0.5 ? 1 : 2`) is no constant.
//!
//! Only bindings declared by a top-level statement of the module are
//! exported: those are the ones whose cell can be a module global. A
//! CommonJS wrap's `export const X = _cjs.X` never stores its global (its
//! getter reads `module.exports` live), so a wrap exports nothing.
//!
//! The initializer `1 << 17` is a point only because bitwise operators over
//! single values keep their exact interval (`model::range::eval_exact`);
//! without it a shift is the whole int32 range.
//!
//! `PERRY_FACTS_CONST_NUMBERS=0` (`FactsConfig::no_const_numbers`) turns the
//! exact intervals off and exports nothing.

use crate::bindings::ModuleBindings;
use crate::gen::Gen;

/// Integers a double holds exactly.
const MAX_EXACT: u64 = 1 << 53;

/// Fill `ModuleBindings::const_numbers` (module docs).
pub(crate) fn collect(g: &Gen<'_>, bindings: &mut [ModuleBindings]) {
    // The per-unit solve sees other units' writes of a binding only through
    // contracts: it exports nothing.
    if g.m.aborted || g.view.is_some() || !g.m.const_numbers {
        return;
    }
    let nodes = crate::bindings::BindingNodes::new(&g.m);
    for (mi, mb) in bindings.iter_mut().enumerate().take(g.modules.len()) {
        let mut out: Vec<(u32, i64)> = Vec::new();
        if g.modules[mi].1.is_commonjs_wrap() {
            continue;
        }
        for s in g.init_of(mi) {
            // `mutable: false` is a `const`: the HIR does not tell a `let`
            // from a `var`, and a `var` read before its initializer runs
            // yields `undefined`, which the binding's node does not hold.
            let perry_hir::Stmt::Let {
                id, mutable: false, ..
            } = s
            else {
                continue;
            };
            let Some(&n) = g.mods[mi].locals.get(id) else {
                continue;
            };
            let Some((lo, hi)) = crate::bindings::number_range_of(&g.m, &nodes.of(n)) else {
                continue;
            };
            // Negative control (`PERRY_FACTS_SABOTAGE=constnum`): any
            // bounded Number binding is its lower bound.
            if (lo == hi || g.m.const_number_sabotage) && lo.unsigned_abs() <= MAX_EXACT {
                out.push((*id, lo));
            }
        }
        out.sort_unstable();
        out.dedup_by_key(|(id, _)| *id);
        mb.const_numbers = out;
    }
}
