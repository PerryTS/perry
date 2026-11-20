//! b1 (DESIGN-b1-field-read-call rev 2): a method call proven to be a
//! field element read, `r.m(a0, …, an)` where `m` is exactly
//! `m(p0, …, pn) { return this.f[p_k] ?? L; }`.
//!
//! The fact holds at a call site when, at every copy (`SiteFacts` and
//! `call_arg_ranges` are joined over copies):
//!
//! 1. the call's verdict is T0 and its one target body is F;
//! 2. F is a plain method (not async or a generator; plain parameters), and
//!    the call passes exactly as many arguments as F declares;
//! 3. F's body is exactly `return this.f[p_k]` or `return this.f[p_k] ?? L`
//!    (`L` a primitive literal);
//! 4. F's `this.f` read is T0: an own data slot on every receiver;
//! 5. the read can neither throw nor run user code:
//!    (a) the call's argument `k` is a Number with a finite integer
//!    interval, so ToPropertyKey runs nothing;
//!    (b) every copy of F's element read has only program Array tokens as
//!    its receiver (no primitive, ⊤ or marker): the base is never nullish or
//!    a proxy;
//!    (c) no unit solved a native-prototype or global write, and interpreted
//!    code cannot run, so no index accessor sits on the arrays' prototype
//!    chain;
//!    (d) no `defineProperty` of an element or named key on those arrays.
//!
//! The caller's receivers flow into F's `this` (1), so they are a subset of
//! every set F's facts were joined over (4, 5b, 5d): the facts hold for
//! every value the caller can pass.
//!
//! **In the floor (DESIGN-p4l1 §2.8, §3; PR-C3).** `FieldReadCall::array` is
//! the representation of everything F's element read may read from
//! (`bindings::arr_rep` over every copy of its receiver), and `in_floor`
//! says this call's own argument `k` is an integer in `[0, array.floor)` of
//! arrays that are dense below it. The slot is then a present own element
//! at every such call, whatever keys F's other callers pass: the body's
//! read joins them all and keeps its `undefined`, the call does not.
//! `PERRY_FACTS_ARRAY_FIELDS=0` exports neither.

use crate::bindings::{arr_rep, owner_metadata_written, ArrRep, ModuleBindings};
use crate::derive::{SiteFacts, Verdict};
use crate::gen::Gen;
use crate::model::{TokKind, TOP};
use perry_hir::{Expr, LogicalOp, Stmt};

/// The primitive literal `L` of `?? L`. A Number is its f64 bits.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum FieldReadDefault {
    Number(u64),
    String(String),
    Bool(bool),
    Null,
    Undefined,
}

impl FieldReadDefault {
    fn of(e: &Expr) -> Option<Self> {
        Some(match e {
            Expr::Number(v) => Self::Number(v.to_bits()),
            Expr::Integer(v) => Self::Number((*v as f64).to_bits()),
            Expr::String(s) => Self::String(s.clone()),
            Expr::Bool(b) => Self::Bool(*b),
            Expr::Null => Self::Null,
            Expr::Undefined => Self::Undefined,
            _ => return None,
        })
    }
}

/// A call proven to be `this.f[arg_k] ?? default` on its receiver.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FieldReadCall {
    /// The field `f`.
    pub field: String,
    /// The argument whose value is the index (`p_k`).
    pub index_arg: u32,
    /// `L` of `?? L`.
    pub default: Option<FieldReadDefault>,
    /// F's `IndexGet::element_offset` (its element facts' key).
    pub element_offset: u32,
    /// F's `this.f` read's byte offset.
    pub field_offset: u32,
    /// What F's element read reads from, when that is only plain program
    /// Arrays (module docs).
    pub array: Option<ArrRep>,
    /// This call's index is an integer in `[0, array.floor)` and the arrays
    /// are dense below the floor: the slot is a present own element.
    pub in_floor: bool,
    /// F's element read is at rest (I32 at rest, PR B;
    /// `rest_elems::site_floor`): every copy's receiver holds only Arrays of
    /// at-rest templates, the least floor of which this is.
    pub rest: Option<u32>,
}

