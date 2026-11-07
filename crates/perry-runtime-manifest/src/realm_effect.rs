//! What a built-in function does to its inputs, declared at the runtime's own
//! definition of the function (`#[realm(Path, effects = { ... })]` on the
//! `fn_info!` that builds the body) and carried, typed, into the compiler's
//! realm table. Owned when read from the manifest, borrowed in the generated
//! static: one type, `Cow` fields.
//!
//! Anything a declaration does not state does not happen. A function with no
//! declaration (or with `open`) is unknown code.
use serde::{Deserialize, Serialize};
use std::borrow::Cow;

/// Where a slot starts: the receiver, a positional argument, the arguments
/// from an index on, the function's own result, or the value the N-th
/// declared `calls` (0-based, in declaration order) returned.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum RealmSlotRoot {
    This,
    Arg(u8),
    Rest(u8),
    Result,
    Returned(u8),
}

/// One step from a slot into what it holds: its elements (array elements,
/// Map/Set entries, iterator values) or one named field.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum RealmSlotStep {
    Elems,
    Field(Cow<'static, str>),
}

/// `this`, `arg(0)`, `elems(this)`, `field(arg(0), key)`, ...
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RealmSlot {
    pub root: RealmSlotRoot,
    /// Outermost step last: `elems(field(this, k))` is `[field k, elems]`.
    #[serde(default)]
    pub steps: Cow<'static, [RealmSlotStep]>,
}

/// The coercion and iteration protocols a native may run on a slot: the
/// solver's own protocols minus "any key", which is never declarable.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RealmProtocol {
    ToPrimitive,
    Iterate,
    IterNext,
    IterResult,
    Json,
    HasInstance,
    OwnKeys,
    /// Node's `util.inspect(v, { depth: -1 })` (an argument-type error's
    /// message on an object without a named constructor): the
    /// `[util.inspect.custom]` hook and the few labels printed before the
    /// depth check; no own value is read.
    ShallowInspect,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RealmPrim {
    Undefined,
    Null,
    Bool,
    Num,
    Str,
    Bigint,
    Symbol,
}

