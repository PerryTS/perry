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
/// from an index on, the function's own result, the value the N-th
/// declared `calls` (0-based, in declaration order) returned, or the
/// function value called itself (`callee`): a closure the runtime made
/// keeps what it captured in internal slots of that value
/// (`islot(callee, self)`: the object whose declaration installed it),
/// which a closure body reads instead of `this`.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum RealmSlotRoot {
    This,
    Arg(u8),
    Rest(u8),
    Result,
    Returned(u8),
    Callee,
}

/// One step from a slot into what it holds: its elements (array elements,
/// Map/Set entries, iterator values), one named field, or one internal
/// slot of a host object (`islot(s, reason)`: the abort reason of a signal),
/// which no program key names and no [[Get]] reads.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum RealmSlotStep {
    Elems,
    Field(Cow<'static, str>),
    Internal(Cow<'static, str>),
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

impl RealmProtocol {
    /// Keys a protocol reads on its operand; a read keeps the slot without
    /// making the value in that slot escape or exposing its members.
    pub fn receiver_keys(self) -> &'static [&'static str] {
        match self {
            Self::ToPrimitive => &["@@toPrimitive", "valueOf", "toString"],
            Self::Iterate => &["@@iterator"],
            Self::IterNext => &["next", "return", "throw"],
            Self::IterResult => &["done", "value"],
            Self::Json => &["toJSON"],
            Self::HasInstance => &["@@hasInstance", "prototype"],
            Self::OwnKeys => &[],
            Self::ShallowInspect => &[
                "@for:nodejs.util.inspect.custom",
                "@@toStringTag",
                "stack",
                "message",
                "name",
                "errors",
                "byteLength",
                "href",
                "constructor",
            ],
        }
    }

    /// Await's PromiseResolve reads the thenable's slot, not its chain.
    pub fn await_receiver_keys() -> &'static [&'static str] {
        &["then"]
    }
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
        elems: Cow<'static, [RealmOperand]>,
        /// Own data fields the call defines on the object, with their values.
        #[serde(default)]
        fields: Cow<'static, [RealmField]>,
        /// Internal slots the call gives the object (`islot` steps read
        /// them), with their values.
        #[serde(default, skip_serializing_if = "no_slots")]
        slots: Cow<'static, [RealmField]>,
    },
    /// A promise born by the call: it fulfills with one of `value` (a
    /// thenable among them is adopted) or rejects with one of `reject`.
    Promise {
        value: Cow<'static, [RealmOperand]>,
        #[serde(default)]
        reject: Cow<'static, [RealmOperand]>,
    },
    /// The host function value the realm declares at this path (a closure
    /// the body makes); its own declaration says what calling it does.
    Native(Cow<'static, str>),
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
    /// The value `JSON.parse` makes of the text the call reads: ordinary
    /// objects and arrays of such values, and primitives.
    Json,
    /// A value code the program cannot see supplies: the native hands a
    /// value to unknown code (`escapes`) and gives back what that code
    /// produces (a user body stream that errors).
    Unknown,
    /// Any one of these values (at least two; no union inside a union).
    Union(Cow<'static, [RealmOperand]>),
}

/// One own data field of a fresh object: its key and its value.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RealmField {
    pub key: Cow<'static, str>,
    pub value: RealmOperand,
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
    /// Hands the slot's values to code no declaration describes (fetch's
    /// `init.body`: node iterates an async-iterable body, which calls the
    /// program's methods): they escape, and what they reach.
    Escapes(RealmSlot),
    /// The body hands the slot's values to `init` of every enabled
    /// async_hooks hook as the resource (`async_hook_slots::init_resource`):
    /// code no declaration describes gets them while a hook may be enabled.
    AsyncResource(RealmSlot),
    /// `stores(fresh)`: the body stores only into objects it creates itself
    /// (a ToObject wrapper, a temporary, its fresh result); it keeps nothing
    /// in an object the program already holds.
    StoresFresh,
    /// What the call returns; several `returns` are a union.
    Returns(RealmOperand),
    /// Compares the slot's values by identity (SameValue, SameValueZero,
    /// IsStrictlyEqual, a Map hash of an object): the program could tell a
    /// copy from the original there (DESIGN-p4l4 §2.2). Reads nothing,
    /// keeps nothing.
    Identity(RealmSlot),
    /// Unknown code: today's handling, named.
    Open,
}