type Shape = (String, usize, Option<FieldReadDefault>, u32, u32);

/// `PERRY_FACTS_FIELD_READ_CALLS=0` turns the fact off.
pub(crate) fn enabled() -> bool {
    !matches!(
        std::env::var("PERRY_FACTS_FIELD_READ_CALLS").as_deref(),
        Ok("0") | Ok("off") | Ok("false")
    )
}

/// The shape of F's body (3): field, parameter index, default, element
/// offset, field offset.
fn shape(f: &perry_hir::Function) -> Option<Shape> {
    if f.is_async
        || f.is_generator
        || f.params
            .iter()
            .any(|p| p.is_rest || p.default.is_some() || p.arguments_object.is_some())
    {
        return None;
    }
    let [Stmt::Return(Some(e))] = f.body.as_slice() else {
        return None;
    };
    let (read, default) = match e {
        Expr::Logical {
            op: LogicalOp::Coalesce,
            left,
            right,
        } => (left.as_ref(), Some(FieldReadDefault::of(right)?)),
        e => (e, None),
    };
    let Expr::IndexGet {
        object,
        index,
        element_offset,
        ..
    } = read
    else {
        return None;
    };
    let Expr::PropertyGet {
        object: base,
        property,
        byte_offset,
        ..
    } = object.as_ref()
    else {
        return None;
    };
    let Expr::LocalGet(p) = index.as_ref() else {
        return None;
    };
    if !matches!(base.as_ref(), Expr::This) || *byte_offset == 0 || *element_offset == 0 {
        return None;
    }
    let k = f.params.iter().position(|q| q.id == *p)?;
    Some((property.clone(), k, default, *element_offset, *byte_offset))
}

/// F by its HIR identity: an instance or static class method, or a
/// function. Getters, setters and constructors are not methods a call
/// resolves to by this route.
fn method(m: &perry_hir::Module, func_id: u32) -> Option<&perry_hir::Function> {
    m.classes
        .iter()
        .flat_map(|c| c.methods.iter().chain(c.static_methods.iter()))
        .chain(m.functions.iter())
        .find(|f| f.id == func_id)
}