/// A value a native passes or returns: primitives of the listed kinds, the
/// value held in a slot, an object born by the call (its prototype is a
/// realm node; its elements are the listed slots), the object the species
/// constructor of a slot makes, or one of a few such values.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum RealmOperand {
    Prim(Cow<'static, [RealmPrim]>),
    Slot(RealmSlot),
    Fresh {
        proto: Cow<'static, str>,
        #[serde(default)]
        elems: Cow<'static, [RealmSlot]>,
    },
    /// ArraySpeciesCreate on `of`: reads `of.constructor` and its
    /// `@@species`; a program constructor is run as `new C(...args)` and what
    /// it returns is the value, else a fresh Array. `elems` are defined into
    /// it (CreateDataPropertyOrThrow, which runs a Proxy's traps).
    Species {
        of: RealmSlot,
        #[serde(default)]
        args: Cow<'static, [RealmOperand]>,
        #[serde(default)]
        elems: Cow<'static, [RealmSlot]>,
    },
    /// BoundFunctionCreate(this, arg(0), rest(1)) (`Function.prototype.bind`):
    /// a bound function exotic object whose target is the receiver, whose
    /// bound `this` is the first argument and whose bound arguments are the
    /// rest. Calling it calls the target with those.
    Bound,
    /// Any one of these values (at least two; no union inside a union).
    Union(Cow<'static, [RealmOperand]>),
}

/// The verbs.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum RealmEffect {
    /// Reads properties of the slots (ordinary [[Get]], [[HasProperty]]),
    /// which may run the program's getters and Proxy traps on them; retains
    /// nothing and calls no function-valued slot as a callback.
    Reads(Cow<'static, [RealmSlot]>),
    /// Runs exactly this protocol on the slot.
    Coerce {
        slot: RealmSlot,
        protocol: RealmProtocol,
    },
    /// Calls the function-valued slot with this receiver and arguments.
    Calls {
        callee: RealmSlot,
        this: RealmOperand,
        args: Cow<'static, [RealmOperand]>,
    },
    /// Keeps `src` in `dst` (ordinary [[Set]] into elements or a field).
    Stores { src: RealmSlot, dst: RealmSlot },
    /// `stores(fresh)`: the body stores only into objects it creates itself
    /// (a ToObject wrapper, a temporary, its fresh result); it keeps nothing
    /// in an object the program already holds.
    StoresFresh,
    /// What the call returns; several `returns` are a union.
    Returns(RealmOperand),
    /// Unknown code: today's handling, named.
    Open,
}

impl RealmSlot {
    fn validate(&self, at: &str) -> Result<(), String> {
        for step in self.steps.iter() {
            if let RealmSlotStep::Field(key) = step {
                if key.is_empty() {
                    return Err(format!("{at}: effect field key is empty"));
                }
            }
        }
        Ok(())
    }
}

impl RealmOperand {
    fn validate(&self, at: &str, names: &dyn Fn(&str) -> bool) -> Result<(), String> {
        match self {
            Self::Prim(kinds) if kinds.is_empty() => {
                Err(format!("{at}: prim() names no primitive kind"))
            }
            Self::Prim(_) => Ok(()),
            Self::Slot(slot) => slot.validate(at),
            Self::Bound => Ok(()),
            Self::Fresh { proto, elems } => {
                if !names(proto) {
                    return Err(format!(
                        "{at}: fresh object prototype {proto} is not a realm node"
                    ));
                }
                elems.iter().try_for_each(|slot| slot.validate(at))
            }
            Self::Species { of, args, elems } => {
                of.validate(at)?;
                args.iter().try_for_each(|a| a.validate(at, names))?;
                elems.iter().try_for_each(|slot| slot.validate(at))
            }
            Self::Union(members) => {
                if members.len() < 2 {
                    return Err(format!("{at}: a union needs at least two values"));
                }
                if members.iter().any(|m| matches!(m, Self::Union(_))) {
                    return Err(format!("{at}: a union inside a union"));
                }
                members.iter().try_for_each(|m| m.validate(at, names))
            }
        }
    }
}

/// Checks a declaration's effects against the realm graph: `open` stands
/// alone, `result` is only a store destination, `returned(N)` names one of
/// the declared calls and is only a value (never a callee, read or
/// destination; it may be coerced: `sort` runs ToNumber on its comparator's
/// result), fresh prototypes exist.
pub fn validate_effects(
    at: &str,
    effects: &[RealmEffect],
    names: &dyn Fn(&str) -> bool,
) -> Result<(), String> {
    if effects.len() > 1 && effects.iter().any(|e| matches!(e, RealmEffect::Open)) {
        return Err(format!(
            "{at}: `open` cannot be combined with other effects"
        ));
    }
    let calls = effects
        .iter()
        .filter(|e| matches!(e, RealmEffect::Calls { .. }))
        .count();
    let value = |slot: &RealmSlot| match slot.root {
        RealmSlotRoot::Result => Err(format!(
            "{at}: `result` names the call's own result; only `stores(.. -> ..)` may target it"
        )),
        RealmSlotRoot::Returned(n) if usize::from(n) >= calls => Err(format!(
            "{at}: `returned({n})` names no declared call ({calls} declared)"
        )),
        _ => slot.validate(at),
    };
    let not_result = |slot: &RealmSlot| match slot.root {
        RealmSlotRoot::Returned(n) => Err(format!(
            "{at}: `returned({n})` is a value: a store source, a coerced value or an operand only"
        )),
        _ => value(slot),
    };
    fn slots_in<'a>(o: &'a RealmOperand, out: &mut Vec<&'a RealmSlot>) {
        match o {
            RealmOperand::Prim(_) | RealmOperand::Bound => {}
            RealmOperand::Slot(slot) => out.push(slot),
            RealmOperand::Fresh { elems, .. } => out.extend(elems.iter()),
            RealmOperand::Species { of, args, elems } => {
                out.push(of);
                args.iter().for_each(|a| slots_in(a, out));
                out.extend(elems.iter());
            }
            RealmOperand::Union(members) => members.iter().for_each(|m| slots_in(m, out)),
        }
    }
    let operand = |o: &RealmOperand| {
        o.validate(at, names)?;
        let mut slots = Vec::new();
        slots_in(o, &mut slots);
        slots.into_iter().try_for_each(value)
    };
    for effect in effects {
        match effect {
            RealmEffect::Reads(slots) => {
                if slots.is_empty() {
                    return Err(format!("{at}: reads() names no slot"));
                }
                slots.iter().try_for_each(not_result)?;
            }
            // A call's result may be coerced (sort: ToNumber of the comparator's result).
            RealmEffect::Coerce { slot, .. } => value(slot)?,
            RealmEffect::Calls { callee, this, args } => {
                not_result(callee)?;
                operand(this)?;
                args.iter().try_for_each(operand)?;
            }
            RealmEffect::Stores { src, dst } => {
                value(src)?;
                if let RealmSlotRoot::Returned(n) = dst.root {
                    return Err(format!(
                        "{at}: `returned({n})` is a value, not a store destination"
                    ));
                }
                dst.validate(at)?;
            }
            RealmEffect::Returns(value) => operand(value)?,
            RealmEffect::StoresFresh | RealmEffect::Open => {}
        }
    }
    Ok(())
}

