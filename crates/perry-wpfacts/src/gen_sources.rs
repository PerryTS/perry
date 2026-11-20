//! A local that holds only sync generator objects whose iteration
//! protocol is the intrinsic one: for-of over it reads `next` through the
//! generator object's chain (ECMA-262 7.4.2 GetIteratorFromMethod, 7.4.3
//! GetIterator) and finds `%GeneratorPrototype%.next` (27.5.1), and
//! `[Symbol.iterator]` finds `%IteratorPrototype%[@@iterator]`, which
//! returns the object itself. So the loop resumes the generator's body
//! (27.5.3.3 GeneratorResume), and an early exit calls
//! `%GeneratorPrototype%.return`.
//!
//! The proof, per value the local may hold (every value of the node and of
//! its summary-instance copies):
//! - it is the step closure of a sync generator (`FuncInfo::gen_step`, the
//!   model's generator object), not escaped (no unknown code holds it, no
//!   reflective operation reached it: `Model::mark_hard` escapes) and not
//!   HARD;
//! - it has no own field, any-key field or accessor at all: its layout
//!   stays the birth layout, whose first slot is the record (the direct
//!   resume reads it there; an instance with own keys keeps the brand but
//!   leaves that layout);
//! - its [[Prototype]] holder (`FuncInfo::gen_proto`) holds only the
//!   original `prototype` object of its generator function (no
//!   replacement), unescaped and not HARD, with no such field or accessor;
//! - past that object the chain is `%GeneratorPrototype%` then
//!   `%IteratorPrototype%` (the model's collapsed `ObjectProto`): each key
//!   is a final read there (DEC 250: `next`, `return`, `throw` are locked
//!   image members) or nothing writes it there (`native_written`: no store
//!   into ⊤ and no write onto the runtime prototypes under it).
//! Beside them the local may hold `undefined` or `null` (a transformed
//! generator body hoists its locals with `undefined`): GetIterator throws
//! a TypeError on those when the loop starts (7.4.3 GetMethod), before any
//! step, so no step sees them; the record's start keeps the runtime path.
//! Beside them the local may hold the iterator object a `GetIterator` site
//! stands for (`Model::get_iterator_src`, the `[iterator#n]` template: what
//! its source's `@@iterator` returns when that is not the source). When
//! every value of that source (and of its summary-instance copies) is such a
//! generator, `undefined` or `null`, `@@iterator` is
//! `%IteratorPrototype%[@@iterator]` (27.1.4.1), which returns the generator
//! itself, or GetIterator throws: the object is never born, and what the
//! site returns is the source's value, which reaches the local as itself.
//! With interpreted code in the program nothing is claimed.

use crate::bindings::ModuleBindings;
use crate::gen::Gen;
use crate::model::{Model, NodeId, TmplKind, Tok, TokKind, TOP};
use crate::pts::{FxHashMap, FxHashSet};

const UNDEF: u16 = crate::model::prim::UNDEF;
const NULL: u16 = crate::model::prim::NULL;

const KEYS: [&str; 4] = ["next", "return", "throw", crate::builtins::WK_ITERATOR];

pub fn enabled() -> bool {
    !matches!(
        std::env::var("PERRY_FACTS_GENERATOR_SOURCES").as_deref(),
        Ok("0")
    )
}

/// The object `t` has no own value or accessor under any of `keys`, and
/// nothing stored into it under an unknown key.
fn no_own(m: &Model, t: Tok, keys: &[Option<u32>]) -> bool {
    let holds = |n: NodeId| {
        m.nodes
            .get(n as usize)
            .is_some_and(|node| node.prim != 0 || !node.pts.is_empty())
    };
    !m.is_escaped(t)
        && !m.hard.contains_key(&t)
        && !m.field_any.get(&t).is_some_and(|&n| holds(n))
        && keys.iter().flatten().all(|&a| {
            !m.fields.get(&(t, a)).is_some_and(|&n| holds(n)) && !m.accessors.contains_key(&(t, a))
        })
}

/// The generator object `t` has no own key at all: no field or accessor
/// under any key, and nothing stored under an unknown key.
fn no_own_key(m: &Model, t: Tok, owned: &FxHashSet<Tok>) -> bool {
    no_own(m, t, &[]) && !owned.contains(&t)
}

