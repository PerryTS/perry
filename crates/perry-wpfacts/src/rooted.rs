//! DECISIONS 149b: receiver-rooted stores. A store's object is rooted in
//! its function G when every value it can be is reached from G's `this` by
//! a chain of loads (`this.a[i].b`, or a local of G only ever assigned such
//! a chain, `node = this.root; .. node = node.left`): the store writes into
//! the instance G runs on, which the whole program runs in the caller's
//! own objects. A greatest fixpoint over G's own locals; a closure is its
//! own function (a local it may assign is not rooted).

use perry_hir::walker::{stmt_any_expr, walk_expr_children};
use perry_hir::{Expr, Stmt};
use std::collections::{BTreeMap, BTreeSet};

type Local = perry_hir::types::LocalId;

/// The rooted stores of a function body (by expression address, like
/// `CtorPhase::phase_stores`), each with whether it is deep (DECISIONS
/// 149d): its object is a load from a rooted value (`this.a`, `this.a[i]`,
/// a local only ever assigned such loads), not `this` itself.
pub fn stores(body: &[Stmt]) -> BTreeMap<usize, bool> {
    let mut writes: Vec<(Local, Option<&Expr>)> = Vec::new();
    let mut lets = BTreeSet::new();
    collect_lets(body, &mut lets, &mut writes);
    let mut bad = BTreeSet::new();
    let mut with = false;
    for s in body {
        stmt_any_expr(s, &mut |e| {
            scan(e, &mut writes, &mut bad, &mut with);
            false
        });
    }
    let mut out = BTreeMap::new();
    if with {
        // `with` may assign any local through its object environment.
        return out;
    }
    let mut rooted: BTreeSet<Local> = lets.difference(&bad).copied().collect();
    loop {
        let keep: BTreeSet<Local> = rooted
            .iter()
            .copied()
            .filter(|id| {
                writes
                    .iter()
                    .filter(|(w, _)| w == id)
                    .all(|(_, v)| v.is_none_or(|v| is_rooted(v, &rooted)))
            })
            .collect();
        if keep.len() == rooted.len() {
            break;
        }
        rooted = keep;
    }
    // The deep locals: the same greatest fixpoint within the rooted ones.
    let mut deep: BTreeSet<Local> = rooted.clone();
    loop {
        let keep: BTreeSet<Local> = deep
            .iter()
            .copied()
            .filter(|id| {
                writes
                    .iter()
                    .filter(|(w, _)| w == id)
                    .all(|(_, v)| v.is_none_or(|v| is_deep(v, &rooted, &deep)))
            })
            .collect();
        if keep.len() == deep.len() {
            break;
        }
        deep = keep;
    }
    for s in body {
        stmt_any_expr(s, &mut |e| {
            collect_stores(e, &rooted, &deep, &mut out);
            false
        });
    }
    out
}

fn is_rooted(e: &Expr, rooted: &BTreeSet<Local>) -> bool {
    match e {
        Expr::This => true,
        Expr::PropertyGet { object, .. } | Expr::IndexGet { object, .. } => {
            is_rooted(object, rooted)
        }
        Expr::LocalGet(id) => rooted.contains(id),
        Expr::LocalSet(_, v) => is_rooted(v, rooted),
        _ => false,
    }
}

fn is_deep(e: &Expr, rooted: &BTreeSet<Local>, deep: &BTreeSet<Local>) -> bool {
    match e {
        Expr::PropertyGet { object, .. } | Expr::IndexGet { object, .. } => {
            is_rooted(object, rooted)
        }
        Expr::LocalGet(id) => deep.contains(id),
        Expr::LocalSet(_, v) => is_deep(v, rooted, deep),
        _ => false,
    }
}

/// `let` locals of the body and their initializers (a `let` this misses
/// is not a candidate: only precision is lost).
fn collect_lets<'a>(
    ss: &'a [Stmt],
    lets: &mut BTreeSet<Local>,
    writes: &mut Vec<(Local, Option<&'a Expr>)>,
) {
    for s in ss {
        match s {
            Stmt::Let { id, init, .. } => {
                lets.insert(*id);
                writes.push((*id, init.as_ref()));
            }
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                collect_lets(then_branch, lets, writes);
                if let Some(b) = else_branch {
                    collect_lets(b, lets, writes);
                }
            }
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
                collect_lets(body, lets, writes)
            }
            Stmt::For { init, body, .. } => {
                if let Some(i) = init {
                    collect_lets(std::slice::from_ref(i.as_ref()), lets, writes);
                }
                collect_lets(body, lets, writes);
            }
            Stmt::Labeled { body, .. } => {
                collect_lets(std::slice::from_ref(body.as_ref()), lets, writes)
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                collect_lets(body, lets, writes);
                if let Some(c) = catch {
                    collect_lets(&c.body, lets, writes);
                }
                if let Some(f) = finally {
                    collect_lets(f, lets, writes);
                }
            }
            Stmt::Switch { cases, .. } => {
                for c in cases {
                    collect_lets(&c.body, lets, writes);
                }
            }
            _ => {}
        }
    }
}

/// Every assignment of a local in the body (closures excluded: a local a
/// closure may assign is `bad`).
fn scan<'a>(
    e: &'a Expr,
    writes: &mut Vec<(Local, Option<&'a Expr>)>,
    bad: &mut BTreeSet<Local>,
    with: &mut bool,
) {
    match e {
        Expr::LocalSet(id, v) => writes.push((*id, Some(v))),
        Expr::ScopedTemp { id, value, .. } => writes.push((*id, Some(value))),
        Expr::WithGet { .. } | Expr::WithSet { .. } => *with = true,
        Expr::Closure {
            mutable_captures, ..
        } => bad.extend(mutable_captures.iter().copied()),
        _ => {}
    }
    walk_expr_children(e, &mut |c| scan(c, writes, bad, with));
}

fn collect_stores(
    e: &Expr,
    rooted: &BTreeSet<Local>,
    deep: &BTreeSet<Local>,
    out: &mut BTreeMap<usize, bool>,
) {
    match e {
        Expr::PropertySet { object, .. }
        | Expr::PropertyUpdate { object, .. }
        | Expr::IndexSet { object, .. }
        | Expr::IndexUpdate { object, .. }
            if is_rooted(object, rooted) =>
        {
            out.insert(e as *const Expr as usize, is_deep(object, rooted, deep));
        }
        // Its two stores (target, receiver) share the site's expression.
        Expr::PutValueSet {
            target, receiver, ..
        } if is_rooted(target, rooted) && is_rooted(receiver, rooted) => {
            out.insert(e as *const Expr as usize, is_deep(target, rooted, deep));
        }
        _ => {}
    }
    walk_expr_children(e, &mut |c| collect_stores(c, rooted, deep, out));
}
