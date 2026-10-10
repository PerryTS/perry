//! A `perry/thread` worker's realm inherits its spawner's class definition
//! evaluations.
//!
//! perry/thread agents share module-once initialization: a worker never runs
//! module init (#8546), so no class definition of the program is evaluated in
//! its realm. A class's literal members come from the image's static
//! declaration (`declarations.rs`), which every agent reads. A computed
//! member's key, though, is per evaluation: the evaluating agent keeps it on
//! its own class holder. Without more, a worker's holder has no keys, and its
//! prototype birth stops at the first computed member (losing every member
//! declared after it).
//!
//! So a worker's realm inherits the evaluations its spawner sees, as if its
//! own module init had evaluated each class definition to the same keys: at
//! spawn the spawner copies, for every class whose definition it has seen
//! evaluated, the keys of the class's computed instance members as
//! pointer-free records ([`InheritedEvaluations`]); the worker re-registers
//! a class's computed members under those keys — through the same
//! registration entry points the definition calls — when it mints the
//! class's holder, before the holder's prototype is born. A nested spawn
//! hands on what its spawner sees: its own holders, else what it inherited.
//!
//! Keys: a string by its bytes. A registered (`Symbol.for`) or well-known
//! symbol by its process-wide identity: it is the same symbol in every
//! agent. A unique `Symbol()` is a value of the spawner's heap, so the
//! worker's realm gets a unique symbol of its own with the same description,
//! which is what evaluating `Symbol(d)` in that realm would give.
//!
//! The records are the worker agent's own immutable snapshot, taken by its
//! spawner and read only when one of its holders is minted; no agent ever
//! writes another agent's records, and nothing is kept per image.

use std::sync::Arc;

use super::declarations::{
    class_declaration, ClassMemberDecl, CLASS_DECLARATIONS, CLASS_MEMBER_GETTER,
    CLASS_MEMBER_METHOD, CLASS_MEMBER_SETTER,
};

/// A computed member's evaluated property key, without heap pointers.
#[derive(Clone, Debug, PartialEq, Eq)]
enum InheritedKey {
    /// A string key's bytes (WTF-8).
    String(Box<[u8]>),
    /// A registered or well-known symbol: process-lifetime identity.
    ProcessSymbol(usize),
    /// A unique symbol's description (`None`: `Symbol()`).
    UniqueSymbol(Option<Box<[u8]>>),
}

#[derive(Clone, Debug)]
struct InheritedMember {
    definition_order: u32,
    key: InheritedKey,
}

/// The computed instance member keys of every class whose definition the
/// spawner had seen evaluated, sorted by class id.
#[derive(Debug, Default)]
pub struct InheritedEvaluations(Vec<(u32, Box<[InheritedMember]>)>);

impl InheritedEvaluations {
    fn class(&self, class_id: u32) -> Option<&[InheritedMember]> {
        self.0
            .binary_search_by_key(&class_id, |e| e.0)
            .ok()
            .map(|i| &*self.0[i].1)
    }
}

crate::perry_thread_local! {
    /// The evaluations this agent's realm inherited from its spawner
    /// (`None` on an agent that evaluates its own module graph).
    static INHERITED: std::cell::RefCell<Option<Arc<InheritedEvaluations>>> =
        std::cell::RefCell::new(None);
}

fn inherited() -> Option<Arc<InheritedEvaluations>> {
    INHERITED.with(|cell| cell.borrow().clone())
}

/// The record of a kept key value, or `None` for none (`undefined`) or a
/// member kept with no key (`null`, a test-seeded name).
///
/// # Safety
/// `value` is a value read from a traced slot of this agent.
unsafe fn inherited_key(value: f64) -> Option<InheritedKey> {
    let js = crate::JSValue::from_bits(value.to_bits());
    if js.is_pointer() && crate::symbol::js_is_symbol(value) != 0 {
        let sym = crate::symbol::sym_key_from_f64(value);
        if sym == 0 {
            return None;
        }
        if crate::symbol::is_global_registered_symbol(sym)
            || crate::symbol::is_well_known_symbol(sym)
        {
            return Some(InheritedKey::ProcessSymbol(sym));
        }
        let description =
            crate::symbol::symbol_description_text(sym as *const crate::symbol::SymbolHeader);
        return Some(InheritedKey::UniqueSymbol(
            description.map(|d| d.to_vec().into_boxed_slice()),
        ));
    }
    if !js.is_any_string() {
        return None;
    }
    let mut buf = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    crate::string::js_string_key_bytes(js, &mut buf)
        .map(|bytes| InheritedKey::String(bytes.to_vec().into_boxed_slice()))
}

