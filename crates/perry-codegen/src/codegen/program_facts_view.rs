//! wpfacts (whole-program invariance facts, slice 1): the part of the
//! program's facts one module's codegen can observe.
//!
//! Slice 1 removed no check; codegen consumed only two things:
//! - `digest`, which goes into the module's object-cache key (an edit that
//!   changes a fact this module could use recompiles it);
//! - `contract_section`, the claims about the class templates this module
//!   defines. It is non-empty only in a contract build
//!   (`PERRY_CHECK_CONTRACTS=1`) and becomes a read-only `.perry_facts`
//!   section the runtime checks its dynamic operations against. Release
//!   builds carry no section.

use crate::module::LlModule;

/// The module's view. Slice 2 adds `elision`: the receiver facts of this
/// module's bindings and the layout facts of the classes they can hold,
/// which the zero-guard tier (`expr/wpfacts_access.rs`) consumes. The driver
/// folds a hash of it into `digest`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProgramFactsView {
    pub digest: u64,
    pub contract_section: std::sync::Arc<Vec<u8>>,
    pub elision: std::sync::Arc<ElisionFacts>,
}

/// What a binding (a local, or a function's `this`) can hold, when that is
/// only instances of prefix-stable classes (F3 + F4), plus null/undefined
/// when `nullable`. Bindings with any other value have no entry.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct FactsRecv {
    pub class_ids: Vec<u32>,
    pub nullable: bool,
}

/// A definite data birth field of a prefix-stable class (F1, F1', F2, F6).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FactsField {
    pub key: String,
    /// The birth slot (F3: the value lives there for the object's life).
    pub slot: u32,
    pub readonly: bool,
    /// F2 kind mask (`perry_wpfacts::KindFact::mask` layout: prims, bit 7
    /// object).
    pub kind_mask: u16,
    /// F6: readonly, holds only typed arrays that are never exposed,
    /// detached, transferred or resized.
    pub ta_stable: bool,
    /// F6 element kind (`perry_hir::TYPED_ARRAY_KIND_*`) when every typed
    /// array the field holds is an owner of that one kind.
    pub ta_kind: Option<u8>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct FactsClass {
    pub display: String,
    pub fixed_layout: bool,
    pub fields: Vec<FactsField>,
}

/// The slice-2 elision inputs of one module.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct ElisionFacts {
    /// Local id → receiver facts.
    pub locals: std::collections::BTreeMap<u32, FactsRecv>,
    /// HIR function id → receiver facts of its `this`.
    pub this_of: std::collections::BTreeMap<u32, FactsRecv>,
    /// Class id → layout facts (prefix-stable classes only).
    pub classes: std::collections::BTreeMap<u32, FactsClass>,
    /// Step 5b: closure parameter (local id) → the classes fninfer infers
    /// for it (a candidate, not a fact: a typed body's entry check proves
    /// it, `codegen/typed_body.rs`).
    pub receiver_params: std::collections::BTreeMap<u32, Vec<String>>,
    /// Contract build: every elided site asserts its facts at run time.
    pub contracts: bool,
}

impl ElisionFacts {
    pub fn is_empty(&self) -> bool {
        self.locals.is_empty() && self.this_of.is_empty()
    }

    pub fn field(&self, class_id: u32, key: &str) -> Option<&FactsField> {
        self.classes
            .get(&class_id)?
            .fields
            .iter()
            .find(|f| f.key == key)
    }
}

/// Emit `bytes` as this module's `.perry_facts` blob, kept alive through
/// `@llvm.used` (no code references it; the runtime finds it by section).
pub(crate) fn emit_contract_section(llmod: &mut LlModule, module_prefix: &str, bytes: &[u8]) {
    if bytes.is_empty() {
        return;
    }
    let section = if llmod.target_triple.contains("apple") {
        "__DATA,__perry_facts"
    } else {
        ".perry_facts"
    };
    let mut lit = String::with_capacity(bytes.len() * 3 + 3);
    lit.push_str("c\"");
    for &b in bytes {
        if (32..127).contains(&b) && b != b'"' && b != b'\\' {
            lit.push(b as char);
        } else {
            lit.push_str(&format!("\\{b:02X}"));
        }
    }
    lit.push('"');
    let name = format!("{module_prefix}__perry_facts");
    llmod.add_raw_global(format!(
        "@{name} = internal constant [{} x i8] {lit}, section \"{section}\", align 8",
        bytes.len()
    ));
    llmod.add_raw_global(format!(
        "@llvm.used = appending global [1 x ptr] [ptr @{name}], section \"llvm.metadata\""
    ));
}
