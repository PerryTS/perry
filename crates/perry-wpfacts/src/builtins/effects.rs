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
//!   (an edge; a spread argument's values are its elements, an
//!   argument's `elems` a load of any key on it).
//! - `stores(fresh)`: nothing beyond the fresh result.
//! - `returns(v)`: a primitive, the receiver token, a slot's values, a
//!   fresh object born at the call site (`site_tmpl`): an array holding its
//!   `elems`, or a built-in object of another prototype (its family is
//!   opaque: undescribed members and own values stay unknown), or the bound
//!   function `bound` makes (`bound_into_result`), or the array
//!   `species(this, args, elems)` makes: ArraySpeciesCreate on the
//!   receiver, a fresh array holding `elems` whose program species
//!   constructor is constructed with `args` and takes them too
//!   (`collections::species_from`, `Cons::NewProgram`).
//! - `field(slot, k)`: as a value, the [[Get]] of `k` on the slot (a
//!   `Cons::Load`, which runs the program's getters) wherever the slot is
//!   used; as a store destination, a store under `k` into the slot's
//!   objects.
//!
//! A constructor declared this way (`new Name(...)`, `declared_ctor`) runs
//! its declaration with no receiver: `result` is the object it returns.

use super::collections::cb_call;
use super::*;
use crate::pts::FxHashMap;
use perry_hir::realm_manifest::{
    RealmEffect, RealmEntry, RealmField, RealmKind, RealmOperand, RealmPrim, RealmProtocol,
    RealmSlot, RealmSlotRoot, RealmSlotStep,
};
use std::borrow::Cow;
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
type ByName = FxHashMap<&'static str, &'static RealmEntry>;

/// The realm's declared natives by (owner path, key), its declared global
/// constructors by name, and its `provider = bound_method` members whose
/// effects are declared at the function their dispatch arm calls
/// (`effects_body`), by name; once per process: the realm table is a
/// static, this only indexes it.
fn index() -> &'static (ByProto, ByName, ByName) {
    static INDEX: OnceLock<(ByProto, ByName, ByName)> = OnceLock::new();
    INDEX.get_or_init(|| {
        let mut by: ByProto = FxHashMap::default();
        let mut ctors = FxHashMap::default();
        let mut members = FxHashMap::default();
        for e in crate::native_effects::realm() {
            if e.effects.is_empty() {
                continue;
            }
            if e.facts.contains("\"effects_body\"") {
                members.insert(e.name, e);
                continue;
            }
            match e.name.rsplit_once('.') {
                Some((owner, key)) => {
                    by.entry(owner).or_default().insert(key, e);
                }
                None if e.kind == RealmKind::Constructor => {
                    ctors.insert(e.name, e);
                }
                // A host function value no object holds (`%FsStatsPredicate%`,
                // the closure a body makes): a `native(..)` operand names it.
                None if e.name.starts_with('%') && e.name.ends_with('%') => {
                    members.insert(e.name, e);
                }
                None => {}
            }
        }
        (by, ctors, members)
    })
}

/// The declaration of the bound_method member `name` (`node:fs.existsSync`)
/// whose effects the runtime declares at its body, in forms `run_native`
/// runs and without a receiver (a module member is called on its module
/// object, which no declaration of this form names).
pub(crate) fn declared_member(name: &str) -> Option<&'static RealmEntry> {
    let e = *index().2.get(name)?;
    (interpretable(e.effects) && !names_this(e.effects)).then_some(e)
}

fn by_proto() -> &'static ByProto {
    &index().0
}

/// The declaration of the global constructor `name`, when the runtime
/// declares what `new name(...)` does, not `open`, in forms `run_construct`
/// runs (no receiver: a construct has none).
pub(crate) fn declared_ctor(name: &str) -> Option<&'static RealmEntry> {
    let e = *index().1.get(name)?;
    (interpretable(e.effects) && !names_this(e.effects)).then_some(e)
}

