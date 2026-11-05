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
/// from an index on, or the function's own result.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum RealmSlotRoot {
    This,
    Arg(u8),
    Rest(u8),
    Result,
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
/// value held in a slot, or an object born by the call (its prototype is a
/// realm node; its elements are the listed slots).
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
}

/// The six verbs.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum RealmEffect {
    /// Reads fields/elements of the slots (ordinary [[Get]]); retains
    /// nothing, calls no function-valued slot.
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
            Self::Fresh { proto, elems } => {
                if !names(proto) {
                    return Err(format!(
                        "{at}: fresh object prototype {proto} is not a realm node"
                    ));
                }
                elems.iter().try_for_each(|slot| slot.validate(at))
            }
        }
    }
}

/// Checks a declaration's effects against the realm graph: `open` stands
/// alone, `result` is only a store destination, fresh prototypes exist.
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
    let not_result = |slot: &RealmSlot| {
        if slot.root == RealmSlotRoot::Result {
            Err(format!(
                "{at}: `result` names the call's own result; only `stores(.. -> ..)` may target it"
            ))
        } else {
            slot.validate(at)
        }
    };
    let operand = |o: &RealmOperand| match o {
        RealmOperand::Slot(slot) => not_result(slot),
        other => other.validate(at, names),
    };
    for effect in effects {
        match effect {
            RealmEffect::Reads(slots) => {
                if slots.is_empty() {
                    return Err(format!("{at}: reads() names no slot"));
                }
                slots.iter().try_for_each(not_result)?;
            }
            RealmEffect::Coerce { slot, .. } => not_result(slot)?,
            RealmEffect::Calls { callee, this, args } => {
                not_result(callee)?;
                operand(this)?;
                args.iter().try_for_each(operand)?;
            }
            RealmEffect::Stores { src, dst } => {
                not_result(src)?;
                dst.validate(at)?;
            }
            RealmEffect::Returns(value) => operand(value)?,
            RealmEffect::Open => {}
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
            RealmOperand::Fresh { proto, elems } => format!(
                "{p}RealmOperand::Fresh {{ proto: ::std::borrow::Cow::Borrowed({proto:?}), elems: {} }}",
                list(elems, |s| slot(p, s))
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
                    RealmEffect::Open => format!("{p}RealmEffect::Open"),
                })
                .collect::<Vec<_>>()
                .join(", ")
        )
    }
}
