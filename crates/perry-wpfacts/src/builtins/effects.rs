//! A built-in that declares its effects where the runtime defines it
//! (`#[realm(Path, effects = { ... })]`, read through
//! `crate::native_effects::realm`) runs here: one interpreter of the
//! declaration, the same for every declared native. The solver keeps no
//! model per name for such a native; its declaration is the authority.
//!
//! A family's method dispatcher (`array_method`) asks `declared` for the
//! method its call resolved to, after the hand-written arms that are still
//! there: a declared native with no hand arm runs `run_native`. An
//! undeclared native, one declared `open`, or one whose declaration uses a
//! form this interpreter does not run keeps today's path.
//!
//! What each verb does to the graph (a slot is a set of value nodes):
//! - `reads(slot)`: [[Get]] of what the slot names. A slot under a field
//!   step (`field(this, length)`) is a load of that key: the getter under
//!   it, own or on the chain, runs with the slot's object as `this`. A bare
//!   slot or its elements: the solver's ordinary chain reach of the
//!   receiver, as the hand models had it.
//! - `coerce(slot, P)`: `Cons::Implicit { proto: P }` on the slot's node.
//! - `calls(callee, this, args)`: `cb_call` with the operands; its result
//!   node is `returned(N)` for the N-th declared call. A callee argument the
//!   call does not pass is no function: no call, nothing returned.
//! - `stores(src -> dst)`: the source values flow into the destination node
//!   (an edge; a spread argument's elements by a load of any key).
//! - `stores(fresh)`: nothing beyond the fresh result.
//! - `returns(v)`: a primitive, the receiver token, a slot's values, a
//!   fresh array born at the call site (`site_tmpl`) holding its `elems`,
//!   or the bound function `bound` makes (`bound_into_result`).
//!
//! A slot's field step is [[Get]] wherever the slot is used: its value is
//! what the load reads.

use super::collections::cb_call;
use super::*;
use crate::pts::FxHashMap;
use perry_hir::realm_manifest::{
    RealmEffect, RealmEntry, RealmOperand, RealmPrim, RealmProtocol, RealmSlot, RealmSlotRoot,
    RealmSlotStep,
};
use std::sync::OnceLock;

/// The intrinsic prototype a family's methods are declared on, by its
/// realm path.
fn realm_proto(fam: Family) -> Option<&'static str> {
    match fam {
        Family::Array => Some("Array.prototype"),
        _ => None,
    }
}

type ByProto = FxHashMap<&'static str, FxHashMap<&'static str, &'static RealmEntry>>;

/// The realm's declared natives by (owner path, key), once per process: the
/// realm table is a static, this only indexes it.
fn by_proto() -> &'static ByProto {
    static INDEX: OnceLock<ByProto> = OnceLock::new();
    INDEX.get_or_init(|| {
        let mut by: ByProto = FxHashMap::default();
        for e in crate::native_effects::realm() {
            if e.effects.is_empty() {
                continue;
            }
            if let Some((owner, key)) = e.name.rsplit_once('.') {
                by.entry(owner).or_default().insert(key, e);
            }
        }
        by
    })
}

/// The declaration of `fam`'s prototype method `name`, when the runtime
/// declares its effects, not `open`, in forms `run_native` runs.
pub(super) fn declared(fam: Family, name: &str) -> Option<&'static RealmEntry> {
    declared_at(realm_proto(fam)?, name)
}

/// The declaration of the native `owner.name` (`Function.prototype.bind`),
/// when the runtime declares its effects in forms `run_native` runs.
pub(super) fn declared_at(owner: &str, name: &str) -> Option<&'static RealmEntry> {
    let e = *by_proto().get(owner)?.get(name)?;
    interpretable(e.effects).then_some(e)
}