fn names_this(effects: &[RealmEffect]) -> bool {
    fn slot(s: &RealmSlot) -> bool {
        s.root == RealmSlotRoot::This
    }
    fn operand(o: &RealmOperand) -> bool {
        match o {
            RealmOperand::Prim(_) => false,
            RealmOperand::Slot(s) => slot(s),
            RealmOperand::Fresh { elems, fields, .. } => {
                elems.iter().any(operand) || fields.iter().any(|f| operand(&f.value))
            }
            RealmOperand::Native(_) => false,
            RealmOperand::Species { of, args, elems } => {
                slot(of) || args.iter().any(operand) || elems.iter().any(slot)
            }
            RealmOperand::Union(us) => us.iter().any(operand),
            // BoundFunctionCreate binds the receiver.
            RealmOperand::Bound => true,
        }
    }
    effects.iter().any(|e| match e {
        RealmEffect::Reads(ss) => ss.iter().any(slot),
        RealmEffect::Coerce { slot: s, .. } => slot(s),
        RealmEffect::Calls { callee, this, args } => {
            slot(callee) || operand(this) || args.iter().any(operand)
        }
        RealmEffect::Stores { src, dst } => slot(src) || slot(dst),
        RealmEffect::Returns(o) => operand(o),
        RealmEffect::StoresFresh | RealmEffect::Open => false,
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

/// Every effect is one `run_native` runs: not `open`, fresh objects only
/// `Array.prototype` arrays (or another prototype's object with no
/// elements), `bound` and species construction (of the receiver) only as
/// the result, slots: `elems` and `field` steps over the receiver or one
/// argument (`field(field(arg(0), pathname), length)`), or one `field`
/// over the result.
pub(crate) fn interpretable(effects: &[RealmEffect]) -> bool {
    fn slot(s: &RealmSlot) -> bool {
        matches!(
            (s.root, &s.steps[..]),
            (_, [])
                | (RealmSlotRoot::This | RealmSlotRoot::Arg(_), [_, ..])
                | (RealmSlotRoot::Result, [RealmSlotStep::Field(_)])
        )
    }
    fn operand(o: &RealmOperand) -> bool {
        match o {
            RealmOperand::Prim(_) => true,
            RealmOperand::Slot(s) => slot(s),
            // An array of its elements, an ordinary object of its own data
            // fields, or another prototype's object with neither.
            RealmOperand::Fresh {
                proto,
                elems,
                fields,
            } => match (&proto[..], elems.is_empty(), fields.is_empty()) {
                (_, true, true) => true,
                ("Array.prototype", _, true) => elems.iter().all(operand),
                ("Object.prototype", true, _) => fields.iter().all(|f| operand(&f.value)),
                _ => false,
            },
            // A host function value whose own declaration runs.
            RealmOperand::Native(path) => declared_member(path).is_some(),
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
            RealmEffect::Returns(RealmOperand::Species { of, args, elems }) => {
                of.root == RealmSlotRoot::This
                    && of.steps.is_empty()
                    && args.iter().all(operand)
                    && elems.iter().all(slot)
            }
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
        RealmProtocol::ShallowInspect => Protocol::ShallowInspect,
    }
}

/// Where a slot's values are: value nodes, and nodes whose elements are
/// the values (an argument under `elems`: a load of any key reads them).
#[derive(Default)]
struct Vals {
    nodes: Vec<NodeId>,
    spread: Vec<NodeId>,
}

/// One run of a declaration: native `entry` called with receiver token `t`
/// (`None`: a construct, which has no receiver).
struct Run {
    t: Option<Tok>,
    call: CallId,
    c: CallSite,
    recv: Option<NodeId>,
    returned: Vec<NodeId>,
}

impl Run {
    fn recv(&mut self, m: &mut Model) -> NodeId {
        *self.recv.get_or_insert_with(|| {
            let n = m.node_l("arr-recv");
            m.add_tok(
                n,
                self.t
                    .expect("a declaration naming `this` runs on a receiver"),
            );
            n
        })
    }

    fn root(&mut self, m: &mut Model, r: RealmSlotRoot) -> Vals {
        let mut v = Vals::default();
        match r {
            RealmSlotRoot::This => v.nodes.push(self.recv(m)),
            // Spread positions are not kept: with a spread argument,
            // argument `i` is a direct argument at index `i` or before, or
            // an element a spread argument supplies. A spread argument's
            // values are its elements, and so are the Rest values.
            RealmSlotRoot::Arg(i) if !self.c.spread.is_empty() => {
                v.nodes.extend(self.c.args.iter().take(i as usize + 1));
                for s in self.c.spread.clone() {
                    v.nodes.push(spread_elems(m, s));
                }
            }
            RealmSlotRoot::Arg(i) => v.nodes.extend(self.c.args.get(i as usize)),
            RealmSlotRoot::Rest(i) => {
                v.nodes.extend(self.c.args.iter().skip(i as usize));
                for s in self.c.spread.clone() {
                    v.nodes.push(spread_elems(m, s));
                }
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
                nodes: vec![m.field_any(
                    self.t
                        .expect("a declaration naming `this` runs on a receiver"),
                )],
                spread: vec![],
            },
            // An argument's elements: the values a load of any key reads.
            (r, [RealmSlotStep::Elems]) => Vals {
                nodes: vec![],
                spread: self.root(m, r).nodes,
            },
            // A field: [[Get]] of the key on each value (a spread list's
            // elements included), which runs its getter with the value as
            // `this`.
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
            // A step over a slot more than one deep: the same step on the
            // values of the slot it is taken in.
            (r, [inner @ .., last]) if !inner.is_empty() => {
                let of = RealmSlot {
                    root: r,
                    steps: Cow::Owned(inner.to_vec()),
                };
                let Some(n) = self.node(m, &of) else {
                    return Vals::default();
                };
                match last {
                    RealmSlotStep::Elems => Vals {
                        nodes: vec![],
                        spread: vec![n],
                    },
                    RealmSlotStep::Field(k) => {
                        let key = Key::Named(m.atom(k));
                        let dst = m.node_l("native-get");
                        m.cons(n, Cons::Load { key, dst });
                        Vals {
                            nodes: vec![dst],
                            spread: vec![],
                        }
                    }
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
            RealmOperand::Fresh {
                proto,
                elems,
                fields,
            } => {
                let out = self.fresh(m, proto, elems, fields);
                let n = m.node_l("native-fresh");
                m.add_tok(n, out);
                n
            }
            RealmOperand::Native(path) => {
                let identity = declared_member(path)
                    .expect("interpretable() takes a declared native only")
                    .name;
                let f = m.tok(TokKind::BuiltinFn(crate::model::Native::Member { identity }));
                let n = m.node_l("native-fn");
                m.add_tok(n, f);
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
            RealmOperand::Species { .. } => {
                unreachable!("interpretable() takes species as a result only")
            }
            RealmOperand::Bound => unreachable!("interpretable() takes bound as a result only"),
        }
    }

    /// A new object born at this call site: an array holding `elems`'
    /// values, an ordinary object (`hostrecord`: own data fields, then
    /// `Object.prototype`) holding `fields`, or a built-in object of
    /// prototype `proto` (neither).
    fn fresh(
        &mut self,
        m: &mut Model,
        proto: &Cow<'static, str>,
        elems: &[RealmOperand],
        fields: &[RealmField],
    ) -> Tok {
        let what = match proto {
            _ if !fields.is_empty() => "hostrecord",
            Cow::Borrowed("Array.prototype") => "Array",
            // The Date object `new Date(t)` makes (constructors.rs).
            Cow::Borrowed("Date.prototype") => "Date",
            Cow::Borrowed(p) => p,
            Cow::Owned(p) if p == "Array.prototype" => "Array",
            Cow::Owned(_) => "builtin",
        };
        let out = site_tmpl(m, self.call, what);
        if !elems.is_empty() {
            let ofa = m.field_any(out);
            for e in elems {
                let v = self.operand(m, e);
                m.edge(v, ofa);
            }
        }
        for f in fields {
            let key = m.atom(&f.key);
            let at = m.field(out, key);
            let v = self.operand(m, &f.value);
            m.edge(v, at);
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
                let t = self
                    .t
                    .expect("a declaration naming `this` runs on a receiver");
                ret_tok(m, self.call, t)
            }
            RealmOperand::Slot(s) => {
                let v = self.slot(m, s);
                flow(m, &v, self.c.ret);
            }
            RealmOperand::Fresh {
                proto,
                elems,
                fields,
            } => {
                let out = self.fresh(m, proto, elems, fields);
                ret_tok(m, self.call, out);
            }
            RealmOperand::Native(_) => {
                let n = self.operand(m, o);
                m.edge(n, self.c.ret);
            }
            RealmOperand::Union(us) => {
                for u in us.iter() {
                    self.returns(m, u);
                }
            }
            // A constructor declaration never binds (`names_this`): a bound
            // function always has its receiver as the target.
            RealmOperand::Bound => {
                if let Some(t) = self.t {
                    super::collections::bound_into_result(m, t, self.call)
                }
            }
            // ArraySpeciesCreate on the receiver (`interpretable`): the
            // builtin's array holds `elems`; a program species constructor
            // is constructed with `args` and takes them as well. A species
            // slot names `this`, so a constructor declaration has no t.
            RealmOperand::Species { args, elems, .. } => {
                let elems: Vec<RealmOperand> =
                    elems.iter().cloned().map(RealmOperand::Slot).collect();
                let out = self.fresh(m, &Cow::Borrowed("Array.prototype"), &elems, &[]);
                if let Some(t) = self.t {
                    let args = args.iter().map(|a| self.operand(m, a)).collect();
                    super::collections::species_from(m, out, t, self.call, Some(args));
                }
                ret_tok(m, self.call, out);
            }
        }
    }
}

/// The arguments spread argument `s` supplies: its elements (an element
/// read, as a program callee's parameters read it), never a member a
/// computed key could name (an array's `indexOf`, its `@@iterator`).
fn spread_elems(m: &mut Model, s: NodeId) -> NodeId {
    let n = m.node_l("native-spread-elems");
    let key = m.elem_key();
    m.cons(s, Cons::Load { key, dst: n });
    n
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
pub(crate) fn run_native(m: &mut Model, entry: &'static RealmEntry, t: Tok, call: CallId) {
    run(m, entry, Some(t), call)
}

/// `new Name(...)` at `call` of a declared global constructor
/// (`declared_ctor`): each declared effect, with no receiver.
pub(crate) fn run_construct(m: &mut Model, entry: &'static RealmEntry, call: CallId) {
    run(m, entry, None, call)
}

fn run(m: &mut Model, entry: &'static RealmEntry, t: Option<Tok>, call: CallId) {
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
                    match &s.steps[..] {
                        // A field read at any depth: its load runs the getter.
                        [.., RealmSlotStep::Field(_)] => {
                            run.slot(m, s);
                        }
                        // The elements of a value read off a slot
                        // (`elems(field(arg(0), pathname))`): a load of any
                        // key on those values, which runs their getters.
                        [_, .., RealmSlotStep::Elems] => {
                            let v = run.slot(m, s);
                            let ek = m.elem_key();
                            let dst = m.node_l("native-elems-read");
                            for n in v.spread {
                                m.cons(n, Cons::Load { key: ek, dst });
                            }
                        }
                        // A bare slot or its elements: the ordinary chain
                        // reach of the receiver.
                        _ => {}
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
            // A store under a key: into the destination slot's objects.
            RealmEffect::Stores { src, dst }
                if matches!(&*dst.steps, [RealmSlotStep::Field(_)]) =>
            {
                let [RealmSlotStep::Field(k)] = &*dst.steps else {
                    unreachable!("the guard matched one field step")
                };
                let Some(value) = run.node(m, src) else {
                    continue;
                };
                let base = run.root(m, dst.root);
                let key = Key::Named(m.atom(k));
                for n in base.nodes {
                    let site = m.anon_store_site();
                    m.cons(
                        n,
                        Cons::Store {
                            key,
                            src: value,
                            site,
                        },
                    );
                }
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
