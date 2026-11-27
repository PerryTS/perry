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
//! - `coerce(slot, P)`: `Cons::Implicit { proto: P, region: self.thrown }` on the slot's node.
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
//!
//! `callee` is the function value the call runs (`run_member`): a closure
//! native's captures are internal slots of that value. A closure native
//! whose declaration names `callee` is one value per object that holds it
//! (`Native::Member::holder`): a fresh object's own field of it gets its own
//! token, whose `islot(callee, self)` is that object, as the runtime
//! makes one closure per object capturing it (`fs.watch`'s `on`). Such a
//! declaration runs only on a call of the value (never by name, never as a
//! construct), and such a native is a value only as a fresh object's field.

use super::collections::cb_call_in;
use super::*;
use crate::pts::FxHashMap;
use perry_hir::realm_manifest::{
    RealmEffect, RealmEntry, RealmField, RealmKind, RealmOperand, RealmOwnValue, RealmPrim,
    RealmProtocol, RealmSlot, RealmSlotRoot, RealmSlotStep,
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
/// constructors by name, its `provider = bound_method` members whose
/// effects are declared at the function their dispatch arm calls
/// (`effects_body`), by name, and its declared global functions (`fetch`)
/// by name; once per process: the realm table is a static, this only
/// indexes it.
fn index() -> &'static (ByProto, ByName, ByName, ByName) {
    static INDEX: OnceLock<(ByProto, ByName, ByName, ByName)> = OnceLock::new();
    INDEX.get_or_init(|| {
        let mut by: ByProto = FxHashMap::default();
        let mut ctors = FxHashMap::default();
        let mut members = FxHashMap::default();
        let mut functions = FxHashMap::default();
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
                // Never a global binding, whatever its kind: before the global functions.
                None if e.name.starts_with('%') && e.name.ends_with('%') => {
                    members.insert(e.name, e);
                }
                None if e.kind == RealmKind::Function => {
                    functions.insert(e.name, e);
                }
                None => {}
            }
        }
        (by, ctors, members, functions)
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

/// The host function value a `native(path)` operand names: the realm's
/// function, method or constructor of that path (`node:fs.FileReadStream`,
/// the constructor `fs.ReadStream` and `fs.FileReadStream` hold), whether or
/// not the runtime declares what a call of it does. The operand
/// is the value, never a call: a call of it runs its declaration when it
/// has one (`declared_member`) and takes the undeclared native's path
/// otherwise, as every other reference to it does (`hostval_member`).
pub(crate) fn native_value(path: &str) -> Option<&'static RealmEntry> {
    use perry_hir::realm_manifest::RealmKind as K;
    declared_member(path).or_else(|| {
        super::members::realm_entry(path)
            .filter(|e| matches!(e.kind, K::Function | K::Method | K::Constructor))
    })
}

/// The declaration of the bound_method constructor `name`
/// (`node:inspector.Session`) whose effects the runtime declares at its
/// body (`declared_member`): what `new` of it does, in forms `run_construct`
/// runs.
/// A constructor a host object holds (`Intl.Collator`, declared at its own
/// definition) is found by owner and key, as `declared_static`.
pub(crate) fn declared_construct(name: &str) -> Option<&'static RealmEntry> {
    declared_member(name)
        .or_else(|| {
            let (owner, key) = name.rsplit_once('.')?;
            declared_at(owner, key).filter(|e| !names_this(e.effects))
        })
        .filter(|e| e.kind == RealmKind::Constructor && !names_callee(e.effects))
}

/// The declaration `super(...)` into realm constructor `identity` runs
/// (`run_super_construct`): a module or host constructor's
/// (`declared_construct`), else the global constructor's (`declared_ctor`).
pub(crate) fn declared_super_construct(identity: &str) -> Option<&'static RealmEntry> {
    declared_construct(identity).or_else(|| declared_ctor(identity))
}

/// What `super(...)` into realm constructor `identity` defines on the
/// instance, by its declaration (`declared_super_construct`).
pub(crate) struct ConstructLayout {
    /// The own data keys of its `returns(fresh(..))` results, in order.
    pub keys: Vec<&'static str>,
    /// Those every result defines (none when a result is not `fresh`).
    pub definite: Vec<&'static str>,
    /// The declaration hands the instance (`result`) to other code.
    pub leaks: bool,
}

pub(crate) fn construct_layout(identity: &str) -> Option<ConstructLayout> {
    let e = declared_super_construct(identity)?;
    let mut keys: Vec<&'static str> = Vec::new();
    let mut definite: Option<Vec<&'static str>> = None;
    for fx in e.effects.iter() {
        let RealmEffect::Returns(o) = fx else {
            continue;
        };
        let own: Vec<&'static str> = match o {
            RealmOperand::Fresh { fields, .. } => fields.iter().map(|f| f.key.as_ref()).collect(),
            _ => Vec::new(),
        };
        for k in &own {
            if !keys.contains(k) {
                keys.push(k);
            }
        }
        definite = Some(match definite {
            None => own,
            Some(d) => d.into_iter().filter(|k| own.contains(k)).collect(),
        });
    }
    Some(ConstructLayout {
        keys,
        definite: definite.unwrap_or_default(),
        leaks: names_root(e.effects, RealmSlotRoot::Result),
    })
}

fn by_proto() -> &'static ByProto {
    &index().0
}

/// The declaration of the global constructor `name`, when the runtime
/// declares what `new name(...)` does, not `open`, in forms `run_construct`
/// runs (no receiver: a construct has none).
pub(crate) fn declared_ctor(name: &str) -> Option<&'static RealmEntry> {
    let e = *index().1.get(name)?;
    (interpretable(e.effects) && !names_this(e.effects) && !names_callee(e.effects)).then_some(e)
}

fn names_this(effects: &[RealmEffect]) -> bool {
    names_root(effects, RealmSlotRoot::This)
}

/// The one internal slot of a closure native's value (`islot(callee, self)`):
/// the object whose declaration installed it (`Native::Member::holder`).
const CALLEE_SELF: &str = "self";

/// Whether the declaration reads the function value it runs as (`callee`):
/// it runs only on a call of that value (`run_member`).
pub(crate) fn names_callee(effects: &[RealmEffect]) -> bool {
    names_root(effects, RealmSlotRoot::Callee)
}

