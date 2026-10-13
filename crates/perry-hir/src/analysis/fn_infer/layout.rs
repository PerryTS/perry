//! Birth layouts and family slot facts (fninfer; the static slot index that
//! slice A's proven-shape access and sitearm's `Slot::Const` consume).
//!
//! A **birth layout** is the ordered list of own keys every instance of a
//! constructor gets at fixed positions: the straight-line prefix of the
//! constructor's `this.k = v` stores, taken while nothing else can reach
//! `this` (no `this` in a value, no closure, no call that receives `this`, no
//! branch, no return). A derived class inherits its parent's layout when the
//! parent's constructor body is entirely such a prefix. Object literals are
//! layouts too (their keys, in order). A keyless birth followed by those
//! stores puts key `i` at inline position `i` (positional shapes), so the
//! layout names the slot.
//!
//! A **family slot** is the slot of a key `k` read or written on a receiver
//! (a parameter or `this`) that is the same in every candidate layout:
//! - the receiver's inferred classes/templates, when it has a type;
//! - otherwise every layout that contains `k` (the "family of k", e.g. tsc's
//!   AST node classes, which all put `kind` at 2).
//!
//! These are candidates like every fninfer fact. A consumer proves them per
//! receiver with the shape (its key list names `k` at the slot, or an exact
//! ShapeId whose layout is this one) before it loads or stores the slot.

use std::collections::{HashMap, HashSet};

use super::{this_store_key, walk_stmts, Unit};
use crate::ir::*;
use crate::types::{FuncId, LocalId, Type};

/// Ordered birth keys per class/template name, and whether the constructor
/// body was entirely the store prefix (only then may a subclass append).
#[derive(Debug, Clone, Default)]
pub struct Layout {
    pub keys: Vec<String>,
    pub complete: bool,
}

/// Slot facts of one module.
#[derive(Debug, Default)]
pub struct LayoutFacts {
    pub layouts: HashMap<String, Layout>,
    /// Per function: (receiver, key, slot) for every key access on a
    /// parameter (`p<i>`) or `this` that has a family slot.
    pub slots: HashMap<FuncId, Vec<(String, String, u32)>>,
    /// Key accesses on parameters/`this` in the module, and those with a slot.
    pub accesses: usize,
    pub accesses_with_slot: usize,
}

fn mentions_this_or_closure(e: &Expr) -> bool {
    let mut hit = false;
    fn walk(e: &Expr, hit: &mut bool) {
        if *hit {
            return;
        }
        if matches!(e, Expr::This | Expr::Closure { .. }) {
            *hit = true;
            return;
        }
        crate::walker::walk_expr_children(e, &mut |c| walk(c, hit));
    }
    walk(e, &mut hit);
    hit
}

/// The `this.k = v` value of a store statement, if `e` is one.
fn this_store_value(e: &Expr) -> Option<&Expr> {
    match e {
        Expr::PropertySet { object, value, .. } if matches!(object.as_ref(), Expr::This) => {
            Some(value)
        }
        Expr::PutValueSet { target, value, .. } if matches!(target.as_ref(), Expr::This) => {
            Some(value)
        }
        _ => None,
    }
}

/// The store prefix of a constructor body. `parent` is spliced in at the
/// `super(…)` call of a derived constructor.
fn prefix(body: &[Stmt], parent: Option<&Layout>) -> Layout {
    let mut keys: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut need_super = parent.is_some();
    for s in body {
        match s {
            Stmt::Expr(Expr::SuperCall(args)) if need_super => {
                if args.iter().any(mentions_this_or_closure) {
                    return Layout {
                        keys,
                        complete: false,
                    };
                }
                let p = parent.expect("derived");
                for k in &p.keys {
                    if seen.insert(k.clone()) {
                        keys.push(k.clone());
                    }
                }
                need_super = false;
                if !p.complete {
                    return Layout {
                        keys,
                        complete: false,
                    };
                }
            }
            Stmt::Expr(e) if !need_super => {
                if let (Some(k), Some(v)) = (this_store_key(e), this_store_value(e)) {
                    if mentions_this_or_closure(v) {
                        return Layout {
                            keys,
                            complete: false,
                        };
                    }
                    if seen.insert(k.clone()) {
                        keys.push(k);
                    }
                } else if mentions_this_or_closure(e) {
                    return Layout {
                        keys,
                        complete: false,
                    };
                }
            }
            Stmt::Let { init, .. } if !need_super => {
                if init.as_ref().is_some_and(mentions_this_or_closure) {
                    return Layout {
                        keys,
                        complete: false,
                    };
                }
            }
            Stmt::PreallocateBoxes(_) | Stmt::PreallocateTdzBoxes(_) => {}
            _ => {
                return Layout {
                    keys,
                    complete: false,
                }
            }
        }
    }
    Layout {
        keys,
        complete: !need_super,
    }
}

