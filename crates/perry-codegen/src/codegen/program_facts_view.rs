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
    /// Observational diagnostics; excluded from every emitted-code digest.
    pub fallback: std::sync::Arc<crate::fallback_diag::Facts>,
    pub digest: u64,
    /// Known body classifications; absent without the verification mode.
    pub verification_bodies:
        Option<std::sync::Arc<std::collections::BTreeMap<(u32, String), bool>>>,
    pub verification_module: Option<String>,
    pub verification_sites: Option<std::sync::Arc<Vec<crate::verify_facts::SiteClaim>>>,
    pub dynamic_code_reach: perry_hir::dynamic_code::DynamicCodeReach,
    /// The program's realm reach R, on the entry module only: its `main`
    /// emits the runtime layouts R reaches (DESIGN §3.2 item 9). Other
    /// modules emit no realm data.
    pub realm_reach: Option<std::sync::Arc<perry_hir::realm_reach::RealmReach>>,
    pub contract_section: std::sync::Arc<Vec<u8>>,
    pub elision: std::sync::Arc<ElisionFacts>,
}

/// What a binding (a local, or a function's `this`) can hold: instances of
/// these prefix-stable classes (F3 + F4), plus null/undefined when
/// `nullable`, plus anything else when `checked` (⊤ from interpreted or
/// unmodeled code, other objects, primitives). A `checked` binding's sites
/// compare the receiver's class id against `class_ids` and take their
/// complete generic operation on a miss (decision 150, 5b case 2); the
/// others need no receiver test beyond the nullish one. Bindings with no
/// prefix-stable class have no entry.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct FactsRecv {
    pub class_ids: Vec<u32>,
    pub nullable: bool,
    pub checked: bool,
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
    /// A declared key (`perry_hir::ClassFieldOrigin::Declared`): the slot is
    /// known, presence is not. A read pairs the load with the hole compare;
    /// stores stay on the checked route (an add orders the key).
    pub declared: bool,
    /// A constructor-store reservation code can observe before the
    /// constructor stores it: the slot is the class's own (codegen's layout),
    /// presence is not. A read pairs the load with the hole compare; stores
    /// stay on the checked route (filling the hole adds the key).
    pub reserved: bool,
    /// The field holds only these bodies of this module (FVA): a call
    /// through it is a call of them. Empty when it may hold anything else.
    pub fns: Vec<FactsBody>,
    /// Holder facts: every value any write of any unit stores into the field
    /// is an instance of one of these prefix-stable classes, or (when
    /// `nullable`) null/undefined. A read of the field is then a receiver the
    /// facts prove (`wpfacts_access::receiver`): the field's proof travels
    /// with its holder. Empty when the field can hold anything else (⊤, a
    /// function, a builtin, another primitive, an instance of a class that
    /// is not prefix-stable), when its presence is not known (`declared`),
    /// or when interpreted code can run.
    pub classes: Vec<u32>,
    pub nullable: bool,
}