/// Rule SINKBIRTH: the call operands a declaration's result object takes
/// its contents from: bit 0 the receiver (`this`), bit 1 + i argument i;
/// `u32::MAX` when they come from anything else (a callback's result, a
/// rest list, the callee), when another effect names the result (a
/// callback is handed it, a store writes it or into it), or for a
/// constructor (`super(...)`'s object is the caller's).
fn result_need(effects: &[RealmEffect], ctor: bool) -> u32 {
    const ALL: u32 = u32::MAX;
    if ctor || names_root(effects, RealmSlotRoot::Result) {
        return ALL;
    }
    fn slot(s: &RealmSlot) -> u32 {
        match s.root {
            RealmSlotRoot::This => 1,
            RealmSlotRoot::Arg(i) if i < 31 => 1 << (1 + i),
            _ => ALL,
        }
    }
    fn operand(o: &RealmOperand) -> u32 {
        match o {
            RealmOperand::Prim(_)
            | RealmOperand::Native(_)
            | RealmOperand::Namespace(_)
            | RealmOperand::Never => 0,
            RealmOperand::Slot(s) => slot(s),
            RealmOperand::Fresh {
                elems,
                fields,
                slots,
                ..
            } => elems
                .iter()
                .map(operand)
                .chain(fields.iter().chain(slots.iter()).map(|f| operand(&f.value)))
                .fold(0, |a, b| a | b),
            RealmOperand::Promise { value, reject } => value
                .iter()
                .chain(reject.iter())
                .map(|v| operand(v))
                .fold(0, |a, b| a | b),
            RealmOperand::Species { of, args, elems } => {
                slot(of)
                    | args
                        .iter()
                        .map(operand)
                        .chain(elems.iter().map(slot))
                        .fold(0, |a, b| a | b)
            }
            RealmOperand::Union(us) => us.iter().map(operand).fold(0, |a, b| a | b),
            _ => ALL,
        }
    }
    effects
        .iter()
        .map(|e| match e {
            RealmEffect::Returns(o) => operand(o),
            _ => 0,
        })
        .fold(0, |a, b| a | b)
}

fn names_root(effects: &[RealmEffect], root: RealmSlotRoot) -> bool {
    let slot = |s: &RealmSlot| s.root == root;
    fn operand(o: &RealmOperand, slot: &dyn Fn(&RealmSlot) -> bool, root: RealmSlotRoot) -> bool {
        match o {
            RealmOperand::Prim(_) => false,
            RealmOperand::Slot(s) => slot(s),
            RealmOperand::Fresh {
                elems,
                fields,
                slots,
                ..
            } => {
                elems.iter().any(|e| operand(e, slot, root))
                    || fields
                        .iter()
                        .chain(slots.iter())
                        .any(|f| operand(&f.value, slot, root))
            }
            RealmOperand::Promise { value, reject } => value
                .iter()
                .chain(reject.iter())
                .any(|v| operand(v, slot, root)),
            RealmOperand::Native(_) | RealmOperand::Namespace(_) => false,
            RealmOperand::Species { of, args, elems } => {
                slot(of) || args.iter().any(|a| operand(a, slot, root)) || elems.iter().any(slot)
            }
            RealmOperand::Union(us) => us.iter().any(|u| operand(u, slot, root)),
            // BoundFunctionCreate binds the receiver.
            RealmOperand::Bound => root == RealmSlotRoot::This,
            RealmOperand::Json | RealmOperand::Unknown | RealmOperand::Never => false,
        }
    }
    let operand = |o: &RealmOperand| operand(o, &slot, root);
    effects.iter().any(|e| match e {
        RealmEffect::Reads(ss) => ss.iter().any(slot),
        RealmEffect::Coerce { slot: s, .. } => slot(s),
        RealmEffect::Calls { callee, this, args } => {
            slot(callee) || operand(this) || args.iter().any(operand)
        }
        RealmEffect::Stores { src, dst } => slot(src) || slot(dst),
        RealmEffect::Escapes(s)
        | RealmEffect::AsyncResource(s)
        | RealmEffect::Identity(s)
        | RealmEffect::StoresBytes(s) => slot(s),
        RealmEffect::Returns(o) => operand(o),
        RealmEffect::StoresFresh | RealmEffect::Open => false,
    })
}

/// The declaration of the global function `name` (`fetch`), when the
/// runtime declares what a call of it does in forms `run_function` runs
/// (no receiver: a global function ignores `this`).
pub(crate) fn declared_function(name: &str) -> Option<&'static RealmEntry> {
    let e = *index().3.get(name)?;
    (interpretable(e.effects) && !names_this(e.effects) && !names_callee(e.effects)).then_some(e)
}

/// The declaration of the constructor static `identity`
/// (`AbortSignal.any`), when the runtime declares what a call of it does
/// without naming `this`, in forms `run_function` runs.
pub(crate) fn declared_static(identity: &str) -> Option<&'static RealmEntry> {
    let (owner, key) = identity.rsplit_once('.')?;
    if owner.ends_with(".prototype") || key.starts_with('<') {
        return None;
    }
    declared_at(owner, key).filter(|e| !names_this(e.effects) && !names_callee(e.effects))
}

/// The declaration of prototype method `identity`
/// (`Performance.prototype.now`) when the runtime declares what a call
/// does without naming `this`: what the call does to its arguments and
/// result, whatever the receiver (a receiver of another brand throws
/// before any of it: the declaration is a superset).
pub(crate) fn declared_proto_method(identity: &str) -> Option<&'static RealmEntry> {
    let (owner, key) = identity.rsplit_once('.')?;
    if !owner.ends_with(".prototype") || key.starts_with('<') {
        return None;
    }
    declared_at(owner, key).filter(|e| !names_this(e.effects))
}

/// The declaration of prototype method `identity`
/// (`EventTarget.prototype.addEventListener`) when the runtime declares
/// what a call does to its receiver as well (`this`): it runs per receiver
/// token (`Model::declared_proto_call`). A receiver of another brand
/// throws before any of it (the declaration is a superset).
pub(crate) fn declared_receiver_method(identity: &str) -> Option<&'static RealmEntry> {
    let (owner, key) = identity.rsplit_once('.')?;
    if !owner.ends_with(".prototype") || key.starts_with('<') {
        return None;
    }
    declared_at(owner, key).filter(|e| names_this(e.effects))
}

