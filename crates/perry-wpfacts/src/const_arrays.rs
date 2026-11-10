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
//! A literal `[c0, …, cn-1]` has **constant contents** when each element is
//! a Number literal, or a read `N` or `-N` of a module `const` that holds one
//! Number for the whole program (`const_numbers`, N6: `const N = -16`, then
//! `[N, -N, 1, -1]`). The elements are recorded by template at generation
//! (`Model::arr_const`: each run of the literal births the same values) and
//! resolved against the constant Numbers at export (`collect`). A binding
//! all of whose values are immutable tokens of such literals with the same
//! contents holds, at every index and for the whole program, exactly those
//! Numbers (`BindingFacts::const_elems`).
//!
//! `PERRY_FACTS_CONST_ARRAYS=0` (`FactsConfig::no_const_arrays`) records
//! nothing: no token is immutable, no binding has constant contents.

use crate::bindings::{ModuleBindings, Rep};
use crate::gen::Gen;
use crate::model::{Model, NodeId, TmplKind, Tok, TokKind, TOP};
use perry_hir::{Expr, UnaryOp};

/// One element of an array literal that may be a constant Number.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Elem {
    /// A Number literal, as its bit pattern (`-0` and `NaN` are their own
    /// bits: the birth stores the literal as that double).
    Bits(u64),
    /// A read of local `local` of module `module`, negated or not: a
    /// constant when `const_numbers` gives that binding one Number.
    Const { module: u32, local: u32, neg: bool },
}

/// The elements of an array literal of module `module`, when every one may
/// be a constant Number (`Elem`).
pub(crate) fn literal_elems(module: u32, elems: &[Expr]) -> Option<Vec<Elem>> {
    // `-N` after the driver folded a literal `const N` into its reads is
    // the minus of a literal.
    let read = |e: &Expr, neg: bool| match e {
        Expr::LocalGet(local) => Some(Elem::Const {
            module,
            local: *local,
            neg,
        }),
        Expr::Number(n) if neg => Some(Elem::Bits((-*n).to_bits())),
        Expr::Integer(i) if neg => Some(Elem::Bits((-(*i as f64)).to_bits())),
        _ => None,
    };
    elems
        .iter()
        .map(|e| match e {
            Expr::Number(n) => Some(Elem::Bits(n.to_bits())),
            Expr::Integer(i) => Some(Elem::Bits((*i as f64).to_bits())),
            Expr::Unary {
                op: UnaryOp::Neg,
                operand,
            } => read(operand, true),
            e => read(e, false),
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

    /// Plain Array token `t` with a fixed length is never written. One unit
    /// of a per-unit solve sees the other units' writes only through
    /// contracts (`Config::unit`): no Array is immutable there, as no
    /// binding is a constant Number (`const_numbers`).
    pub(crate) fn arr_immutable(&self, t: Tok) -> bool {
        self.const_arrays
            && !self.unit_mode
            && !self.arr_stored.contains(&t)
            && !self.cell_arrs.contains_key(&t)
    }
}

/// The constant contents of node `n`: every value is an immutable token
/// (the caller's `ArrRep::im`) of a literal of constant Numbers, all with
/// the same elements. `number`: the one Number a module binding holds
/// (`const_numbers`).
fn const_elems(
    m: &Model,
    n: NodeId,
    number: &dyn Fn(u32, u32) -> Option<i64>,
) -> Option<Vec<u64>> {
    let node = &m.nodes[n as usize];
    let mut out: Option<Vec<u64>> = None;
    for t in node.pts.to_sorted() {
        let TokKind::Obj(tm, _) = m.toks[t as usize] else {
            return None;
        };
        if !matches!(m.tmpls[tm as usize].kind, TmplKind::Arr { plain: true, .. }) {
            return None;
        }
        let c = m
            .arr_const
            .get(&tm)?
            .iter()
            .map(|e| match *e {
                Elem::Bits(b) => Some(b),
                Elem::Const { module, local, neg } => {
                    // `-N` of `N = 0` is `-0`, as the unary minus gives.
                    let v = number(module, local)? as f64;
                    let v = if neg { -v } else { v };
                    Some(v.to_bits())
                }
            })
            .collect::<Option<Vec<u64>>>()?;
        match &out {
            Some(o) if *o != c => return None,
            _ => out = Some(c),
        }
    }
    out
}

/// Fill `BindingFacts::const_elems` (module docs). Runs after
/// `const_numbers::collect`, whose constants an element may read.
pub(crate) fn collect(g: &Gen<'_>, bindings: &mut [ModuleBindings]) {
    if g.m.aborted || !g.m.const_arrays {
        return;
    }
    let numbers: Vec<Vec<(u32, i64)>> = bindings.iter().map(|b| b.const_numbers.clone()).collect();
    let number = |module: u32, local: u32| {
        let v = numbers.get(module as usize)?;
        let i = v.binary_search_by_key(&local, |p| p.0).ok()?;
        Some(v[i].1)
    };
    for (mi, mb) in bindings.iter_mut().enumerate().take(g.mods.len()) {
        for (id, b) in mb.locals.iter_mut() {
            if !matches!(b.rep, Rep::Arr(r) if r.im) {
                continue;
            }
            if let Some(&n) = g.mods[mi].locals.get(&*id) {
                b.const_elems = const_elems(&g.m, n, &number);
            }
        }
    }
}
