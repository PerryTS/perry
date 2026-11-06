//! Compile-time coverage records. These bytes never enter the executable.
use super::*;
use serde_json::{json, Value};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug)]
pub(crate) enum Fact {
    Receiver,
    Slot,
    Kind,
    Presence,
    HolderClasses,
    TypedOwner,
    Bodies,
    ClassAlias,
    Static,
    FixedLayout,
    InterpreterGuard,
    FunctionBind,
    FunctionBorrow,
    ArrayIteration,
    IteratorClose,
    Super,
    OwnerBrand,
    ExactExtent,
    MinimumExtent,
    Separation,
    NumberInput,
    IndexedMetadata,
    NumericBody,
    NumericResult,
    ProtoHolder,
    ProtoAdd,
}
impl Fact {
    pub(super) fn name(self) -> &'static str {
        match self {
            Self::Receiver => "receiver set",
            Self::Slot => "field slot",
            Self::Kind => "field kind",
            Self::Presence => "declared/reserved presence",
            Self::HolderClasses => "holder classes",
            Self::TypedOwner => "typed owner",
            Self::Bodies => "callee bodies",
            Self::ClassAlias => "class alias",
            Self::Static => "constant static",
            Self::FixedLayout => "fixed layout",
            Self::InterpreterGuard => "interpreter guard",
            Self::FunctionBind => "function bind",
            Self::FunctionBorrow => "function borrow",
            Self::ArrayIteration => "array iteration",
            Self::IteratorClose => "iterator close",
            Self::Super => "super link",
            Self::OwnerBrand => "indexed owner brand",
            Self::ExactExtent => "indexed exact extent",
            Self::MinimumExtent => "indexed minimum extent",
            Self::Separation => "indexed separation",
            Self::NumberInput => "number input",
            Self::IndexedMetadata => "indexed metadata",
            Self::NumericBody => "numeric body",
            Self::NumericResult => "numeric result",
            Self::ProtoHolder => "prototype holder",
            Self::ProtoAdd => "prototype add",
        }
    }
}

struct Inventory {
    claims: BTreeMap<String, Value>,
    copies: BTreeMap<(String, u64), Vec<Value>>,
}
static RECORDS: Mutex<Inventory> = Mutex::new(Inventory {
    claims: BTreeMap::new(),
    copies: BTreeMap::new(),
});

pub(crate) fn consumed(fact: Fact, operands: impl FnOnce() -> Value) {
    if !enabled() {
        return;
    }
    record(fact.name(), operands(), false);
}

pub(crate) fn checked(fact: Fact, operands: Value) {
    record(fact.name(), operands, true);
}

fn record(predicate: &str, operands: Value, checked: bool) {
    let (module, offset, copy, emission) = MODULE.with(|m| {
        let m = m.borrow();
        match m.as_ref() {
            Some(m) => (
                m.name.clone(),
                SITE.with(|s| s.get().map(|s| s.0.saturating_sub(1))),
                m.copy.clone(),
                m.ordinal,
            ),
            None => (
                "<program>".to_owned(),
                None,
                "<layout planning>".to_owned(),
                0,
            ),
        }
    });
    let site_copy = COPY_SITE.with(|s| s.get());
    let identity = json!([module, offset, copy, site_copy, predicate, operands]).to_string();
    let mut records = RECORDS.lock().expect("verification coverage poisoned");
    let entry = records.claims.entry(identity.clone()).or_insert_with(|| json!({
        "identity":identity, "module":module, "byte_offset":offset, "copy":{"body":copy,"site":site_copy},
        "predicate":predicate,"operands":operands,"check":null,
        "reason":"not covered before slice S6"}));
    if checked {
        entry["check"] = json!(predicate);
        entry["reason"] = Value::Null;
        let emissions = entry
            .as_object_mut()
            .unwrap()
            .entry("raw_emissions")
            .or_insert_with(|| json!([]))
            .as_array_mut()
            .unwrap();
        if !emissions.contains(&json!(emission)) {
            emissions.push(json!(emission));
        }
    }
}

pub(crate) fn field(p: &crate::expr::wpfacts_access::FieldProof, key: &str, checked: bool) {
    if !enabled() {
        return;
    }
    for (fact, operands) in [
        (
            Fact::Receiver,
            json!({"classes":p.class_ids,"nullable":p.nullable,"guarded":p.guarded()}),
        ),
        (
            Fact::Slot,
            json!({"classes":p.class_ids,"key":key,"slots":p.slots}),
        ),
        (
            Fact::Kind,
            json!({"classes":p.class_ids,"key":key,"mask":p.kind_mask}),
        ),
    ] {
        if checked {
            self::checked(fact, operands);
        } else {
            record(fact.name(), operands, false);
        }
    }
}

pub(crate) fn entry(id: u32, name: &str, copy: &str) {
    let operands = json!({"body_id":id,"body_name":name,"emitted_copy":copy});
    record("inactive body", operands, true);
}

pub fn take_manifest() -> Vec<String> {
    let mut inventory = RECORDS.lock().expect("verification coverage poisoned");
    let claims = std::mem::take(&mut inventory.claims);
    let copies = std::mem::take(&mut inventory.copies);
    claims.into_values().map(|mut r| {
        if let Some(emissions) = r.as_object_mut().unwrap().remove("raw_emissions") {
            let mut checks = Vec::new();
            let mut optimized = true;
            for emission in emissions.as_array().unwrap() {
                let identity = (r["module"].as_str().unwrap().to_owned(), emission.as_u64().unwrap());
                match copies.get(&identity) {
                    Some(c) => checks.extend(c.iter().cloned()),
                    None => optimized = false,
                }
            }
            assert!(optimized, "checked claim did not reach final copy accounting: {r}");
            if checks.is_empty() {
                r["check"] = Value::Null;
                r["dead"] = json!("LLVM optimization removed every use of this failure site's static metadata");
            } else {
                r["emitted_checks"] = json!(checks);
            }
        }
        r.to_string()
    }).collect()
}

pub(crate) fn optimized(origins: &[(String, u64)]) {
    let mut inventory = RECORDS.lock().expect("verification coverage poisoned");
    for origin in origins {
        inventory.copies.entry(origin.clone()).or_default();
    }
}

pub(crate) fn emitted(module: &str, emission: u64, copy: Value) {
    let mut inventory = RECORDS.lock().expect("verification coverage poisoned");
    let copies = inventory
        .copies
        .entry((module.to_owned(), emission))
        .or_default();
    if !copies.contains(&copy) {
        copies.push(copy);
    }
}

pub(crate) fn number(id: u32) {
    checked(Fact::NumberInput, json!({"local":id}));
}

pub(crate) fn normal_call_miss(class: &str, key: &str) {
    if !enabled() {
        return;
    }
    record(
        "normal-build call-miss",
        json!({"class":class,"key":key}),
        true,
    );
}

pub(crate) fn reset() {
    let mut inventory = RECORDS.lock().expect("verification coverage poisoned");
    inventory.claims.clear();
    inventory.copies.clear();
}