/// The declaration of getter `key` of `proto` (`AbortController.prototype`,
/// `signal`), when the runtime declares what a call of it does in forms
/// `run_native` runs: a load of the key reads the getter's declared result
/// (`getter_load`), never the getter function.
pub(super) fn declared_getter(proto: &str, key: &str) -> Option<&'static RealmEntry> {
    declared_at(&format!("{proto}.{key}"), "<get>")
}

/// `PERRY_FACTS_DECLARED_GETTER_RESULTS=0`: a load of an accessor whose
/// getter the runtime declares, on a builtin object whose realm holder
/// locks it (a RegExp's `flags`), reads an unknown value as before
/// (`loads::bobj_declared_getter`); a declared object's own prototype
/// (`AbortController.prototype`) keeps its declared results.
pub(super) fn declared_getter_results() -> bool {
    !matches!(
        std::env::var("PERRY_FACTS_DECLARED_GETTER_RESULTS").as_deref(),
        Ok("0") | Ok("off") | Ok("false")
    )
}

/// `dst` ⊇ what getter `e` (`declared_getter`) of key `a` returns on
/// receiver `t`: its declaration runs as a member call's does (`run_native`,
/// one evaluator of declared results), in the one getter call on `t`
/// (`collections::getter_call`); a `fresh` result is born at that call.
pub(super) fn getter_load(m: &mut Model, e: &'static RealmEntry, t: Tok, a: Atom, dst: NodeId) {
    let (call, made) = super::collections::getter_call(m, t, a);
    if made {
        run_native(m, e, t, call);
    }
    let ret = m.calls[call as usize].ret;
    m.edge(ret, dst);
}

/// `src` stored through declared setter `e` of key `a` on receiver `t`
/// (ECMA-262 10.1.9.2 step 7: Call(setter, Receiver, « V »)).
pub(super) fn setter_run(m: &mut Model, e: &'static RealmEntry, t: Tok, a: Atom, src: NodeId) {
    let (call, made) = super::collections::setter_call(m, t, a);
    if made {
        run_native(m, e, t, call);
    }
    let arg = m.calls[call as usize].args[0];
    m.edge(src, arg);
}

/// The declaration of setter `key` of `proto` (`AbortSignal.prototype`,
/// `onabort`), when it only keeps its argument in a slot of its receiver
/// (an event handler's internal slot): a store through it writes that slot
/// directly (`setter_store`), no call runs.
pub(super) fn declared_setter(proto: &str, key: &str) -> Option<&'static RealmEntry> {
    let e = declared_at(&format!("{proto}.{key}"), "<set>")?;
    e.effects
        .iter()
        .all(|f| match f {
            RealmEffect::Stores { src, dst } => {
                src.root == RealmSlotRoot::Arg(0)
                    && src.steps.is_empty()
                    && dst.root == RealmSlotRoot::This
                    && !dst.steps.is_empty()
                    && dst.steps.iter().all(|st| {
                        matches!(st, RealmSlotStep::Field(_) | RealmSlotStep::Internal(_))
                    })
            }
            RealmEffect::Returns(RealmOperand::Prim(_)) => true,
            _ => false,
        })
        .then_some(e)
}

/// What setter `e` (`declared_setter`) keeps of `src` on receiver `t`.
pub(super) fn setter_store(m: &mut Model, e: &'static RealmEntry, t: Tok, src: NodeId) {
    for f in e.effects.iter() {
        let RealmEffect::Stores { dst, .. } = f else {
            continue;
        };
        let [inner @ .., last] = &*dst.steps else {
            unreachable!("declared_setter() takes receiver slots only")
        };
        let mut at = m.node_l("native-set-this");
        m.add_tok(at, t);
        for st in inner {
            let key = step_key(m, st);
            let next = m.node_l("native-set");
            m.cons(at, Cons::Load { key, dst: next });
            at = next;
        }
        let key = step_key(m, last);
        let site = m.anon_store_site();
        m.cons(
            at,
            Cons::Store {
                key,
                src,
                site,
                region: m.thrown,
            },
        );
    }
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
    (interpretable(e.effects) && !names_callee(e.effects)).then_some(e)
}