/// What this agent sees evaluated, for a worker it is about to spawn: the
/// keys on its own minted holders, else the records it inherited itself.
pub(crate) fn inherited_evaluations_for_spawn() -> Arc<InheritedEvaluations> {
    let own_inherited = inherited();
    let mut out: Vec<(u32, Box<[InheritedMember]>)> = Vec::new();
    let Ok(index) = CLASS_DECLARATIONS.read() else {
        return Arc::new(InheritedEvaluations(out));
    };
    for (class_id, decl) in index.entries() {
        if !decl.has_computed_members() {
            continue;
        }
        let members: Vec<InheritedMember> =
            match crate::object::class_value::class_value_if_minted(class_id) {
                Some(_) => decl
                    .members()
                    .iter()
                    .filter(|m| m.name.is_null())
                    .filter_map(|m| {
                        let value =
                            super::declarations::computed_key_value(class_id, m.definition_order)?;
                        // SAFETY: read from this agent's live holder just now.
                        let key = unsafe { inherited_key(value) }?;
                        Some(InheritedMember {
                            definition_order: m.definition_order,
                            key,
                        })
                    })
                    .collect(),
                None => own_inherited
                    .as_ref()
                    .and_then(|seed| seed.class(class_id))
                    .map(<[InheritedMember]>::to_vec)
                    .unwrap_or_default(),
            };
        if !members.is_empty() {
            out.push((class_id, members.into_boxed_slice()));
        }
    }
    drop(index);
    out.sort_by_key(|e| e.0);
    Arc::new(InheritedEvaluations(out))
}

/// Start the calling worker agent's realm with `evaluations` (before any of
/// its holders can be minted).
pub(crate) fn adopt_inherited_evaluations(evaluations: Arc<InheritedEvaluations>) {
    INHERITED.with(|cell| *cell.borrow_mut() = Some(evaluations));
}

/// Did this agent's realm inherit an evaluation of class `class_id`?
pub(crate) fn inherits_class(class_id: u32) -> bool {
    inherited().is_some_and(|seed| seed.class(class_id).is_some())
}

/// This agent's value for `key`.
///
/// # Safety
/// Runs inside the holder mint's no-collect scope: the returned value is
/// not rooted.
unsafe fn materialize(key: &InheritedKey) -> f64 {
    match key {
        InheritedKey::String(bytes) => {
            let s = crate::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
            f64::from_bits(crate::value::JSValue::string_ptr(s).bits())
        }
        InheritedKey::ProcessSymbol(sym) => crate::value::js_nanbox_pointer(*sym as i64),
        InheritedKey::UniqueSymbol(None) => crate::symbol::js_symbol_new_empty(),
        InheritedKey::UniqueSymbol(Some(description)) => {
            let s =
                crate::string::js_string_from_bytes(description.as_ptr(), description.len() as u32);
            crate::symbol::js_symbol_new(f64::from_bits(
                crate::value::JSValue::string_ptr(s).bits(),
            ))
        }
    }
}

/// The holder of class `class_id` was just minted on this agent: when the
/// realm inherited the class's evaluation, register the class's computed
/// instance members under the inherited keys, as the definition did.
///
/// Called by the mint before the prototype is born, inside its no-collect
/// scope (the holder is already this agent's class value).
pub(crate) fn replay_inherited_evaluation(class_id: u32) {
    let Some(seed) = inherited() else {
        return;
    };
    let Some(members) = seed.class(class_id) else {
        return;
    };
    let Some(decl) = class_declaration(class_id) else {
        return;
    };
    let undefined = f64::from_bits(crate::value::TAG_UNDEFINED);
    for inherited in members {
        let Some(member) = decl
            .members()
            .iter()
            .find(|m| m.name.is_null() && m.definition_order == inherited.definition_order)
        else {
            continue;
        };
        // SAFETY: the mint's no-collect scope covers the key's allocation and
        // its registration; the member's code is the image's.
        unsafe {
            let key = materialize(&inherited.key);
            replay_member(class_id, member, key, undefined);
        }
    }
}

/// Register computed instance member `member` of `class_id` under `key`,
/// exactly as its class definition's registration call does.
///
/// # Safety
/// `key` is a live property key of this agent.
unsafe fn replay_member(class_id: u32, member: &ClassMemberDecl, key: f64, owner: f64) {
    let code = member.code as i64;
    let order = member.definition_order as i64;
    match member.kind {
        CLASS_MEMBER_METHOD => super::parent_static::js_register_class_computed_method(
            class_id as i64,
            key,
            code,
            member.param_count as i64,
            0,
            member.has_rest() as i64,
            order,
            0,
            owner,
        ),
        CLASS_MEMBER_GETTER => super::parent_static::js_register_class_computed_accessor(
            class_id as i64,
            key,
            code,
            0,
            0,
            order,
            owner,
        ),
        CLASS_MEMBER_SETTER => super::parent_static::js_register_class_computed_accessor(
            class_id as i64,
            key,
            0,
            code,
            0,
            order,
            owner,
        ),
        _ => {}
    }
}

/// A decl prototype birth of class `class_id` stopped at computed member
/// `member`, which this agent has no key for. That is right while a
/// definition of the class is still being evaluated, but never in a realm
/// that inherited a key for it: the holder mint replays inherited keys
/// before any birth, so a miss here means the members after `member` would
/// silently be lost. Fail loudly.
pub(crate) fn assert_birth_stop_unnamed(class_id: u32, member: &ClassMemberDecl) {
    let Some(seed) = inherited() else {
        return;
    };
    let Some(members) = seed.class(class_id) else {
        return;
    };
    if members
        .iter()
        .any(|m| m.definition_order == member.definition_order)
    {
        eprintln!(
            "perry: internal error: class {class_id:#x}'s prototype was born without its \
             computed member {} although this agent's realm inherited its key \
             (inherited_evaluation.rs)",
            member.definition_order
        );
        std::process::abort();
    }
}
