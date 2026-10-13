//! Callee-identity facts (fninfer, owner decision 140; the fact source for
//! slice B, proven-callee `new`, and for direct closure calls).
//!
//! For each value this pass tracks a small set of the constructors and
//! function bodies it may hold: a class (`ClassRef`) or a function body
//! (`FuncRef`, a closure). The sets flow through locals, module globals,
//! function returns and property names (flow-insensitive, joined per name
//! over the whole module), so `new (C || (C = alloc.getNodeConstructor()))`
//! resolves when `getNodeConstructor` is a closure returning a class.
//!
//! Like every fninfer fact these are candidates, never proofs: a consumer
//! admits the direct form at the site only behind an O(1) identity compare
//! of the callee value, and every other value takes the generic path. A
//! wrong or missing fact costs speed, never correctness. `undefined`/`null`
//! contribute nothing (calling them throws on the generic path anyway).

use std::collections::{BTreeSet, HashMap};

use super::{walk_stmts, Unit};
use crate::ir::*;
use crate::types::{FuncId, GlobalId, LocalId};

/// Members kept before a set widens to unknown.
pub const CALLEE_CAP: usize = 4;
const ROUNDS: usize = 6;

/// One possible callee.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CalleeId {
    Class(String),
    Func(FuncId),
}

/// A value's possible callees. `Unknown` is top.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Callees {
    #[default]
    None,
    Set(BTreeSet<CalleeId>),
    Unknown,
}

impl Callees {
    fn one(c: CalleeId) -> Self {
        Callees::Set(std::iter::once(c).collect())
    }
    fn join(&self, o: &Callees) -> Callees {
        match (self, o) {
            (Callees::Unknown, _) | (_, Callees::Unknown) => Callees::Unknown,
            (Callees::None, x) | (x, Callees::None) => x.clone(),
            (Callees::Set(a), Callees::Set(b)) => {
                let u: BTreeSet<CalleeId> = a.union(b).cloned().collect();
                if u.len() > CALLEE_CAP {
                    Callees::Unknown
                } else {
                    Callees::Set(u)
                }
            }
        }
    }
    fn ids(&self) -> Option<&BTreeSet<CalleeId>> {
        match self {
            Callees::Set(s) => Some(s),
            _ => None,
        }
    }
}

/// Result of [`infer_callees`].
#[derive(Debug, Default)]
pub struct CalleeFacts {
    /// `new <expr>(…)` sites (`NewDynamic`), keyed by source byte offset.
    pub new_sites: HashMap<u32, Callees>,
    /// Calls whose callee is a value (not a `FuncRef`/static method):
    /// counts of all, and resolved to 1..=CALLEE_CAP bodies.
    pub value_calls: usize,
    pub value_calls_resolved: usize,
    pub value_calls_single: usize,
}

impl CalleeFacts {
    pub fn new_resolved(&self) -> usize {
        self.new_sites
            .values()
            .filter(|c| c.ids().is_some())
            .count()
    }
    pub fn new_single(&self) -> usize {
        self.new_sites
            .values()
            .filter(|c| c.ids().is_some_and(|s| s.len() == 1))
            .count()
    }
}

#[derive(Default, PartialEq)]
struct St {
    locals: HashMap<LocalId, Callees>,
    globals: HashMap<GlobalId, Callees>,
    rets: HashMap<FuncId, Callees>,
    props: HashMap<String, Callees>,
}

fn val(e: &Expr, st: &St) -> Callees {
    match e {
        Expr::ClassRef(n) => Callees::one(CalleeId::Class(n.clone())),
        // A class expression inside a function body is evaluated afresh;
        // every evaluation instantiates the same template.
        Expr::ClassExprFresh { template, .. } => Callees::one(CalleeId::Class(template.clone())),
        Expr::FuncRef(f) => Callees::one(CalleeId::Func(*f)),
        Expr::Closure { func_id, .. } => Callees::one(CalleeId::Func(*func_id)),
        Expr::Undefined | Expr::Null => Callees::None,
        Expr::LocalGet(l) => st.locals.get(l).cloned().unwrap_or_default(),
        Expr::GlobalGet(g) => st.globals.get(g).cloned().unwrap_or_default(),
        Expr::LocalSet(_, v) | Expr::GlobalSet(_, v) => val(v, st),
        Expr::Sequence(v) => v.last().map(|x| val(x, st)).unwrap_or_default(),
        Expr::Logical { left, right, .. } => val(left, st).join(&val(right, st)),
        Expr::Conditional {
            then_expr,
            else_expr,
            ..
        } => val(then_expr, st).join(&val(else_expr, st)),
        Expr::PropertyGet { property, .. } => {
            st.props.get(property).cloned().unwrap_or(Callees::Unknown)
        }
        Expr::Call { callee, .. } => {
            let mut acc = Callees::None;
            match val(callee, st) {
                Callees::Set(s) => {
                    for c in s {
                        acc = acc.join(&match c {
                            CalleeId::Func(f) => st.rets.get(&f).cloned().unwrap_or_default(),
                            // `C()` of a class throws; nothing flows.
                            CalleeId::Class(_) => Callees::None,
                        });
                    }
                    acc
                }
                _ => Callees::Unknown,
            }
        }
        _ => Callees::Unknown,
    }
}

fn add<K: std::hash::Hash + Eq>(m: &mut HashMap<K, Callees>, k: K, v: Callees) {
    let e = m.entry(k).or_default();
    *e = e.join(&v);
}

