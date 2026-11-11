//! −0 tolerance (representation RFC §10.3 as amended, PR D; owner decision
//! 2026-11-10).
//!
//! A Number node may be I32 when every value it holds is an integer in the
//! int32 range or `-0`, and no use of the node can observe the sign of a
//! zero. The interval rules (`range::eval_with`) give the operations that
//! may make `-0` (`0 * negative`, `-x`, `%` of a negative dividend) their
//! integer hull with the `-0` bit (`Iv::nz`) instead of ⊤. This module
//! decides, after the solve, whether that bit matters:
//!
//! - **Zero-sign demand** (`Node::zsign`) flows backwards, like int demand
//!   (`demand`), but fail-closed: it is seeded at every use that observes the
//!   sign of a zero and at every use nothing classifies, and it crosses every
//!   edge that delivers a value unchanged (copies, filters, guards, argument
//!   bindings, a run's result) and every integer rule whose result may itself
//!   be `-0`.
//! - **Seeds**: a use the generator did not classify (`Node::zs_use`, see
//!   `gen/zero_sign.rs`: a division, `**`, `+x`, a template literal, `throw`,
//!   a native's operand); an argument of a call whose callee is a native, an
//!   unknown or a bound function (`Model::with_native`, `zs_call_args`); a
//!   node whose uses the generator does not see (`Node::zs_known` clear: a
//!   builtin's temporary, a protocol read), when it holds Numbers; the
//!   operand of `%` (its dividend: the result has its sign), `Math.sign`,
//!   `Math.min`/`max`, a ToNumeric operand without an integer rule (`/`,
//!   `**`); every `Cons` not classified below; a field or element slot of an
//!   object that escapes, is hard, foreign, ⊤ or a marker (unknown code reads
//!   it); the elements of a spread argument array of an observing callee.
//! - **Absorbing uses** (no seed, no flow): `===`, `==`, relational compares,
//!   truthiness, index keys, bitwise operands, `Math.abs`, `Math.imul`,
//!   ToPrimitive, ToString / ToIntegerOrInfinity operands of the string
//!   methods (`string_methods`), a discarded value.
//!
//! Then every node whose interval has the bit either keeps the interval
//! without it (no zero-sign demand: `-0` and `0` are the same to every use)
//! or becomes ⊤ (some use may observe it). No exported interval carries the
//! bit, so every consumer of an interval keeps its meaning: an integer in
//! `[lo, hi]`, never an observable `-0`.
//!
//! `PERRY_FACTS_ZERO_SIGN=0`: the strict rules; nothing here runs.

use crate::model::{prim, Cons, Model, NodeId, Protocol, RangeOp, TokKind, NO_NODE, TOP};

/// `Model::bind_done`'s tag of a completed argument binding (`bind_args`).
const BOUND: u32 = u32::MAX - 1;

/// PR D, demand half: int demand also flows through value filters, guards
/// and a summarized run's result (`demand::solve`). On with the tolerance;
/// `PERRY_FACTS_FILTER_DEMAND=0` measures the tolerance without it.
pub(crate) fn filter_demand() -> bool {
    !matches!(
        std::env::var("PERRY_FACTS_FILTER_DEMAND").as_deref(),
        Ok("0") | Ok("off") | Ok("false")
    )
}

/// What a constraint on node `n` does with the sign of a zero `n` holds.
enum Use {
    /// The sign cannot change what the constraint does.
    Absorb,
    /// The constraint observes it (or is not classified).
    Observe,
    /// The values reach `dst` unchanged: demand on `dst` is demand on `n`.
    To(NodeId),
    /// `from`'s values reach `dst` unchanged (`n` is only a test).
    Between(NodeId, NodeId),
}

