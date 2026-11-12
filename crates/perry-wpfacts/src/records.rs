//! Value records (DESIGN-p4l4 rev 3.2 §2, VR-1): which object-literal
//! templates are represented as their fields by value, and why each other
//! one stays a heap object.
//!
//! A derived fact over the solved model, written into the template's own
//! facts (`TemplateFacts::record`, `TemplateFacts::record_elems`); nothing
//! here changes the solve. Off unless `PERRY_FACTS_VALUE_RECORDS=1`
//! (`Model::value_records`): every template is then `REJECTED Off`.
//!
//! - R0-R3 are template properties (kind, size, layout, immutability,
//!   immediate fields).
//! - R4-R5 classify every constraint on every node that holds one of the
//!   template's tokens, by an exhaustive `match` over `Cons` with no `_` arm
//!   (`record_use`, B2): a new `Cons` variant does not compile until it is
//!   classified.
//! - R6 constrains the Arr templates whose elements are the record
//!   (`record_elems`). Their operations must be the record-array intrinsics
//!   codegen implements (`RECORD_ARRAY_INTRINSICS`), empty until VR-4b, so
//!   until then every record held by an array is rejected (`R6Op`).
//! - Admission is a greatest fixpoint: start with every candidate admitted
//!   and remove violators until nothing changes (§2.3). The step only
//!   removes, so it terminates.
//!
//! Known limits (the ledger states them): natives are default-deny, only the
//! realm declarations with `effects` describe their slots, so a record
//! argument of any other native rejects; `X | null` is rejected (`prims` of a
//! record holder are within `undefined`); two literal sites of one interface
//! are two templates and their join rejects.
use crate::derive::{SiteFacts, TemplateFacts};
use crate::gen::Gen;
use crate::model::*;
use crate::pts::{FxHashMap, FxHashSet};
use std::fmt;

/// The largest record in JSValue words (§8): a fact about the type, in the
/// facts digest.
pub const RECORD_MAX_WORDS: u16 = 16;

/// The record-array operations codegen lowers as intrinsics (§5.3), by the
/// name `array_use` gives a constraint (`#elem` is the element `pop` reads,
/// `iterate` the for-of). VR-4b lowers these; any other operation on a
/// record array is `R6Op`.
pub const RECORD_ARRAY_INTRINSICS: &[&str] =
    &["push", "pop", "#elem", "length", "[index]", "iterate"];

/// VR-4b: where an array literal is born, when it is the whole value of its
/// holder (`Model::arr_birth_holder`). Codegen births a record array at
/// exactly these, for an empty literal: `let x = []` / `x = []` (the local),
/// `o.k = []` (the store's offset), `k = []` (a class field initializer).
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ArrBirth {
    Local(u32, u32),
    Store(u32, u32),
    Field(u32, u32, String),
}

/// The nodes the boxed program's for-of makes beside its bindings
/// (`Gen::runtime_glue`, the iterator's close), by label.
const FOR_OF_GLUE: &[&str] = &["glue-pass", "close-iterator", "for-value", "iterated"];

/// A record lane's representation in SSA (§1.2); in memory every lane is one
/// valid JSValue word.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FieldRep {
    F64,
    I32,
    Imm,
}

impl FieldRep {
    pub fn name(self) -> &'static str {
        match self {
            FieldRep::F64 => "f64",
            FieldRep::I32 => "i32",
            FieldRep::Imm => "imm",
        }
    }
}

/// An admitted template's layout: `birth_keys` order, one word per field.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct RecordLayout {
    pub fields: Vec<(String, FieldRep)>,
    pub words: u16,
}

/// Why a template is a heap object (§7): a closed list.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum RecordReject {
    /// `PERRY_FACTS_VALUE_RECORDS` is not `1`.
    Off,
    R0Kind(String),
    R0Size(u16),
    R0Hard(String),
    R0Foreign,
    R0Interp(String),
    R1Layout(String),
    R1Accessor(String),
    R2Write(String),
    R3Pointer(String),
    R4Identity(String),
    R5Load(String),
    R5Join(String),
    R5Escape(String),
    R5StoreHeap(String),
    R5Throw,
    R5Proto(String),
    R5Contract(String),
    R5Iterate(String),
    R5Protocol(String),
    R5ThroughContainer(String),
    R5Native {
        site: String,
        slot: String,
        why: String,
    },
    R5UntypedCall(String),
    /// Owner ruling 2026-11-10 (§4.4): a helper whose parameter joins the
    /// record with other values receives a materialized copy; it may only
    /// read it. `why`: `retain`, `mutate`, `identity` or `call`.
    R5RetainingHelper {
        f: String,
        why: String,
    },
    R6ArrayHolder(String),
    R6Join(String),
    R6Store(String),
    R6Op(String),
    /// Rejected because a linked template was.
    Cascade(TmplId),
}

impl fmt::Display for RecordReject {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use RecordReject::*;
        match self {
            Off => write!(f, "Off"),
            R0Kind(s) => write!(f, "R0Kind({s})"),
            R0Size(n) => write!(f, "R0Size({n})"),
            R0Hard(s) => write!(f, "R0Hard({s})"),
            R0Foreign => write!(f, "R0Foreign"),
            R0Interp(s) => write!(f, "R0Interp({s})"),
            R1Layout(s) => write!(f, "R1Layout({s})"),
            R1Accessor(s) => write!(f, "R1Accessor({s})"),
            R2Write(s) => write!(f, "R2Write({s})"),
            R3Pointer(s) => write!(f, "R3Pointer({s})"),
            R4Identity(s) => write!(f, "R4Identity({s})"),
            R5Load(s) => write!(f, "R5Load({s})"),
            R5Join(s) => write!(f, "R5Join({s})"),
            R5Escape(s) => write!(f, "R5Escape({s})"),
            R5StoreHeap(s) => write!(f, "R5StoreHeap({s})"),
            R5Throw => write!(f, "R5Throw"),
            R5Proto(s) => write!(f, "R5Proto({s})"),
            R5Contract(s) => write!(f, "R5Contract({s})"),
            R5Iterate(s) => write!(f, "R5Iterate({s})"),
            R5Protocol(s) => write!(f, "R5Protocol({s})"),
            R5ThroughContainer(s) => write!(f, "R5ThroughContainer({s})"),
            R5Native { site, slot, why } => write!(f, "R5Native({site}, {slot}, {why})"),
            R5UntypedCall(s) => write!(f, "R5UntypedCall({s})"),
            R5RetainingHelper { f: func, why } => write!(f, "R5RetainingHelper({func}, {why})"),
            R6ArrayHolder(s) => write!(f, "R6ArrayHolder({s})"),
            R6Join(s) => write!(f, "R6Join({s})"),
            R6Store(s) => write!(f, "R6Store({s})"),
            R6Op(s) => write!(f, "R6Op({s})"),
            Cascade(t) => write!(f, "Cascade({t})"),
        }
    }
}

/// A rejection and where it was found (a node label, a call site), empty
/// for a template property.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Rejected {
    pub why: RecordReject,
    pub at: String,
}

impl Rejected {
    pub fn off() -> Self {
        Rejected {
            why: RecordReject::Off,
            at: String::new(),
        }
    }
    fn new(why: RecordReject, at: impl Into<String>) -> Self {
        Rejected { why, at: at.into() }
    }
}

/// An Arr template linked to a record template (R6): dump and ledger only.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct RecordArray {
    pub tmpl: TmplId,
    pub display: String,
    pub elems_of: TmplId,
    pub verdict: Result<(), Rejected>,
}

/// The protocols a sanctioned boundary may run on a record (§4.1): no
/// identity, no retention, keys and order fixed by the template.
fn sanctioned(p: Protocol) -> bool {
    matches!(
        p,
        Protocol::ToPrimitive
            | Protocol::Inspect
            | Protocol::ShallowInspect
            | Protocol::Json
            | Protocol::OwnKeys
    )
}

/// What one constraint on a node holding a record token allows (§2.1).
enum Use {
    Allowed,
    /// Allowed when call `CallId` is a typed program call (`R5UntypedCall`).
    TypedCall(CallId),
    Reject(RecordReject),
    /// A record-array intrinsic (`RECORD_ARRAY_INTRINSICS`).
    Op,
}