/// Every effect is one `run_native` runs: not `open`, no species
/// construction, fresh objects only `Array.prototype` arrays, `bound` only
/// as the result, slots at most one `elems` or `field` step deep over the
/// receiver or one argument.
pub(crate) fn interpretable(effects: &[RealmEffect]) -> bool {
    fn slot(s: &RealmSlot) -> bool {
        matches!(
            (s.root, &s.steps[..]),
            (_, [])
                | (
                    RealmSlotRoot::This | RealmSlotRoot::Arg(_),
                    [RealmSlotStep::Elems | RealmSlotStep::Field(_)]
                )
        )
    }
    fn operand(o: &RealmOperand) -> bool {
        match o {
            RealmOperand::Prim(_) => true,
            RealmOperand::Slot(s) => slot(s),
            RealmOperand::Fresh { proto, elems } => {
                proto == "Array.prototype" && elems.iter().all(slot)
            }
            RealmOperand::Species { .. } | RealmOperand::Bound => false,
            RealmOperand::Union(us) => us.iter().all(operand),
        }
    }
    !effects.is_empty()
        && effects.iter().all(|e| match e {
            RealmEffect::Reads(ss) => ss.iter().all(slot),
            RealmEffect::Coerce { slot: s, .. } => slot(s),
            RealmEffect::Calls { callee, this, args } => {
                slot(callee) && operand(this) && args.iter().all(operand)
            }
            RealmEffect::Stores { src, dst } => slot(src) && slot(dst),
            RealmEffect::StoresFresh => true,
            RealmEffect::Returns(RealmOperand::Bound) => true,
            RealmEffect::Returns(o) => operand(o),
            RealmEffect::Open => false,
        })
}

fn prim_bits(ps: &[RealmPrim]) -> u16 {
    ps.iter().fold(0, |b, p| {
        b | match p {
            RealmPrim::Undefined => prim::UNDEF,
            RealmPrim::Null => prim::NULL,
            RealmPrim::Bool => prim::BOOL,
            RealmPrim::Num => prim::NUM,
            RealmPrim::Str => prim::STR,
            RealmPrim::Bigint => prim::BIGINT,
            RealmPrim::Symbol => prim::SYMBOL,
        }
    })
}

fn protocol(p: RealmProtocol) -> Protocol {
    match p {
        RealmProtocol::ToPrimitive => Protocol::ToPrimitive,
        RealmProtocol::Iterate => Protocol::Iterate,
        RealmProtocol::IterNext => Protocol::IterNext,
        RealmProtocol::IterResult => Protocol::IterResult,
        RealmProtocol::Json => Protocol::Json,
        RealmProtocol::HasInstance => Protocol::HasInstance,
        RealmProtocol::OwnKeys => Protocol::OwnKeys,
    }
}

/// Where a slot's values are: value nodes, and nodes whose elements are
/// the values (a spread argument, an argument under `elems`).
#[derive(Default)]
struct Vals {
    nodes: Vec<NodeId>,
    spread: Vec<NodeId>,
}

/// One run of a declaration: native `entry` called with receiver token `t`.
struct Run {
    t: Tok,
    call: CallId,
    c: CallSite,
    recv: Option<NodeId>,
    returned: Vec<NodeId>,
}

impl Run {
    fn recv(&mut self, m: &mut Model) -> NodeId {
        *self.recv.get_or_insert_with(|| {
            let n = m.node_l("arr-recv");
            m.add_tok(n, self.t);
            n
        })
    }

    fn root(&mut self, m: &mut Model, r: RealmSlotRoot) -> Vals {
        let mut v = Vals::default();
        match r {
            RealmSlotRoot::This => v.nodes.push(self.recv(m)),
            RealmSlotRoot::Arg(i) => v.nodes.extend(self.c.args.get(i as usize)),
            RealmSlotRoot::Rest(i) => {
                v.nodes.extend(self.c.args.iter().skip(i as usize));
                v.spread.extend(&self.c.spread);
            }
            RealmSlotRoot::Result => v.nodes.push(self.c.ret),
            RealmSlotRoot::Returned(n) => v.nodes.extend(self.returned.get(n as usize)),
        }
        v
    }