/// A plain compiled body of this module that a binding or a field holds
/// (FVA): a closure expression (`perry_closure_<prefix>__<id>`) or a
/// function declaration, with no rest parameter and no `arguments`.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FactsBody {
    pub func_id: u32,
    /// Declared parameters: a call passing fewer pads with `undefined`.
    pub params: u32,
    pub closure: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct FactsClass {
    pub display: String,
    pub fixed_layout: bool,
    pub fields: Vec<FactsField>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct FactsIndexedOwner {
    pub kind: u8,
    pub min_len: u32,
    pub exact_len: Option<u32>,
    pub allocations: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FactsNumericExpr {
    Integer(i64),
    Number(u64),
    Param(u32),
    Binary(perry_hir::BinaryOp, Box<Self>, Box<Self>),
    Unary(perry_hir::UnaryOp, Box<Self>),
    Sqrt(Box<Self>),
}
impl std::hash::Hash for FactsNumericExpr {
    fn hash<H: std::hash::Hasher>(&self, h: &mut H) {
        std::mem::discriminant(self).hash(h);
        match self {
            Self::Integer(n) => n.hash(h),
            Self::Number(n) => n.hash(h),
            Self::Param(n) => n.hash(h),
            Self::Binary(op, l, r) => {
                std::mem::discriminant(op).hash(h);
                l.hash(h);
                r.hash(h);
            }
            Self::Unary(op, v) => {
                std::mem::discriminant(op).hash(h);
                v.hash(h);
            }
            Self::Sqrt(v) => v.hash(h),
        }
    }
}
impl FactsNumericExpr {
    pub fn from_hir(e: &perry_hir::Expr) -> Option<Self> {
        use perry_hir::Expr;
        Some(match e {
            Expr::Integer(n) => Self::Integer(*n),
            Expr::Number(n) => Self::Number(n.to_bits()),
            Expr::LocalGet(id) => Self::Param(*id),
            Expr::Binary { op, left, right } => Self::Binary(
                *op,
                Box::new(Self::from_hir(left)?),
                Box::new(Self::from_hir(right)?),
            ),
            Expr::Unary { op, operand } => Self::Unary(*op, Box::new(Self::from_hir(operand)?)),
            Expr::MathSqrt(e) => Self::Sqrt(Box::new(Self::from_hir(e)?)),
            _ => return None,
        })
    }
    pub(crate) fn to_hir(&self) -> perry_hir::Expr {
        use perry_hir::Expr;
        match self {
            Self::Integer(n) => Expr::Integer(*n),
            Self::Number(n) => Expr::Number(f64::from_bits(*n)),
            Self::Param(id) => Expr::LocalGet(*id),
            Self::Binary(op, l, r) => Expr::Binary {
                op: *op,
                left: Box::new(l.to_hir()),
                right: Box::new(r.to_hir()),
            },
            Self::Unary(op, v) => Expr::Unary {
                op: *op,
                operand: Box::new(v.to_hir()),
            },
            Self::Sqrt(v) => Expr::MathSqrt(Box::new(v.to_hir())),
        }
    }
}
/// A value's one representation (wpfacts `Rep`). It has no order (the
/// derived one is not the lattice); every encoding decision goes through
/// `FactsRep::encoding`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FactsRep {
    Boxed,
    F64,
    I32,
    /// Only plain program Arrays (wpfacts `Rep::Arr`, DESIGN-p4l1 §2.1):
    /// the slot holds the NaN-boxed pointer word, as a Boxed one does.
    Arr(FactsArrRep),
}

/// wpfacts `ArrRep`: what every Array the value may hold has for its whole
/// life (`length >= floor` and `capacity >= floor`; `dense`: no hole below
/// `floor`; `fixed`: never grows, so never forwarded), and the
/// representation of its elements.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FactsArrRep {
    pub elem: FactsElemRep,
    pub floor: u32,
    pub dense: bool,
    pub fixed: bool,
}

/// wpfacts `ElemRep`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FactsElemRep {
    Boxed,
    F64,
    I32,
}

/// wpfacts `Enc`: how a slot's word is stored and passed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FactsEnc {
    Boxed,
    F64,
    I32,
}

