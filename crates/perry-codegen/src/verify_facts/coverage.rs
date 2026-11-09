//! Compile-time coverage records. These bytes never enter the executable.
use super::*;
use serde_json::{json, Value};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug)]
pub(crate) enum Fact {
    Receiver,
    Slot,
    SlotKey,
    Kind,
    Presence,
    HolderClasses,
    TypedOwner,
    Bodies,
    ClassAlias,
    Static,
    StaticGuard,
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
    Int32Input,
    ConstLength,
    IndexArgument,
    IndexedMetadata,
    NumericBody,
    NumericResult,
    ProtoHolder,
    ProtoAdd,
    ElementRead,
    FieldReadCall,
    CanonicalNumber,
    Signature,
    ArrayRep,
}
impl Fact {
    pub(super) fn name(self) -> &'static str {
        match self {
            Self::Receiver => "receiver set",
            Self::Slot => "field slot",
            Self::SlotKey => "field slot key",
            Self::Kind => "field kind",
            Self::Presence => "declared/reserved presence",
            Self::HolderClasses => "holder classes",
            Self::TypedOwner => "typed owner",
            Self::Bodies => "callee bodies",
            Self::ClassAlias => "class alias",
            Self::Static => "constant static",
            Self::StaticGuard => "static member guard",
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
            Self::Int32Input => "int32 input",
            Self::ConstLength => "array constant length",
            Self::IndexArgument => "index argument",
            Self::IndexedMetadata => "indexed metadata",
            Self::NumericBody => "numeric body",
            Self::NumericResult => "numeric result",
            Self::ProtoHolder => "prototype holder",
            Self::ProtoAdd => "prototype add",
            Self::ElementRead => "proven element read",
            Self::FieldReadCall => "field read call",
            Self::CanonicalNumber => "canonical number",
            Self::Signature => "signature",
            Self::ArrayRep => "array rep",
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
    let identity = record(fact.name(), operands, true);
    if matches!(
        fact,
        Fact::Bodies | Fact::StaticGuard | Fact::NumberInput | Fact::Int32Input | Fact::ConstLength
    ) {
        let mut inventory = RECORDS.lock().expect("verification coverage poisoned");
        unit_report_validation(inventory.claims.get_mut(&identity).unwrap());
    }
}

fn unit_report_validation(row: &mut Value) {
    row["reporting_validation"] =
        json!("Runtime unit subprocess controls; no false-claim program witness on main at S0");
}

fn record(predicate: &str, operands: Value, checked: bool) -> String {
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
        "category":"consumed predicate","consumed":true,
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
    identity
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
        if checked && matches!(fact, Fact::Slot) {
            // Key membership does not establish data attributes, address or lane.
            self::checked(Fact::SlotKey, operands);
        } else if checked {
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
    claims
        .into_values()
        .map(|mut r| {
            if let Some(emissions) = r.as_object_mut().unwrap().remove("raw_emissions") {
                r["lowered_emissions"] = emissions.clone();
                let mut checks = Vec::new();
                let mut optimized = true;
                for emission in emissions.as_array().unwrap() {
                    let identity = (
                        r["module"].as_str().unwrap().to_owned(),
                        emission.as_u64().unwrap(),
                    );
                    match copies.get(&identity) {
                        Some(c) => checks.extend(c.iter().cloned()),
                        None => optimized = false,
                    }
                }
                assert!(
                    optimized,
                    "checked claim did not reach final copy accounting: {r}"
                );
                if checks.is_empty() {
                    r["check"] = Value::Null;
                    r["dead"] = json!(
                        "No live use of the failure metadata after lowering and LLVM optimization"
                    );
                } else {
                    r["emitted_checks"] = json!(checks);
                }
            }
            r.to_string()
        })
        .collect()
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

pub(crate) fn int32(id: u32) {
    checked(Fact::Int32Input, json!({"local":id}));
}

pub(crate) fn const_length(id: u32) {
    checked(Fact::ConstLength, json!({"local":id}));
}

pub(crate) fn normal_call_miss(class: &str, key: &str) {
    if !enabled() {
        return;
    }
    let identity = record(
        "normal-build call-miss",
        json!({"class":class,"key":key}),
        true,
    );
    let mut inventory = RECORDS.lock().expect("verification coverage poisoned");
    let row = inventory.claims.get_mut(&identity).unwrap();
    row["category"] = json!("normal-build site");
    row["consumed"] = json!(false);
    unit_report_validation(row);
}

pub(crate) fn reset() {
    let mut inventory = RECORDS.lock().expect("verification coverage poisoned");
    inventory.claims.clear();
    inventory.copies.clear();
}

pub(super) fn sites(claims: impl IntoIterator<Item = SiteClaim>) {
    let mut inventory = RECORDS.lock().expect("verification coverage poisoned");
    for claim in claims {
        let identity = json!(["solver site", claim.module, claim.ordinal]).to_string();
        inventory.claims.entry(identity.clone()).or_insert_with(|| {
            json!({
                "identity":identity, "category":"solver site", "consumed":false,
                "module":claim.module, "byte_offset":claim.byte_offset, "key":claim.key,
                "copy":{"solver_bodies":claim.bodies}, "predicate":claim.verdict.name(),
                "operands":{"is_call":claim.is_call}, "check":null,
                "reason":format!("not covered before slice S{}",claim.verdict.slice())
            })
        });
    }
}