/// R4/R5: the exhaustive classification of a constraint on a node that
/// holds a token of a candidate record template (B2: no `_` arm).
fn record_use(c: &Cons, m: &Model, keys: &[Atom]) -> Use {
    use RecordReject::*;
    use Use::{Allowed, Reject, TypedCall};
    let key = |k: &Key| match k {
        Key::Named(a) => m.atoms[*a as usize].clone(),
        Key::Any(_) => "[computed]".to_string(),
    };
    let layout = |k: &Key| matches!(k, Key::Named(a) if keys.contains(a));
    match c {
        Cons::Identity { route } => match route {
            IdentityRoute::JsonReplacer => Reject(R5Protocol("JsonReplacer".into())),
            r => Reject(R4Identity(r.name().into())),
        },
        // Value flow and field reads.
        Cons::Load { key: k, .. } | Cons::FieldTruthy { key: k, .. } if layout(k) => Allowed,
        // A protocol's well-known-symbol lookup (`@@toStringTag` of
        // `Object.prototype.toString`, `@@toPrimitive`): a miss on a literal
        // (R1 admits only never-written `undefined` keys beside the layout).
        Cons::Load {
            key: Key::Named(a), ..
        } if m.atoms[*a as usize].starts_with("@@__perry_wk_") => Allowed,
        Cons::Load { key: k, .. } | Cons::FieldTruthy { key: k, .. } => Reject(R5Load(key(k))),
        // A resolution reads `then` (a protocol, 27.2.1.3.2 step 9).
        Cons::Fulfil { .. } => Reject(R5Load("then".into())),
        Cons::BirthField { key: k, .. } if layout(k) => Allowed,
        Cons::BirthField { key: k, .. } => Reject(R2Write(key(k))),
        Cons::ArgInto { call, .. } | Cons::RetInto { call, .. } => TypedCall(*call),
        Cons::CallArgs { .. } | Cons::Call { .. } | Cons::CallThis { .. } => {
            Reject(R5Load("()".into()))
        }
        Cons::MCall { key: k, .. } => Reject(R5Load(format!("{}()", key(k)))),
        Cons::IdxLoad { .. } | Cons::IdxOwnLoad { .. } | Cons::OwnGate { .. } => {
            Reject(R5Load("[computed]".into()))
        }
        Cons::IdxMCall { .. } => Reject(R5Load("[computed]()".into())),
        Cons::OwnTest { .. } => Reject(R5Load("own test".into())),
        // Presence: no protocol, no identity.
        Cons::Falsy { .. }
        | Cons::NotNullish { .. }
        | Cons::NotUndefined { .. }
        | Cons::Guard { .. }
        | Cons::Range { .. }
        | Cons::Defined { .. }
        | Cons::TopIfTop { .. }
        | Cons::Raise { .. } => Allowed,
        // Arithmetic and coercion: ToPrimitive on the record.
        Cons::Implicit { proto, .. } if sanctioned(*proto) => Allowed,
        Cons::Implicit { proto, .. } => Reject(R5Protocol(proto.name().into())),
        Cons::Arith { .. } | Cons::Plus { .. } => Reject(R5Protocol("ToPrimitive".into())),
        // Stores into the record or its layout.
        Cons::Store { key: k, .. } => Reject(R2Write(key(k))),
        Cons::DeleteKey { key: a, .. } | Cons::DefineKey { key: a, .. } => {
            Reject(R2Write(m.atoms[*a as usize].clone()))
        }
        Cons::DefineAll { .. }
        | Cons::DefineDesc { .. }
        | Cons::AddsKeys
        | Cons::AnnexBDefine { .. }
        | Cons::UnnamedSet { .. } => Reject(R2Write(c.kind_name().into())),
        Cons::RuntimeStore { .. } => Reject(R5StoreHeap("realm".into())),
        Cons::IndexHole | Cons::IndexKept | Cons::LenIndex { .. } | Cons::LenPermuted { .. } => {
            Reject(R2Write("[index]".into()))
        }
        Cons::ArrayLenArg { .. } => Reject(R5StoreHeap("Array(x)".into())),
        // A record used as a property key: ToPropertyKey.
        Cons::IdxKey { .. }
        | Cons::IdxOwnKey { .. }
        | Cons::IdxKeyCall { .. }
        | Cons::ElemUndef { .. }
        | Cons::StoreIndexed { .. }
        | Cons::StoreKeyed { .. }
        | Cons::DefineKeyed { .. }
        | Cons::AccessorKeyed { .. } => Reject(R5Protocol("ToPropertyKey".into())),
        // Escape.
        Cons::TopSink { .. }
        | Cons::TopIfAny { .. }
        | Cons::RequireSpec { .. }
        | Cons::Hard { .. }
        | Cons::Miss { .. }
        | Cons::BoundCall { .. }
        | Cons::ShareBuffer { .. }
        | Cons::ViewedBuffer { .. } => Reject(R5Escape(c.kind_name().into())),
        // Prototype and construction.
        Cons::ProtoKey { .. }
        | Cons::Chain { .. }
        | Cons::EnumChain { .. }
        | Cons::ProtoFrom { .. }
        | Cons::ProtoGiven { .. }
        | Cons::ProtoOf { .. }
        | Cons::ProtoMethods { .. }
        | Cons::ProtoValues { .. }
        | Cons::ProtoClosure { .. }
        | Cons::CreateProto { .. }
        | Cons::ParentGiven { .. }
        | Cons::ChainUnion { .. }
        | Cons::Super { .. }
        | Cons::SuperFrom { .. }
        | Cons::SuperCtor { .. }
        | Cons::SuperCtorParent { .. }
        | Cons::NativeParent { .. }
        | Cons::PlainKey { .. }
        | Cons::StoreTopProto { .. }
        | Cons::HolderStep { .. }
        | Cons::VisitHolders { .. }
        | Cons::VisitStop { .. }
        | Cons::HasInstance { .. }
        | Cons::New { .. }
        | Cons::NewNamed { .. }
        | Cons::NewProgram { .. }
        | Cons::NewTargetHard { .. }
        | Cons::ReflectNew { .. }
        | Cons::ReflectNewTarget { .. }
        | Cons::ThisFrom { .. }
        | Cons::ThisTo { .. }
        | Cons::ThisInst { .. }
        | Cons::OverrideThis { .. }
        | Cons::BuiltinCallee => Reject(R5Proto(c.kind_name().into())),
        Cons::NativeThis { .. } => Reject(R5Native {
            site: "native".into(),
            slot: "this".into(),
            why: "a record as a native's receiver".into(),
        }),
        // Contracts and units.
        Cons::HandOver { .. }
        | Cons::CallerValue { .. }
        | Cons::OwnOnly { .. }
        | Cons::ContractThis { .. }
        | Cons::StoreOut { .. }
        | Cons::CjsPublish { .. } => Reject(R5Contract(c.kind_name().into())),
        // Iteration, spread, enumeration.
        Cons::Spread { .. }
        | Cons::SpreadFrom { .. }
        | Cons::IterNext { .. }
        | Cons::FromArrayLike { .. }
        | Cons::ForInBody { .. }
        | Cons::ForInKeys { .. }
        | Cons::EnumKeyStore { .. }
        | Cons::EnumKeyAny { .. }
        | Cons::OwnValues { .. } => Reject(R5Iterate(c.kind_name().into())),
        // Accessors.
        Cons::Accessor { .. }
        | Cons::AccessorInstall { .. }
        | Cons::AnyAccessors { .. }
        | Cons::AnnexBLookup { .. } => Reject(R1Accessor(c.kind_name().into())),
    }
}

/// What a constraint on a node holding a record array allows (R6(d)): the
/// intrinsics, presence and value flow; anything else is `R6Op`.
fn array_use(c: &Cons, m: &Model) -> Use {
    use RecordReject::*;
    let named = |k: &Key| match k {
        Key::Named(a) => m.atoms[*a as usize].clone(),
        Key::Any(_) => "[computed]".to_string(),
    };
    let op = |name: String| {
        if RECORD_ARRAY_INTRINSICS.contains(&name.as_str()) {
            Use::Op
        } else {
            Use::Reject(R6Op(name))
        }
    };
    match c {
        Cons::Falsy { .. }
        | Cons::NotNullish { .. }
        | Cons::NotUndefined { .. }
        | Cons::Guard { .. }
        | Cons::Range { .. }
        | Cons::Defined { .. }
        | Cons::TopIfTop { .. }
        | Cons::Raise { .. } => Use::Allowed,
        Cons::ArgInto { call, .. } | Cons::RetInto { call, .. } => Use::TypedCall(*call),
        Cons::MCall { key, .. } | Cons::Load { key, .. } | Cons::FieldTruthy { key, .. } => {
            op(named(key))
        }
        Cons::Fulfil { .. } => op("then".into()),
        Cons::IdxLoad { .. } | Cons::IdxOwnLoad { .. } => op("[index]".into()),
        Cons::IndexHole | Cons::IndexKept => Use::Reject(R6Store("hole".into())),
        // `push`'s own store of its argument (`Gen::elem_store`).
        Cons::Store { site, .. }
            if m.stores
                .get(*site as usize)
                .is_some_and(|s| s.desc == "push") =>
        {
            op("push".into())
        }
        Cons::Store { key, .. } => Use::Reject(R6Store(named(key))),
        // `pop`'s move of the length; a permutation's is rejected at its
        // receiver (`Model::arr_permuted`).
        Cons::LenIndex { .. } => op("pop".into()),
        Cons::LenPermuted { .. } => Use::Reject(R6Store("[index]".into())),
        Cons::TopSink { .. }
        | Cons::TopIfAny { .. }
        | Cons::RequireSpec { .. }
        | Cons::Hard { .. } => Use::Reject(R6ArrayHolder(c.kind_name().into())),
        _ => op(c.kind_name().into()),
    }
}

fn node_at(m: &Model, n: NodeId) -> String {
    format!("{}#{n}", m.labels[n as usize])
}

