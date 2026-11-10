//! Immutable Arrays and their constant contents (fact IM; CENSUS-p4 §5
//! lever 6, CENSUS-residual N3).
//!
//! An Array token is **immutable** when nothing writes it after its birth:
//! it is a plain program Array (`bindings::arr_rep` admits it: not foreign,
//! escaped or hard, no named or metadata write), its length never moves
//! (`ArrRep::fixed`), it does not leave its unit (`Model::cell_arrs`), and
//! no write reached it (`Model::arr_stored`). The writes are:
//! - a store under any key, named, index or computed (`Model::store_into`:
//!   `a[i] = v`, `a.x = v`, an `Object.assign` into it, a declared setter);
//! - an `Array.prototype` method that is not in `method_reads_only`, with the
//!   token as its receiver, however it is reached (`builtins::array_method`:
//!   the method call, a folded HIR form, `.call`/`.apply`, a computed key);
//! - a declared native that stores into its receiver's elements
//!   (`builtins::effects`, `elems(this)`).
//!
//! Every other way to change an Array already makes it something `arr_rep`
//! refuses or moves its length to ⊤ (`solve::arr_len`): `delete`,
//! `defineProperty`, `setPrototypeOf` (hard), unknown code (escaped), `pop`,
//! `length = n`, a generic `reverse`/`copyWithin`.
//!
//! A literal `[c0, …, cn-1]` whose elements are all Number literals has
//! **constant contents** (`Model::arr_const`, by template: each run of the
//! literal births the same values). A binding all of whose values are
//! immutable tokens of such literals with the same contents holds, at every
//! index and for the whole program, exactly those Numbers
//! (`BindingFacts::const_elems`).
//!
//! `PERRY_FACTS_CONST_ARRAYS=0` (`FactsConfig::no_const_arrays`) records
//! nothing: no token is immutable, no binding has constant contents.

use crate::model::{Model, NodeId, TmplKind, Tok, TokKind, TOP};
use perry_hir::Expr;

/// The elements of an array literal as Number bit patterns, when every one
/// is a Number literal. A `-0` or `NaN` element is its own bits: the birth
/// stores each literal as that double.
pub(crate) fn literal_numbers(elems: &[Expr]) -> Option<Vec<u64>> {
    elems
        .iter()
        .map(|e| match e {
            Expr::Number(n) => Some(n.to_bits()),
            Expr::Integer(i) => Some((*i as f64).to_bits()),
            _ => None,
        })
        .collect()
}

/// `Array.prototype` methods that never write their receiver: they read its
/// `length` and elements, and what they make is a new object. A callback
/// they run gets the receiver, and a store there is the program's own
/// (`Model::store_into`). Every other name, and a computed key, may write.
pub(crate) fn method_reads_only(name: Option<&str>) -> bool {
    matches!(
        name,
        Some(
            crate::builtins::WK_ITERATOR
                | "next"
                | "return"
                | "throw"
                | "at"
                | "indexOf"
                | "lastIndexOf"
                | "includes"
                | "join"
                | "toString"
                | "toLocaleString"
                | "slice"
                | "concat"
                | "map"
                | "filter"
                | "forEach"
                | "every"
                | "some"
                | "find"
                | "findIndex"
                | "findLast"
                | "findLastIndex"
                | "reduce"
                | "reduceRight"
                | "flat"
                | "flatMap"
                | "keys"
                | "values"
                | "entries"
                | "toReversed"
                | "toSorted"
                | "toSpliced"
                | "with"
                | "hasOwnProperty"
        )
    )
}

impl Model {
    /// Array `t` is written after its birth (the module docs list the
    /// writes). Negative control `PERRY_FACTS_SABOTAGE=arrconst`: a write is
    /// not recorded, so a stored-to literal keeps its constant contents.
    pub(crate) fn arr_written(&mut self, t: Tok) {
        if t == TOP || !self.const_arrays || self.arr_const_sabotage {
            return;
        }
        if self.is_arr(t) {
            self.arr_stored.insert(t);
        }
    }

    /// Plain Array token `t` with a fixed length is never written.
    pub(crate) fn arr_immutable(&self, t: Tok) -> bool {
        self.const_arrays && !self.arr_stored.contains(&t) && !self.cell_arrs.contains_key(&t)
    }
}

/// The constant contents of node `n`: every value is an immutable token
/// (the caller's `ArrRep::im`) of a Number literal, all with the same
/// elements.
pub(crate) fn const_elems(m: &Model, n: NodeId) -> Option<Vec<u64>> {
    let node = &m.nodes[n as usize];
    let mut out: Option<&Vec<u64>> = None;
    for t in node.pts.to_sorted() {
        let TokKind::Obj(tm, _) = m.toks[t as usize] else {
            return None;
        };
        if !matches!(m.tmpls[tm as usize].kind, TmplKind::Arr { plain: true, .. }) {
            return None;
        }
        let c = m.arr_const.get(&tm)?;
        match out {
            Some(o) if o != c => return None,
            _ => out = Some(c),
        }
    }
    out.cloned()
}