impl RealmSlot {
    fn validate(&self, at: &str) -> Result<(), String> {
        for step in self.steps.iter() {
            if let RealmSlotStep::Field(key) | RealmSlotStep::Internal(key) = step {
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
            Self::Bound | Self::Json | Self::Unknown => Ok(()),
            Self::Fresh {
                proto,
                elems,
                fields,
                slots,
            } => {
                if !names(proto) {
                    return Err(format!(
                        "{at}: fresh object prototype {proto} is not a realm node"
                    ));
                }
                if fields.iter().chain(slots.iter()).any(|f| f.key.is_empty()) {
                    return Err(format!("{at}: fresh field key is empty"));
                }
                elems.iter().try_for_each(|e| e.validate(at, names))?;
                fields
                    .iter()
                    .chain(slots.iter())
                    .try_for_each(|f| f.value.validate(at, names))
            }
            Self::Promise { value, reject } => {
                if value.is_empty() {
                    return Err(format!("{at}: promise() names no fulfillment value"));
                }
                value
                    .iter()
                    .chain(reject.iter())
                    .try_for_each(|v| v.validate(at, names))
            }
            Self::Native(path) => {
                if !names(path) {
                    return Err(format!("{at}: native {path} is not a realm node"));
                }
                Ok(())
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
/// alone, `result` is only a store destination (or, as it is, a store
/// source), `returned(N)` names one of
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
            RealmOperand::Prim(_)
            | RealmOperand::Bound
            | RealmOperand::Json
            | RealmOperand::Unknown
            | RealmOperand::Native(_) => {}
            RealmOperand::Slot(slot) => out.push(slot),
            RealmOperand::Fresh {
                elems,
                fields,
                slots,
                ..
            } => {
                elems.iter().for_each(|e| slots_in(e, out));
                fields
                    .iter()
                    .chain(slots.iter())
                    .for_each(|f| slots_in(&f.value, out));
            }
            RealmOperand::Promise { value, reject } => {
                value
                    .iter()
                    .chain(reject.iter())
                    .for_each(|v| slots_in(v, out));
            }
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
                // A function the result holds may be called later (a
                // signal's `onabort` when its timer fires), with the
                // result as `this`; the result itself is no function the
                // call makes and calls.
                let later = callee.root == RealmSlotRoot::Result && !callee.steps.is_empty();
                if later {
                    callee.validate(at)?;
                } else {
                    not_result(callee)?;
                }
                match this {
                    RealmOperand::Slot(s) if later && s.root == RealmSlotRoot::Result => {
                        s.validate(at)?
                    }
                    _ => operand(this)?,
                }

                // A callback may be handed the result or what it holds
                // (node's ReadableStream calls `start` with a controller
                // whose stream is the result, `cancel` with the reason the
                // result keeps): an argument may name `result`.
                args.iter().try_for_each(|a| {
                    a.validate(at, names)?;
                    let mut slots = Vec::new();
                    slots_in(a, &mut slots);
                    slots.into_iter().try_for_each(|s| match s.root {
                        RealmSlotRoot::Result => s.validate(at),
                        _ => value(s),
                    })
                })?;
            }
            RealmEffect::Stores { src, dst } => {
                // The call's own result may be kept as it is (`watchFile`
                // keeps the watcher it returns, which a later call on the
                // same path returns again).
                if src.root == RealmSlotRoot::Result && src.steps.is_empty() {
                    src.validate(at)?;
                } else {
                    value(src)?;
                }
                if let RealmSlotRoot::Returned(n) = dst.root {
                    return Err(format!(
                        "{at}: `returned({n})` is a value, not a store destination"
                    ));
                }
                dst.validate(at)?;
            }
            RealmEffect::Returns(value) => operand(value)?,
            RealmEffect::Escapes(slot) => not_result(slot)?,
            // An input of the call: not its result, not a call's return.
            RealmEffect::Identity(slot) => not_result(slot)?,
            // The runtime hands over the object it made and returns.
            RealmEffect::AsyncResource(slot) => slot.validate(at)?,
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
            RealmSlotRoot::Callee => format!("{p}RealmSlotRoot::Callee"),
        };
        let steps = list(&s.steps, |step| match step {
            RealmSlotStep::Elems => format!("{p}RealmSlotStep::Elems"),
            RealmSlotStep::Field(k) => {
                format!("{p}RealmSlotStep::Field(::std::borrow::Cow::Borrowed({k:?}))")
            }
            RealmSlotStep::Internal(k) => {
                format!("{p}RealmSlotStep::Internal(::std::borrow::Cow::Borrowed({k:?}))")
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
            RealmOperand::Json => format!("{p}RealmOperand::Json"),
            RealmOperand::Unknown => format!("{p}RealmOperand::Unknown"),
            RealmOperand::Fresh {
                proto,
                elems,
                fields,
                slots,
            } => {
                let field = |f: &RealmField| {
                    format!(
                        "{p}RealmField {{ key: ::std::borrow::Cow::Borrowed({:?}), value: {} }}",
                        f.key,
                        operand(p, &f.value)
                    )
                };
                format!(
                    "{p}RealmOperand::Fresh {{ proto: ::std::borrow::Cow::Borrowed({proto:?}), elems: {}, fields: {}, slots: {} }}",
                    list(elems, |e| operand(p, e)),
                    list(fields, field),
                    list(slots, field)
                )
            }
            RealmOperand::Promise { value, reject } => format!(
                "{p}RealmOperand::Promise {{ value: {}, reject: {} }}",
                list(value, |v| operand(p, v)),
                list(reject, |v| operand(p, v))
            ),
            RealmOperand::Native(path) => {
                format!("{p}RealmOperand::Native(::std::borrow::Cow::Borrowed({path:?}))")
            }
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
                    RealmEffect::Escapes(s) => format!("{p}RealmEffect::Escapes({})", slot(p, s)),
                    RealmEffect::AsyncResource(s) => {
                        format!("{p}RealmEffect::AsyncResource({})", slot(p, s))
                    }
                    RealmEffect::StoresFresh => format!("{p}RealmEffect::StoresFresh"),
                    RealmEffect::Identity(s) => format!("{p}RealmEffect::Identity({})", slot(p, s)),

                    RealmEffect::Open => format!("{p}RealmEffect::Open"),
                })
                .collect::<Vec<_>>()
                .join(", ")
        )
    }
}

fn no_slots(slots: &Cow<'static, [RealmField]>) -> bool {
    slots.is_empty()
}

#[cfg(test)]
mod operand_tests {
    use super::*;