    fn slot(&mut self, m: &mut Model, s: &RealmSlot) -> Vals {
        match (s.root, &s.steps[..]) {
            (r, []) => self.root(m, r),
            // The receiver's elements: its element node.
            (RealmSlotRoot::This, [RealmSlotStep::Elems]) => Vals {
                nodes: vec![m.field_any(self.t)],
                spread: vec![],
            },
            // An argument's elements: the values a load of any key reads.
            (r, [RealmSlotStep::Elems]) => Vals {
                nodes: vec![],
                spread: self.root(m, r).nodes,
            },
            // A field: [[Get]] of the key on each value, which runs its
            // getter with the value as `this`.
            (r, [RealmSlotStep::Field(k)]) => {
                let key = Key::Named(m.atom(k));
                let dst = m.node_l("native-get");
                for n in self.root(m, r).nodes {
                    m.cons(n, Cons::Load { key, dst });
                }
                Vals {
                    nodes: vec![dst],
                    spread: vec![],
                }
            }
            _ => unreachable!("interpretable() refused this slot"),
        }
    }

    /// The slot's values as one node (`None`: the slot holds nothing, an
    /// argument the call does not pass).
    fn node(&mut self, m: &mut Model, s: &RealmSlot) -> Option<NodeId> {
        let v = self.slot(m, s);
        match (&v.nodes[..], &v.spread[..]) {
            ([], []) => None,
            (&[n], []) => Some(n),
            _ => {
                let out = m.node_l("native-slot");
                flow(m, &v, out);
                Some(out)
            }
        }
    }

    fn operand(&mut self, m: &mut Model, o: &RealmOperand) -> NodeId {
        match o {
            RealmOperand::Prim(ps) => {
                let n = m.node_l("num");
                m.add_prim(n, prim_bits(ps));
                n
            }
            RealmOperand::Slot(s) => match self.node(m, s) {
                Some(n) => n,
                None => {
                    let n = m.node_l("native-absent");
                    m.add_prim(n, prim::UNDEF);
                    n
                }
            },
            RealmOperand::Fresh { elems, .. } => {
                let out = self.fresh(m, elems);
                let n = m.node_l("native-fresh");
                m.add_tok(n, out);
                n
            }
            RealmOperand::Union(us) => {
                let out = m.node_l("native-union");
                for u in us.iter() {
                    let n = self.operand(m, u);
                    m.edge(n, out);
                }
                out
            }
            RealmOperand::Species { .. } => unreachable!("interpretable() refused species"),
            RealmOperand::Bound => unreachable!("interpretable() takes bound as a result only"),
        }
    }

    /// A new array born at this call site holding `elems`' values.
    fn fresh(&mut self, m: &mut Model, elems: &[RealmSlot]) -> Tok {
        let out = site_tmpl(m, self.call, "Array");
        let ofa = m.field_any(out);
        for s in elems {
            let v = self.slot(m, s);
            flow(m, &v, ofa);
        }
        out
    }

    fn this_arg(&mut self, m: &mut Model, o: &RealmOperand) -> ThisArg {
        match o {
            RealmOperand::Prim(ps) if prim_bits(ps) == prim::UNDEF => ThisArg::Undef,
            RealmOperand::Slot(s) => match self.node(m, s) {
                Some(n) => ThisArg::Node(n),
                None => ThisArg::Undef,
            },
            o => ThisArg::Node(self.operand(m, o)),
        }
    }

