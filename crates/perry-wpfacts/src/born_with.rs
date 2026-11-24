//! ONCHANGE3 rule A (born-with): an own key a body stores on the object it
//! has just allocated, through a known method, before the object can be
//! observed, is part of that allocation site's birth.
//!
//! `const e = new E(); e.on(f); this.e = e;` with `on(cb) { this.K = cb;
//! return this; }`: `on` runs before anything else can name `e`, and its
//! body stores K first, so every object of that `new` holds an own data
//! property K from then on (ECMA-262 10.1.9.2 OrdinarySetWithOwnDescriptor:
//! with no accessor of K on the chain and a writable data member, [[Set]]
//! creates the own data property; only [[Delete]] removes it, and a delete
//! marks the token hard). A read `recv.K` whose receivers are only such
//! tokens is the own slot (10.1.8.1 OrdinaryGet: own property first): its
//! values are the stores of K that reach the token, never the prototype's
//! member. Later stores are in that value set (the flow-insensitive field).
//!
//! The run (generation, `runs`): a straight-line statement list from the
//! `const e = new E(<literals>)` on, made only of statements that run no
//! code that could name `e`: other `const x = new D(<literals>)`, closure
//! bindings, and method calls `x.m(<locals, closures, literals>)` (never
//! with `e` as an argument). The check (derive, `derive`): each such call's
//! callee set is only store-only bodies (`store_only_keys`); every `new` of
//! the run is of a class whose constructor chain does not leak `this`; no
//! accessor of a stored key exists in the program. Then nothing between the
//! allocation and the last call on `e` can reach `e` (it is in no binding
//! but `e` and the closures, which escape only into the fresh objects'
//! stored slots), and the keys the calls on `e` store are born with it.

use crate::model::{Atom, NodeId, Tok, TokKind, NO_CTX};
use perry_hir::types::LocalId;
use perry_hir::{Expr, Function, Stmt};

/// One run of a statement list (`runs`), in generation's terms.
#[derive(Clone, Debug)]
pub struct BornRec {
    pub module: u32,
    /// The binding of the fresh object.
    pub e: NodeId,
    /// The bindings of every `new` of the run (`e` included).
    pub news: Vec<NodeId>,
    /// The method calls of the run, in order: (the callee's PropertyGet
    /// byte offset, key, the receiver is `e`).
    pub calls: Vec<(u32, Atom, bool)>,
}

/// A syntactic run: `e`, the other `new` locals, the calls (offset, key, on e).
pub(crate) struct Run {
    pub e: LocalId,
    pub news: Vec<LocalId>,
    pub calls: Vec<(u32, String, bool)>,
}

fn literal(e: &Expr) -> bool {
    matches!(
        e,
        Expr::Undefined
            | Expr::Null
            | Expr::Bool(_)
            | Expr::Number(_)
            | Expr::Integer(_)
            | Expr::String(_)
    )
}

fn fresh(s: &Stmt) -> Option<LocalId> {
    match s {
        Stmt::Let {
            id,
            mutable: false,
            init: Some(Expr::New { args, .. }),
            ..
        } if args.iter().all(literal) => Some(*id),
        _ => None,
    }
}

/// The runs of `ss` (see the module doc). A binding assigned anywhere else
/// is no fresh `const` (`mutable: false`).
pub(crate) fn runs(ss: &[Stmt]) -> Vec<Run> {
    let mut out = Vec::new();
    for i in 0..ss.len() {
        let Some(e) = fresh(&ss[i]) else { continue };
        let mut news = vec![e];
        let mut calls = Vec::new();
        for s in &ss[i + 1..] {
            if let Some(x) = fresh(s) {
                news.push(x);
                continue;
            }
            match s {
                Stmt::Let {
                    init: Some(Expr::Closure { .. }),
                    ..
                } => continue,
                Stmt::Expr(Expr::Call { callee, args, .. }) => {
                    let Expr::PropertyGet {
                        object,
                        property,
                        byte_offset,
                        ..
                    } = callee.as_ref()
                    else {
                        break;
                    };
                    let Expr::LocalGet(x) = object.as_ref() else {
                        break;
                    };
                    let quiet = args.iter().all(|a| match a {
                            Expr::LocalGet(l) => *l != e,
                            Expr::Closure { .. } => true,
                            a => literal(a),
                        });
                    if !quiet {
                        break;
                    }
                    calls.push((*byte_offset, property.clone(), *x == e));
                }
                _ => break,
            }
        }
        // Calls on `e` after the last one add nothing.
        while calls.last().is_some_and(|c| !c.2) {
            calls.pop();
        }
        if !calls.is_empty() {
            out.push(Run { e, news, calls });
        }
    }
    out
}