/// Template-level rules R0-R3 and the layout (§1.2): `None` for a template
/// that is no object literal (it gets no ledger line).
fn static_rules(
    g: &Gen,
    tf: &TemplateFacts,
    interp_guard: Option<&str>,
    ignore_r2: bool,
) -> Option<Result<RecordLayout, Rejected>> {
    use RecordReject::*;
    let m = &g.m;
    let tpl = &m.tmpls[tf.id as usize];
    match tpl.kind {
        TmplKind::Lit {
            enum_image: false, ..
        } => {}
        TmplKind::Class {
            literal_shape: true,
            ..
        } => {}
        _ => return None,
    }
    let r = |why| Some(Err(Rejected::new(why, "")));
    if let Some(i) = interp_guard {
        return r(R0Interp(i.to_string()));
    }
    if m.foreign.contains_key(&tf.id) {
        return r(R0Foreign);
    }
    if tf.birth_keys.iter().any(|k| k == "__proto__") || tpl.null_proto {
        return r(R0Kind("__proto__".into()));
    }
    let words = tf.birth_keys.len();
    if words > RECORD_MAX_WORDS as usize {
        return r(R0Size(words.min(u16::MAX as usize) as u16));
    }
    if let Some(h) = &tf.hard {
        return r(R0Hard(h.clone()));
    }
    if let Some(e) = &tf.escape {
        return r(R5Escape(e.clone()));
    }
    if !tf.fixed_layout || !tf.prefix_stable || tf.adds_keys != 0 {
        return r(R1Layout(format!(
            "fixed_layout={} prefix_stable={} adds_keys={}",
            tf.fixed_layout as u8, tf.prefix_stable as u8, tf.adds_keys
        )));
    }
    if let Some(c) = &tf.chain {
        return r(R1Layout(c.clone()));
    }
    if !tpl.accessor_keys.is_empty() || tpl.has_computed_members {
        return r(R1Accessor("layout".into()));
    }
    let toks = m.tmpl_toks(tf.id);
    let mut fields = Vec::with_capacity(words);
    for (k, &atom) in tf.birth_keys.iter().zip(tpl.birth_keys.iter()) {
        let Some(f) = tf.fields.iter().find(|f| !f.private && &f.key == k) else {
            return r(R1Layout(format!("no field {k}")));
        };
        if !f.definite || !f.data_field {
            return r(R1Layout(format!("{k} is not a definite data field")));
        }
        if !f.readonly && !ignore_r2 {
            return r(R2Write(k.clone()));
        }
        let allowed = prim::NUM | prim::BOOL | prim::UNDEF | prim::NULL;
        let kf = &f.kind;
        if kf.top
            || !kf.objs.is_empty()
            || kf.funcs > 0
            || kf.builtin_objs > 0
            || kf.prims & !allowed != 0
        {
            return r(R3Pointer(format!("{k}:{}", kf.desc())));
        }
        // §1.2 (finding 11): the lane's rep is the field node's, over every
        // token of the template.
        let rep = toks
            .iter()
            .filter_map(|&t| m.fields.get(&(t, atom)).copied())
            .map(|n| crate::bindings::rep_of_node(g, n))
            .reduce(crate::bindings::Rep::join);
        let rep = match rep {
            Some(crate::bindings::Rep::F64) => FieldRep::F64,
            Some(crate::bindings::Rep::I32) => FieldRep::I32,
            _ => FieldRep::Imm,
        };
        fields.push((k.clone(), rep));
    }
    // A key outside the layout is only a read that misses (a protocol's
    // `toString`, `toJSON`, ...): never written, it holds `undefined`.
    if let Some(f) = tf.fields.iter().find(|f| {
        (!tf.birth_keys.contains(&f.key) || f.private)
            && (f.writers_ctor + f.writers_post > 0
                || f.definite
                || f.kind.top
                || f.kind.prims & !prim::UNDEF != 0
                || !f.kind.objs.is_empty()
                || f.kind.funcs > 0
                || f.kind.builtin_objs > 0)
    }) {
        return r(R1Layout(format!(
            "a field outside the birth keys: {}",
            f.key
        )));
    }
    Some(Ok(RecordLayout {
        fields,
        words: words as u16,
    }))
}