/// `t` is a sync generator object whose for-of protocol is the intrinsic
/// one (the module comment).
fn intrinsic_generator(
    m: &Model,
    t: Tok,
    keys: &[Option<u32>],
    op: Tok,
    owned: &FxHashSet<Tok>,
) -> bool {
    let TokKind::Func(f, _) = m.toks[t as usize] else {
        return false;
    };
    let fi = &m.funcs[f as usize];
    let Some(gp) = fi.gen_proto else {
        return false;
    };
    if fi.gen_async || !no_own_key(m, t, owned) {
        return false;
    }
    let node = &m.nodes[gp as usize];
    if node.prim != 0 || node.pts.is_empty() {
        return false;
    }
    let originals = node.pts.to_sorted().into_iter().all(|q| {
        let TokKind::Proto(tm) = m.toks[q as usize] else {
            return false;
        };
        let TmplKind::FnInst { func } = m.tmpls[tm as usize].kind else {
            return false;
        };
        m.funcs[func as usize].gen_fn_protos == Some(gp) && no_own(m, q, keys)
    });
    originals
        && keys.iter().all(|a| match a {
            // A key no program code names is written by none of it.
            None => true,
            Some(a) => {
                crate::builtins::locked_chain_read(m, t, *a)
                    || !crate::builtins::native_written(m, op, *a)
            }
        })
}

/// `t` is a `GetIterator` site's iterator object whose source holds only
/// intrinsic generators (or `undefined`/`null`): it is never born (the
/// module comment).
fn unborn_iterator(
    m: &Model,
    t: Tok,
    copies: &FxHashMap<NodeId, Vec<NodeId>>,
    generator: &dyn Fn(Tok) -> bool,
) -> bool {
    let TokKind::Obj(tm, _) = m.toks[t as usize] else {
        return false;
    };
    let Some(&src) = m.get_iterator_src.get(&tm) else {
        return false;
    };
    std::iter::once(src)
        .chain(copies.get(&src).into_iter().flatten().copied())
        .all(|n| {
            m.nodes.get(n as usize).is_some_and(|node| {
                node.prim & !(UNDEF | NULL) == 0
                    && node
                        .pts
                        .to_sorted()
                        .into_iter()
                        .all(|s| s != TOP && generator(s))
            })
        })
}

pub(crate) fn collect(
    g: &Gen<'_>,
    bindings: &mut [ModuleBindings],
    interpreted: bool,
    sabotage: bool,
    mut probe: Option<&mut Vec<String>>,
) {
    let m = &g.m;
    if !enabled() || m.aborted || m.unit_mode || interpreted {
        return;
    }
    // A marker stands for values that are not in the node that holds it.
    if !m.param_marker.is_empty() || m.arg_bits.iter().any(|&w| w != 0) {
        if let Some(p) = probe.as_deref_mut() {
            p.push("# gen_sources off=markers".to_string());
        }
        return;
    }
    let keys: Vec<Option<u32>> = KEYS.iter().map(|k| m.atom_index.get(*k).copied()).collect();
    let op = m
        .tok_index
        .get(&TokKind::ObjectProto)
        .copied()
        .unwrap_or(TOP);
    // Every object with an own field (that holds a value) or accessor.
    let mut owned: FxHashSet<Tok> = FxHashSet::default();
    for (&(t, _), &n) in &m.fields {
        if m.nodes
            .get(n as usize)
            .is_some_and(|node| node.prim != 0 || !node.pts.is_empty())
        {
            owned.insert(t);
        }
    }
    owned.extend(m.accessors.keys().map(|&(t, _)| t));
    let mut copies: FxHashMap<NodeId, Vec<NodeId>> = FxHashMap::default();
    for inst in 0..m.sum.insts.len() as u32 {
        m.inst_copies(inst, |orig, copy| {
            copies.entry(orig).or_default().push(copy)
        });
    }
    for (mi, mb) in bindings.iter_mut().enumerate().take(g.mods.len()) {
        for (id, b) in mb.locals.iter_mut() {
            let Some(&n) = g.mods[mi].locals.get(id) else {
                continue;
            };
            let mut nodes = vec![n];
            nodes.extend(copies.get(&n).into_iter().flatten().copied());
            let mut toks: Vec<Tok> = Vec::new();
            let mut prim = 0;
            for &c in &nodes {
                if let Some(node) = m.nodes.get(c as usize) {
                    prim |= node.prim;
                    node.pts.sorted_into(&mut toks);
                }
            }
            toks.sort_unstable();
            toks.dedup();
            if toks.is_empty() || prim & !(UNDEF | NULL) != 0 {
                if let Some(p) = probe.as_deref_mut().filter(|_| !toks.is_empty()) {
                    p.push(format!(
                        "# gen_sources module={mi} local={id} verdict=no prim={prim:#x} toks={}",
                        toks.len()
                    ));
                }
                continue;
            }
            let generator = |t: Tok| intrinsic_generator(m, t, &keys, op, &owned);
            let ok = sabotage
                || toks.iter().all(|&t| {
                    t != TOP && (generator(t) || unborn_iterator(m, t, &copies, &generator))
                });
            b.generator_source = ok;
            if let Some(p) = probe.as_deref_mut() {
                p.push(format!(
                    "# gen_sources module={mi} local={id} verdict={} toks={}",
                    if ok { "generator" } else { "no" },
                    toks.len()
                ));
            }
        }
    }
}