fn classify(m: &Model, n: NodeId, c: &Cons) -> Use {
    match *c {
        Cons::Range { op, dst, right, .. } => match op {
            // The result carries the operand's value; it differs for `-0`
            // only where the result may be `-0` itself.
            // A loop test's bound: the clip passes the left operand's values.
            RangeOp::Clip(_) if right => Use::Absorb,
            RangeOp::Add
            | RangeOp::Sub
            | RangeOp::Mul
            | RangeOp::Neg
            | RangeOp::Step(_)
            | RangeOp::Clip(_)
            | RangeOp::Integral
            | RangeOp::ToNumber => {
                if m.nodes[dst as usize].range.nz {
                    Use::To(dst)
                } else {
                    Use::Absorb
                }
            }
            // The dividend's sign is the result's; a zero divisor is NaN
            // either way.
            RangeOp::Mod if right => Use::Absorb,
            RangeOp::Mod | RangeOp::Sign | RangeOp::Min | RangeOp::Max => Use::Observe,
            RangeOp::Abs
            | RangeOp::BitAnd
            | RangeOp::BitOr
            | RangeOp::BitXor
            | RangeOp::Shl
            | RangeOp::Shr
            | RangeOp::UShr
            | RangeOp::BitNot
            | RangeOp::Clz32
            | RangeOp::Imul
            | RangeOp::SClip(_) => Use::Absorb,
        },
        // A ToNumeric operand: its integer rule decides (above); without
        // one (`/`, `**`) the result depends on the sign.
        Cons::Arith { dst } => {
            let ruled = m.nodes[n as usize]
                .cons
                .iter()
                .any(|c| matches!(c, Cons::Range { dst: d, .. } if *d == dst));
            if ruled || m.zero_sign_sabotage {
                Use::Absorb
            } else {
                Use::Observe
            }
        }
        // `+`: a String operand concatenates (`"" + -0` is "0"); the
        // numeric part is a copy into the sum's Number half.
        Cons::Plus { num, .. } => Use::To(num),
        Cons::Guard { from, dst, .. } => Use::Between(from, dst),
        Cons::Defined { dst }
        | Cons::NotNullish { dst }
        | Cons::Falsy { dst }
        | Cons::NotUndefined { dst } => Use::To(dst),
        Cons::RetInto { call, .. } | Cons::ArgInto { call, .. } => {
            let ret = m.calls[call as usize].ret;
            if ret == NO_NODE {
                Use::Absorb
            } else {
                Use::To(ret)
            }
        }
        Cons::Implicit { proto, .. } => match proto {
            // ToPrimitive of a Number is the Number; what it feeds is its
            // own use.
            Protocol::ToPrimitive => Use::Absorb,
            _ => Use::Observe,
        },
        // Keys (ToPropertyKey(-0) is "0"), lengths, tests, throw regions,
        // identity of objects: the sign of a Number changes none of them.
        Cons::IdxKey { .. }
        | Cons::IdxOwnKey { .. }
        | Cons::IdxKeyCall { .. }
        | Cons::AccessorKeyed { .. }
        | Cons::StoreKeyed { .. }
        | Cons::StoreIndexed { .. }
        | Cons::DefineKeyed { .. }
        | Cons::ElemUndef { .. }
        | Cons::LenIndex { .. }
        | Cons::LenPermuted { .. }
        | Cons::ArrayLenArg { .. }
        | Cons::Raise { .. }
        | Cons::Identity { .. }
        | Cons::Hard { .. }
        | Cons::TopIfTop { .. }
        | Cons::OwnTest { .. }
        | Cons::OwnGate { .. } => Use::Absorb,
        // Constraints of objects: a store's value reaches its slot by an
        // edge (`Store`, `BirthField`, spreads, iteration, `Array.from`);
        // calling, constructing or defining on a Number throws or does
        // nothing with it.
        Cons::Store { .. }
        | Cons::BirthField { .. }
        | Cons::Spread { .. }
        | Cons::SpreadFrom { .. }
        | Cons::IterNext { .. }
        | Cons::FromArrayLike { .. }
        | Cons::IndexHole
        | Cons::IndexKept
        | Cons::AddsKeys
        | Cons::Call { .. }
        | Cons::New { .. }
        | Cons::NewProgram { .. }
        | Cons::ReflectNew { .. }
        | Cons::ReflectNewTarget { .. }
        | Cons::NewNamed { .. }
        | Cons::CallArgs { .. }
        | Cons::BuiltinCallee
        | Cons::ThisTo { .. }
        | Cons::DeleteKey { .. }
        | Cons::DefineKey { .. }
        | Cons::DefineAll { .. }
        | Cons::DefineDesc { .. }
        | Cons::CreateProto { .. }
        | Cons::ChainUnion { .. }
        | Cons::ProtoGiven { .. }
        | Cons::ParentGiven { .. }
        | Cons::ProtoClosure { .. }
        | Cons::ProtoMethods { .. }
        | Cons::AnyAccessors { .. }
        | Cons::UnnamedSet { .. }
        | Cons::StoreTopProto { .. }
        | Cons::SuperCtor { .. }
        | Cons::SuperCtorParent { .. }
        | Cons::Accessor { .. }
        | Cons::AccessorInstall { .. }
        | Cons::AnnexBDefine { .. }
        | Cons::ProtoFrom { .. }
        | Cons::ProtoOf { .. }
        | Cons::ProtoValues { .. }
        | Cons::PlainKey { .. }
        | Cons::NewTargetHard { .. }
        | Cons::NativeParent { .. }
        | Cons::ShareBuffer { .. }
        | Cons::HolderStep { .. }
        | Cons::VisitHolders { .. }
        | Cons::VisitStop { .. }
        | Cons::ForInBody { .. }
        | Cons::ForInKeys { .. }
        | Cons::CjsPublish { .. } => Use::Absorb,
        // A property read or a method call on a Number runs its prototype's
        // code with the Number as `this` (`(-0).toLocaleString()` is "-0");
        // escapes, runtime stores, other units' contracts and a ⊤ result's
        // trigger hand the value to code the solve does not see.
        Cons::Load { .. }
        | Cons::ProtoKey { .. }
        | Cons::OwnValues { .. }
        | Cons::Chain { .. }
        | Cons::IdxLoad { .. }
        | Cons::IdxOwnLoad { .. }
        | Cons::MCall { .. }
        | Cons::IdxMCall { .. }
        | Cons::CallThis { .. }
        | Cons::NativeThis { .. }
        | Cons::ThisInst { .. }
        | Cons::ThisFrom { .. }
        | Cons::Super { .. }
        | Cons::SuperFrom { .. }
        | Cons::AnnexBLookup { .. }
        | Cons::HasInstance { .. }
        | Cons::Miss { .. }
        | Cons::TopSink { .. }
        | Cons::RuntimeStore { .. }
        | Cons::HandOver { .. }
        | Cons::StoreOut { .. }
        | Cons::OwnOnly { .. }
        | Cons::CallerValue { .. }
        | Cons::ContractThis { .. }
        | Cons::OverrideThis { .. }
        | Cons::TopIfAny { .. } => Use::Observe,
    }
}