fn class_layouts(m: &Module) -> HashMap<String, Layout> {
    let by_name: HashMap<&str, &Class> = m.classes.iter().map(|c| (c.name.as_str(), c)).collect();
    let mut out: HashMap<String, Layout> = HashMap::new();
    fn layout_of<'a>(
        name: &str,
        by_name: &HashMap<&str, &'a Class>,
        out: &mut HashMap<String, Layout>,
        depth: usize,
    ) -> Option<Layout> {
        if let Some(l) = out.get(name) {
            return Some(l.clone());
        }
        if depth > 16 {
            return None;
        }
        let c = by_name.get(name)?;
        if c.name.starts_with("__AnonShape_") {
            let l = Layout {
                keys: c.fields.iter().map(|f| f.name.clone()).collect(),
                complete: true,
            };
            out.insert(name.to_string(), l.clone());
            return Some(l);
        }
        // Declared instance fields, computed or private members, and
        // dynamic/native heritage have a layout this pass does not model.
        if !c.fields.is_empty()
            || c.extends_expr.is_some()
            || c.native_extends.is_some()
            || !c.type_params.is_empty()
        {
            return None;
        }
        let parent = match &c.extends_name {
            Some(p) => Some(layout_of(p, by_name, out, depth + 1)?),
            None => None,
        };
        let l = match &c.constructor {
            Some(ctor) => prefix(&ctor.body, parent.as_ref()),
            // No constructor: a base class has no own keys; a derived one
            // runs its parent's constructor unchanged.
            None => parent.unwrap_or(Layout {
                keys: Vec::new(),
                complete: true,
            }),
        };
        out.insert(name.to_string(), l.clone());
        Some(l)
    }
    for c in &m.classes {
        let _ = layout_of(&c.name, &by_name, &mut out, 0);
    }
    out
}

/// The slot of `key` common to every layout in `cands` (all must contain it).
fn common_slot(cands: &[&Layout], key: &str) -> Option<u32> {
    let mut slot: Option<usize> = None;
    for l in cands {
        let i = l.keys.iter().position(|k| k == key)?;
        if slot.is_some_and(|s| s != i) {
            return None;
        }
        slot = Some(i);
    }
    slot.map(|s| s as u32)
}

fn named_members(t: &Type) -> Option<Vec<String>> {
    match t {
        Type::Named(n) => Some(vec![n.clone()]),
        Type::Union(ms) => {
            let mut out = Vec::new();
            for m in ms {
                match m {
                    Type::Named(n) => out.push(n.clone()),
                    // A nullable receiver: the slot facts hold for the objects.
                    Type::Null | Type::Void => {}
                    _ => return None,
                }
            }
            (!out.is_empty()).then_some(out)
        }
        _ => None,
    }
}