/// Const Rust for the generated compiler static (no JSON at solve time).
pub mod emit {
    use super::*;

    fn list<T>(items: &[T], f: impl Fn(&T) -> String) -> String {
        format!(
            "::std::borrow::Cow::Borrowed(&[{}])",
            items.iter().map(f).collect::<Vec<_>>().join(", ")
        )
    }

    fn slot(p: &str, s: &RealmSlot) -> String {
        let root = match s.root {
            RealmSlotRoot::This => format!("{p}RealmSlotRoot::This"),
            RealmSlotRoot::Arg(i) => format!("{p}RealmSlotRoot::Arg({i})"),
            RealmSlotRoot::Rest(i) => format!("{p}RealmSlotRoot::Rest({i})"),
            RealmSlotRoot::Result => format!("{p}RealmSlotRoot::Result"),
            RealmSlotRoot::Returned(i) => format!("{p}RealmSlotRoot::Returned({i})"),
        };
        let steps = list(&s.steps, |step| match step {
            RealmSlotStep::Elems => format!("{p}RealmSlotStep::Elems"),
            RealmSlotStep::Field(k) => {
                format!("{p}RealmSlotStep::Field(::std::borrow::Cow::Borrowed({k:?}))")
            }
        });
        format!("{p}RealmSlot {{ root: {root}, steps: {steps} }}")
    }

    fn operand(p: &str, o: &RealmOperand) -> String {
        match o {
            RealmOperand::Prim(kinds) => format!(
                "{p}RealmOperand::Prim({})",
                list(kinds, |k| format!("{p}RealmPrim::{k:?}"))
            ),
            RealmOperand::Slot(s) => format!("{p}RealmOperand::Slot({})", slot(p, s)),
            RealmOperand::Bound => format!("{p}RealmOperand::Bound"),
            RealmOperand::Fresh { proto, elems } => format!(
                "{p}RealmOperand::Fresh {{ proto: ::std::borrow::Cow::Borrowed({proto:?}), elems: {} }}",
                list(elems, |s| slot(p, s))
            ),
            RealmOperand::Species { of, args, elems } => format!(
                "{p}RealmOperand::Species {{ of: {}, args: {}, elems: {} }}",
                slot(p, of),
                list(args, |a| operand(p, a)),
                list(elems, |s| slot(p, s))
            ),
            RealmOperand::Union(members) => format!(
                "{p}RealmOperand::Union({})",
                list(members, |m| operand(p, m))
            ),
        }
    }

    /// `p` is the path prefix of the schema module, e.g. `crate::realm_manifest::`.
    pub fn effects(p: &str, effects: &[RealmEffect]) -> String {
        format!(
            "&[{}]",
            effects
                .iter()
                .map(|e| match e {
                    RealmEffect::Reads(slots) =>
                        format!("{p}RealmEffect::Reads({})", list(slots, |s| slot(p, s))),
                    RealmEffect::Coerce { slot: s, protocol } => format!(
                        "{p}RealmEffect::Coerce {{ slot: {}, protocol: {p}RealmProtocol::{protocol:?} }}",
                        slot(p, s)
                    ),
                    RealmEffect::Calls { callee, this, args } => format!(
                        "{p}RealmEffect::Calls {{ callee: {}, this: {}, args: {} }}",
                        slot(p, callee),
                        operand(p, this),
                        list(args, |a| operand(p, a))
                    ),
                    RealmEffect::Stores { src, dst } => format!(
                        "{p}RealmEffect::Stores {{ src: {}, dst: {} }}",
                        slot(p, src),
                        slot(p, dst)
                    ),
                    RealmEffect::Returns(o) => format!("{p}RealmEffect::Returns({})", operand(p, o)),
                    RealmEffect::StoresFresh => format!("{p}RealmEffect::StoresFresh"),
                    RealmEffect::Open => format!("{p}RealmEffect::Open"),
                })
                .collect::<Vec<_>>()
                .join(", ")
        )
    }
}