/// A slot of token `t` some code the solve does not see may read.
fn unseen_slot(m: &Model, t: crate::model::Tok) -> bool {
    if t == TOP || m.is_marker(t) || m.bare_marker(t) || m.is_escaped(t) || m.hard.contains_key(&t)
    {
        return true;
    }
    match m.toks[t as usize] {
        TokKind::Obj(tm, _) => m.foreign.contains_key(&tm),
        _ => false,
    }
}

/// The views of one Array's element storage (`Model::field_any`, what index
/// stores write; `num_fields`, what index reads read; `all_fields`, the
/// aggregate the element facts read, `bindings::tok_arr_rep`; the numeric
/// field): `f(a, b)` for each pair that must share zero-sign demand (and,
/// `demand::solve`, int demand). The views hold the same slots, so a reader
/// of one is a reader of the slot every other one describes; each pair is
/// given both ways.
pub(crate) fn slot_views(m: &Model, f: &mut dyn FnMut(NodeId, NodeId)) {
    let num_key = m
        .atoms
        .iter()
        .position(|a| a.as_str() == crate::model::NUM_KEY);
    for (&t, &all) in &m.all_fields {
        // An object's aggregate joins every named field; no fact reads it
        // as one slot (a field's facts read its own node).
        if !m.is_elem_array(t) {
            continue;
        }
        let mut views: Vec<NodeId> = Vec::new();
        views.extend(m.field_any.get(&t).copied());
        views.extend(m.num_fields.get(&t).copied());
        if let Some(a) = num_key {
            views.extend(m.fields.get(&(t, a as crate::model::Atom)).copied());
        }
        for v in views {
            f(all, v);
            f(v, all);
        }
    }
}

/// Every slot node of token `t` is sign-observed.
fn seed_slots(m: &Model, seed: &mut [bool], t: crate::model::Tok) {
    if let Some(&x) = m.all_fields.get(&t) {
        seed[x as usize] = true;
    }
    if let Some(&x) = m.num_fields.get(&t) {
        seed[x as usize] = true;
    }
    for a in m.field_keys.get(&t).into_iter().flatten() {
        if let Some(&x) = m.fields.get(&(t, *a)) {
            seed[x as usize] = true;
        }
    }
}