    #[test]
    fn identity_names_an_input_of_the_call() {
        let slot = |root| RealmSlot {
            root,
            steps: Cow::Borrowed(&[]),
        };
        let names = |_: &str| true;
        for ok in [
            RealmSlotRoot::This,
            RealmSlotRoot::Arg(0),
            RealmSlotRoot::Rest(1),
        ] {
            validate_effects("t", &[RealmEffect::Identity(slot(ok))], &names).unwrap();
        }
        for bad in [RealmSlotRoot::Result, RealmSlotRoot::Returned(0)] {
            assert!(validate_effects("t", &[RealmEffect::Identity(slot(bad))], &names).is_err());
        }
        let e: RealmEffect =
            serde_json::from_str(r#"{"identity":{"root":{"arg":0},"steps":["elems"]}}"#).unwrap();
        assert!(matches!(e, RealmEffect::Identity(ref s) if s.root == RealmSlotRoot::Arg(0)));
        assert!(super::emit::effects("p::", &[e]).contains("p::RealmEffect::Identity("));
    }

    #[test]
    fn json_and_unknown_operands_parse_and_round_trip() {
        for (text, want) in [
            ("\"json\"", RealmOperand::Json),
            ("\"unknown\"", RealmOperand::Unknown),
        ] {
            let got: RealmOperand = serde_json::from_str(text).unwrap();
            assert_eq!(got, want);
            assert_eq!(serde_json::to_string(&got).unwrap(), text);
        }
        let union: RealmOperand = serde_json::from_str(r#"{"union":["json","unknown"]}"#).unwrap();
        assert_eq!(
            union,
            RealmOperand::Union(vec![RealmOperand::Json, RealmOperand::Unknown].into())
        );
        assert!(serde_json::from_str::<RealmOperand>("\"jsonn\"").is_err());
    }
}

#[cfg(test)]
mod reach_key_tests {
    use super::*;
    #[test]
    fn hasinstance_declares_both_key_reads() {
        assert_eq!(
            RealmProtocol::HasInstance.receiver_keys(),
            &["@@hasInstance", "prototype"]
        );
        let read: RealmEffect =
            serde_json::from_str(r#"{"reads":[{"root":"this","steps":[{"field":"prototype"}]}]}"#)
                .unwrap();
        assert!(
            matches!(read, RealmEffect::Reads(ref s) if s[0].steps.as_ref() == [RealmSlotStep::Field("prototype".into())])
        );
    }
}