/// Derive every template's record verdict (§2) into `templates`; returns
/// the linked Arr templates for the ledger.
pub(crate) fn derive(
    g: &Gen,
    templates: &mut [TemplateFacts],
    sites: &[SiteFacts],
    interp_guard: Option<&str>,
    sabotage: Option<&str>,
) -> Vec<RecordArray> {
    let m = &g.m;
    if !m.value_records || m.aborted {
        return Vec::new();
    }
    // R0-R3.
    // S0 negative control `vrwrite` (§10.3): R2 ignored.
    let ignore_r2 = sabotage == Some("vrwrite");
    let mut verdict: FxHashMap<TmplId, Result<RecordLayout, Rejected>> = FxHashMap::default();
    for tf in templates.iter() {
        if let Some(v) = static_rules(g, tf, interp_guard, ignore_r2) {
            verdict.insert(tf.id, v);
        }
    }
    let mut s = Scan::new(g, sites);
    s.ignore_r2 = ignore_r2;
    // S0 negative control `vruntyped` (§10.3): R5UntypedCall ignored.
    s.ignore_untyped = sabotage == Some("vruntyped");
    s.tmpl_module = templates.iter().map(|t| (t.id, t.module)).collect();
    // S0 negative controls (DESIGN-p4l4 §10.3): an identity route ignored.
    s.ignored_route = match sabotage {
        Some("vrident" | "vrinline") => Some("==="),
        Some("vrswitch") => Some("switch"),
        _ => None,
    };
    // Arr template -> the record template it holds (R6(a)), and the
    // rejected ones.
    let mut linked: FxHashMap<TmplId, TmplId> = FxHashMap::default();
    let mut arr_rejected: FxHashMap<TmplId, Rejected> = FxHashMap::default();
    // The greatest fixpoint (§2.3).
    loop {
        let admitted: FxHashSet<TmplId> = verdict
            .iter()
            .filter(|(_, v)| v.is_ok())
            .map(|(t, _)| *t)
            .collect();
        let mut rejects: Vec<(TmplId, Rejected)> = Vec::new();
        let mut arr_rejects: Vec<(TmplId, Rejected)> = Vec::new();
        let wrappers = s.boundary_wrappers(&admitted);
        s.link_arrays(&admitted, &wrappers, &mut linked, &mut arr_rejects);
        let live_arrs: FxHashMap<TmplId, TmplId> = linked
            .iter()
            .filter(|(a, tm)| admitted.contains(tm) && !arr_rejected.contains_key(a))
            .map(|(a, tm)| (*a, *tm))
            .collect();
        s.scan(
            &admitted,
            &live_arrs,
            &linked,
            &wrappers,
            &mut rejects,
            &mut arr_rejects,
        );
        let mut changed = false;
        for (a, why) in arr_rejects {
            if !arr_rejected.contains_key(&a) {
                arr_rejected.insert(a, why.clone());
                changed = true;
                // R6 is a rule of the record template: its array's violation
                // rejects it.
                if let Some(tm) = linked.get(&a) {
                    rejects.push((*tm, why));
                }
            }
        }
        // The owner ruling's reject names the helper (§4.4): it is the cause
        // of the other rejects its copy region leads to, so it comes first.
        rejects.sort_by_key(|(_, r)| !matches!(r.why, RecordReject::R5RetainingHelper { .. }));
        for (tm, why) in rejects {
            if let Some(v) = verdict.get_mut(&tm) {
                if v.is_ok() {
                    *v = Err(why);
                    changed = true;
                }
            }
        }
        // The converse: a rejected record rejects its arrays.
        for (a, tm) in linked.iter() {
            if !arr_rejected.contains_key(a) && verdict.get(tm).is_some_and(|v| v.is_err()) {
                arr_rejected.insert(*a, Rejected::new(RecordReject::Cascade(*tm), ""));
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    let mut arrays: Vec<RecordArray> = linked
        .iter()
        .map(|(&a, &tm)| RecordArray {
            tmpl: a,
            display: m.tmpls[a as usize].display.clone(),
            elems_of: tm,
            verdict: match arr_rejected.get(&a) {
                Some(r) => Err(r.clone()),
                None => Ok(()),
            },
        })
        .collect();
    arrays.sort_by_key(|a| a.tmpl);
    for tf in templates.iter_mut() {
        let Some(v) = verdict.remove(&tf.id) else {
            tf.record = Err(Rejected::new(RecordReject::R0Kind(tf.kind.into()), ""));
            continue;
        };
        tf.record_elems = arrays
            .iter()
            .filter(|a| a.elems_of == tf.id && a.verdict.is_ok() && v.is_ok())
            .map(|a| a.tmpl)
            .collect();
        tf.record = v;
    }
    arrays
}

/// The model indexes one scan needs: built once per derive.
struct Scan<'a, 'g> {
    g: &'a Gen<'g>,
    /// Field node -> its owner token.
    field_owner: FxHashMap<NodeId, Tok>,
    /// Argument node -> (call, position); `u32::MAX`: a spread argument.
    arg_of: FxHashMap<NodeId, Vec<(CallId, u32)>>,
    /// Result node -> call.
    ret_of: FxHashMap<NodeId, Vec<CallId>>,
    /// Call -> its source site record.
    rec_of: FxHashMap<CallId, &'a crate::signatures::CallSiteRec>,
    /// Typed program call sites: (module, offset) -> the one body.
    targets: FxHashMap<(u32, u32), FnIdx>,
    /// Method call sites whose receivers hold program bodies (or ⊤).
    program_methods: FxHashSet<(u32, u32)>,
    /// Throw region nodes.
    regions: FxHashSet<NodeId>,
    /// Summary instance copies of a node.
    copies: FxHashMap<NodeId, Vec<NodeId>>,
    /// A copy's shared node.
    orig_of: FxHashMap<NodeId, NodeId>,
    /// Locals a closure references (`Model::captures` by a function some
    /// body creates, `FuncInfo::parent`): a closure environment cell, a heap
    /// slot (closure capture by value is VR-7).
    captured: FxHashSet<NodeId>,
    /// The nodes holding a token of each object-literal or Arr template.
    holders: FxHashMap<TmplId, Vec<NodeId>>,
    /// `this` of a literal-shape class's constructor (and its instance
    /// copies): its stores of the layout keys are the literal's birth.
    ctor_this: FxHashMap<NodeId, TmplId>,
    /// The element key's atom.
    elem_atom: Option<Atom>,
    /// The identity route a negative control ignores (`vrident`,
    /// `vrinline`: `===`; `vrswitch`: `switch`).
    ignored_route: Option<&'static str>,
    /// The negative control `vrwrite` ignores R2.
    ignore_r2: bool,
    /// The negative control `vruntyped` ignores R5UntypedCall.
    ignore_untyped: bool,
    /// Each template's module (`TemplateFacts::module`).
    tmpl_module: FxHashMap<TmplId, Option<u32>>,
    /// A method call's receiver node.
    mcall_recv: FxHashMap<CallId, NodeId>,
    /// VR-4b: the nodes whose value codegen types as a record array at a
    /// use: a local's binding and a named read with a source offset (and
    /// their instance copies). An operation on any other holder is one
    /// codegen would lower as an ordinary Array's.
    typed: FxHashSet<NodeId>,
    /// VR-4b: the for-of bindings (`Model::for_of_relays`, with copies).
    relays: FxHashSet<NodeId>,
    /// VR-4b: an object's aggregate nodes (its fields under any key,
    /// `Model::all_fields` and `Model::field_any` of a token that is no
    /// Array): no value of the program, what reads them is.
    aggregates: FxHashSet<NodeId>,
    /// VR-4b: the Arr templates a `GetIterator` stands for (`[iterator#n]`):
    /// the boxed program's iterator over a for-of source, which the for-of
    /// intrinsic never makes for a record array.
    relay_tmpls: FxHashSet<TmplId>,
    /// The `new` calls: their result is the object they birth.
    new_calls: FxHashSet<CallId>,
    /// The `new` calls of literal-shape classes (an object literal's birth):
    /// an argument is a field value of the literal.
    literal_births: FxHashSet<CallId>,
}

impl<'a, 'g> Scan<'a, 'g> {
    fn new(g: &'a Gen<'g>, sites: &[SiteFacts]) -> Self {
        let m = &g.m;
        let mut field_owner = FxHashMap::default();
        for (&(t, _), &n) in m.fields.iter() {
            field_owner.insert(n, t);
        }
        for (&t, &n) in m.field_any.iter().chain(m.all_fields.iter()) {
            field_owner.insert(n, t);
        }
        let mut arg_of: FxHashMap<NodeId, Vec<(CallId, u32)>> = FxHashMap::default();
        let mut ret_of: FxHashMap<NodeId, Vec<CallId>> = FxHashMap::default();
        let mut regions: FxHashSet<NodeId> = FxHashSet::default();
        regions.insert(m.thrown);
        for (c, cs) in m.calls.iter().enumerate() {
            for (i, &a) in cs.args.iter().enumerate() {
                arg_of.entry(a).or_default().push((c as CallId, i as u32));
            }
            for &a in &cs.spread {
                arg_of.entry(a).or_default().push((c as CallId, u32::MAX));
            }
            ret_of.entry(cs.ret).or_default().push(c as CallId);
            regions.insert(cs.region);
        }
        for (n, node) in m.nodes.iter().enumerate() {
            if m.labels[n].starts_with("RAISE") {
                regions.insert(n as NodeId);
            }
            for c in &node.cons {
                if let Cons::Raise { region } = c {
                    regions.insert(*region);
                }
            }
        }
        let mut rec_of = FxHashMap::default();
        for r in &g.call_sites_at {
            if r.call != NO_CTX {
                rec_of.insert(r.call, r);
            }
        }
        let by_body: FxHashMap<(u32, u32), FnIdx> = m
            .funcs
            .iter()
            .enumerate()
            .filter(|(_, f)| f.func_id != u32::MAX)
            .map(|(i, f)| ((f.module, f.func_id), i as FnIdx))
            .collect();
        let targets = crate::signatures::call_targets(g, sites)
            .into_iter()
            .filter_map(|(k, (t, _))| by_body.get(&t).map(|&f| (k, f)))
            .collect();
        let program_methods = sites
            .iter()
            .filter(|s| s.is_call && (s.top || !s.targets.is_empty()))
            .map(|s| (s.module, s.byte_offset))
            .collect();
        let mut copies: FxHashMap<NodeId, Vec<NodeId>> = FxHashMap::default();
        let mut orig_of: FxHashMap<NodeId, NodeId> = FxHashMap::default();
        for inst in 0..m.sum.insts.len() as u32 {
            m.inst_copies(inst, |orig, copy| {
                copies.entry(orig).or_default().push(copy);
                orig_of.insert(copy, orig);
            });
        }
        let with_copies = |ns: &mut dyn Iterator<Item = NodeId>| -> FxHashSet<NodeId> {
            let mut out = FxHashSet::default();
            for n in ns {
                out.insert(n);
                out.extend(copies.get(&n).into_iter().flatten().copied());
            }
            out
        };
        let typed = with_copies(
            &mut g
                .mods
                .iter()
                .flat_map(|mo| mo.locals.values().copied())
                .chain(g.load_sites.iter().map(|&(_, _, _, n)| n)),
        );
        let relays = with_copies(&mut m.for_of_relays.iter().copied());
        let aggregates: FxHashSet<NodeId> = m
            .all_fields
            .iter()
            .chain(m.field_any.iter())
            .filter(|(t, _)| {
                !matches!(m.toks[**t as usize], TokKind::Obj(tm, _)
                    if matches!(m.tmpls[tm as usize].kind, TmplKind::Arr { .. }))
            })
            .map(|(_, n)| *n)
            .collect();
        let relay_tmpls: FxHashSet<TmplId> = m
            .tmpls
            .iter()
            .enumerate()
            .filter(|(_, t)| {
                matches!(t.kind, TmplKind::Arr { .. }) && t.display.contains("[iterator#")
            })
            .map(|(i, _)| i as TmplId)
            .collect();
        let mut holders: FxHashMap<TmplId, Vec<NodeId>> = FxHashMap::default();
        for (n, node) in m.nodes.iter().enumerate() {
            if node.pts.is_empty() {
                continue;
            }
            for t in node.pts.to_sorted() {
                if let TokKind::Obj(tm, _) = m.toks[t as usize] {
                    if matches!(
                        m.tmpls[tm as usize].kind,
                        TmplKind::Lit { .. }
                            | TmplKind::Arr { .. }
                            | TmplKind::Class {
                                literal_shape: true,
                                ..
                            }
                    ) {
                        let v = holders.entry(tm).or_default();
                        if v.last() != Some(&(n as NodeId)) {
                            v.push(n as NodeId);
                        }
                    }
                }
            }
        }
        let mut ctor_this: FxHashMap<NodeId, TmplId> = FxHashMap::default();
        for (tm, t) in m.tmpls.iter().enumerate() {
            if let (
                TmplKind::Class {
                    literal_shape: true,
                    ..
                },
                Some(f),
            ) = (&t.kind, t.ctor)
            {
                let this = m.funcs[f as usize].this;
                ctor_this.insert(this, tm as TmplId);
                for &c in copies.get(&this).into_iter().flatten() {
                    ctor_this.insert(c, tm as TmplId);
                }
            }
        }
        let mut new_calls: FxHashSet<CallId> = FxHashSet::default();
        let mut literal_births: FxHashSet<CallId> = FxHashSet::default();
        let mut mcall_recv: FxHashMap<CallId, NodeId> = FxHashMap::default();
        for (n, node) in m.nodes.iter().enumerate() {
            for c in &node.cons {
                match c {
                    Cons::MCall { call, .. } | Cons::IdxMCall { call, .. } => {
                        mcall_recv.entry(*call).or_insert(n as NodeId);
                    }
                    Cons::New { call }
                    | Cons::NewNamed { call, .. }
                    | Cons::NewProgram { call }
                    | Cons::ReflectNew { call, .. } => {
                        new_calls.insert(*call);
                        let shapes = !node.pts.is_empty()
                            && node.pts.to_sorted().into_iter().all(|t| {
                                matches!(m.toks[t as usize], TokKind::ClassObj(tm)
                                    if matches!(m.tmpls[tm as usize].kind, TmplKind::Class { literal_shape: true, .. }))
                            });
                        if shapes && matches!(c, Cons::New { .. } | Cons::NewNamed { .. }) {
                            literal_births.insert(*call);
                        }
                    }
                    _ => {}
                }
            }
        }
        let elem_atom = m.atoms.iter().position(|a| a == ELEM).map(|a| a as Atom);
        let captured = m
            .captures
            .iter()
            .filter(|&&(_, f)| (f as usize) < m.funcs.len() && m.funcs[f as usize].parent != NO_FN)
            .map(|&(n, _)| n)
            .collect();
        Scan {
            g,
            captured,
            ignored_route: None,
            ignore_r2: false,
            ignore_untyped: false,
            tmpl_module: FxHashMap::default(),
            elem_atom,
            holders,
            ctor_this,
            new_calls,
            literal_births,
            mcall_recv,
            typed,
            relays,
            aggregates,
            relay_tmpls,
            field_owner,
            arg_of,
            ret_of,
            rec_of,
            targets,
            program_methods,
            regions,
            copies,
            orig_of,
        }
    }

    /// The node, or one of its summary instance copies (where a summarized
    /// body's effects run), carries a sanctioned protocol (§4.1).
    fn coerced(&self, n: NodeId) -> bool {
        let m = &self.g.m;
        std::iter::once(n)
            .chain(self.copies.get(&n).into_iter().flatten().copied())
            .any(|x| {
                m.nodes[x as usize]
                    .cons
                    .iter()
                    .any(|k| matches!(k, Cons::Implicit { proto, .. } if sanctioned(*proto)))
            })
    }

    /// The nodes holding a token of any of `tms`, ascending.
    fn nodes_of(&self, tms: impl Iterator<Item = TmplId>) -> Vec<NodeId> {
        let mut v: Vec<NodeId> = tms
            .flat_map(|tm| self.holders.get(&tm).into_iter().flatten().copied())
            .collect();
        v.sort_unstable();
        v.dedup();
        v
    }

    /// An Array token's element nodes: the `#elem` key and the any-key
    /// field (where a declared native's `elems(this)` stores).
    fn elems(&self, t: Tok) -> Vec<NodeId> {
        let m = &self.g.m;
        self.elem_atom
            .and_then(|a| m.fields.get(&(t, a)).copied())
            .into_iter()
            .chain(m.field_any.get(&t).copied())
            .collect()
    }

    /// The source call a summary instance's call `c` copies (instance calls
    /// have no site record): the call whose argument `i` (`None`: result)
    /// is the shared node of `n`.
    fn source_call(&self, c: CallId, n: NodeId, i: Option<u32>) -> CallId {
        if self.rec_of.contains_key(&c) {
            return c;
        }
        let by_node = self.orig_of.get(&n).and_then(|&o| match i {
            Some(i) => self
                .arg_of
                .get(&o)
                .into_iter()
                .flatten()
                .find(|(c0, j)| *j == i && self.rec_of.contains_key(c0))
                .map(|(c0, _)| *c0),
            None => self
                .ret_of
                .get(&o)
                .into_iter()
                .flatten()
                .find(|c0| self.rec_of.contains_key(c0))
                .copied(),
        });
        by_node.or_else(|| self.source_by_ret(c)).unwrap_or(c)
    }

    /// The source call of instance call `c` through its own result node,
    /// a copy of the source call's result (when its argument is a body node
    /// every instance shares).
    fn source_by_ret(&self, c: CallId) -> Option<CallId> {
        let ret = self.g.m.calls.get(c as usize)?.ret;
        let o = self.orig_of.get(&ret)?;
        self.ret_of
            .get(o)
            .into_iter()
            .flatten()
            .find(|c0| self.rec_of.contains_key(c0))
            .copied()
    }

    /// `source_call` for a binding constraint (`ArgInto`, `RetInto`) of `n`.
    fn source_any(&self, c: CallId, n: NodeId) -> CallId {
        if self.rec_of.contains_key(&c) {
            return c;
        }
        let by_node = self.orig_of.get(&n).and_then(|o| {
            self.arg_of
                .get(o)
                .into_iter()
                .flatten()
                .map(|(c0, _)| *c0)
                .chain(self.ret_of.get(o).into_iter().flatten().copied())
                .find(|c0| self.rec_of.contains_key(c0))
        });
        by_node.or_else(|| self.source_by_ret(c)).unwrap_or(c)
    }

    fn site(&self, c: CallId) -> String {
        match self.rec_of.get(&c) {
            Some(r) => format!("{}:{}", self.module_name(r.module), r.offset),
            // A call no source site records (a summary instance's, or one the
            // solver or a builtin makes): its receiver or result, for the
            // ledger reader.
            None => {
                let m = &self.g.m;
                let cs = m.calls.get(c as usize);
                let recv = self
                    .mcall_recv
                    .get(&c)
                    .map(|&r| format!(" recv={}", node_at(m, r)))
                    .unwrap_or_default();
                let ret = cs.map(|cs| node_at(m, cs.ret)).unwrap_or_default();
                format!("call#{c} ret={ret}{recv}")
            }
        }
    }

    fn module_name(&self, mi: u32) -> String {
        self.g
            .modules
            .get(mi as usize)
            .map(|(p, _)| p.rsplit('/').next().unwrap_or(p).to_string())
            .unwrap_or_else(|| mi.to_string())
    }

    /// The program body call `c` is a typed call of (B3), or why not.
    fn typed_target(&self, c: CallId) -> Result<FnIdx, RecordReject> {
        let m = &self.g.m;
        let untyped = |why: &str| RecordReject::R5UntypedCall(format!("{} {why}", self.site(c)));
        let Some(r) = self.rec_of.get(&c) else {
            return Err(untyped("no source call"));
        };
        match self.targets.get(&(r.module, r.offset)) {
            Some(&f) if crate::signatures::may_type(self.g, &m.funcs[f as usize]) => Ok(f),
            Some(_) => Err(untyped("callee takes no typed signature")),
            None => Err(untyped("not one body")),
        }
    }

    /// A call whose callee is a native only (no program body, no ⊤).
    fn native_call(&self, c: CallId) -> bool {
        let m = &self.g.m;
        let Some(r) = self.rec_of.get(&c) else {
            return false;
        };
        match r.callee {
            Some(n) => {
                let node = &m.nodes[n as usize];
                !node.pts.is_empty()
                    && node
                        .pts
                        .to_sorted()
                        .into_iter()
                        .all(|t| matches!(m.toks[t as usize], TokKind::BuiltinFn(_)))
            }
            None => !self.program_methods.contains(&(r.module, r.offset)),
        }
    }

    /// The record templates and Arr templates whose tokens `n` holds.
    fn held(
        &self,
        n: NodeId,
        admitted: &FxHashSet<TmplId>,
        arrs: &FxHashMap<TmplId, TmplId>,
    ) -> (Vec<TmplId>, Vec<TmplId>, bool) {
        let m = &self.g.m;
        let node = &m.nodes[n as usize];
        let (mut recs, mut ars, mut other) = (Vec::new(), Vec::new(), false);
        if node.pts.is_empty() {
            return (recs, ars, other);
        }
        for t in node.pts.to_sorted() {
            match m.toks[t as usize] {
                TokKind::Obj(tm, _) if admitted.contains(&tm) => {
                    if !recs.contains(&tm) {
                        recs.push(tm)
                    }
                }
                TokKind::Obj(tm, _) if arrs.contains_key(&tm) => {
                    if !ars.contains(&tm) {
                        ars.push(tm)
                    }
                }
                // The boxed program's iterator over a for-of source.
                TokKind::Obj(tm, _) if self.relay_tmpls.contains(&tm) => {}
                // S0 `vrarrjoin`: another Array beside a record array.
                TokKind::Obj(tm, _)
                    if m.record_array_join_sabotage
                        && matches!(m.tmpls[tm as usize].kind, TmplKind::Arr { .. }) => {}
                _ => other = true,
            }
        }
        (recs, ars, other)
    }

    /// R6(a): link every Arr template whose element node holds a candidate
    /// record's tokens (and nothing else), or that shares a holder with a
    /// linked one while its own element node is empty.
    fn link_arrays(
        &self,
        admitted: &FxHashSet<TmplId>,
        wrappers: &FxHashSet<TmplId>,
        linked: &mut FxHashMap<TmplId, TmplId>,
        arr_rejects: &mut Vec<(TmplId, Rejected)>,
    ) {
        use RecordReject::*;
        let m = &self.g.m;
        for tm in 0..m.tmpls.len() as TmplId {
            if !matches!(m.tmpls[tm as usize].kind, TmplKind::Arr { .. })
                || linked.contains_key(&tm)
                || wrappers.contains(&tm)
                || self.relay_tmpls.contains(&tm)
            {
                continue;
            }
            let mut of: Option<TmplId> = None;
            let mut bad: Option<String> = None;
            for e in m.tmpl_toks(tm).into_iter().flat_map(|t| self.elems(t)) {
                let node = &m.nodes[e as usize];
                for x in node.pts.to_sorted() {
                    if let TokKind::Obj(r, _) = m.toks[x as usize] {
                        if admitted.contains(&r) {
                            if of.is_some_and(|o| o != r) {
                                bad = Some("two record templates".into());
                            }
                            of = Some(r);
                            continue;
                        }
                    }
                    bad.get_or_insert_with(|| "an element that is no record".into());
                }
                if node.prim != 0 {
                    bad.get_or_insert_with(|| format!("elements {}", prim::name(node.prim)));
                }
            }
            if let Some(r) = of {
                linked.insert(tm, r);
                if let Some(b) = bad {
                    arr_rejects.push((tm, Rejected::new(R6Join(b), "")));
                }
            }
        }
        // An Arr template sharing a holder with a linked one: linked too
        // when its element node is empty (chess.ts:179/222, an unpushed `[]`).
        let mut grew = true;
        while grew {
            grew = false;
            for n in self.nodes_of(linked.keys().copied()) {
                let node = &m.nodes[n as usize];
                // An object's fields under any key hold every field's
                // Arrays: no holder of the program.
                if node.pts.is_empty() || self.aggregates.contains(&n) {
                    continue;
                }
                let toks = node.pts.to_sorted();
                let mut of: Option<TmplId> = None;
                for &t in &toks {
                    if let TokKind::Obj(a, _) = m.toks[t as usize] {
                        if let Some(&r) = linked.get(&a) {
                            of = Some(r);
                        }
                    }
                }
                let Some(r) = of else { continue };
                for &t in &toks {
                    let TokKind::Obj(a, _) = m.toks[t as usize] else {
                        continue;
                    };
                    if linked.contains_key(&a)
                        || wrappers.contains(&a)
                        || self.relay_tmpls.contains(&a)
                        || !matches!(m.tmpls[a as usize].kind, TmplKind::Arr { .. })
                    {
                        continue;
                    }
                    let empty = m.tmpl_toks(a).iter().flat_map(|&t| self.elems(t)).all(|e| {
                        m.nodes[e as usize].pts.is_empty() && m.nodes[e as usize].prim == 0
                    });
                    // S0 `vrarrjoin`: the other Array stays what it is.
                    if !empty && m.record_array_join_sabotage {
                        continue;
                    }
                    linked.insert(a, r);
                    grew = true;
                    if !empty {
                        arr_rejects.push((a, Rejected::new(R6Join(node_at(m, n)), "")));
                    }
                }
            }
        }
        let _ = admitted;
    }

    /// The boundary-only wrappers (§4.3): object and array literal templates
    /// (not records) whose every holder only reaches a sanctioned native
    /// argument, or a field of another such wrapper, and that program code
    /// never reads. A record stored into one is materialized with it at the
    /// boundary. A greatest fixpoint, like admission.
    fn boundary_wrappers(&self, admitted: &FxHashSet<TmplId>) -> FxHashSet<TmplId> {
        let m = &self.g.m;
        let mut set: FxHashSet<TmplId> = self
            .holders
            .keys()
            .copied()
            .filter(|tm| !admitted.contains(tm))
            .filter(|&tm| {
                m.tmpl_toks(tm)
                    .iter()
                    .all(|t| !m.is_escaped(*t) && !m.hard.contains_key(t))
            })
            .collect();
        loop {
            let before = set.len();
            let snapshot = set.clone();
            set.retain(|tm| {
                self.holders[tm].iter().all(|&h| {
                    // The native work of a sanctioned protocol on the
                    // argument (`Model::implicit`, the array methods the
                    // printer runs): classified at the protocol.
                    let label = m.labels[h as usize];
                    if label.starts_with("implicit-")
                        || label == "tostringtag-recv"
                        || label == "arr-recv"
                    {
                        return true;
                    }
                    if self.regions.contains(&h) {
                        return false;
                    }
                    let births_only = self
                        .ret_of
                        .get(&h)
                        .into_iter()
                        .flatten()
                        .all(|&c| self.new_calls.contains(&self.source_call(c, h, None)));
                    if !births_only {
                        return false;
                    }
                    if let Some(&o) = self.field_owner.get(&h) {
                        match m.toks[o as usize] {
                            TokKind::Obj(ot, _) if snapshot.contains(&ot) => {}
                            _ => return false,
                        }
                    }
                    let cons = &m.nodes[h as usize].cons;
                    let coerced = self.coerced(h);
                    for &(c, i) in self.arg_of.get(&h).into_iter().flatten() {
                        let c = if i == u32::MAX {
                            c
                        } else {
                            self.source_call(c, h, Some(i))
                        };
                        if self.literal_births.contains(&c) && i != u32::MAX {
                            continue;
                        }
                        if i == u32::MAX || !self.native_call(c) || !coerced {
                            return false;
                        }
                    }
                    let birth = self.ctor_this.get(&h) == Some(tm);
                    cons.iter().all(|k| {
                        (birth && matches!(k, Cons::Store { .. }))
                            || matches!(
                                k,
                                Cons::Falsy { .. }
                                    | Cons::NotNullish { .. }
                                    | Cons::NotUndefined { .. }
                                    | Cons::Guard { .. }
                                    | Cons::Defined { .. }
                                    | Cons::TopIfTop { .. }
                                    | Cons::Raise { .. }
                                    | Cons::BirthField { .. }
                            )
                            || matches!(k, Cons::Implicit { proto, .. } if sanctioned(*proto))
                    })
                })
            });
            if set.len() == before {
                break;
            }
        }
        set
    }

    /// The nodes that hold a materialized copy (owner ruling, §4.4): a
    /// typed call's argument whose parameter joins a record with other
    /// values is Boxed, so the record is copied at the call; the copy's
    /// nodes inside the helper (and wherever it passes it) may only read it.
    fn copy_regions(&self, admitted: &FxHashSet<TmplId>) -> FxHashMap<NodeId, FnIdx> {
        let m = &self.g.m;
        let mut region: FxHashMap<NodeId, FnIdx> = FxHashMap::default();
        let mut work: Vec<NodeId> = Vec::new();
        for (c, cs) in m.calls.iter().enumerate() {
            let Ok(f) = self.typed_target(c as CallId) else {
                continue;
            };
            let fi = &m.funcs[f as usize];
            for (i, &a) in cs.args.iter().enumerate() {
                let (recs, _, _) = self.held(a, admitted, &FxHashMap::default());
                if recs.is_empty() {
                    continue;
                }
                let Some(&p) = fi.params.get(i) else { continue };
                let mut ps = vec![p];
                ps.extend(self.copies.get(&p).into_iter().flatten().copied());
                let mixed = ps.iter().any(|&p| {
                    let (r, ars, other) = self.held(p, admitted, &FxHashMap::default());
                    other
                        || !ars.is_empty()
                        || r.len() > 1
                        || m.nodes[p as usize].prim & !prim::UNDEF != 0
                }) || {
                    let mut all: Vec<TmplId> = Vec::new();
                    for &p in &ps {
                        all.extend(self.held(p, admitted, &FxHashMap::default()).0);
                    }
                    all.sort_unstable();
                    all.dedup();
                    all.len() > 1
                };
                if mixed {
                    for p in ps {
                        if region.insert(p, f).is_none() {
                            work.push(p);
                        }
                    }
                    // A mixed argument (`show(c ? 5 : u)`) is evaluated into
                    // the Boxed parameter: the record is materialized there,
                    // so the argument node holds the copy too.
                    let (_, ars, other) = self.held(a, admitted, &FxHashMap::default());
                    let arg_mixed =
                        other || !ars.is_empty() || m.nodes[a as usize].prim & !prim::UNDEF != 0;
                    if arg_mixed && region.insert(a, f).is_none() {
                        work.push(a);
                    }
                }
            }
        }
        while let Some(n) = work.pop() {
            let f = region[&n];
            if self.field_owner.contains_key(&n) {
                continue;
            }
            for &s in &m.nodes[n as usize].succ {
                if let std::collections::hash_map::Entry::Vacant(e) = region.entry(s) {
                    e.insert(f);
                    work.push(s);
                }
            }
        }
        region
    }

    /// R4-R6 over every node; pushes each violation.
    fn scan(
        &self,
        admitted: &FxHashSet<TmplId>,
        arrs: &FxHashMap<TmplId, TmplId>,
        linked: &FxHashMap<TmplId, TmplId>,
        wrappers: &FxHashSet<TmplId>,
        rejects: &mut Vec<(TmplId, Rejected)>,
        arr_rejects: &mut Vec<(TmplId, Rejected)>,
    ) {
        use RecordReject::*;
        let m = &self.g.m;
        if admitted.is_empty() {
            return;
        }
        let region = self.copy_regions(admitted);
        let keys: FxHashMap<TmplId, Vec<Atom>> = admitted
            .iter()
            .map(|&tm| (tm, m.tmpls[tm as usize].birth_keys.clone()))
            .collect();
        for n in self.nodes_of(admitted.iter().chain(arrs.keys()).copied()) {
            let (recs, ars, other) = self.held(n, admitted, arrs);
            if recs.is_empty() && ars.is_empty() {
                continue;
            }
            let node = &m.nodes[n as usize];
            // A protocol's own relay (`Model::implicit`): its reads are the
            // protocol's, classified at the `Implicit` on the value.
            if m.labels[n as usize].starts_with("implicit-") {
                // VR-4b: a native's protocol that reads through an object
                // reached a record array (§2 `R5ThroughContainer`): the
                // runtime's readers have no accessor for one before VR-5.
                for &a in &ars {
                    arr_rejects.push((
                        a,
                        Rejected::new(
                            R5ThroughContainer(m.labels[n as usize].to_string()),
                            node_at(m, n),
                        ),
                    ));
                }
                continue;
            }
            let at = node_at(m, n);
            let owner = self.field_owner.get(&n).map(|&t| match m.toks[t as usize] {
                TokKind::Obj(o, _) => Some(o),
                _ => None,
            });
            // R4-R5 for the records the node holds.
            if !recs.is_empty() {
                let mut why: Option<RecordReject> = None;
                if let Some(&f) = region.get(&n) {
                    why = self.copy_use(n, f);
                } else {
                    let elem_of_arr = owner.flatten().is_some_and(|o| arrs.contains_key(&o));
                    let in_wrapper = owner.flatten().is_some_and(|o| wrappers.contains(&o));
                    if in_wrapper {
                        // §4.3: a field of a boundary-only wrapper; the
                        // boundary materializes the whole argument tree.
                    } else if owner.is_some() && !elem_of_arr {
                        let o = match owner.flatten() {
                            Some(o) => m.tmpls[o as usize].display.clone(),
                            None => "a builtin object".into(),
                        };
                        // An iterator's value slot over one of the record's
                        // own arrays (`for (const u of played)`): the for-of
                        // intrinsic of §5.3, R6's to decide.
                        let iterates_own = owner.flatten().is_some_and(|o| {
                            self.relay_tmpls.contains(&o)
                                || matches!(m.tmpls[o as usize].kind, TmplKind::BObj { what, .. } if what.contains("Iter"))
                        }) && recs
                            .iter()
                            .any(|tm| linked.values().any(|r| r == tm));
                        // The for-of intrinsic (§5.3) copies each element
                        // out; the iterator's slot is the boxed program's.
                        if !(iterates_own && RECORD_ARRAY_INTRINSICS.contains(&"iterate")) {
                            why = Some(if iterates_own {
                                R6Op("iterate".into())
                            } else {
                                R5StoreHeap(o)
                            });
                        }
                    } else if self.regions.contains(&n) {
                        why = Some(R5Throw);
                    } else if self.captured.contains(&n) {
                        why = Some(R5StoreHeap("closure capture".into()));
                    } else if !elem_of_arr
                        && (other
                            || !ars.is_empty()
                            || recs.len() > 1
                            || node.prim & !prim::UNDEF != 0)
                    {
                        why = Some(R5Join(at.clone()));
                    }
                    if why.is_none() {
                        why = self.call_slots(n, &recs, linked, &region);
                    }
                    if why.is_none() {
                        let birth = self.ctor_this.get(&n).copied();
                        for &tm in &recs {
                            for c in &node.cons {
                                if birth == Some(tm) {
                                    if let Cons::Store {
                                        key: Key::Named(a), ..
                                    } = c
                                    {
                                        if keys[&tm].contains(a) {
                                            continue;
                                        }
                                    }
                                }
                                match record_use(c, m, &keys[&tm]) {
                                    Use::Allowed => {}
                                    Use::Reject(R4Identity(r))
                                        if self.ignored_route == Some(r.as_str()) => {}
                                    Use::Reject(R2Write(_)) if self.ignore_r2 => {}
                                    Use::TypedCall(call)
                                        if self.own_array_call(call, &recs, linked) => {}
                                    Use::TypedCall(call) => {
                                        let src = self.source_any(call, n);
                                        match self.typed_target(src) {
                                            Err(_) if self.ignore_untyped => {}
                                            Err(e) => why = Some(e),
                                            Ok(f) => {
                                                let arg = match c {
                                                    Cons::ArgInto { i, .. } => Some(*i),
                                                    _ => None,
                                                };
                                                why = self.record_slot(src, f, arg, &region, &recs);
                                            }
                                        }
                                    }
                                    Use::Reject(e) => why = Some(e),
                                    Use::Op => {}
                                }
                                if why.is_some() {
                                    break;
                                }
                            }
                        }
                    }
                }
                if let Some(w) = why {
                    for &tm in &recs {
                        rejects.push((tm, Rejected::new(w.clone(), at.clone())));
                    }
                }
            }
            // R6(b)-(d) for the record arrays the node holds.
            if !ars.is_empty() {
                let tms: FxHashSet<TmplId> = ars.iter().map(|a| arrs[a]).collect();
                let mut why: Option<RecordReject> = None;
                // A for-of binding of the iterator arm also holds the
                // `undefined` its glue passes through (`arrayRecordPayload`).
                // The boxed program's for-of plumbing: nothing codegen
                // lowers for a record array (the intrinsic replaces the arm).
                let orig = self
                    .orig_of
                    .get(&n)
                    .copied()
                    .filter(|o| (*o as usize) < m.nodes.len())
                    .unwrap_or(n);
                // An object's fields under any key: its readers are checked.
                if self.aggregates.contains(&orig) {
                    continue;
                }
                if RECORD_ARRAY_INTRINSICS.contains(&"iterate")
                    && FOR_OF_GLUE
                        .iter()
                        .any(|l| m.labels[orig as usize].starts_with(l))
                {
                    continue;
                }
                let relay = self.relays.contains(&n);
                let glue = if relay { prim::UNDEF } else { 0 };
                if other || !recs.is_empty() || tms.len() > 1 || node.prim & !glue != 0 {
                    why = Some(R6Join(at.clone()));
                } else if self.regions.contains(&n) || region.contains_key(&n) {
                    why = Some(R6ArrayHolder(at.clone()));
                } else if self.captured.contains(&n) {
                    why = Some(R6ArrayHolder(format!("captured {at}")));
                } else if let Some(o) = owner {
                    // A record array in another record array or record: VR-7.
                    if o.is_some_and(|o| arrs.contains_key(&o) || admitted.contains(&o)) {
                        why = Some(R6ArrayHolder(format!("nested {at}")));
                    }
                }
                if why.is_none() {
                    if let Some(&(c, i)) = self.arg_of.get(&n).and_then(|v| v.first()) {
                        let c = &if i == u32::MAX {
                            c
                        } else {
                            self.source_call(c, n, Some(i))
                        };
                        let i = &i;
                        why = match self.typed_target(*c) {
                            Ok(_) if *i != u32::MAX => None,
                            Ok(_) => Some(R6Op("spread".into())),
                            Err(_) if self.native_call(*c) => {
                                Some(R6ArrayHolder(format!("native argument {}", self.site(*c))))
                            }
                            Err(e) => Some(e),
                        };
                    }
                }
                if why.is_none() {
                    if let Some(&c) = self.ret_of.get(&n).and_then(|v| v.first()) {
                        let c = self.source_call(c, n, None);
                        if self.new_calls.contains(&c) {
                        } else if let Err(e) = self.typed_target(c) {
                            why = Some(e);
                        }
                    }
                }
                // VR-4b: codegen births a record array for its holder, types
                // an operation's receiver by its binding or named read, and
                // lowers the record's layout in the record's own module.
                if why.is_none() && m.arr_permuted.contains(&orig) {
                    why = Some(R6Op("permute".into()));
                }
                if why.is_none() {
                    if let Some(a) = ars.iter().find(|a| !m.arr_birth_holder.contains_key(a)) {
                        why = Some(R6ArrayHolder(format!(
                            "birth {}",
                            m.tmpls[*a as usize].display
                        )));
                    }
                }
                if why.is_none() {
                    let f = m.node_fn[n as usize];
                    let here = (f != NO_FN).then(|| m.funcs[f as usize].module);
                    let home = tms.iter().next().and_then(|tm| self.tmpl_module[tm]);
                    if here.is_some() && home.is_some() && here != home {
                        why = Some(R6ArrayHolder(format!("another module {at}")));
                    }
                }
                if why.is_none() {
                    // One body, every instance copy: the same representation.
                    let sibs = std::iter::once(orig)
                        .chain(self.copies.get(&orig).into_iter().flatten().copied());
                    for s in sibs {
                        let sn = &m.nodes[s as usize];
                        if s == n || (sn.pts.is_empty() && sn.prim == 0) {
                            continue;
                        }
                        let (r2, a2, o2) = self.held(s, admitted, arrs);
                        let t2: FxHashSet<TmplId> = a2.iter().map(|a| arrs[a]).collect();
                        // S0 `vrarrjoin`: a copy that holds another Array.
                        if m.record_array_join_sabotage && t2.is_empty() && !o2 && r2.is_empty() {
                            continue;
                        }
                        if o2 || !r2.is_empty() || sn.prim & !glue != 0 || t2 != tms {
                            why = Some(R6Join(format!("copies of {at}")));
                            break;
                        }
                    }
                }
                if why.is_none() {
                    for c in &node.cons {
                        match array_use(c, m) {
                            Use::Allowed => {}
                            Use::TypedCall(call) => {
                                if let Err(e) = self.typed_target(self.source_any(call, n)) {
                                    why = Some(e);
                                }
                            }
                            // An operation on a for-of's own bindings is
                            // the boxed program's iteration protocol (the
                            // program names none of them): the arm the
                            // for-of intrinsic replaces.
                            Use::Reject(R6Op(_))
                                if relay && RECORD_ARRAY_INTRINSICS.contains(&"iterate") => {}
                            Use::Reject(e) => why = Some(e),
                            Use::Op if !self.typed.contains(&n) => {
                                why = Some(R6ArrayHolder(format!("temporary {at}")));
                            }
                            Use::Op => {}
                        }
                        if why.is_some() {
                            break;
                        }
                    }
                }
                if let Some(w) = why {
                    for &a in &ars {
                        arr_rejects.push((a, Rejected::new(w.clone(), at.clone())));
                    }
                }
            }
        }
    }

    /// R5UntypedCall / R5Native for a node that is a call's argument or
    /// result (B3, L1): a typed program call, or a native that runs only a
    /// sanctioned protocol on the slot.
    fn call_slots(
        &self,
        n: NodeId,
        recs: &[TmplId],
        linked: &FxHashMap<TmplId, TmplId>,
        region: &FxHashMap<NodeId, FnIdx>,
    ) -> Option<RecordReject> {
        use RecordReject::*;
        for &(c, i) in self.arg_of.get(&n).into_iter().flatten() {
            // A method of a record array of this record (`played.push(u)`):
            // the array's R6 rules decide it.
            if self.own_array_call(c, recs, linked) {
                continue;
            }
            if i == u32::MAX {
                return Some(R5Iterate(format!("spread argument {}", self.site(c))));
            }
            let c = self.source_call(c, n, Some(i));
            if self.literal_births.contains(&c) {
                continue;
            }
            match self.typed_target(c) {
                Ok(f) => {
                    if let Some(e) = self.record_slot(c, f, Some(i), region, recs) {
                        return Some(e);
                    }
                }
                Err(_) if self.native_call(c) => {
                    if !self.coerced(n) {
                        return Some(R5Native {
                            site: self.site(c),
                            slot: format!("arg({i})"),
                            why: "no declared read or protocol".into(),
                        });
                    }
                }
                Err(_) if self.ignore_untyped => {}
                Err(e) => return Some(e),
            }
        }
        for &c in self.ret_of.get(&n).into_iter().flatten() {
            if self.own_array_call(c, recs, linked) {
                continue;
            }
            let c = self.source_call(c, n, None);
            if self.new_calls.contains(&c) {
                continue;
            }
            match self.typed_target(c) {
                Ok(f) => {
                    if let Some(e) = self.record_slot(c, f, None, region, recs) {
                        return Some(e);
                    }
                }
                Err(e) => {
                    if !self.native_call(c) {
                        return Some(e);
                    }
                    return Some(R5Native {
                        site: self.site(c),
                        slot: "result".into(),
                        why: "a native returns the record".into(),
                    });
                }
            }
        }
        None
    }

    /// A record in a typed call's slot (`arg`: argument `i`, `None`: the
    /// result) crosses in the callee's `Record` signature slot (§5.2,
    /// VR-3b): codegen's typed calls are same-module, and a record's lanes
    /// are lowered where its literal is born, so the call, the callee and
    /// the templates `recs` are of one module. A Boxed parameter of a
    /// helper receives a materialized copy instead (owner ruling §4.4,
    /// `copy_regions`). With record signatures off
    /// (`PERRY_FACTS_RECORD_SIGNATURES=0`) every other slot rejects.
    fn record_slot(
        &self,
        c: CallId,
        f: FnIdx,
        arg: Option<u32>,
        region: &FxHashMap<NodeId, FnIdx>,
        recs: &[TmplId],
    ) -> Option<RecordReject> {
        let m = &self.g.m;
        let boxed_param = arg.is_some_and(|i| {
            m.funcs[f as usize]
                .params
                .get(i as usize)
                .is_some_and(|p| region.contains_key(p))
        });
        if boxed_param {
            return None;
        }
        let slot = match arg {
            Some(i) => format!("arg({i})"),
            None => "result".to_string(),
        };
        let untyped = |why: &str| {
            Some(RecordReject::R5UntypedCall(format!(
                "{} {slot}: {why}",
                self.site(c)
            )))
        };
        if !m.record_signatures {
            return untyped("record signatures are off");
        }
        let fm = m.funcs[f as usize].module;
        if self.rec_of.get(&c).is_some_and(|r| r.module != fm) {
            return untyped("a call from another module");
        }
        if recs
            .iter()
            .any(|tm| self.tmpl_module.get(tm).copied().flatten() != Some(fm))
        {
            return untyped("the literal is born in another module");
        }
        None
    }

    /// Call `c` is a method call on record arrays of one of `recs` only
    /// (`played.push(u)`, `played.pop()`): R6 decides it, as an operation on
    /// the array (`RECORD_ARRAY_INTRINSICS`).
    fn own_array_call(
        &self,
        c: CallId,
        recs: &[TmplId],
        linked: &FxHashMap<TmplId, TmplId>,
    ) -> bool {
        let m = &self.g.m;
        self.mcall_recv.get(&c).is_some_and(|&r| {
            let node = &m.nodes[r as usize];
            !node.pts.is_empty()
                && node.pts.to_sorted().into_iter().all(|t| {
                    matches!(m.toks[t as usize], TokKind::Obj(a, _)
                        if linked.get(&a).is_some_and(|tm| recs.contains(tm)))
                })
        })
    }

    /// The owner ruling's condition on a copy's node (§4.4): read, presence,
    /// a sanctioned protocol, passing it on; anything else rejects the
    /// template as `R5RetainingHelper(f, why)`.
    fn copy_use(&self, n: NodeId, f: FnIdx) -> Option<RecordReject> {
        let m = &self.g.m;
        let full = &m.funcs[f as usize].name;
        let name = full.rsplit(':').next().unwrap_or(full).to_string();
        let reject = |why: &str| {
            Some(RecordReject::R5RetainingHelper {
                f: name.clone(),
                why: why.into(),
            })
        };
        if self.field_owner.contains_key(&n) || self.regions.contains(&n) {
            return reject("retain");
        }
        for &(c, i) in self.arg_of.get(&n).into_iter().flatten() {
            if i == u32::MAX {
                return reject("retain");
            }
            let c = self.source_call(c, n, Some(i));
            if self.typed_target(c).is_err() {
                if !(self.native_call(c) && self.coerced(n)) {
                    return reject("retain");
                }
            }
        }
        if self.ret_of.contains_key(&n) {
            return reject("retain");
        }
        for c in &m.nodes[n as usize].cons {
            let use_kind = match c {
                Cons::Identity { .. } => "identity",
                Cons::Load { .. }
                | Cons::FieldTruthy { .. }
                | Cons::Fulfil { .. }
                | Cons::Falsy { .. }
                | Cons::NotNullish { .. }
                | Cons::NotUndefined { .. }
                | Cons::Guard { .. }
                | Cons::Range { .. }
                | Cons::Defined { .. }
                | Cons::TopIfTop { .. }
                | Cons::Raise { .. }
                | Cons::ArgInto { .. }
                | Cons::Arith { .. }
                | Cons::Plus { .. } => continue,
                Cons::Implicit { proto, .. } if sanctioned(*proto) => continue,
                Cons::Store { .. }
                | Cons::DeleteKey { .. }
                | Cons::DefineKey { .. }
                | Cons::DefineAll { .. }
                | Cons::DefineDesc { .. }
                | Cons::AddsKeys
                | Cons::ProtoFrom { .. }
                | Cons::AnnexBDefine { .. }
                | Cons::IndexHole
                | Cons::IndexKept => "mutate",
                Cons::RetInto { .. }
                | Cons::TopSink { .. }
                | Cons::TopIfAny { .. }
                | Cons::RuntimeStore { .. }
                | Cons::Hard { .. } => "retain",
                _ => "call",
            };
            return reject(use_kind);
        }
        None
    }
}

/// The `records` ledger section (§7): one line per literal template and per
/// linked Arr template, then the limits and the summary.
pub fn ledger_lines(templates: &[TemplateFacts], arrays: &[RecordArray]) -> Vec<String> {
    let mut out = Vec::new();
    let mut admitted = 0usize;
    let mut rejected = 0usize;
    for t in templates {
        if !matches!(t.kind, "lit" | "litclass") {
            continue;
        }
        match &t.record {
            Ok(l) => {
                admitted += 1;
                let fields: Vec<String> = l
                    .fields
                    .iter()
                    .map(|(k, r)| format!("{}:{}", crate::model::esc_key(k), r.name()))
                    .collect();
                let elems = if t.record_elems.is_empty() {
                    String::new()
                } else {
                    format!(
                        " elems-of=[{}]",
                        t.record_elems
                            .iter()
                            .map(|a| a.to_string())
                            .collect::<Vec<_>>()
                            .join(",")
                    )
                };
                out.push(format!(
                    "# record {} tmpl={} words={} ADMITTED fields={}{elems}",
                    t.display,
                    t.id,
                    l.words,
                    fields.join(",")
                ));
            }
            Err(r) if r.why == RecordReject::Off => {}
            Err(r) => {
                rejected += 1;
                let at = if r.at.is_empty() {
                    String::new()
                } else {
                    format!(" at {}", r.at)
                };
                out.push(format!(
                    "# record {} tmpl={} words={} REJECTED {}{at} keys={}",
                    t.display,
                    t.id,
                    t.birth_keys.len(),
                    r.why,
                    t.birth_keys
                        .iter()
                        .map(|k| crate::model::esc_key(k))
                        .collect::<Vec<_>>()
                        .join(",")
                ));
            }
        }
    }
    for a in arrays {
        match &a.verdict {
            Ok(()) => out.push(format!(
                "# record-array {} tmpl={} elems-of={} ADMITTED",
                a.display, a.tmpl, a.elems_of
            )),
            Err(r) => out.push(format!(
                "# record-array {} tmpl={} elems-of={} REJECTED {}{}",
                a.display,
                a.tmpl,
                a.elems_of,
                r.why,
                if r.at.is_empty() {
                    String::new()
                } else {
                    format!(" at {}", r.at)
                }
            )),
        }
    }
    if admitted + rejected > 0 || !arrays.is_empty() {
        out.push(
            "# records-limits natives are default-deny (only realm declarations with effects describe a slot); \
             X | null rejects; two literal sites of one interface are two templates; \
             a record crosses a typed call of its own module in a Record signature slot \
             (a Boxed helper parameter receives a copy; PERRY_FACTS_RECORD_SIGNATURES=0 rejects instead); \
             record arrays wait for the VR-4b intrinsics"
                .to_string(),
        );
        out.push(format!(
            "# ledger: records admitted {admitted} rejected {rejected}"
        ));
    }
    out
}

/// VR-4b: a field whose every Array is a record array holds no plain
/// program Array, so its rep is the Boxed word (`derive` gave it `Arr`
/// before the record derive ran).
pub(crate) fn mark_fields(m: &Model, templates: &mut [TemplateFacts]) {
    if m.record_arrays.is_empty() {
        return;
    }
    for tf in templates.iter_mut() {
        let ts = m.tmpl_toks(tf.id);
        for f in tf.fields.iter_mut() {
            if !matches!(f.rep, crate::bindings::Rep::Arr(_)) {
                continue;
            }
            let atoms = if f.private {
                &m.private_atoms
            } else {
                &m.atom_index
            };
            let Some(&a) = atoms.get(&f.key) else {
                continue;
            };
            let holds = ts
                .iter()
                .filter_map(|t| m.fields.get(&(*t, a)))
                .flat_map(|n| m.nodes[*n as usize].pts.to_sorted())
                .any(|t| matches!(m.toks[t as usize], TokKind::Obj(tm, _) if m.record_arrays.contains_key(&tm)));
            if holds {
                f.rep = crate::bindings::Rep::Boxed;
            }
        }
    }
}

/// The record template of the record arrays `nodes` hold, when they hold
/// nothing else (an empty node holds nothing; one must hold one).
fn record_array_of(m: &Model, nodes: &[NodeId]) -> Option<TmplId> {
    let mut of: Option<TmplId> = None;
    for &n in nodes {
        let node = &m.nodes[n as usize];
        if node.prim != 0 {
            return None;
        }
        for t in node.pts.to_sorted() {
            let TokKind::Obj(tm, _) = m.toks[t as usize] else {
                return None;
            };
            // S0 `vrarrjoin`: another Array beside a record array.
            if m.record_array_join_sabotage
                && !m.record_arrays.contains_key(&tm)
                && matches!(m.tmpls[tm as usize].kind, TmplKind::Arr { .. })
            {
                continue;
            }
            let r = *m.record_arrays.get(&tm)?;
            if of.is_some_and(|o| o != r) {
                return None;
            }
            of = Some(r);
        }
    }
    of
}

/// Fill `ModuleBindings::record_array_*` (VR-4b): the bindings, named reads
/// and births codegen lowers as a record array.
pub(crate) fn export(g: &Gen<'_>, bindings: &mut [crate::bindings::ModuleBindings]) {
    let m = &g.m;
    if m.record_arrays.is_empty() {
        return;
    }
    let copies = crate::rest_elems::node_copies(m);
    let all = |n: NodeId| -> Vec<NodeId> {
        let mut v = vec![n];
        v.extend(copies.get(&n).into_iter().flatten().copied());
        v
    };
    for (mi, mo) in g.mods.iter().enumerate() {
        let Some(mb) = bindings.get_mut(mi) else {
            continue;
        };
        for (&id, &n) in mo.locals.iter() {
            if let Some(r) = record_array_of(m, &all(n)) {
                mb.record_array_locals.push((id, r));
            }
        }
    }
    let mut reads: FxHashMap<(u32, u32, Atom), Vec<NodeId>> = FxHashMap::default();
    for &(mi, offset, key, n) in &g.load_sites {
        reads.entry((mi, offset, key)).or_default().extend(all(n));
    }
    for ((mi, offset, key), nodes) in reads {
        if let (Some(r), Some(mb)) = (record_array_of(m, &nodes), bindings.get_mut(mi as usize)) {
            mb.record_array_reads
                .push((offset, m.atoms[key as usize].clone(), r));
        }
    }
    for (a, &r) in m.record_arrays.iter() {
        let Some(h) = m.arr_birth_holder.get(a) else {
            continue;
        };
        let (mi, put): (u32, Box<dyn FnOnce(&mut crate::bindings::ModuleBindings)>) =
            match h.clone() {
                ArrBirth::Local(mi, id) => (
                    mi,
                    Box::new(move |mb| mb.record_array_birth_locals.push((id, r))),
                ),
                ArrBirth::Store(mi, at) => (
                    mi,
                    Box::new(move |mb| mb.record_array_birth_stores.push((at, r))),
                ),
                ArrBirth::Field(mi, c, k) => (
                    mi,
                    Box::new(move |mb| mb.record_array_birth_fields.push((c, k, r))),
                ),
            };
        if let Some(mb) = bindings.get_mut(mi as usize) {
            put(mb);
        }
    }
    for mb in bindings.iter_mut() {
        mb.record_array_locals.sort_unstable();
        mb.record_array_locals.dedup();
        mb.record_array_reads.sort();
        mb.record_array_reads.dedup();
        mb.record_array_birth_locals.sort_unstable();
        mb.record_array_birth_locals.dedup();
        mb.record_array_birth_stores.sort_unstable();
        mb.record_array_birth_stores.dedup();
        mb.record_array_birth_fields.sort();
        mb.record_array_birth_fields.dedup();
    }
}