/// Compute layouts and the family slot of every key access on a parameter or
/// `this` in `units`. `templates`: constructor-function template name →
/// its body's unit index. `param_types`/`this_types`: the final signatures.
pub(super) fn infer_layouts(
    m: &Module,
    units: &[Unit<'_>],
    templates: &HashMap<String, usize>,
    param_types: &HashMap<FuncId, Vec<Type>>,
    this_types: &HashMap<FuncId, Type>,
) -> LayoutFacts {
    let mut layouts = class_layouts(m);
    for (name, &ui) in templates {
        layouts
            .entry(name.clone())
            .or_insert_with(|| prefix(units[ui].body, None));
    }
    // The family of a key: every class/template layout (not literal shapes)
    // that has it.
    let family_layouts: Vec<&Layout> = layouts
        .iter()
        .filter(|(n, _)| !n.starts_with("__AnonShape_"))
        .map(|(_, l)| l)
        .collect();
    let mut facts = LayoutFacts::default();
    for u in units {
        let mut recv: HashMap<LocalId, (String, Option<Vec<String>>)> = HashMap::new();
        let ptys = param_types.get(&u.id);
        for (i, p) in u.params.iter().enumerate() {
            let t = ptys.and_then(|v| v.get(i)).cloned().unwrap_or(Type::Any);
            recv.insert(p.id, (format!("p{i}"), named_members(&t)));
        }
        let this_named = this_types.get(&u.id).and_then(named_members);
        let mut keys: Vec<(String, Option<Vec<String>>, String)> = Vec::new();
        let mut on_stmt = |_: &Stmt| {};
        let mut on_expr = |e: &Expr| {
            let (object, property) = match e {
                Expr::PropertyGet {
                    object, property, ..
                }
                | Expr::PropertySet {
                    object, property, ..
                }
                | Expr::PropertyUpdate {
                    object, property, ..
                } => (object.as_ref(), property.clone()),
                Expr::PutValueSet { target, key, .. } => match key.as_ref() {
                    Expr::String(k) => (target.as_ref(), k.clone()),
                    _ => return,
                },
                _ => return,
            };
            match object {
                Expr::LocalGet(id) => {
                    if let Some((name, named)) = recv.get(id) {
                        keys.push((name.clone(), named.clone(), property));
                    }
                }
                Expr::This if u.this_class.is_some() || this_named.is_some() => {
                    let named = this_named
                        .clone()
                        .or_else(|| u.this_class.clone().map(|c| vec![c]));
                    keys.push(("this".to_string(), named, property));
                }
                _ => {}
            }
        };
        walk_stmts(u.body, &mut on_stmt, &mut on_expr);
        let mut fn_slots = Vec::new();
        for (r, named, key) in keys {
            facts.accesses += 1;
            let slot = match &named {
                Some(ns) => {
                    let cands: Option<Vec<&Layout>> = ns.iter().map(|n| layouts.get(n)).collect();
                    cands.and_then(|c| common_slot(&c, &key))
                }
                None => {
                    let cands: Vec<&Layout> = family_layouts
                        .iter()
                        .copied()
                        .filter(|l| l.keys.iter().any(|k| *k == key))
                        .collect();
                    if cands.is_empty() {
                        None
                    } else {
                        common_slot(&cands, &key)
                    }
                }
            };
            if let Some(s) = slot {
                facts.accesses_with_slot += 1;
                fn_slots.push((r, key, s));
            }
        }
        if !fn_slots.is_empty() {
            facts.slots.insert(u.id, fn_slots);
        }
    }
    facts.layouts = layouts;
    facts
}

/// Dump lines: `Y <module> <name> <complete> <keys,…>` per layout,
/// `Z <module> <fid> <recv>.<key>=<slot> …` per function with slots, and
/// `X <module> <accesses> <with slot>`.
pub(super) fn dump_layouts(path: &str, f: &LayoutFacts, out: &mut String) {
    let mut names: Vec<_> = f
        .layouts
        .iter()
        .filter(|(n, _)| !n.starts_with("__AnonShape_"))
        .collect();
    names.sort_by(|a, b| a.0.cmp(b.0));
    for (n, l) in names {
        out.push_str(&format!(
            "Y\t{path}\t{n}\t{}\t{}\n",
            u8::from(l.complete),
            l.keys.join(",")
        ));
    }
    let mut fids: Vec<_> = f.slots.keys().copied().collect();
    fids.sort_unstable();
    for fid in fids {
        let s: Vec<String> = f.slots[&fid]
            .iter()
            .map(|(r, k, i)| format!("{r}.{k}={i}"))
            .collect();
        out.push_str(&format!("Z\t{path}\t{fid}\t{}\n", s.join(" ")));
    }
    out.push_str(&format!(
        "X\t{path}\t{}\t{}\n",
        f.accesses, f.accesses_with_slot
    ));
}