impl FactsRep {
    /// The slot encoding: an `Arr` slot holds the Boxed word.
    pub fn encoding(self) -> FactsEnc {
        match self {
            FactsRep::Boxed | FactsRep::Arr(_) => FactsEnc::Boxed,
            FactsRep::F64 => FactsEnc::F64,
            FactsRep::I32 => FactsEnc::I32,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FactsNumericFunction {
    pub params: Vec<u32>,
    pub expression: FactsNumericExpr,
}

/// The slice-2 elision inputs of one module.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct ElisionFacts {
    pub dynamic_code_reach: perry_hir::dynamic_code::DynamicCodeReach,
    /// Local id → receiver facts.
    pub locals: std::collections::BTreeMap<u32, FactsRecv>,
    /// HIR function id → receiver facts of its `this`.
    pub this_of: std::collections::BTreeMap<u32, FactsRecv>,
    /// Local id → the bodies it holds, when it holds nothing else (FVA): a
    /// call through it is a call of them.
    pub local_bodies: std::collections::BTreeMap<u32, Vec<FactsBody>>,
    /// Local id → the class id of the one class object it holds, when it
    /// holds nothing else (FVA 3.4): `x.k(...)` is `C.k(...)`.
    pub local_classes: std::collections::BTreeMap<u32, u32>,
    pub indexed_owners: std::collections::BTreeMap<u32, FactsIndexedOwner>,
    pub number_inputs: std::collections::BTreeSet<u32>,
    /// Number bindings whose every value is an int32, never `-0` (the
    /// wpfacts interval proof, `BindingFacts::int_range`).
    pub int32_inputs: std::collections::BTreeSet<u32>,
    /// Local id → the representation the whole-program facts give it
    /// (representation RFC §10.3, wpfacts `BindingFacts::rep`). A local the
    /// facts cover takes exactly this one; codegen does not choose.
    pub local_reps: std::collections::BTreeMap<u32, FactsRep>,
    /// The `int32_inputs` whose interval is non-negative.
    pub nonneg_int32_inputs: std::collections::BTreeSet<u32>,
    /// Local id → it holds only strings and ordinary Arrays: `x.length` is a
    /// Number.
    pub length_number_inputs: std::collections::BTreeSet<u32>,
    /// Local id → the one constant `length` of every value it holds: each
    /// is a program Array literal whose birth length never moves (wpfacts
    /// `BindingFacts::const_length`).
    pub const_length_inputs: std::collections::BTreeMap<u32, i64>,
    /// Method call byte offset → each argument's integer interval at the
    /// call (wpfacts `ModuleBindings::call_arg_ranges`).
    pub call_arg_ranges: std::collections::BTreeMap<u32, Vec<Option<(i64, i64)>>>,
    /// Method call byte offsets whose result is a Number at every copy
    /// (wpfacts `ModuleBindings::number_calls`, `facts_rep::number_call`).
    pub number_calls: std::collections::BTreeSet<u32>,
    /// `IndexGet::element_offset` → the read's receivers are program Arrays
    /// whose storage never moves (wpfacts `ModuleBindings::element_reads`,
    /// `expr::index_get::proven_array`).
    pub element_reads: std::collections::BTreeMap<u32, FactsElementRead>,
    /// Method call byte offset → the call is a field element read of its
    /// receiver (wpfacts `ModuleBindings::field_read_calls`, b1).
    pub field_read_calls: std::collections::BTreeMap<u32, FactsFieldReadCall>,
    /// P2: typed signatures by HIR function id (wpfacts
    /// `ModuleBindings::signatures`, DESIGN-p2 §1).
    pub signatures: std::collections::BTreeMap<u32, FactsSignature>,
    /// P2: call byte offset -> the one same-module body (HIR function id) a
    /// typed entry serves (wpfacts `typed_calls`, DESIGN-p2 §2).
    pub typed_calls: std::collections::BTreeMap<u32, u32>,
    /// Element reads (by `element_offset`) whose value the facts prove a
    /// plain Number; `true` when it may also be `undefined` (wpfacts
    /// `canonical_numbers`, P3 B1).
    pub number_element_reads: std::collections::BTreeMap<u32, bool>,
    /// Check mode: element reads (by `element_offset`) whose past-the-end
    /// `undefined` the facts withheld, with the least length lower bound of
    /// their receivers (wpfacts `ModuleBindings::element_floors`, PR-C1;
    /// `expr::index_get::elem_floor`).
    pub element_floors: std::collections::BTreeMap<u32, u32>,
    pub indexed_metadata_pristine: bool,
    pub numeric_functions: std::collections::BTreeMap<u32, FactsNumericFunction>,
    /// Class id → its own statics that hold their definition for the
    /// program's life (FVA 3.4): a call of one is its declaration, unguarded.
    pub class_statics: std::collections::BTreeMap<u32, Vec<String>>,
    /// Class id → layout facts (prefix-stable classes only).
    pub classes: std::collections::BTreeMap<u32, FactsClass>,
    /// Contract build: every elided site asserts its facts at run time.
    pub contracts: bool,
    /// Interpreted code can run (decision 150, `ProgramFacts::interp_guard`):
    /// every proof also tests the runtime's `PERRY_INTERP_PROTO_WRITTEN`
    /// word (set-only: interpreted code wrote an intrinsic or marked
    /// prototype) and takes the site's complete generic operation once it
    /// is set.
    pub interp_guard: bool,
    /// Compiled code never writes `%Function.prototype%.bind`, `.name` or
    /// `.length` (wpfacts `fnproto_bind_written`): a `bind` site on a
    /// base-shape function needs no `Pristine(Function)` test, only the
    /// `interp_guard` word when that is set (`expr/function_bind.rs`).
    pub fnproto_bind_unwritten: bool,
    /// #13645: compiled code never changes which `@@iterator` an array
    /// finds (wpfacts `array_iteration_written`), and no interpreted code
    /// runs: array iteration needs no iteration-member read
    /// (`expr::array_iteration_facts`).
    pub array_iteration_unwritten: bool,
    /// #13645: `%ArrayIteratorPrototype%` and its chain never gain or change
    /// `next` or `return` (wpfacts `array_iterator_close_written`), and no
    /// interpreted code runs: an array record never checks `next`, and its
    /// IteratorClose finds no `return` (the closing of an array record is
    /// unobservable and emitted as nothing).
    pub array_iterator_close_unwritten: bool,
    /// Whole-program constructor heritage proofs, keyed by class identity:
    /// the class the evaluated `extends` value is (`None`: no class parent).
    pub super_unrelinked: std::collections::BTreeMap<u32, Option<u32>>,
    /// Compiled code cannot replace Function.prototype.call/apply.
    pub fnproto_borrow_unwritten: bool,
    /// The source offsets (the callee read's, as the solver records a call
    /// site) of this module's method calls whose verdict is
    /// `Verdict::NativeMember`: the member they read is proven to be the
    /// intrinsic. A host-object call (`console.log(...)`) binds to the
    /// intrinsic only here; every other one reads the property and calls it.
    pub native_member_calls: std::collections::BTreeSet<u32>,
}

impl ElisionFacts {
    pub fn is_empty(&self) -> bool {
        self.indexed_owners.is_empty()
            && self.number_inputs.is_empty()
            && self.int32_inputs.is_empty()
            && self.local_reps.is_empty()
            && self.length_number_inputs.is_empty()
            && self.const_length_inputs.is_empty()
            && self.call_arg_ranges.is_empty()
            && self.number_calls.is_empty()
            && self.element_reads.is_empty()
            && self.field_read_calls.is_empty()
            && self.signatures.is_empty()
            && self.typed_calls.is_empty()
            && self.number_element_reads.is_empty()
            && self.locals.is_empty()
            && self.this_of.is_empty()
            && self.local_bodies.is_empty()
            && self.local_classes.is_empty()
            && self.class_statics.is_empty()
            && self.native_member_calls.is_empty()
    }

    pub fn field(&self, class_id: u32, key: &str) -> Option<&FactsField> {
        self.classes
            .get(&class_id)?
            .fields
            .iter()
            .find(|f| f.key == key)
    }
}

/// Defined by `perry-runtime`'s `contracts` module (feature `contract-checks`).
pub(crate) const CONTRACT_RUNTIME_SYMBOL: &str =
    "perry_contract_build_needs_runtime_feature_contract_checks";

/// Emit `bytes` as this module's `.perry_facts` blob, kept alive through
/// `@llvm.used` (no code references it; the runtime finds it by section).
///
/// The blob also references `CONTRACT_RUNTIME_SYMBOL`, which only a runtime
/// built with `contract-checks` defines: linking a contract build against a
/// runtime without the checks fails at link time naming the feature, instead
/// of producing a binary that checks nothing.
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
    // A real relocation (not just an `@llvm.used` entry): the reference must
    // reach the linker.
    let ref_section = if llmod.target_triple.contains("apple") {
        "__DATA,__perry_fref"
    } else {
        ".perry_facts_ref"
    };
    llmod.add_raw_global(format!("@{CONTRACT_RUNTIME_SYMBOL} = external global i8"));
    llmod.add_raw_global(format!(
        "@{name}_ref = internal constant ptr @{CONTRACT_RUNTIME_SYMBOL}, section \"{ref_section}\", align 8"
    ));
    llmod.add_raw_global(format!(
        "@llvm.used = appending global [2 x ptr] [ptr @{name}, ptr @{name}_ref], \
         section \"llvm.metadata\""
    ));
}

/// wpfacts `ElementRead`: every receiver of the read is a program Array of
/// exactly `len` elements whose length, storage and element descriptors no
/// operation of the program ever changes.
/// b1: a method call proven to be `this.field[arg] ?? default` on its
/// receiver, which can neither throw nor run user code (wpfacts
/// P2: a function's typed signature (wpfacts `Signature`, DESIGN-p2 §1).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FactsSignature {
    pub params: Vec<FactsRep>,
    /// Each parameter's int interval (T1).
    pub ranges: Vec<Option<(i64, i64)>>,
    pub ret: FactsRep,
    /// The class id of the one class every `this` holds (E2).
    pub this_class: Option<u32>,
}

/// `field_read_calls`).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FactsFieldReadCall {
    pub field: String,
    /// The argument that is the index.
    pub index_arg: u32,
    /// The Number default of `?? L`, as f64 bits.
    pub default: Option<u64>,
    /// The method body's `IndexGet::element_offset` and `this.f` offset.
    pub element_offset: u32,
    pub field_offset: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FactsElementRead {
    pub len: u32,
    /// The key is an integer in `[0, len)`: the read needs no bound.
    pub key_in_bounds: bool,
}