fn round(st: &St, m: &Module, units: &[Unit<'_>], anon: &HashMap<&str, Vec<&str>>) -> St {
    let mut next = St::default();
    let visit = |body: &[Stmt], ret_of: Option<FuncId>, next: &mut St| {
        let mut rets = Callees::None;
        let mut sets: Vec<(LocalId, Callees)> = Vec::new();
        let mut on_stmt = |s: &Stmt| match s {
            Stmt::Let {
                id, init: Some(e), ..
            } => sets.push((*id, val(e, st))),
            Stmt::Return(Some(e)) => rets = rets.join(&val(e, st)),
            _ => {}
        };
        let mut esets: Vec<(LocalId, Callees)> = Vec::new();
        let mut gsets: Vec<(GlobalId, Callees)> = Vec::new();
        let mut psets: Vec<(String, Callees)> = Vec::new();
        let mut on_expr = |e: &Expr| match e {
            Expr::LocalSet(id, v) => esets.push((*id, val(v, st))),
            Expr::Update { id, .. } => esets.push((*id, Callees::Unknown)),
            Expr::GlobalSet(id, v) => gsets.push((*id, val(v, st))),
            Expr::PropertySet {
                property, value, ..
            } => psets.push((property.clone(), val(value, st))),
            Expr::Object(props) => {
                for (k, v) in props {
                    psets.push((k.clone(), val(v, st)));
                }
            }
            // A plain object literal lowers to `new __AnonShape_*(values…)`
            // with its keys as the synthesized class's fields, in order.
            Expr::New {
                class_name, args, ..
            } if anon.contains_key(class_name.as_str()) => {
                for (k, v) in anon[class_name.as_str()].iter().zip(args.iter()) {
                    psets.push((k.to_string(), val(v, st)));
                }
            }
            Expr::ObjectSpread { parts } => {
                for (k, v) in parts {
                    if let Some(k) = k {
                        psets.push((k.clone(), val(v, st)));
                    }
                }
            }
            _ => {}
        };
        walk_stmts(body, &mut on_stmt, &mut on_expr);
        for (id, v) in sets.into_iter().chain(esets) {
            add(&mut next.locals, id, v);
        }
        for (id, v) in gsets {
            add(&mut next.globals, id, v);
        }
        for (k, v) in psets {
            add(&mut next.props, k, v);
        }
        if let Some(f) = ret_of {
            next.rets.insert(f, rets);
        }
    };
    visit(&m.init, None, &mut next);
    for u in units {
        visit(u.body, Some(u.id), &mut next);
        if u.is_async || u.is_generator {
            next.rets.insert(u.id, Callees::Unknown);
        }
        // A parameter's value comes from callers this pass does not track.
        for p in u.params {
            next.locals.insert(p.id, Callees::Unknown);
        }
    }
    next
}

/// Infer callee identities for every `new <expr>` site and value call of `m`.
pub(super) fn infer_callees(m: &Module, units: &[Unit<'_>]) -> CalleeFacts {
    let anon: HashMap<&str, Vec<&str>> = m
        .classes
        .iter()
        .filter(|c| c.name.starts_with("__AnonShape_"))
        .map(|c| {
            (
                c.name.as_str(),
                c.fields.iter().map(|f| f.name.as_str()).collect(),
            )
        })
        .collect();
    let mut st = St::default();
    for _ in 0..ROUNDS {
        let next = round(&st, m, units, &anon);
        if next == st {
            break;
        }
        st = next;
    }
    let mut out = CalleeFacts::default();
    let mut on_stmt = |_: &Stmt| {};
    let mut on_expr = |e: &Expr| match e {
        Expr::NewDynamic {
            callee,
            byte_offset,
            ..
        } => {
            out.new_sites.insert(*byte_offset, val(callee, &st));
        }
        Expr::Call { callee, .. }
            if matches!(
                callee.as_ref(),
                Expr::LocalGet(_) | Expr::GlobalGet(_) | Expr::PropertyGet { .. }
            ) =>
        {
            out.value_calls += 1;
            if let Some(s) = val(callee, &st).ids() {
                if s.iter().all(|c| matches!(c, CalleeId::Func(_))) {
                    out.value_calls_resolved += 1;
                    if s.len() == 1 {
                        out.value_calls_single += 1;
                    }
                }
            }
        }
        _ => {}
    };
    walk_stmts(&m.init, &mut on_stmt, &mut on_expr);
    for u in units {
        walk_stmts(u.body, &mut on_stmt, &mut on_expr);
    }
    out
}

fn fmt_callees(c: &Callees) -> String {
    match c {
        Callees::None => "none".into(),
        Callees::Unknown => "unknown".into(),
        Callees::Set(s) => s
            .iter()
            .map(|c| match c {
                CalleeId::Class(n) => format!("class:{n}"),
                CalleeId::Func(f) => format!("fn:{f}"),
            })
            .collect::<Vec<_>>()
            .join("/"),
    }
}

/// Dump lines: `N <module> <byte_offset> <callees>` per `new <expr>` site and
/// `C <module> <new sites> <resolved> <single> <value calls> <resolved> <single>`.
pub(super) fn dump_callees(path: &str, f: &CalleeFacts, out: &mut String) {
    let mut sites: Vec<_> = f.new_sites.iter().collect();
    sites.sort_by_key(|(k, _)| **k);
    for (off, c) in sites {
        out.push_str(&format!("N\t{path}\t{off}\t{}\n", fmt_callees(c)));
    }
    out.push_str(&format!(
        "C\t{path}\t{}\t{}\t{}\t{}\t{}\t{}\n",
        f.new_sites.len(),
        f.new_resolved(),
        f.new_single(),
        f.value_calls,
        f.value_calls_resolved,
        f.value_calls_single
    ));
}