/// Fill `ModuleBindings::field_read_calls` (module docs). `interpreted`:
/// interpreted code can run (`ProgramFacts::interp_guard`). `sabotage`: the
/// negative control `PERRY_FACTS_SABOTAGE=fieldreadcall` drops conditions
/// 1, 4 and 5 (the shape and argument count still hold). `floor_sabotage`:
/// the negative control `PERRY_FACTS_SABOTAGE=arrb1` claims `in_floor` for
/// every call into a dense floor, whatever its key.
pub(crate) fn collect(
    g: &Gen<'_>,
    sites: &[SiteFacts],
    bindings: &mut [ModuleBindings],
    interpreted: bool,
    sabotage: bool,
    floor_sabotage: bool,
) {
    let m = &g.m;
    // 5c.
    if !sabotage && (m.aborted || interpreted || !m.native_writes.is_empty()) {
        return;
    }
    // 4: F's field read, by (module, byte offset, key).
    let field_ok = |mi: u32, offset: u32, key: &str| {
        let mut copies = sites
            .iter()
            .filter(|s| s.module == mi && s.byte_offset == offset && !s.is_call && s.key == key)
            .peekable();
        copies.peek().is_some()
            && copies.all(|s| matches!(s.verdict, Verdict::T0Mono | Verdict::T0Set))
    };
    // 5b, 5d: every copy of F's element read.
    let elements_ok = |mi: u32, offset: u32| {
        let copies: Vec<_> = g
            .element_sites
            .iter()
            .filter(|e| e.module == mi && e.byte_offset == offset)
            .collect();
        !copies.is_empty()
            && copies.iter().all(|e| {
                let node = &m.nodes[e.recv as usize];
                node.prim == 0
                    && !node.pts.is_empty()
                    && !node.pts.contains_tok(TOP)
                    && node
                        .pts
                        .to_sorted()
                        .into_iter()
                        .all(|t| match m.toks[t as usize] {
                            TokKind::Obj(tm, _) => {
                                m.is_arr(t)
                                    && !m.is_marker(t)
                                    && !m.is_escaped(t)
                                    && !m.foreign.contains_key(&tm)
                                    && !owner_metadata_written(m, tm)
                            }
                            _ => false,
                        })
            })
    };
    // A summarized body's receiver node has one copy per instance
    // (`Inst::fresh`), as `element_keys` reads a key's.
    let mut inst_copies: std::collections::HashMap<crate::model::NodeId, Vec<_>> =
        Default::default();
    if m.array_fields || !m.rest_tmpls.is_empty() {
        for inst in 0..m.sum.insts.len() as u32 {
            m.inst_copies(inst, |orig, copy| {
                inst_copies.entry(orig).or_default().push(copy)
            });
        }
    }
    // Every copy of F's element read's receiver.
    let recvs_of = |mi: u32, offset: u32| -> Vec<crate::model::NodeId> {
        let mut nodes = Vec::new();
        for e in &g.element_sites {
            if e.module == mi && e.byte_offset == offset && e.op == crate::derive::ElementOp::Read {
                nodes.push(e.recv);
                nodes.extend(inst_copies.get(&e.recv).into_iter().flatten().copied());
            }
        }
        nodes
    };
    // What F's element read reads from.
    let array_of = |mi: u32, offset: u32| -> Option<ArrRep> {
        m.array_fields
            .then(|| arr_rep(m, &recvs_of(mi, offset)))
            .flatten()
    };
    for s in sites {
        let one_body = s.target_fns.len() == 1 || sabotage && !s.target_fns.is_empty();
        if !s.is_call || !one_body {
            continue;
        }
        if !sabotage && !matches!(s.verdict, Verdict::T0Mono | Verdict::T0Set) {
            continue;
        }
        let (fm, fid) = s.target_fns[0];
        let Some((_, hir)) = g.modules.get(fm as usize) else {
            continue;
        };
        let Some(f) = method(hir, fid) else {
            continue;
        };
        let Some((field, k, default, element_offset, field_offset)) = shape(f) else {
            continue;
        };
        let mb = &bindings[s.module as usize];
        // 2 and 5a: exactly F's arguments, argument k an integer Number.
        let Ok(at) = mb
            .call_arg_ranges
            .binary_search_by_key(&s.byte_offset, |(o, _)| *o)
        else {
            continue;
        };
        let args = &mb.call_arg_ranges[at].1;
        if args.len() != f.params.len() {
            continue;
        }
        if !sabotage
            && (args.get(k).is_none_or(|r| r.is_none())
                || !field_ok(fm, field_offset, &field)
                || !elements_ok(fm, element_offset))
        {
            continue;
        }
        let array = array_of(fm, element_offset);
        let rest = crate::rest_elems::site_floor(m, &recvs_of(fm, element_offset));
        let in_floor = array.is_some_and(|r| {
            r.dense
                && (floor_sabotage
                    || args
                        .get(k)
                        .copied()
                        .flatten()
                        .is_some_and(|(lo, hi)| lo >= 0 && hi < i64::from(r.floor)))
        });
        bindings[s.module as usize].field_read_calls.push((
            s.byte_offset,
            FieldReadCall {
                field,
                index_arg: k as u32,
                default,
                element_offset,
                field_offset,
                array,
                in_floor,
                rest,
            },
        ));
    }
    for mb in bindings.iter_mut() {
        mb.field_read_calls.sort_by_key(|(o, _)| *o);
        mb.field_read_calls.dedup_by_key(|(o, _)| *o);
    }
}