/// Every effect is one `run_native` runs: not `open`, fresh objects only
/// `Array.prototype` arrays (or another prototype's object with no
/// elements), `bound` and species construction (of the receiver) only as
/// the result, slots: `elems` and `field` steps over the receiver or one
/// argument (`field(field(arg(0), pathname), length)`), or `field` /
/// `islot` steps over the result (`field(islot(result, listeners), handleEvent)`).
pub(crate) fn interpretable(effects: &[RealmEffect]) -> bool {
    fn slot(s: &RealmSlot) -> bool {
        matches!(
            (s.root, &s.steps[..]),
            (_, [])
                | (
                    RealmSlotRoot::This | RealmSlotRoot::Arg(_) | RealmSlotRoot::Callee,
                    [_, ..]
                )
        ) || (s.root == RealmSlotRoot::Result
            && s.steps
                .iter()
                .all(|st| matches!(st, RealmSlotStep::Field(_) | RealmSlotStep::Internal(_))))
    }
    fn operand(o: &RealmOperand) -> bool {
        operand_in(o, false)
    }
    // `field`: the value of a fresh object's own data field, where a
    // closure native that reads its captures (`callee`) is installed.
    fn operand_in(o: &RealmOperand, field: bool) -> bool {
        match o {
            RealmOperand::Prim(_) => true,
            RealmOperand::Slot(s) => slot(s),
            // An array of its elements, an ordinary object of its own data
            // fields, or another prototype's object with neither.
            // Any of them may carry internal slots.
            RealmOperand::Fresh {
                proto,
                elems,
                fields,
                slots,
            } => {
                (match (&proto[..], elems.is_empty(), fields.is_empty()) {
                    (_, true, true) => true,
                    ("Array.prototype", _, true) => elems.iter().all(operand),
                    ("Array.prototype", _, false) => false,
                    // An ordinary object, or another prototype's object
                    // with own data fields (a PerformanceMark's `name`,
                    // `entryType`, `startTime`, `duration`).
                    (_, true, false) => fields.iter().all(|f| operand_in(&f.value, true)),
                    _ => false,
                }) && slots.iter().all(|f| operand(&f.value))
            }
            RealmOperand::Promise { value, reject } => {
                value.iter().chain(reject.iter()).all(operand)
            }
            // A host function value whose own declaration runs.
            RealmOperand::Native(path) => {
                native_value(path).is_some_and(|e| field || !names_callee(e.effects))
            }
            // The realm's one namespace object of that path.
            RealmOperand::Namespace(path) => super::members::realm_entry(path)
                .is_some_and(|d| d.kind == perry_hir::realm_manifest::RealmKind::Namespace),
            RealmOperand::Species { .. } | RealmOperand::Bound => false,
            RealmOperand::Union(us) => us.iter().all(|u| operand_in(u, field)),
            RealmOperand::Json | RealmOperand::Unknown => true,
            // No value at all: a whole result only (the manifest's check).
            RealmOperand::Never => true,
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
            // Bytes into a byte view's backing store: no value flows.
            RealmEffect::StoresBytes(s) => slot(s),
            // An observation of the slot's values: no flow to model.
            RealmEffect::Identity(_) => true,
            RealmEffect::Escapes(s) => slot(s),
            RealmEffect::AsyncResource(s) => slot(s),
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
        RealmProtocol::AsyncIterate => Protocol::AsyncIterate,
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
    /// The entry whose declaration runs.
    name: &'static str,
    t: Option<Tok>,
    /// The function value called (`callee`): set by `run_member` only.
    callee: Option<Tok>,
    call: CallId,
    c: CallSite,
    recv: Option<NodeId>,
    returned: Vec<NodeId>,
    /// Rule SINKBIRTH: the call operands the declared result's contents
    /// come from (`result_need`).
    need: u32,
    /// `super(...)` into this constructor (ECMA-262 13.3.7.1): the instance
    /// under construction, the object OrdinaryCreateFromConstructor makes
    /// with NewTarget's prototype (10.1.13). The declared result's `fresh`
    /// is this object, and `result` names it.
    new_target: Option<Tok>,
}

impl Run {
    fn recv(&mut self, m: &mut Model) -> NodeId {
        *self.recv.get_or_insert_with(|| {
            let n = m.node_l("arr-recv");
            // No receiver (`this` undefined, `declared_proto_call`): the
            // slot holds no value.
            if let Some(t) = self.t {
                m.add_tok(n, t);
            }
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
            RealmSlotRoot::Arg(i) => v.nodes.extend(self.c.arg(i as usize)),
            RealmSlotRoot::Rest(i) => {
                v.nodes.extend(self.c.args_from(i as usize).iter());
                for s in self.c.spread.clone() {
                    v.nodes.push(spread_elems(m, s));
                }
            }
            RealmSlotRoot::Result => match self.new_target {
                Some(t) => {
                    let n = m.node_l("super-construct-result");
                    m.add_tok(n, t);
                    v.nodes.push(n);
                }
                None => v.nodes.push(self.c.ret),
            },
            RealmSlotRoot::Returned(n) => v.nodes.extend(self.returned.get(n as usize)),
            RealmSlotRoot::Callee => {
                let f = self
                    .callee
                    .expect("a declaration naming `callee` runs on a call of its value");
                let n = m.node_l("native-callee");
                m.add_tok(n, f);
                v.nodes.push(n);
            }
        }
        v
    }

    fn slot(&mut self, m: &mut Model, s: &RealmSlot) -> Vals {
        match (s.root, &s.steps[..]) {
            // An internal slot of the called value: its holder (`self`), or
            // what the declaration stored there (one known token: its field
            // node itself, no [[Get]]).
            (RealmSlotRoot::Callee, [RealmSlotStep::Internal(k), rest @ ..]) => {
                let n = if k == CALLEE_SELF {
                    self.callee_self(m)
                } else {
                    self.callee_slot(m, k)
                };
                self.walk(m, n, rest)
            }
            (r, []) => self.root(m, r),
            // The values iterating a slot yields, or its own enumerable
            // properties' values: taken from the slot the step is in.
            (r, [inner @ .., st @ (RealmSlotStep::Iterated | RealmSlotStep::OwnValues)]) => {
                let of = RealmSlot {
                    root: r,
                    steps: Cow::Owned(inner.to_vec()),
                };
                let Some(n) = self.node(m, &of) else {
                    return Vals::default();
                };
                Vals {
                    nodes: vec![self.value_step(m, n, st)],
                    spread: vec![],
                }
            }
            // The receiver's elements: its element node, on a receiver
            // that keeps no elements in its any-key field what a read at an
            // index finds (rule W10-IDX, `Model::array_like_elems`).
            (RealmSlotRoot::This, [RealmSlotStep::Elems]) => Vals {
                nodes: self.t.map(|t| m.array_like_elems(t)).into_iter().collect(),
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
            (r, [st @ (RealmSlotStep::Field(_) | RealmSlotStep::Internal(_))]) => {
                let key = step_key(m, st);
                let dst = m.node_l("native-get");
                for n in self.root(m, r).nodes {
                    m.cons(n, Cons::Load { key, dst });
                }
                Vals {
                    nodes: vec![dst],
                    spread: vec![],
                }
            }
            // The prototype of the values of the slot the step is taken in.
            (r, [inner @ .., RealmSlotStep::Proto]) => {
                let of = RealmSlot {
                    root: r,
                    steps: Cow::Owned(inner.to_vec()),
                };
                let Some(n) = self.node(m, &of) else {
                    return Vals::default();
                };
                Vals {
                    nodes: vec![self.proto_of(m, n)],
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
                    RealmSlotStep::Iterated | RealmSlotStep::OwnValues | RealmSlotStep::Proto => {
                        unreachable!("a value step is taken above")
                    }
                    st @ (RealmSlotStep::Field(_)
                    | RealmSlotStep::Internal(_)
                    | RealmSlotStep::StrKey) => {
                        let key = step_key(m, st);
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

    /// `islot(callee, self)`: the object the called closure native was
    /// made for (its token's holder). A value of the native with no holder
    /// (none is made: `interpretable` takes such a native as a fresh
    /// object's field only) would be any object: ⊤.
    fn callee_self(&mut self, m: &mut Model) -> NodeId {
        let f = self
            .callee
            .expect("a declaration naming `callee` runs on a call of its value");
        let n = m.node_l("native-callee-self");
        match m.toks[f as usize] {
            TokKind::BuiltinFn(crate::model::Native::Member {
                holder: Some(h), ..
            }) => m.add_tok(n, h),
            _ => m.add_top(n, |_| "a closure native's captures without a holder".into()),
        }
        n
    }

    /// Internal slot `k` of the called value: the field the declaration
    /// stores into (`stores(result -> islot(callee, watchers))`) and reads.
    fn callee_slot(&mut self, m: &mut Model, k: &str) -> NodeId {
        let f = self
            .callee
            .expect("a declaration naming `callee` runs on a call of its value");
        let key = internal_atom(m, k);
        m.field(f, key)
    }

    /// The steps `steps` taken from the values of node `n`, outermost last.
    fn walk(&mut self, m: &mut Model, n: NodeId, steps: &[RealmSlotStep]) -> Vals {
        let mut cur = Vals {
            nodes: vec![n],
            spread: vec![],
        };
        for st in steps {
            let base = match (&cur.nodes[..], &cur.spread[..]) {
                (&[b], []) => b,
                _ => {
                    let out = m.node_l("native-slot");
                    flow(m, &cur, out);
                    out
                }
            };
            cur = match st {
                RealmSlotStep::Elems => Vals {
                    nodes: vec![],
                    spread: vec![base],
                },
                RealmSlotStep::Iterated | RealmSlotStep::OwnValues => Vals {
                    nodes: vec![self.value_step(m, base, st)],
                    spread: vec![],
                },
                RealmSlotStep::Proto => Vals {
                    nodes: vec![self.proto_of(m, base)],
                    spread: vec![],
                },
                st @ (RealmSlotStep::Field(_)
                | RealmSlotStep::Internal(_)
                | RealmSlotStep::StrKey) => {
                    let key = step_key(m, st);
                    let dst = m.node_l("native-get");
                    m.cons(base, Cons::Load { key, dst });
                    Vals {
                        nodes: vec![dst],
                        spread: vec![],
                    }
                }
            };
        }
        cur
    }

    /// `proto(n)`: ToObject of `n`'s values (ECMA-262 7.1.18: a primitive's
    /// wrapper, born at this call; `null` and `undefined` throw), then
    /// [[GetPrototypeOf]] of each (`Cons::ProtoOf`, as
    /// `Object.getPrototypeOf` reads it): an object or `null`.
    fn proto_of(&mut self, m: &mut Model, n: NodeId) -> NodeId {
        let o = super::to_object(m, n, self.call);
        let dst = m.node_l("native-proto");
        m.cons(o, Cons::ProtoOf { dst });
        dst
    }

    /// The values step `st` takes from node `n`: what iterating it yields
    /// (`collections::iterate`: the array-like elements and the protocol's
    /// `next().value`), or the values of its own enumerable properties
    /// (`Cons::OwnValues`: no inherited member).
    fn value_step(&mut self, m: &mut Model, n: NodeId, st: &RealmSlotStep) -> NodeId {
        match st {
            RealmSlotStep::Iterated => super::collections::iterate(m, self.call, n),
            RealmSlotStep::OwnValues => {
                let dst = m.node_l("native-own-values");
                m.cons(n, Cons::OwnValues { dst });
                dst
            }
            _ => unreachable!("value_step takes a value step"),
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
                slots,
            } => {
                let out = self.fresh(m, proto, elems, fields, false);
                self.slots(m, out, slots);
                let n = m.node_l("native-fresh");
                m.add_tok(n, out);
                n
            }
            RealmOperand::Promise { value, reject } => {
                let p = self.promise(m, value, reject);
                let n = m.node_l("native-promise");
                m.add_tok(n, p);
                n
            }
            // The namespace object the realm declares there: the token every
            // other reference to it is (`Builtin(Module(name))`).
            RealmOperand::Namespace(path) => {
                let name = super::members::realm_entry(path)
                    .expect("interpretable() takes a declared namespace only")
                    .name;
                let t = m.tok(TokKind::Builtin(BuiltinNs::Module(name)));
                let n = m.node_l("native-namespace");
                m.add_tok(n, t);
                n
            }
            RealmOperand::Native(path) => {
                let identity = native_value(path)
                    .expect("interpretable() takes a realm native only")
                    .name;
                let f = m.tok(TokKind::BuiltinFn(crate::model::Native::Member {
                    identity,
                    holder: None,
                }));
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
            // The manifest admits `never` as a whole result only.
            RealmOperand::Never => unreachable!("never is a whole result only"),
            // What JSON.parse makes (`builtins::json_value`).
            RealmOperand::Json => {
                let n = m.node_l("native-json");
                let positions = m.json_positions;
                let (j, ja) = crate::builtins::json_value(m, self.call, positions, false);
                m.add_tok(n, j);
                m.add_tok(n, ja);
                // JSON text holds no other primitive (ECMA-262 25.5.1);
                // the operand takes no reviver.
                m.add_prim(n, prim::JSON);
                n
            }
            // What unknown code hands back: the declaration escaped the
            // value that makes it.
            RealmOperand::Unknown => {
                let n = m.node_l("native-unknown");
                let name = self.name;
                m.add_top(n, |_| format!("{name}: a value of unknown code"));
                m.add_prim(n, prim::ALL);
                n
            }
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
        result: bool,
    ) -> Tok {
        // `own`: an object of the realm prototype itself (no hand model
        // makes one), whose declared fields are its definite birth keys.
        let mut own = false;
        let what = match proto {
            // An ordinary object, with or without own fields
            // (`navigator.locks`).
            Cow::Borrowed("Object.prototype") => "hostrecord",
            // OrdinaryObjectCreate(null) (`fresh(null)`): an ordinary object
            // whose chain ends at itself (node's `_events`).
            Cow::Borrowed("null") => "nullrecord",
            Cow::Owned(p) if p == "Object.prototype" => "hostrecord",
            Cow::Borrowed("Array.prototype") => "Array",
            // The Date object `new Date(t)` makes (constructors.rs).
            Cow::Borrowed("Date.prototype") => "Date",
            // An object of prototype %ArrayBuffer.prototype% the realm makes
            // (`Body.arrayBuffer()`) is the one AllocateArrayBuffer makes
            // (25.1.3.1): the ArrayBuffer of `new ArrayBuffer(n)`
            // (constructors.rs), with its modelled members (`slice`,
            // 25.1.6.7). Likewise a shared one (25.2.2.1).
            Cow::Borrowed("ArrayBuffer.prototype") => "ArrayBuffer",
            Cow::Borrowed("SharedArrayBuffer.prototype") => "SharedArrayBuffer",
            // The URLSearchParams object a URL's `searchParams` getter returns
            // is the one `new URLSearchParams()` makes (constructors.rs),
            // with its modelled members (WHATWG URL 6.2).
            Cow::Borrowed("URLSearchParams.prototype") => "URLSearchParams",
            Cow::Borrowed(p) => {
                own = true;
                p
            }
            Cow::Owned(p) if p == "Array.prototype" => "Array",
            Cow::Owned(_) => "builtin",
        };
        // The call's result (`returns`) is born in the result's context
        // (rule SINKBIRTH, `site_tmpl_result`).
        let out = match result {
            true => site_tmpl_result(m, self.call, what, self.need),
            false => site_tmpl(m, self.call, what),
        };
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
            let v = self.held(m, &f.value, out);
            m.edge(v, at);
        }
        // The birth defines its fields as own data properties
        // (CreateDataProperty, as a literal's), before any program code
        // sees the object: definite birth keys of the template, kept only
        // where every declared birth of the template defines them.
        if let (true, Some(tm)) = (own, m.tmpl_of(out)) {
            let keys: Vec<Atom> = fields.iter().map(|f| m.atom(&f.key)).collect();
            let first = m.realm_born.insert(tm);
            let tp = &mut m.tmpls[tm as usize];
            if first {
                tp.definite = keys.clone();
            } else {
                tp.definite.retain(|a| keys.contains(a));
            }
            for a in keys {
                if !tp.birth_keys.contains(&a) {
                    tp.birth_keys.push(a);
                }
            }
        }
        out
    }

    /// The value of an own field of fresh object `holder`: a closure native
    /// whose declaration reads its captures (`names_callee`) is the value
    /// made for this holder, with `islot(callee, self)` = `holder`; any
    /// other value as `operand` makes it.
    fn held(&mut self, m: &mut Model, o: &RealmOperand, holder: Tok) -> NodeId {
        match o {
            RealmOperand::Native(path) => {
                let e = native_value(path).expect("interpretable() takes a realm native only");
                if !names_callee(e.effects) {
                    return self.operand(m, o);
                }
                let f = m.tok(TokKind::BuiltinFn(crate::model::Native::Member {
                    identity: e.name,
                    holder: Some(holder),
                }));
                let n = m.node_l("native-fn");
                m.add_tok(n, f);
                n
            }
            RealmOperand::Union(us) => {
                let out = m.node_l("native-union");
                for u in us.iter() {
                    let n = self.held(m, u, holder);
                    m.edge(n, out);
                }
                out
            }
            _ => self.operand(m, o),
        }
    }

    /// The internal slots of fresh object `out`: one field each, under a
    /// key no program property names (`internal_key`).
    fn slots(&mut self, m: &mut Model, out: Tok, slots: &[RealmField]) {
        for f in slots {
            let key = internal_atom(m, &f.key);
            let at = m.field(out, key);
            let v = self.operand(m, &f.value);
            m.edge(v, at);
        }
    }

    /// A promise born at this call site: it fulfills with `value` (a
    /// thenable is adopted, as resolve does) or rejects with `reject`
    /// (what a rejection handler reads: `thrown`).
    fn promise(&mut self, m: &mut Model, value: &[RealmOperand], reject: &[RealmOperand]) -> Tok {
        let p = site_tmpl(m, self.call, "Promise");
        let v = promise_value(m, p);
        for o in value {
            let n = self.operand(m, o);
            adopt(m, self.call, n, v);
        }
        for o in reject {
            let n = self.operand(m, o);
            let thrown = m.thrown;
            m.edge(n, thrown);
        }
        p
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
            // `super(...)`: the object made is the instance under
            // construction; the declared fields and slots are its own.
            RealmOperand::Fresh {
                elems,
                fields,
                slots,
                ..
            } if self.new_target.is_some() => {
                let out = self.new_target.expect("guarded");
                if !elems.is_empty() {
                    let ofa = m.field_any(out);
                    for e in elems.iter() {
                        let v = self.operand(m, e);
                        m.edge(v, ofa);
                    }
                }
                for f in fields.iter() {
                    let key = m.atom(&f.key);
                    let at = m.field(out, key);
                    let v = self.held(m, &f.value, out);
                    m.edge(v, at);
                }
                self.slots(m, out, slots);
                ret_tok(m, self.call, out);
            }
            RealmOperand::Fresh {
                proto,
                elems,
                fields,
                slots,
            } => {
                let out = self.fresh(m, proto, elems, fields, true);
                self.slots(m, out, slots);
                ret_tok(m, self.call, out);
            }
            RealmOperand::Promise { value, reject } => {
                let p = self.promise(m, value, reject);
                ret_tok(m, self.call, p);
            }
            RealmOperand::Native(_)
            | RealmOperand::Namespace(_)
            | RealmOperand::Json
            | RealmOperand::Unknown => {
                let n = self.operand(m, o);
                m.edge(n, self.c.ret);
            }
            RealmOperand::Union(us) => {
                for u in us.iter() {
                    self.returns(m, u);
                }
            }
            // The call returns no value on any path: it throws (a realm
            // TypeError, THROWN-REALM's), so nothing reaches the result.
            RealmOperand::Never => {}
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
                let out = self.fresh(m, &Cow::Borrowed("Array.prototype"), &elems, &[], true);
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

/// The key of internal slot `name`: `[[name]]`, as the promise's settled
/// value is `[[PromiseResult]]` (`promise_value`); a family's chain has no member
/// under it (`loads::family_load`).
pub(crate) fn internal_atom(m: &mut Model, name: &str) -> Atom {
    m.atom(&format!("[[{name}]]"))
}

/// Whether key `name` is an internal slot's (`internal_atom`).
pub(crate) fn is_internal_key(name: &str) -> bool {
    name.len() > 4 && name.starts_with("[[") && name.ends_with("]]")
}

fn step_key(m: &mut Model, st: &RealmSlotStep) -> Key {
    match st {
        RealmSlotStep::Field(k) => Key::Named(m.atom(k)),
        RealmSlotStep::Internal(k) => Key::Named(internal_atom(m, k)),
        // Read back: any String key (the exclusion is the store's).
        RealmSlotStep::StrKey => Key::any(),
        RealmSlotStep::Elems
        | RealmSlotStep::Iterated
        | RealmSlotStep::OwnValues
        | RealmSlotStep::Proto => {
            unreachable!("an elements or value step has no key")
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
pub(crate) fn run_native(m: &mut Model, entry: &'static RealmEntry, t: Tok, call: CallId) {
    run(m, entry, Some(t), None, call, None)
}

/// `call` of declared native function value `f` (`declared_member`):
/// each declared effect, with `f` as the receiver token and as `callee`.
pub(crate) fn run_member(m: &mut Model, entry: &'static RealmEntry, f: Tok, call: CallId) {
    run(m, entry, Some(f), Some(f), call, None)
}

/// `new Name(...)` at `call` of a declared global constructor
/// (`declared_ctor`): each declared effect, with no receiver.
pub(crate) fn run_construct(m: &mut Model, entry: &'static RealmEntry, call: CallId) {
    run(m, entry, None, None, call, None)
}

/// `call` of a declared global function (`declared_function`): each
/// declared effect, with no receiver.
pub(crate) fn run_function(m: &mut Model, entry: &'static RealmEntry, call: CallId) {
    run(m, entry, None, None, call, None)
}

/// `super(...)` at `call` into declared constructor `entry`
/// (`declared_construct`): each declared effect, with no receiver, the
/// object it makes being `inst`, the instance under construction.
pub(crate) fn run_super_construct(
    m: &mut Model,
    entry: &'static RealmEntry,
    call: CallId,
    inst: Tok,
) {
    run(m, entry, None, None, call, Some(inst))
}

fn run(
    m: &mut Model,
    entry: &'static RealmEntry,
    t: Option<Tok>,
    callee: Option<Tok>,
    call: CallId,
    new_target: Option<Tok>,
) {
    crate::family_reach::run(m, entry.name);
    if let Some(p) = m.prof.as_mut() {
        let site = m.calls[call as usize].site;
        let (runs, sites) = p.natives.entry(entry.name).or_default();
        *runs += 1;
        sites.insert(site);
    }
    let _c = &m.calls[call as usize];
    let mut run = Run {
        name: entry.name,
        t,
        callee,
        call,
        c: m.calls[call as usize].clone(),
        recv: None,
        returned: Vec::new(),
        need: result_need(&entry.effects, new_target.is_some()),
        new_target,
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
            // A byte copy into the slot's byte views: an element read of a
            // typed array already carries its element type
            // (`typed_array_elem_prim`, ECMA-262 10.4.5.15) and no [[Set]]
            // runs, so nothing flows; the slot's values are not kept.
            RealmEffect::StoresBytes(_) => {}
            // The body compares the slot's values by identity:
            // recorded for the value-record derive only.
            RealmEffect::Identity(s) if m.value_records => {
                let v = run.slot(m, s);
                for n in v.nodes {
                    m.identity(n, IdentityRoute::Native);
                }
                if !v.spread.is_empty() {
                    let n = m.node_l("native-identity");
                    flow(
                        m,
                        &Vals {
                            nodes: vec![],
                            spread: v.spread,
                        },
                        n,
                    );
                    m.identity(n, IdentityRoute::Native);
                }
            }
            RealmEffect::Identity(_) => {}
            RealmEffect::AsyncResource(s) => {
                if let Some(n) = run.node(m, s) {
                    super::async_resource(m, n, entry.name);
                }
            }
            RealmEffect::Escapes(s) => {
                if let Some(n) = run.node(m, s) {
                    let reason = m.reason(&format!(
                        "{} hands its {} to unknown code",
                        entry.name,
                        slot_text(s)
                    ));
                    m.sink_node(n, reason);
                }
            }
            RealmEffect::Coerce { slot, protocol: p } => {
                let v = run.slot(m, slot);
                let reason = m.reason(&format!("{} coerces its {}", entry.name, slot_text(slot)));
                let proto = protocol(*p);
                for n in v.nodes {
                    m.cons(
                        n,
                        Cons::Implicit {
                            reason,
                            proto,
                            region: m.thrown,
                        },
                    );
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
                    m.cons(
                        n,
                        Cons::Implicit {
                            reason,
                            proto,
                            region: m.thrown,
                        },
                    );
                }
            }
            RealmEffect::Calls { callee, this, args } => {
                let ret = match run.node(m, callee) {
                    Some(cb) => {
                        let this = run.this_arg(m, this);
                        let args: Vec<NodeId> = args.iter().map(|a| run.operand(m, a)).collect();
                        {
                            // A declared effect says nothing of when the
                            // callback runs: its throws stay program-wide.
                            let thrown = m.thrown;
                            cb_call_in(m, call, cb, this, args, thrown)
                        }
                    }
                    None => m.node_l("cb-ret"),
                };
                run.returned.push(ret);
            }
            // A store under a key: into the objects of the slot the last
            // step is taken in (`islot(islot(this, signal), reason)`: the
            // receiver's signal).
            RealmEffect::Stores { src, dst }
                if matches!(
                    dst.steps.last(),
                    Some(
                        RealmSlotStep::Field(_)
                            | RealmSlotStep::Internal(_)
                            | RealmSlotStep::StrKey
                    )
                ) =>
            {
                let [inner @ .., st] = &*dst.steps else {
                    unreachable!("the guard matched a key step")
                };
                let Some(value) = run.node(m, src) else {
                    continue;
                };
                if let (RealmSlotRoot::Callee, [], RealmSlotStep::Internal(k)) =
                    (dst.root, inner, st)
                {
                    if k != CALLEE_SELF {
                        let at = run.callee_slot(m, k);
                        m.edge(value, at);
                        continue;
                    }
                }
                let base = if inner.is_empty() {
                    run.root(m, dst.root).nodes
                } else {
                    let of = RealmSlot {
                        root: dst.root,
                        steps: Cow::Owned(inner.to_vec()),
                    };
                    run.node(m, &of).into_iter().collect()
                };
                // `strkey(this)` (a Web Storage item, perry-runtime
                // web_storage.rs `sync_named_property`): the String
                // ToString(value) makes, under any String key but an own
                // member name of the declaring method's prototype (the
                // runtime's reserved names: Storage.prototype's members and
                // its `length`); never the value itself.
                let (key, value) = match st {
                    RealmSlotStep::StrKey => {
                        let holder = entry.name.rsplit_once('.').map_or("", |(h, _)| h);
                        let names: Vec<String> = super::members::realm_entry(holder)
                            .map(|e| {
                                e.own
                                    .iter()
                                    .filter(|p| !p.symbol)
                                    .map(|p| p.key.to_string())
                                    .collect()
                            })
                            .unwrap_or_default();
                        let s = m.node_l("native-strkey");
                        m.add_prim(s, crate::model::prim::STR);
                        (Key::Any(m.except_shape(names)), s)
                    }
                    _ => (step_key(m, st), value),
                };
                for n in base {
                    let site = m.anon_store_site();
                    m.cons(
                        n,
                        Cons::Store {
                            key,
                            src: value,
                            site,
                            region: m.thrown,
                        },
                    );
                }
            }
            RealmEffect::Stores { src, dst } => {
                // A store into the receiver's elements writes the Array.
                if let (RealmSlotRoot::This, Some(t)) = (dst.root, run.t) {
                    m.arr_written(t);
                    m.len_event(t, run.call, true);
                }
                let v = run.slot(m, src);
                // Rule W10-IDX: a store into the receiver's elements is a
                // [[Set]] under an index key (`Model::array_like_put`),
                // never one under any String key of a receiver that keeps
                // no elements in its any-key field.
                let d = match (dst.root, &*dst.steps, run.t) {
                    (RealmSlotRoot::This, [RealmSlotStep::Elems], Some(t)) => {
                        Some(m.array_like_put(t))
                    }
                    _ => run.node(m, dst),
                };
                if let Some(d) = d {
                    flow(m, &v, d);
                }
            }
            RealmEffect::Returns(o) => run.returns(m, o),
            RealmEffect::Open => unreachable!("interpretable() refused open"),
        }
    }
}

/// A program [[Set]] of key `key` on `t`, a declared fresh object
/// (`builtins::declared_box`) whose prototype P declares a method storing
/// into `strkey(this)` (`Storage.prototype.setItem`): WebIDL 3.9.2's named
/// property setter, as perry-runtime runs it (web_storage.rs: a String key
/// set on a Storage object is setItem): that method's coercion of the
/// stored argument (ToString: program `toString` / `valueOf` may run) and
/// a String under the key unless it is one of P's own names (the runtime
/// keeps the item off the object then). `None`: no such method; else the
/// key and value the store writes, `None` inside when it writes nothing.
pub(crate) fn strkey_set(
    m: &mut Model,
    t: Tok,
    key: Key,
    src: NodeId,
) -> Option<Option<(Key, NodeId)>> {
    let TokKind::Obj(tm, _) = m.toks[t as usize] else {
        return None;
    };
    let TmplKind::BObj { what, .. } = m.tmpls[tm as usize].kind else {
        return None;
    };
    let holder = super::members::realm_entry(what)?;
    let (entry, arg) = holder.own.iter().find_map(|o| {
        let RealmOwnValue::Reference(r) = o.value else {
            return None;
        };
        let (owner, name) = r.rsplit_once('.')?;
        let e = declared_at(owner, name)?;
        e.effects.iter().find_map(|f| match f {
            RealmEffect::Stores { src, dst }
                if dst.root == RealmSlotRoot::This
                    && matches!(&*dst.steps, [RealmSlotStep::StrKey]) =>
            {
                Some((e, src.root))
            }
            _ => None,
        })
    })?;
    for f in entry.effects.iter() {
        if let RealmEffect::Coerce { slot, protocol: p } = f {
            if slot.root == arg && slot.steps.is_empty() {
                let reason = m.reason(&format!("{} coerces its {}", entry.name, slot_text(slot)));
                let proto = protocol(*p);
                m.cons(
                    src,
                    Cons::Implicit {
                        reason,
                        proto,
                        region: m.thrown,
                    },
                );
            }
        }
    }
    let names: Vec<String> = holder
        .own
        .iter()
        .filter(|o| !o.symbol)
        .map(|o| o.key.to_string())
        .collect();
    let key = match key {
        Key::Named(a) if names.iter().any(|n| *n == m.atoms[a as usize]) => return Some(None),
        Key::Named(a) => Key::Named(a),
        Key::Any(_) => Key::Any(m.except_shape(names)),
    };
    let s = m.node_l("native-strkey");
    m.add_prim(s, crate::model::prim::STR);
    Some(Some((key, s)))
}

fn slot_text(s: &RealmSlot) -> String {
    let mut out = match s.root {
        RealmSlotRoot::This => "this".to_string(),
        RealmSlotRoot::Arg(i) => format!("arg({i})"),
        RealmSlotRoot::Rest(i) => format!("rest({i})"),
        RealmSlotRoot::Result => "result".to_string(),
        RealmSlotRoot::Returned(i) => format!("returned({i})"),
        RealmSlotRoot::Callee => "callee".to_string(),
    };
    for st in s.steps.iter() {
        out = match st {
            RealmSlotStep::Elems => format!("elems({out})"),
            RealmSlotStep::Iterated => format!("iterated({out})"),
            RealmSlotStep::OwnValues => format!("values({out})"),
            RealmSlotStep::StrKey => format!("strkey({out})"),
            RealmSlotStep::Proto => format!("proto({out})"),
            RealmSlotStep::Field(k) => format!("field({out}, {k})"),
            RealmSlotStep::Internal(k) => format!("islot({out}, {k})"),
        };
    }
    out
}