    fn returns(&mut self, m: &mut Model, o: &RealmOperand) {
        match o {
            RealmOperand::Prim(ps) => ret_prim(m, self.call, prim_bits(ps)),
            RealmOperand::Slot(s) if s.root == RealmSlotRoot::This && s.steps.is_empty() => {
                ret_tok(m, self.call, self.t)
            }
            RealmOperand::Slot(s) => {
                let v = self.slot(m, s);
                flow(m, &v, self.c.ret);
            }
            RealmOperand::Fresh { elems, .. } => {
                let out = self.fresh(m, elems);
                ret_tok(m, self.call, out);
            }
            RealmOperand::Union(us) => {
                for u in us.iter() {
                    self.returns(m, u);
                }
            }
            RealmOperand::Bound => super::collections::bound_into_result(m, self.t, self.call),
            RealmOperand::Species { .. } => unreachable!("interpretable() refused species"),
        }
    }
}

/// `v`'s values flow into `dst`.
fn flow(m: &mut Model, v: &Vals, dst: NodeId) {
    for &n in &v.nodes {
        if n != dst {
            m.edge(n, dst);
        }
    }
    for &s in &v.spread {
        m.cons(
            s,
            Cons::Load {
                key: Key::any(),
                dst,
            },
        );
    }
}

/// `call` into declared native `entry` with receiver token `t`: each
/// declared effect, in declaration order.
pub(super) fn run_native(m: &mut Model, entry: &'static RealmEntry, t: Tok, call: CallId) {
    if let Some(p) = m.prof.as_mut() {
        let site = m.calls[call as usize].site;
        let (runs, sites) = p.natives.entry(entry.name).or_default();
        *runs += 1;
        sites.insert(site);
    }
    let mut run = Run {
        t,
        call,
        c: m.calls[call as usize].clone(),
        recv: None,
        returned: Vec::new(),
    };
    for e in entry.effects.iter() {
        match e {
            RealmEffect::Reads(ss) => {
                for s in ss.iter() {
                    if matches!(s.steps[..], [RealmSlotStep::Field(_)]) {
                        run.slot(m, s);
                    }
                }
            }
            RealmEffect::StoresFresh => {}
            RealmEffect::Coerce { slot, protocol: p } => {
                let v = run.slot(m, slot);
                let reason = m.reason(&format!("{} coerces its {}", entry.name, slot_text(slot)));
                let proto = protocol(*p);
                for n in v.nodes {
                    m.cons(n, Cons::Implicit { reason, proto });
                }
                if !v.spread.is_empty() {
                    let n = m.node_l("native-elems");
                    flow(
                        m,
                        &Vals {
                            nodes: vec![],
                            spread: v.spread,
                        },
                        n,
                    );
                    m.cons(n, Cons::Implicit { reason, proto });
                }
            }
            RealmEffect::Calls { callee, this, args } => {
                let ret = match run.node(m, callee) {
                    Some(cb) => {
                        let this = run.this_arg(m, this);
                        let args: Vec<NodeId> = args.iter().map(|a| run.operand(m, a)).collect();
                        cb_call(m, call, cb, this, args)
                    }
                    None => m.node_l("cb-ret"),
                };
                run.returned.push(ret);
            }
            RealmEffect::Stores { src, dst } => {
                let v = run.slot(m, src);
                if let Some(d) = run.node(m, dst) {
                    flow(m, &v, d);
                }
            }
            RealmEffect::Returns(o) => run.returns(m, o),
            RealmEffect::Open => unreachable!("interpretable() refused open"),
        }
    }
}

fn slot_text(s: &RealmSlot) -> String {
    let mut out = match s.root {
        RealmSlotRoot::This => "this".to_string(),
        RealmSlotRoot::Arg(i) => format!("arg({i})"),
        RealmSlotRoot::Rest(i) => format!("rest({i})"),
        RealmSlotRoot::Result => "result".to_string(),
        RealmSlotRoot::Returned(i) => format!("returned({i})"),
    };
    for st in s.steps.iter() {
        out = match st {
            RealmSlotStep::Elems => format!("elems({out})"),
            RealmSlotStep::Field(k) => format!("field({out}, {k})"),
        };
    }
    out
}