/// The keys a store-only body stores on its receiver: every statement is
/// `this.K = <parameter>`, then at most `return this` / `return`; simple
/// parameters (no default, no rest: evaluating them runs no code). `None`
/// for any other body.
pub(crate) fn store_only_keys(f: &Function) -> Option<Vec<String>> {
    if f.is_async || f.is_generator || f.params.iter().any(|p| p.default.is_some() || p.is_rest) {
        return None;
    }
    let mut keys = Vec::new();
    let n = f.body.len();
    for (i, s) in f.body.iter().enumerate() {
        match s {
            Stmt::Expr(Expr::PropertySet {
                object,
                property,
                value,
                ..
            }) if matches!(object.as_ref(), Expr::This)
                && matches!(value.as_ref(), Expr::LocalGet(l) if f.params.iter().any(|p| p.id == *l)) =>
            {
                keys.push(property.clone())
            }
            // The same store when the key is a member of the chain: lowered
            // as PutValue (6.2.5.6) on `this` with `this` as the receiver.
            Stmt::Expr(Expr::PutValueSet {
                target,
                key,
                value,
                receiver,
                ..
            }) if matches!(target.as_ref(), Expr::This)
                && matches!(receiver.as_ref(), Expr::This)
                && matches!(value.as_ref(), Expr::LocalGet(l) if f.params.iter().any(|p| p.id == *l)) =>
            {
                let Expr::String(k) = key.as_ref() else {
                    return None;
                };
                keys.push(k.clone())
            }
            Stmt::Return(None, _) if i + 1 == n => {}
            Stmt::Return(Some(Expr::This), _) if i + 1 == n => {}
            _ => return None,
        }
    }
    (!keys.is_empty()).then_some(keys)
}

/// The (token, key) pairs rule A proves (module doc).
pub(crate) fn derive(
    g: &crate::gen::Gen,
    tfs: &[crate::derive::TemplateFacts],
) -> crate::pts::FxHashSet<(Tok, Atom)> {
    let m = &g.m;
    let by_id: crate::pts::FxHashMap<_, _> = tfs.iter().map(|t| (t.id, t)).collect();
    let mut out = crate::pts::FxHashSet::default();
    let one_birth = |n: NodeId| -> Option<Vec<Tok>> {
        let nd = &m.nodes[n as usize];
        if nd.prim != 0 || nd.pts.is_empty() {
            return None;
        }
        let ts = nd.pts.to_sorted();
        ts.iter()
            .all(|&t| matches!(m.toks[t as usize], TokKind::Obj(tm, c) if c != NO_CTX && !g.chain_leaks(tm) && !g.chain_returns(tm)))
            .then_some(ts)
    };
    'rec: for r in &m.born_recs {
        let Some(et) = one_birth(r.e) else { continue };
        let [t] = et[..] else { continue };
        if r.news.iter().any(|&n| one_birth(n).is_none()) {
            continue;
        }
        let mut keys: Vec<Atom> = Vec::new();
        for &(off, a, on_e) in &r.calls {
            let Some(rec) = g
                .call_sites_at
                .iter()
                .find(|c| c.module == r.module && c.offset == off && c.call != NO_CTX)
            else {
                continue 'rec;
            };
            let crate::model::ThisArg::Node(o) = m.calls[rec.call as usize].this else {
                continue 'rec;
            };
            let on = &m.nodes[o as usize];
            if on.prim != 0 || on.pts.is_empty() {
                continue 'rec;
            }
            let key = m.atoms[a as usize].clone();
            let mut stored: Option<Vec<Atom>> = None;
            for rt in on.pts.to_sorted() {
                let TokKind::Obj(tm, _) = m.toks[rt as usize] else {
                    continue 'rec;
                };
                let Some(tf) = by_id.get(&tm) else {
                    continue 'rec;
                };
                if !tf.prefix_stable {
                    continue 'rec;
                }
                let Ok(ns) = crate::derive::lookup(m, tf, rt, &key, a) else {
                    continue 'rec;
                };
                for n in ns {
                    let nd = &m.nodes[n as usize];
                    if nd.prim != 0 || nd.pts.is_empty() {
                        continue 'rec;
                    }
                    for ft in nd.pts.to_sorted() {
                        let TokKind::Func(f, _) = m.toks[ft as usize] else {
                            continue 'rec;
                        };
                        let Some(ks) = m.store_only.get(&f) else {
                            continue 'rec;
                        };
                        stored = Some(match stored {
                            None => ks.clone(),
                            Some(s) => s.into_iter().filter(|k| ks.contains(k)).collect(),
                        });
                    }
                }
            }
            if on_e {
                keys.extend(stored.unwrap_or_default());
            }
        }
        // A setter of a stored key would run code with the receiver (10.1.9.2).
        if m.accessors.keys().any(|(_, k)| keys.contains(k)) {
            continue;
        }
        for k in keys {
            out.insert((t, k));
        }
    }
    out
}
