//! OWN2: a computed read `o[k]` after the store `o[k] = v` of the same
//! bindings reads own properties only (`Cons::IdxOwnLoad`,
//! `solve/own_read.rs`), never the chain's `Object.prototype` members.
//!
//! In a non-suspending function body, `o` is a binding initialised
//! with an object literal of data properties, never assigned and not
//! captured by a closure, and every occurrence of `o` is `o[k]` or
//! `o[k] = v` with one never-assigned key binding `k`, or `return o`: not a
//! `delete`, not the callee of a call (which passes `o` as `this`), not an
//! argument. So until the body returns no other code holds `o`: nothing can
//! delete, redefine, freeze or re-prototype its properties. A read `o[k]` in
//! a statement after an unconditional strict statement `o[k] = v` of the same
//! statement list (closures not entered) runs after that store, with the
//! same key value.
//!
//! The store made `k` own. `o` is extensible and its chain is
//! `Object.prototype`, on which no program accessor runs where facts are
//! claimed: no key can be added there and its members cannot be redefined
//! (DEC 250), `constructor` aside, and an accessor the program installs on
//! `Object.prototype` (`defineProperty`, `__defineSetter__`) is E23, which
//! turns every fact off. So no setter intercepts the store or sees `o` as
//! `this`; an inherited non-writable data property makes a strict store
//! throw, so the read is not reached. A computed store under `__proto__`
//! would re-prototype `o`: a computed key of unknown string never names
//! it (owner rule 10-27), as for every computed store of the solve. An
//! object or ⊤ key (ToPropertyKey runs code, twice) reads as `IdxLoad`
//! (`Cons::IdxOwnKey`); a value of `o` other than a program object too
//! (`own_read_obj`).

use super::*;
use crate::pts::{FxHashMap, FxHashSet};
use perry_hir::tdz_check::{for_each_stmt_deep, for_each_stmts_expr, for_each_stmts_expr_shallow};
use perry_hir::{Expr, Function, Stmt};

fn local(e: &Expr) -> Option<u32> {
    match e {
        Expr::LocalGet(id) => Some(*id),
        _ => None,
    }
}

/// The read `o[k]`, both operands bindings.
fn read(e: &Expr) -> Option<(u32, u32)> {
    match e {
        Expr::IndexGet { object, index, .. } => Some((local(object)?, local(index)?)),
        _ => None,
    }
}

/// The store `o[k] = v`, both operands bindings (PutValue with `o` as the
/// receiver too), and whether a failed [[Set]] throws.
fn store(e: &Expr, strict_fn: bool) -> Option<(u32, u32, bool)> {
    match e {
        Expr::IndexSet { object, index, .. } => Some((local(object)?, local(index)?, strict_fn)),
        Expr::PutValueSet {
            target,
            key,
            receiver,
            strict,
            ..
        } if local(receiver).is_some() && local(receiver) == local(target) => {
            Some((local(target)?, local(key)?, *strict))
        }
        _ => None,
    }
}

/// `{}` (HIR: the empty closed-shape literal) or a literal of data
/// properties without a `__proto__` entry: an ordinary extensible object
/// with no own accessor, whose prototype is `Object.prototype`.
fn plain_literal(e: &Expr) -> bool {
    match e {
        Expr::New {
            class_name, args, ..
        } => args.is_empty() && class_name.starts_with("__AnonShape_"),
        Expr::Object(props) => props.iter().all(|(k, _)| k != "__proto__"),
        _ => false,
    }
}

/// Every statement list of `list`, nested ones included, closures not entered.
fn each_list<'s>(list: &'s [Stmt], f: &mut dyn FnMut(&'s [Stmt])) {
    f(list);
    for s in list {
        match s {
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                each_list(then_branch, f);
                if let Some(e) = else_branch {
                    each_list(e, f);
                }
            }
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } | Stmt::For { body, .. } => {
                each_list(body, f)
            }
            Stmt::Labeled { body, .. } => each_list(std::slice::from_ref(&**body), f),
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                each_list(body, f);
                if let Some(c) = catch {
                    each_list(&c.body, f);
                }
                if let Some(fin) = finally {
                    each_list(fin, f);
                }
            }
            Stmt::Switch { cases, .. } => {
                for c in cases {
                    each_list(&c.body, f);
                }
            }
            _ => {}
        }
    }
}