/// The probe `PERRY_FACTS_ZERO_SIGN_WHY=<text>`: for each element or field
/// slot of a template whose display contains `text` and has zero-sign
/// demand, the path from the slot to the use that observes the sign (each
/// node with its name, a `PERRY_FACTS_RELATIONS` name when the dump names
/// nodes) and why that use is a seed.
fn why_probe(m: &Model, text: &str, seed: &[bool], from: &[NodeId], copies: &[(NodeId, NodeId)]) {
    let orig: crate::pts::FxHashMap<NodeId, NodeId> = copies.iter().map(|&(o, c)| (c, o)).collect();
    let name = |x: NodeId| {
        let nd = &m.nodes[x as usize];
        let own = format!(
            "{}[{}{}]",
            m.node_names.get(x as usize).map_or("?", |s| s.as_str()),
            if nd.zs_use { "u" } else { "" },
            if nd.zs_known { "k" } else { "" }
        );
        match orig.get(&x) {
            Some(&o) => format!(
                "{own}={o}:{}",
                m.node_names.get(o as usize).map_or("?", |s| s.as_str())
            ),
            None => own.to_string(),
        }
    };
    let reason = |x: NodeId| {
        let nd = &m.nodes[x as usize];
        if nd.zs_use {
            "a use the generator did not classify, or a native's argument"
        } else if !nd.zs_known {
            "a node whose uses the generator does not see"
        } else {
            "a sign-observing constraint, or an unseen slot"
        }
    };
    let slots = m
        .all_fields
        .iter()
        .map(|(&t, &x)| (t, x))
        .chain(m.fields.iter().map(|(&(t, _), &x)| (t, x)));
    for (t, x) in slots {
        let TokKind::Obj(tm, _) = m.toks[t as usize] else {
            continue;
        };
        if !m.tmpls[tm as usize].display.contains(text) || !m.nodes[x as usize].zsign {
            continue;
        }
        let mut path = vec![x];
        let mut cur = x;
        while !seed[cur as usize] && from[cur as usize] != NO_NODE && path.len() < 64 {
            cur = from[cur as usize];
            path.push(cur);
        }
        let steps: Vec<String> = path.iter().map(|&p| format!("{p}:{}", name(p))).collect();
        eprintln!(
            "# zero-sign why {}: {} <- {}",
            m.tmpls[tm as usize].display,
            steps.join(" <- "),
            reason(cur)
        );
    }
}

