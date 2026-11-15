//! Immutable Arrays and their constant contents (fact IM; CENSUS-p4 §5
//! lever 6, CENSUS-residual N3).
//!
//! An Array token is **immutable** when nothing writes it after its birth:
//! it is a plain program Array (`bindings::arr_rep` admits it: not foreign,
//! escaped or hard, no named or metadata write), its length never moves
//! (`ArrRep::fixed`), it does not leave its unit (`Model::cell_arrs`), and
//! no write reached it (`Model::arr_stored`). The writes are:
//! - a store under any key, named, index or computed (`Model::store_into`:
//!   `a[i] = v`, `a.x = v`, an `Object.assign` into it, `Reflect.set`);
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
//! **Which bindings.** A token's immutability says nothing about a binding
//! that may be read before it is initialized: a hoisted `var` holds
//! `undefined` until its initializer runs, and the facts do not model that
//! value (the HIR does not tell a `var` from a `let`). So only a **rooted**
//! binding is exported as an immutable Array (`BindingFacts::
//! immutable_array`), and only such a binding has contents: a `const` whose
//! initializer is an array literal, a read of a rooted `const`, a
//! conditional of those, or one of those behind its dead-zone check
//! (`rooted`). Every read of a `const` that can run before its declarator
//! carries a TDZ check that throws (`perry_hir::tdz_check`), so a rooted
//! binding's value, when a read yields one, is one of its literals' Arrays.
//! A parameter, a `let`/`var`, a field and a call result are not rooted.
//!
//! `PERRY_FACTS_CONST_ARRAYS=0` (`FactsConfig::no_const_arrays`) records
//! nothing: no token is immutable, no binding has constant contents.

use crate::bindings::{ModuleBindings, Rep};
use crate::gen::Gen;
use crate::model::{Model, NodeId, TmplKind, Tok, TokKind, TOP};
use crate::pts::FxHashMap;
use perry_hir::{Expr, Stmt, UnaryOp};

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
        if t != TOP {
            self.index_slots_merge(t);
        }
        self.arr_written_at(t);
    }

    /// `arr_written` by a store that keeps every element's index: one
    /// under a fixed index, or a call binding its rest array's argument
    /// at a known position. Its index slots stay exact.
    pub(crate) fn arr_written_at(&mut self, t: Tok) {
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
    nodes: &[NodeId],
    number: &dyn Fn(u32, u32) -> Option<i64>,
) -> Option<Vec<u64>> {
    let mut out: Option<Vec<u64>> = None;
    // Every token of the binding's nodes (`bindings::BindingNodes`).
    let toks = nodes
        .iter()
        .flat_map(|&n| m.nodes[n as usize].pts.to_sorted());
    for t in toks {
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

/// The locals `init` reads when it is a rooted initializer (module docs):
/// an array literal (none), a local, a conditional of those, or one of those
/// after its dead-zone check. `false`: any other expression.
fn root_reads(init: &Expr, out: &mut Vec<u32>) -> bool {
    match init {
        Expr::Array(..) => true,
        Expr::LocalGet(id) => {
            out.push(*id);
            true
        }
        Expr::Conditional {
            then_expr,
            else_expr,
            ..
        } => root_reads(then_expr, out) && root_reads(else_expr, out),
        Expr::Sequence(v) => v.last().is_some_and(|last| root_reads(last, out)),
        _ => false,
    }
}

/// Every `const` of `stmts` and of the statements nested in them, with the
/// locals its initializer reads (`root_reads`; `None`: not a rooted
/// initializer, or two declarations of the local disagree). A closure's
/// body is not visited: its bindings are not rooted.
fn const_roots(stmts: &[Stmt], out: &mut FxHashMap<u32, Option<Vec<u32>>>) {
    for s in stmts {
        match s {
            Stmt::Let {
                id,
                mutable: false,
                init: Some(init),
                ..
            } => {
                let mut reads = Vec::new();
                let root = root_reads(init, &mut reads).then_some(reads);
                match out.get_mut(id) {
                    Some(prev) if *prev != root => *prev = None,
                    Some(_) => {}
                    None => {
                        out.insert(*id, root);
                    }
                }
            }
            // Any other declaration of the local makes it no rooted `const`.
            Stmt::Let { id, .. } => {
                out.insert(*id, None);
            }
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                const_roots(then_branch, out);
                if let Some(e) = else_branch {
                    const_roots(e, out);
                }
            }
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => const_roots(body, out),
            Stmt::For { init, body, .. } => {
                if let Some(i) = init {
                    const_roots(std::slice::from_ref(i.as_ref()), out);
                }
                const_roots(body, out);
            }
            Stmt::Labeled { body, .. } => const_roots(std::slice::from_ref(body.as_ref()), out),
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                const_roots(body, out);
                if let Some(c) = catch {
                    const_roots(&c.body, out);
                }
                if let Some(f) = finally {
                    const_roots(f, out);
                }
            }
            Stmt::Switch { cases, .. } => {
                for c in cases {
                    const_roots(&c.body, out);
                }
            }
            _ => {}
        }
    }
}

/// Local `id` is a rooted `const` (module docs). `depth` bounds a chain of
/// reads (a cycle of `const`s cannot be initialized, and is not rooted).
fn rooted(roots: &FxHashMap<u32, Option<Vec<u32>>>, id: u32, depth: u32) -> bool {
    if depth == 0 {
        return false;
    }
    match roots.get(&id) {
        Some(Some(reads)) => reads.iter().all(|r| rooted(roots, *r, depth - 1)),
        _ => false,
    }
}

/// Fill `BindingFacts::immutable_array` and `BindingFacts::const_elems`
/// (module docs). Runs after `const_numbers::collect`, whose constants an
/// element may read.
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
    let nodes = crate::bindings::BindingNodes::new(&g.m);
    for (mi, mb) in bindings.iter_mut().enumerate().take(g.mods.len()) {
        let module = g.modules[mi].1;
        let mut roots = FxHashMap::default();
        const_roots(&module.init, &mut roots);
        let classes = module.classes.iter().flat_map(|c| {
            c.constructor
                .iter()
                .chain(&c.methods)
                .chain(&c.static_methods)
                .chain(c.getters.iter().map(|(_, f)| f))
                .chain(c.setters.iter().map(|(_, f)| f))
        });
        for f in module.functions.iter().chain(classes) {
            const_roots(&f.body, &mut roots);
        }
        for (id, b) in mb.locals.iter_mut() {
            if !matches!(b.rep, Rep::Arr(r) if r.im) || !rooted(&roots, *id, 16) {
                continue;
            }
            b.immutable_array = true;
            if let Some(&n) = g.mods[mi].locals.get(&*id) {
                b.const_elems = const_elems(&g.m, &nodes.of(n), &number);
            }
        }
    }
}