/// The objects of `f`'s body that only its own keyed accesses touch, each
/// with its one key binding.
fn unshared(f: &Function) -> FxHashMap<u32, u32> {
    let body = &f.body[..];
    let mut lits: FxHashSet<u32> = FxHashSet::default();
    let mut own: FxHashSet<u32> = f.params.iter().map(|p| p.id).collect();
    let mut bad: FxHashSet<u32> = FxHashSet::default();
    let mut allowed: FxHashMap<u32, usize> = FxHashMap::default();
    for_each_stmt_deep(body, &mut |s| match s {
        Stmt::Let { id, init, .. } => {
            own.insert(*id);
            if init.as_ref().is_some_and(plain_literal) {
                lits.insert(*id);
            }
        }
        Stmt::Return(Some(Expr::LocalGet(id))) => *allowed.entry(*id).or_default() += 1,
        Stmt::PreallocateBoxes(ids) | Stmt::PreallocateTdzBoxes(ids) => {
            bad.extend(ids.iter().copied())
        }
        _ => {}
    });
    let mut uses: FxHashMap<u32, usize> = FxHashMap::default();
    let mut key: FxHashMap<u32, u32> = FxHashMap::default();
    for_each_stmts_expr(body, &mut |e| {
        // `o[k]` once, a PutValue store names `o` as target and receiver.
        let keyed = read(e)
            .map(|(o, k)| (o, k, 1))
            .or_else(|| match store(e, true) {
                Some((o, k, _)) if matches!(e, Expr::PutValueSet { .. }) => Some((o, k, 2)),
                Some((o, k, _)) => Some((o, k, 1)),
                None => None,
            });
        if let Some((o, k, n)) = keyed {
            *allowed.entry(o).or_default() += n;
            if *key.entry(o).or_insert(k) != k {
                bad.insert(o);
            }
        }
        match e {
            Expr::LocalGet(id) => *uses.entry(*id).or_default() += 1,
            Expr::LocalSet(id, _) | Expr::Update { id, .. } => {
                bad.insert(*id);
            }
            Expr::Delete(x) => {
                if let Expr::IndexGet { object, .. } | Expr::PropertyGet { object, .. } = x.as_ref()
                {
                    bad.extend(local(object));
                }
            }
            Expr::Call { callee, .. }
            | Expr::CallSpread { callee, .. }
            | Expr::JsCallValue { callee, .. } => {
                if let Expr::IndexGet { object, .. } | Expr::PropertyGet { object, .. } =
                    callee.as_ref()
                {
                    bad.extend(local(object));
                }
            }
            Expr::Closure {
                captures,
                mutable_captures,
                ..
            } => bad.extend(captures.iter().chain(mutable_captures).copied()),
            _ => {}
        }
    });
    key.into_iter()
        .filter(|&(o, k)| {
            lits.contains(&o)
                && own.contains(&k)
                && !bad.contains(&o)
                && !bad.contains(&k)
                && uses.get(&o) == allowed.get(&o)
        })
        .collect()
}

impl<'a> Gen<'a> {
    /// Before `f`'s body is walked: its reads that run after a store of the
    /// same object and key (`own_store_reads`).
    pub(crate) fn note_own_stores(&mut self, f: &'a Function) {
        if f.is_async || f.is_generator || f.was_plain_async {
            return;
        }
        let objs = unshared(f);
        if objs.is_empty() {
            return;
        }
        let reads = &mut self.own_store_reads;
        each_list(&f.body, &mut |list| {
            for (j, s) in list.iter().enumerate() {
                let Stmt::Expr(st) = s else {
                    continue;
                };
                // A failed strict store throws: the read is not reached.
                let Some((o, k, true)) = store(st, f.is_strict) else {
                    continue;
                };
                if objs.get(&o) != Some(&k) {
                    continue;
                }
                for_each_stmts_expr_shallow(&list[j + 1..], &mut |e| {
                    if read(e) == Some((o, k)) {
                        reads.insert(e as *const Expr as usize);
                    }
                });
            }
        });
    }
}