/// Solve zero-sign demand and settle every interval that carries the `-0`
/// bit (module doc). Returns how many Number nodes keep an interval by the
/// tolerance (dump and witness: `ProgramFacts::zero_sign_tolerated`).
pub(crate) fn solve(m: &mut Model) -> u32 {
    if !m.zero_sign {
        return 0;
    }
    let n = m.nodes.len();
    // The nodes whose uses the solve sees without the generator naming them:
    // slots (their readers are loads, the views share demand), a function's parameters and result
    // (read by its body, delivered to its calls), a run's result, the
    // Number half of a `+`.
    let mut known: Vec<NodeId> = Vec::new();
    known.extend(m.fields.values().copied());
    known.extend(m.all_fields.values().copied());
    known.extend(m.num_fields.values().copied());
    known.extend(m.field_any.values().copied());
    for f in &m.funcs {
        known.extend(f.params.iter().copied());
        known.push(f.ret);
    }
    for (i, nd) in m.nodes.iter().enumerate() {
        if nd.param || nd.result {
            known.push(i as NodeId);
        }
        for c in &nd.cons {
            match c {
                Cons::RetInto { .. } => known.push(i as NodeId),
                Cons::Plus { num, .. } => known.push(*num),
                _ => {}
            }
        }
    }
    for k in known {
        if k != NO_NODE {
            m.nodes[k as usize].zs_known = true;
        }
    }
    // A summarized function's instance copies the body's nodes: a copy's
    // uses are the body node's.
    let mut copies: Vec<(NodeId, NodeId)> = Vec::new();
    for inst in 0..m.sum.insts.len() as u32 {
        m.inst_copies(inst, |orig, copy| {
            if orig != NO_NODE {
                copies.push((orig, copy));
            }
        });
    }
    for &(o, c) in &copies {
        let (k, u) = (m.nodes[o as usize].zs_known, m.nodes[o as usize].zs_use);
        let nd = &mut m.nodes[c as usize];
        nd.zs_known |= k;
        nd.zs_use |= u;
    }
    // Seeds.
    let mut seed = vec![false; n];
    for (i, nd) in m.nodes.iter().enumerate() {
        seed[i] = nd.zs_use || (!nd.zs_known && nd.prim & prim::NUM != 0);
    }
    let trace = std::env::var("PERRY_FACTS_ZERO_SIGN_WHY").ok();
    let mut slot_toks: Vec<crate::model::Tok> = m
        .fields
        .keys()
        .map(|&(t, _)| t)
        .chain(m.all_fields.keys().copied())
        .chain(m.num_fields.keys().copied())
        .filter(|&t| unseen_slot(m, t))
        .collect();
    for &s in &m.zs_spread {
        slot_toks.extend(m.nodes[s as usize].pts.to_sorted());
    }
    slot_toks.sort_unstable();
    slot_toks.dedup();
    for t in slot_toks {
        seed_slots(m, &mut seed, t);
    }
    // The reverse graph: `preds[start[d]..start[d + 1]]` are the nodes
    // zero-sign demand on `d` reaches.
    let mut start = vec![0u32; n + 1];
    let each_edge = |m: &Model, seed: &mut [bool], f: &mut dyn FnMut(NodeId, NodeId)| {
        for (src, node) in m.nodes.iter().enumerate() {
            let src = src as NodeId;
            for &d in &node.succ {
                f(src, d);
            }
            for c in &node.cons {
                match classify(m, src, c) {
                    Use::Absorb => {}
                    Use::Observe => seed[src as usize] = true,
                    Use::To(d) => f(src, d),
                    Use::Between(a, d) => f(a, d),
                }
            }
        }
        for &(call, t, tag) in &m.bind_done {
            if tag != BOUND {
                continue;
            }
            let TokKind::Func(func, _) = m.toks[t as usize] else {
                continue;
            };
            let params = &m.funcs[func as usize].params;
            for (&a, &p) in m.calls[call as usize].args.iter().zip(params) {
                if a != NO_NODE && p != NO_NODE {
                    f(a, p);
                }
            }
        }
        for &(o, c) in &copies {
            f(o, c);
            f(c, o);
        }
        slot_views(m, f);
    };
    each_edge(m, &mut seed, &mut |_, d| start[d as usize + 1] += 1);
    for i in 0..n {
        start[i + 1] += start[i];
    }
    let mut fill = start.clone();
    let mut preds = vec![0 as NodeId; start[n] as usize];
    each_edge(m, &mut seed, &mut |s, d| {
        preds[fill[d as usize] as usize] = s;
        fill[d as usize] += 1;
    });
    drop(fill);
    let mut work: Vec<NodeId> = Vec::new();
    for (i, nd) in m.nodes.iter_mut().enumerate() {
        if seed[i] {
            nd.zsign = true;
            work.push(i as NodeId);
        }
    }
    let mut from: Vec<NodeId> = if trace.is_some() {
        vec![NO_NODE; n]
    } else {
        Vec::new()
    };
    while let Some(d) = work.pop() {
        let (a, b) = (start[d as usize] as usize, start[d as usize + 1] as usize);
        for &s in &preds[a..b] {
            let node = &mut m.nodes[s as usize];
            if !node.zsign {
                node.zsign = true;
                work.push(s);
                if let Some(x) = from.get_mut(s as usize) {
                    *x = d;
                }
            }
        }
    }
    if let Some(w) = trace {
        why_probe(m, &w, &seed, &from, &copies);
    }
    // Settle: no interval leaves the solve with the bit.
    let mut tolerated = 0;
    for nd in m.nodes.iter_mut() {
        if !nd.range.nz {
            continue;
        }
        if nd.zsign {
            nd.range = crate::model::Iv::TOP;
        } else {
            nd.range = nd.range.strip();
            nd.zs_tol = true;
            tolerated += (nd.prim & prim::NUM != 0) as u32;
        }
    }
    tolerated
}
