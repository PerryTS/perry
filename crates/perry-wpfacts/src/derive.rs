//! Derived facts F1–F6 read off the solved graph (design section 2), and
//! the per-site verdicts the precision report uses.

mod element;
pub(crate) use element::element_facts;
use element::receiver_set;
pub use element::{ElementFacts, ElementKey, ElementOp, ReceiverValue};

use crate::gen::Gen;
use crate::model::*;
use crate::pts::{FxHashMap, FxHashSet};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct KindFact {
    pub prims: u16,
    /// Object templates the field can hold (display names), sorted.
    pub objs: Vec<String>,
    pub funcs: usize,
    pub builtin_objs: usize,
    pub top: bool,
}

impl KindFact {
    /// A single representation the consumer can rely on: one primitive
    /// kind, or objects (optionally nullable), never ⊤.
    pub fn fixed(&self) -> bool {
        if self.top {
            return false;
        }
        let has_obj = !self.objs.is_empty() || self.funcs > 0 || self.builtin_objs > 0;
        let p = self.prims;
        if !has_obj {
            return matches!(
                p,
                prim::NUM | prim::STR | prim::BOOL | prim::BIGINT | prim::SYMBOL
            );
        }
        p & !(prim::NULL | prim::UNDEF) == 0
    }

    pub fn desc(&self) -> String {
        if self.top {
            return "top".into();
        }
        let mut parts = Vec::new();
        if self.prims != 0 {
            parts.push(prim::name(self.prims));
        }
        if !self.objs.is_empty() {
            parts.push(format!("obj{{{}}}", self.objs.join(",")));
        }
        if self.funcs > 0 {
            parts.push(format!("fn*{}", self.funcs));
        }
        if self.builtin_objs > 0 {
            parts.push(format!("builtin*{}", self.builtin_objs));
        }
        if parts.is_empty() {
            "none".into()
        } else {
            parts.join("|")
        }
    }

    /// Kind bits for the contract section: prims plus bit 7 = object.
    pub fn mask(&self) -> u16 {
        if self.top {
            return 0xffff;
        }
        // The section has one Boolean bit, the `true` one's.
        let mut m = self.prims & !prim::FALSE;
        if self.prims & prim::BOOL != 0 {
            m |= prim::TRUE;
        }
        if !self.objs.is_empty() || self.funcs > 0 || self.builtin_objs > 0 {
            m |= 1 << 7;
        }
        m
    }
}

#[derive(Clone, Debug, PartialEq, Hash)]
pub struct FieldFacts {
    pub key: String,
    /// `key` is a class private field's storage key (`Model::private_atom`):
    /// a key apart from the public key of its spelling. A row is named by
    /// both (`FieldFacts::is`).
    pub private: bool,
    pub slot: Option<u32>,
    pub definite: bool,
    /// A declared key (`Template::declared`): `slot` is known, presence is
    /// not, so a read is the load plus the hole compare.
    pub declared: bool,
    /// A constructor-store reservation that is not `definite`
    /// (`Template::reserved`): `slot` is known, presence is not (the slot
    /// holds the hole until the constructor stores the key), so a read is
    /// the load plus the hole compare, and a store adds the key.
    pub reserved: bool,
    pub data_field: bool,
    pub readonly: bool,
    pub writers_ctor: usize,
    pub writers_post: usize,
    pub kind: KindFact,
    /// F6: readonly, holds only typed arrays nobody can expose, detach,
    /// transfer or resize (B1–B4).
    pub ta_stable: bool,
    /// The one element kind (`perry_hir::TYPED_ARRAY_KIND_*`) of every
    /// typed array the field can hold, when all are `TypedArrayNew` owners
    /// of that kind.
    pub ta_kind: Option<u8>,
    /// A definite data field holding only these compiled bodies
    /// (`bindings::bodies_of` over every token of the layout); empty when
    /// it may hold anything else.
    pub fns: Vec<crate::bindings::BodyRef>,
    /// The field's representation (DESIGN-p4l1 §3.1, finding 8): `Arr` of
    /// what the field holds over every token of the template
    /// (`bindings::arr_rep`) when it is definite, a data field, and the
    /// template is neither escaped nor hard (a `delete` or a
    /// `setPrototypeOf` on an instance can make the read `undefined`
    /// without the field node holding it); Boxed otherwise, and with
    /// `PERRY_FACTS_ARRAY_FIELDS=0`. A read of the field is a value of this
    /// representation (PR-C3: codegen's `FieldProof::array`).
    pub rep: crate::bindings::Rep,
    /// −0 tolerance (PR D): one of the field's nodes keeps its int32 interval
    /// although it may hold `-0`, which no use observes (`Node::zs_tol`).
    pub zero_tol: bool,
    /// I32 at rest, PR C (`rest_fields::admit`): the field is an I32 lane of
    /// its class, stored as an I32 word in every instance for its life.
    pub rest: bool,
    /// The field may hold a String, and every String any store of any unit
    /// puts there is the value of a program string literal
    /// (`literal_strings`, over every token of the template): never with an
    /// escaped template or a ⊤ kind.
    pub literal_strings: bool,
}

#[derive(Clone, Debug, PartialEq, Hash)]
pub struct TemplateFacts {
    pub id: TmplId,
    pub display: String,
    pub kind: &'static str,
    pub module: Option<u32>,
    pub class_id: Option<u32>,
    pub escape: Option<String>,
    pub hard: Option<String>,
    pub chain: Option<String>,
    pub prefix_stable: bool,
    pub fixed_layout: bool,
    pub adds_keys: u32,
    pub birth_keys: Vec<String>,
    pub fields: Vec<FieldFacts>,
    /// A class's own statics that hold their definition for the program's
    /// life (FVA 3.4, `constant_statics`), sorted.
    pub statics: Vec<String>,
    /// The class prototype's own members that are one data property for
    /// the program's life (ECMA-262 15.7.14, `constant_proto_members`),
    /// sorted.
    pub proto_consts: Vec<String>,
    pub sabotaged: bool,
    /// Class ids of the parent chain (leaf's parent first).
    pub ancestors: Vec<u32>,
    /// Value records (DESIGN-p4l4 §2, `records`): the template's instances
    /// are their fields by value, or the reason they are heap objects.
    /// `Off` unless `PERRY_FACTS_VALUE_RECORDS=1`.
    pub record: Result<crate::records::RecordLayout, crate::records::Rejected>,
    /// The Arr templates born as record arrays of this template (§1.4, R6):
    /// every holder of their tokens holds them as `Arr{Record}`. The
    /// design's `record_elems` per Arr template, kept on the element
    /// template because an Arr template has no `TemplateFacts`.
    pub record_elems: Vec<TmplId>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Verdict {
    /// `length` of receivers that are each an Array exotic object or a
    /// primitive String: an own data property that is never configurable
    /// (ECMA-262 10.4.2, 10.4.3), so neither a delete, `defineProperty`
    /// nor a prototype makes it an accessor or removes it.
    OwnLength,
    /// A member read or method call whose receivers are all of one builtin
    /// family (primitive Strings, Array exotic objects, collections of one
    /// kind, or RegExp objects), whose intrinsic prototype chain defines the key, while no
    /// receiver holds the key as an own property and the program writes
    /// the key onto no builtin prototype, neither itself nor through ⊤
    /// (`native_member`): the read/call names that intrinsic member.
    /// Interpreted code is the guard word's (`ProgramFacts::interp_guard`),
    /// as for T0.
    NativeMember,
    T0Mono,
    T0Set,
    /// Proven in the typed body behind the entry guard on the receiver
    /// binding (design §11.4); the generic body keeps the site.
    E0,
    /// A read or call no value reaches: the receiver's (a value call's
    /// callee's) node holds no value and no primitive at the fixpoint, and
    /// none of its nodes left a value out (`dropped`). The solve is an
    /// inclusion over-approximation of every evaluation, and a function
    /// unknown code can enter holds ⊤ in its parameters and receiver
    /// (`escape_fn_params`), so an empty node means the expression never
    /// produces a value: the site is never reached (ECMA-262 13.3.2: the
    /// member access evaluates its base first). Names no slot or body.
    Dead,
    /// A named read whose receivers are each a prefix-stable fact unit with
    /// no accessor, no [[Prototype]] write (`super_relink`: no `__proto__`
    /// store, no computed store whose key may be `__proto__`), no escape and
    /// no hard operation, whose prototype chain is proven not to hold the
    /// key (the `absent_key` walk over the chain): OrdinaryGet (ECMA-262
    /// 10.1.8.1) finds the key as an own data property, one of the values
    /// stored under it, or reaches the chain's end and gives `undefined`.
    /// Also over an Array exotic object (`array_absent`) and over String,
    /// Number and Boolean primitives (`prim_absent`: GetV's wrapper, 7.3.2)
    /// whose families' chains lack a key nothing writes there
    /// (`family_absent`); `undefined`/`null` beside the primitives are
    /// named as their TypeError arm (the read's own type dispatch).
    OwnDataOrAbsent,
    /// A member read or method call whose receiver node holds only
    /// `undefined` and/or `null` (no object token but closed markers, no ⊤,
    /// no other primitive) and left no value out (`dropped`): evaluating the
    /// member access evaluates its base, then GetValue's ToObject throws a
    /// TypeError before any property lookup (ECMA-262 13.3.2.1, 6.2.5.5,
    /// 7.1.18; for a call 13.3.6.1 throws before the arguments are
    /// evaluated). The site runs (unlike Dead) and names no slot or body.
    Throws,
    /// A value call whose callee set is exactly one builtin function object
    /// (one `BuiltinFn` token, nothing else, no ⊤, no primitive): the call
    /// is that intrinsic's [[Call]] (ECMA-262 7.3.13 Call, 10.3.1). A program
    /// store of another value onto the member it was read from adds that
    /// value to the callee node. Only while no unknown code exists
    /// (`!realm_held`): unknown code holds the realm (9.3) and may replace
    /// the member (the intrinsics' members are writable, 10.4.1 / 17), a
    /// write the solve does not replay into the read (`realm_hold_tok`).
    /// Whole-program solves only. `receivers` names the intrinsic.
    IntrinsicCall,
    /// A named read whose receivers are all module namespace exotic objects
    /// of the program's own modules, under a name each module exports
    /// itself (`ns_own`; no `export *` resolution): [[Get]] gives the export
    /// binding's current value, `GetBindingValue` of the module's binding
    /// (ECMA-262 10.4.6.8). No code writes a namespace ([[Set]] 10.4.6.9 and
    /// [[DefineOwnProperty]] 10.4.6.6 change nothing, [[Prototype]] is null
    /// and fixed, 10.4.6.1-2), unknown code holding it included: exact under
    /// a held realm. The claim is the binding, not a value: an `export let`
    /// its module reassigns is read live.
    NsExport,
    No(String),
}

#[derive(Clone, Debug)]
pub struct SiteFacts {
    /// Observational solve identity. Never read by a proof or digest.
    pub(crate) diagnostic_unit: Option<String>,
    /// Observational: per function copy holding the site, its verdict.
    pub(crate) diagnostic_copies: Vec<(String, Verdict)>,
    pub module: u32,
    pub byte_offset: u32,
    pub key: String,
    pub is_call: bool,
    pub receivers: Vec<String>,
    /// The unguarded solved values, before a verdict's entry-guard refinement.
    pub receiver_values: Vec<ReceiverValue>,
    pub receiver_prims: u16,
    pub top: bool,
    pub verdict: Verdict,
    pub fixed_shape: bool,
    /// A call site: the bodies its callee set holds (every body, sorted;
    /// empty when the set holds anything that is not a body).
    pub targets: Vec<String>,
    /// The HIR identity `(module, function id)` of each body in `targets`,
    /// sorted (`field_read_calls`).
    pub(crate) target_fns: Vec<(u32, u32)>,
    /// Per-unit run: the receiver is a parameter whose TS object type
    /// names these keys; its verdict is decided after the census
    /// (`units::typed_param_verdicts`).
    pub(crate) guard_keys: Option<Vec<String>>,
    /// DECISIONS 225: the bodies holding the site, in its module (HIR
    /// function id, and for a synthetic context its name; `Model::act_key`).
    pub bodies: Vec<(u32, String)>,
}

/// `Some` (a reason a claim fails) absorbs `None`; of two reasons the
/// smaller is kept, so the join is commutative and only descends among
/// the finitely many reasons.
fn join_reason(a: &mut Option<String>, b: &Option<String>) {
    match (a.as_mut(), b) {
        (_, None) => {}
        (None, Some(y)) => *a = Some(y.clone()),
        (Some(x), Some(y)) => {
            if y < x {
                *x = y.clone();
            }
        }
    }
}

/// The common prefix of two lists.
fn common_prefix<T: PartialEq>(a: &mut Vec<T>, b: &[T]) {
    let n = a.iter().zip(b).take_while(|(x, y)| x == y).count();
    a.truncate(n);
}

impl KindFact {
    /// The join: what a field may hold, in either.
    pub(crate) fn join(&mut self, o: &KindFact) {
        self.prims |= o.prims;
        for x in &o.objs {
            if !self.objs.contains(x) {
                self.objs.push(x.clone());
            }
        }
        self.objs.sort();
        self.funcs = self.funcs.max(o.funcs);
        self.builtin_objs = self.builtin_objs.max(o.builtin_objs);
        self.top |= o.top;
    }
}

impl FieldFacts {
    /// This row describes key `key`, private (`Model::private_atom`) or
    /// not: a public key spelled like a private storage key is another key.
    pub fn is(&self, key: &str, private: bool) -> bool {
        self.key == key && self.private == private
    }

    /// The join of two descriptions of one field (by `key`): a claim holds
    /// only if both make it; a slot or element kind they disagree on is
    /// none; the writers and the kind grow.
    pub(crate) fn join(&mut self, o: &FieldFacts) {
        if self.slot != o.slot {
            self.slot = None;
        }
        self.definite &= o.definite;
        self.declared &= o.declared;
        // A known slot whose presence one of them does not know.
        self.reserved = (self.definite || self.reserved)
            && (o.definite || o.reserved)
            && !(self.definite && o.definite);
        self.definite &= o.definite;
        self.data_field &= o.data_field;
        self.readonly &= o.readonly;
        self.writers_ctor = self.writers_ctor.max(o.writers_ctor);
        self.writers_post = self.writers_post.max(o.writers_post);
        self.kind.join(&o.kind);
        self.literal_strings &= o.literal_strings;
        self.ta_stable &= o.ta_stable;
        // The join, not the meet: the row must hold for either class.
        self.rep = crate::bindings::Rep::join(self.rep, o.rep);
        if self.ta_kind != o.ta_kind {
            self.ta_kind = None;
        }
        // Each says the field holds only its bodies: both, their union.
        if self.fns.is_empty() || o.fns.is_empty() {
            self.fns.clear();
        } else {
            self.fns.extend(o.fns.iter().cloned());
            self.fns.sort();
            self.fns.dedup();
        }
    }
}

impl TemplateFacts {
    /// The join of two descriptions of one template (by `display`; its id,
    /// kind, module and class id are its identity): a claim holds only if
    /// both make it. Escape, HARD and chain problems absorb; stability and
    /// a fixed layout need both; birth keys and ancestors keep their common
    /// prefix; a field only one has is not definite in the join.
    pub(crate) fn join(&mut self, o: &TemplateFacts) {
        join_reason(&mut self.escape, &o.escape);
        join_reason(&mut self.hard, &o.hard);
        join_reason(&mut self.chain, &o.chain);
        self.prefix_stable &= o.prefix_stable;
        self.fixed_layout &= o.fixed_layout;
        self.adds_keys = self.adds_keys.max(o.adds_keys);
        common_prefix(&mut self.birth_keys, &o.birth_keys);
        common_prefix(&mut self.ancestors, &o.ancestors);
        self.statics.retain(|k| o.statics.contains(k));
        self.proto_consts.retain(|k| o.proto_consts.contains(k));
        self.sabotaged |= o.sabotaged;
        for f in self.fields.iter_mut() {
            match o.fields.iter().find(|g| g.is(&f.key, f.private)) {
                Some(g) => f.join(g),
                None => {
                    f.definite = false;
                    f.reserved = false;
                }
            }
        }
        for g in &o.fields {
            if !self.fields.iter().any(|f| f.is(&g.key, g.private)) {
                let mut g = g.clone();
                g.definite = false;
                g.reserved = false;
                self.fields.push(g);
            }
        }
    }
}

pub fn derive(g: &mut Gen) -> (Vec<TemplateFacts>, Vec<SiteFacts>) {
    let m = &mut g.m;
    // A program-wide kill only for what the compiled program itself does:
    // an accessor it installs on `Object.prototype` (E23, any key). Dynamic
    // code is not a kill (decision 150): its data flow is ⊤, and what it can
    // do to the intrinsic prototypes is the runtime's guard word
    // (`ProgramFacts::interp_guard`).
    let global = m
        .objproto_accessor
        .then(|| "E23 accessor installed on Object.prototype".to_string());
    let objproto = m.tok(TokKind::ObjectProto);
    let objproto_hard = m
        .hard
        .get(&objproto)
        .map(|(k, r)| format!("{} {}", k.code(), m.reasons[*r as usize]));
    let mut out = Vec::new();
    for tm in 0..m.tmpls.len() as TmplId {
        if !m.tmpls[tm as usize].kind.is_fact_unit() {
            continue;
        }
        // Another unit's class: the facts its defining unit published; what
        // this unit does beyond the contract is its census report (§11.3).
        if let Some(f) = m.foreign.get(&tm) {
            out.push(crate::TemplateFacts {
                id: tm,
                ..f.clone()
            });
            continue;
        }
        let tf = template_facts(m, tm, &global, &objproto_hard);
        out.push(tf);
    }
    let sites = site_facts(g, &out);
    let foreign = &g.m.foreign;
    out.retain(|t| !foreign.contains_key(&t.id));
    (out, sites)
}

/// FVA 3.4: the class object's own statics whose read is the one load of
/// the property the class's evaluation created. The class object reaches no
/// unknown code (which could write or delete them), carries no HARD
/// operation (defineProperty, delete), has no computed static member, and
/// the key is no accessor. A static method (or `prototype`) is its
/// declaration for the program's life: no store of the key on the class
/// object exists (`Model::class_writers`: an instance field of that name is
/// another object's property), so a call of it is the declaration's body. A declared static field is
/// one load whatever stores it later: they widen its value set, which every
/// reader already takes (the HIR stores the definition where the class is
/// evaluated, once or more for a nested class).
fn constant_statics(m: &Model, tm: TmplId) -> Vec<String> {
    // Rule C (DECISIONS 251): each evaluation is its own class object; the
    // template's statics are no one evaluation's.
    if m.tmpls[tm as usize].per_evaluation {
        return Vec::new();
    }
    let Some(&cobj) = m.tok_index.get(&TokKind::ClassObj(tm)) else {
        return Vec::new();
    };
    let holds = |n: NodeId| {
        let nd = &m.nodes[n as usize];
        !nd.pts.is_empty() || nd.prim != 0
    };
    if m.is_escaped(cobj)
        || m.hard.contains_key(&cobj)
        || m.field_any.get(&cobj).is_some_and(|&n| holds(n))
    {
        return Vec::new();
    }
    let accessors = m.accessor_keys.get(&cobj);
    let mut out: Vec<String> = Vec::new();
    for &a in m.field_keys.get(&cobj).into_iter().flatten() {
        if accessors.is_some_and(|ks| ks.contains(&a)) {
            continue;
        }
        if !m.fields.get(&(cobj, a)).is_some_and(|&n| holds(n)) {
            continue;
        }
        let stored = m
            .class_writers
            .get(&(tm, a))
            .is_some_and(|w| !w.ctor.is_empty() || !w.post.is_empty());
        if m.static_fields.contains(&(tm, a)) || !stored {
            out.push(m.atoms[a as usize].clone());
        }
    }
    out.sort();
    out.dedup();
    out
}

/// ECMA-262 15.7.14 ClassDefinitionEvaluation: the prototype object of a
/// class is created with each method of the class body (15.4.4
/// DefineMethodProperty) and `constructor` (MakeConstructor) as own,
/// writable, non-enumerable data properties, before any code can reach the
/// object. The prototype's own members whose read is one own data lookup
/// for the program's life: the prototype reaches no unknown code (which
/// could redefine or delete them), carries no HARD operation
/// (defineProperty, delete), takes no computed-key store and has no
/// computed member (either may name the key), and the key is no accessor.
/// A program store of the key is an ordinary [[Set]] on an own writable
/// data property (10.1.9.2 OrdinarySetWithOwnDescriptor): the key stays a
/// data property and the store's value joins the field node the read takes.
/// An object literal's layout is no class (decision 17); each evaluation of
/// a per-evaluation class is its own prototype (Rule C).
fn constant_proto_members(m: &Model, tm: TmplId) -> Vec<String> {
    let tpl = &m.tmpls[tm as usize];
    if tpl.per_evaluation
        || tpl.has_computed_members
        || !matches!(tpl.kind, TmplKind::Class { literal_shape: false, .. })
    {
        return Vec::new();
    }
    let Some(&proto) = m.tok_index.get(&TokKind::Proto(tm)) else {
        return Vec::new();
    };
    let holds = |n: NodeId| {
        let nd = &m.nodes[n as usize];
        !nd.pts.is_empty() || nd.prim != 0
    };
    if m.is_escaped(proto)
        || m.hard.contains_key(&proto)
        || m.field_any.get(&proto).is_some_and(|&n| holds(n))
    {
        return Vec::new();
    }
    let accessors = m.accessor_keys.get(&proto);
    let mut out: Vec<String> = Vec::new();
    for &a in &tpl.proto_members {
        if tpl.accessor_keys.contains(&a) || accessors.is_some_and(|ks| ks.contains(&a)) {
            continue;
        }
        if m.fields.get(&(proto, a)).is_some_and(|&n| holds(n)) {
            out.push(m.atoms[a as usize].clone());
        }
    }
    out.sort();
    out.dedup();
    out
}

/// The static prototype chain tokens of a template (plus every prototype
/// object a `new f()` / literal `__proto__` can name).
fn chain_tokens(m: &Model, tm: TmplId) -> Vec<Tok> {
    let mut seen: FxHashSet<Tok> = FxHashSet::default();
    let mut stack = Vec::new();
    stack.extend(m.tmpl_toks(tm));
    let mut out = Vec::new();
    while let Some(t) = stack.pop() {
        let Some(&pn) = m.proto_node.get(&t) else {
            continue;
        };
        for p in m.nodes[pn as usize].pts.to_sorted() {
            if seen.insert(p) {
                out.push(p);
                stack.push(p);
            }
        }
    }
    out
}

/// DEC 250 (Q2): every object of template `tm` has a chain on which the
/// collapsed `ObjectProto` token is `%Object.prototype%` and nothing else:
/// a class (a literal shape or a declared class) whose prototypes are
/// program class prototypes with no native parent and no `extends`
/// expression, ending at the static seed. A literal (`__proto__` member),
/// `Object.create(p)`, `new f()` (`f.prototype`) or a [[Prototype]] write
/// (`super_relink`) can give the token any builtin prototype, add-open ones
/// (`Map.prototype`, `Array.prototype` under a name) included.
fn plain_chain(m: &Model, tm: TmplId) -> bool {
    if !matches!(m.tmpls[tm as usize].kind, TmplKind::Class { .. }) {
        return false;
    }
    if m.tmpl_toks(tm).iter().any(|t| m.super_relink.contains(t)) {
        return false;
    }
    chain_tokens(m, tm)
        .iter()
        .all(|&p| match &m.toks[p as usize] {
            TokKind::ObjectProto => true,
            TokKind::Proto(c) => {
                let c = &m.tmpls[*c as usize];
                c.native_parent.is_none() && c.parent_value.is_none()
            }
            _ => false,
        })
}

/// RPG1b: per key of template `tm` (`keys`, birth keys first): is it a
/// declared key every add of which runs before any later key can be
/// present? A declared key is born absent (its birth store writes the hole)
/// and added by its post writers only. It is then in enumeration order (the
/// runtime fills its slot in place, `later_late_key_present`) when every
/// post writer of it runs before every writer of every later key
/// ([`ordered_writer`], [`evaluated_before`]). A later key that construction
/// defines or that an unordered writer adds is present at no order this
/// knows. A private field is a birth key at its declaration (#8969) that
/// construction defines (its definition is a birth store of a key that is
/// not declared), so it is such a key: never in order by this rule, and it
/// ends the pass for every earlier key. The caller has checked that no
/// computed key is written.
///
/// One pass from the last key: the later keys' writers are summarized as
/// the once bodies that hold them (the first site of each) and their
/// modules.
fn ordered_declared_adds(m: &mut Model, tm: TmplId, keys: &[Atom]) -> Vec<bool> {
    let mut out = vec![false; keys.len()];
    let tpl = &m.tmpls[tm as usize];
    let birth_len = tpl.birth_keys.len();
    let declared: crate::pts::FxHashSet<Atom> = tpl.declared.iter().copied().collect();
    let cache_stable = cjs_cache_stable(m);
    let empty = FieldWriters::default();
    // The later keys' writers: per once body, its module and first site;
    // the modules holding them, with how many bodies each.
    let mut later_fns: FxHashMap<FnIdx, (u32, SiteId)> = FxHashMap::default();
    let mut later_mods: FxHashMap<u32, u32> = FxHashMap::default();
    let mut firsts: Vec<(FnIdx, u32, SiteId)> = Vec::new();
    for i in (0..keys.len()).rev() {
        let a = keys[i];
        let w = m.writers.get(&(tm, a)).unwrap_or(&empty).clone();
        firsts.clear();
        let mut own_ordered = true;
        for &s in &w.post {
            match ordered_writer(m, s, cache_stable) {
                Some((f, md, false)) => firsts.push((f, md, s)),
                _ => own_ordered = false,
            }
        }
        if i < birth_len && declared.contains(&a) && own_ordered {
            out[i] = firsts.iter().all(|&(f1, m1, s1)| {
                let own_body = match later_fns.get(&f1) {
                    Some(&(_, min)) => min > s1 && later_mods.get(&m1) == Some(&1),
                    None => !later_mods.contains_key(&m1),
                };
                own_body
                    && later_mods
                        .keys()
                        .filter(|&&md| md != m1)
                        .all(|&md| evaluated_before(m, m1, md))
            });
        }
        // Key `i` joins the later keys: unless construction defines it (a
        // non-hole birth store, a private field among them), or a
        // constructor body adds it.
        let hole_births = w
            .ctor
            .iter()
            .all(|&s| m.stores[s as usize].birth && declared.contains(&a));
        if !hole_births {
            break;
        }
        for &s in &w.post {
            let Some((f, md, _)) = ordered_writer(m, s, cache_stable) else {
                return out;
            };
            match later_fns.get_mut(&f) {
                Some((_, min)) => *min = (*min).min(s),
                None => {
                    later_fns.insert(f, (md, s));
                    *later_mods.entry(md).or_default() += 1;
                }
            }
        }
    }
    out
}

/// Where an ordered writer runs: its function, which runs at most once per
/// evaluation of a module and only while it evaluates (`Model::once_fns`),
/// that module, and whether it is inside a loop of the function
/// (`StoreSite::in_loop`). A body the runtime's path registry may run again
/// is once only while the program's `require.cache` has no deletion: the
/// registry evaluates a module again only after its entry was deleted.
fn ordered_writer(m: &Model, s: SiteId, cache_stable: bool) -> Option<(FnIdx, u32, bool)> {
    let st = m.stores.get(s as usize)?;
    let in_loop = st.in_loop?;
    let &(module, reload) = m.once_fns.get(&st.func)?;
    (!reload || cache_stable).then_some((st.func, module, in_loop))
}

/// Module `a`'s evaluation completes before module `b`'s starts: `b`
/// reaches `a` through static evaluation dependencies (`Model::eval_deps`)
/// and `a` does not reach `b` (in a cycle the order is the walk's). Each
/// module's reach is computed once, when first asked (`Model::eval_reach`).
fn evaluated_before(m: &mut Model, a: u32, b: u32) -> bool {
    a != b && module_reaches(m, b, a) && !module_reaches(m, a, b)
}

fn module_reaches(m: &mut Model, from: u32, to: u32) -> bool {
    if !m.eval_reach.contains_key(&from) {
        let n = m.eval_deps.len();
        let mut bits = vec![0u64; n.div_ceil(64)];
        let mut work = vec![from];
        while let Some(x) = work.pop() {
            for &d in m
                .eval_deps
                .get(x as usize)
                .map_or(&[][..], |v| v.as_slice())
            {
                let (w, b) = (d as usize / 64, 1u64 << (d % 64));
                if bits[w] & b == 0 {
                    bits[w] |= b;
                    work.push(d);
                }
            }
        }
        m.eval_reach.insert(from, bits);
    }
    m.eval_reach[&from]
        .get(to as usize / 64)
        .is_some_and(|w| w & (1u64 << (to % 64)) != 0)
}

/// The program's `require.cache` has no deletion and no unknown holder: no
/// token of it is ⊤, HARD (a `delete` marks it) or escaped.
fn cjs_cache_stable(m: &Model) -> bool {
    m.nodes[m.cjs_cache as usize]
        .pts
        .to_sorted()
        .into_iter()
        .all(|t| {
            t != TOP
                && m.tmpl_of(t).is_none_or(|tm| {
                    m.tmpl_toks(tm)
                        .iter()
                        .chain(std::iter::once(&t))
                        .all(|x| !m.hard.contains_key(x) && !m.escaped.contains_key(x))
                })
        })
}

/// The native root of `tm` (DEC 251 flip regression A): the first class
/// along its linked chain (`tm`, then `tmpls[].parent`) whose instance a
/// native constructor births: a native parent without a summary, or an
/// ancestor whose evaluated `extends` is not one class (E11, e.g. `class Own
/// extends RegExp`). Every instance of the chain is then that native exotic
/// object, never a class layout, so no template below the root is
/// fixed-layout or prefix-stable.
fn native_root(m: &Model, tm: TmplId) -> Option<String> {
    let name = |c: TmplId| {
        m.tok_index
            .get(&TokKind::Obj(c, NO_CTX))
            .map_or_else(|| format!("#{c}"), |&t| m.tok_name(t))
    };
    let mut cur = Some(tm);
    for _ in 0..=m.tmpls.len() {
        let c = cur?;
        let tpl = &m.tmpls[c as usize];
        if let Some((_, n)) = &tpl.native_parent {
            if !crate::builtins::native_parent_summarized(n) {
                return Some(if c == tm {
                    "E21 native parent without a summary".into()
                } else {
                    format!(
                        "E21 native parent without a summary on ancestor {}",
                        name(c)
                    )
                });
            }
        }
        if c != tm
            && m.tmpl_toks(c)
                .iter()
                .filter_map(|t| m.hard.get(t))
                .any(|(k, _)| matches!(k, Hard::E11ReflectConstruct))
        {
            return Some(format!(
                "E21 native parent: ancestor {} is born by a native constructor (E11)",
                name(c)
            ));
        }
        cur = tpl.parent;
    }
    Some("E21 native parent: cyclic parent chain".into())
}

fn template_facts(
    m: &mut Model,
    tm: TmplId,
    global: &Option<String>,
    objproto_hard: &Option<String>,
) -> TemplateFacts {
    // The template's own token and every birth-site token: one layout.
    m.tok(TokKind::Obj(tm, NO_CTX));
    let ts = m.tmpl_toks(tm);
    let tpl = m.tmpls[tm as usize].clone();
    let escape = ts
        .iter()
        .find_map(|t| m.escaped.get(t))
        .map(|r| m.reasons[*r as usize].clone());
    let hard = ts
        .iter()
        .find_map(|t| m.hard.get(t))
        .map(|(k, r)| format!("{} {}", k.code(), m.reasons[*r as usize]))
        .or_else(|| {
            ts.iter().find_map(|&t| layout_exit(m, tm, t)).map(|a| {
                format!(
                    "accessor defined on the layout key {} (layout exit)",
                    m.atoms[a as usize]
                )
            })
        });
    // The prototype chain must be stable: no HARD op or escape on any
    // prototype object, no accessor named after a birth key, no computed
    // accessor anywhere.
    let chain = chain_tokens(m, tm);
    let mut chain_problem: Option<String> = None;
    // DECISIONS 242(2,3): an accessor under any key on `Object.prototype`
    // reaches an object only through a key it does not own definitely
    // (`Template::chain_miss`), or through keys made by [[Set]] (a
    // constructor's `this.k =`). A literal's birth keys are defined
    // (CreateDataProperty): no prototype accessor runs for them.
    let star = m.atom(crate::STAR_KEY);
    let set_born = match &tpl.kind {
        TmplKind::Lit { .. }
        | TmplKind::Class {
            literal_shape: true,
            ..
        } => false,
        _ => !tpl.birth_keys.is_empty(),
    };
    // DEC 250 (Q2): where the chain's collapsed prototype token is
    // `Object.prototype` only, an accessor under any key there stands under
    // `constructor` (the one key a program may define on it): only an open
    // miss, or a [[Set]]-born `constructor`, reaches it.
    let chain_miss = if plain_chain(m, tm) {
        tpl.chain_miss_open
            || set_born
                && tpl
                    .birth_keys
                    .iter()
                    .any(|&k| !crate::builtins::object_final_read(&m.atoms[k as usize]))
    } else {
        tpl.chain_miss || set_born
    };
    let objproto_tok = m.tok_index.get(&TokKind::ObjectProto).copied();
    let mut accessor_keys: Vec<Atom> = tpl.accessor_keys.clone();
    for t in &ts {
        if let Some(ks) = m.accessor_keys.get(t) {
            accessor_keys.extend(ks.iter().copied());
        }
    }
    for &p in &chain {
        if p == TOP {
            chain_problem.get_or_insert_with(|| "prototype from unmodeled code".into());
            continue;
        }
        if let Some((k, r)) = m.hard.get(&p) {
            chain_problem.get_or_insert_with(|| {
                format!(
                    "prototype {} {} {}",
                    m.tok_name(p),
                    k.code(),
                    m.reasons[*r as usize]
                )
            });
        }
        if p != m.tok_index[&TokKind::ObjectProto] {
            if let Some(r) = m.escaped.get(&p) {
                chain_problem.get_or_insert_with(|| {
                    format!(
                        "prototype {} escapes: {}",
                        m.tok_name(p),
                        m.reasons[*r as usize]
                    )
                });
            }
        }
        if let Some(ks) = m.accessor_keys.get(&p) {
            let unreached = Some(p) == objproto_tok && !chain_miss;
            accessor_keys.extend(ks.iter().copied().filter(|&k| !(unreached && k == star)));
        }
    }
    if let Some(r) = objproto_hard {
        chain_problem.get_or_insert_with(|| format!("Object.prototype {r}"));
    }
    if let Some(r) = global {
        chain_problem.get_or_insert_with(|| r.clone());
    }
    // This obligation affects every receiver sharing the chain, even when
    // a site's own token has not escaped (DEC220).
    if accessor_keys.contains(&star) {
        chain_problem.get_or_insert_with(|| "accessor under a computed key on the chain".into());
    }
    let escape = if accessor_keys.contains(&star) && escape.is_none() {
        // A computed-key accessor on the chain runs as unknown code with
        // the receiver: treat the template as escaped (kills readonly and
        // kinds, not just the layout facts).
        Some("accessor under a computed key on the chain".to_string())
    } else {
        escape
    };
    if tpl.has_computed_members
        && tpl.birth_keys.is_empty()
        && !matches!(tpl.kind, TmplKind::FnInst { .. })
    {
        chain_problem.get_or_insert_with(|| "computed accessor/layout on the chain".into());
    }
    if let Some(r) = native_root(m, tm) {
        chain_problem.get_or_insert(r);
    }
    let escape = global.clone().or(escape);
    let prefix_stable = escape.is_none() && hard.is_none() && chain_problem.is_none();
    // A named store the solve counted as a key add while the class had only
    // its own birth keys is no add when the settled layout (the parent unit's
    // class as its prefix) holds the key from birth.
    let settled_stores: u32 = m
        .adds_named
        .iter()
        .filter(|((t, a), _)| *t == tm && tpl.birth_keys.contains(a))
        .map(|(_, n)| *n)
        .sum();
    let adds = m
        .adds_keys
        .get(&tm)
        .copied()
        .unwrap_or(0)
        .saturating_sub(settled_stores);
    // Slots: birth order, valid up to the first non-definite / accessor key.
    let mut fields = Vec::new();
    let mut slot_ok = true;
    // A private field's entry is a birth key at its declaration
    // (`compute_birth_keys`, #8969): present wherever a read finds a value (a
    // read before the claim throws the brand check).
    let mut keys: Vec<Atom> = tpl.birth_keys.clone();
    // Fields written later (non-birth) are reported too.
    for t in &ts {
        if let Some(fk) = m.field_keys.get(t) {
            for &a in fk {
                if !keys.contains(&a) {
                    keys.push(a);
                }
            }
        }
    }
    let mut fixed_layout = prefix_stable && adds == 0;
    // Who can make each key present: a writer that runs after construction
    // (or one of a computed key, which can be any key), and any writer at
    // all.
    let any = m.any_writers.get(&tm).cloned().unwrap_or_default();
    let any_key_written = !any.ctor.is_empty() || !any.post.is_empty();
    let written: Vec<(bool, bool)> = keys
        .iter()
        .map(|&a| {
            let w = m.writers.get(&(tm, a));
            let post = w.is_some_and(|w| !w.post.is_empty());
            let ever = post || w.is_some_and(|w| !w.ctor.is_empty());
            (post, ever)
        })
        .collect();
    // RPG1b: a declared key whose every add runs before any later key can be
    // present (`ordered_declared_adds`): the h4 rule below cannot see write
    // order.
    let ordered = if any_key_written || tpl.declared.is_empty() {
        vec![false; keys.len()]
    } else {
        ordered_declared_adds(m, tm, &keys)
    };
    // A key a birth store defines (a field definition): it is added at its
    // place in the chain's definition order, and a constructor body's
    // stores run after its class's definitions.
    let defined = |a: Atom| {
        m.writers
            .get(&(tm, a))
            .is_some_and(|w| w.ctor.iter().any(|&s| m.stores[s as usize].birth))
    };
    for (i, &a) in keys.iter().enumerate() {
        let birth = i < tpl.birth_keys.len();
        let is_accessor = accessor_keys.contains(&a);
        // A class evaluated more than once (`Expr::ClassExprFresh`) is
        // constructed through its class object: the runtime allocates the
        // declared width empty and appends each key when it is stored, so
        // its birth keys are reserved, never owned at birth. A key the
        // construction always adds keeps the template's order; a later key
        // keeps its slot only while every earlier one is such a key.
        let added = birth && tpl.definite.contains(&a) && !is_accessor;
        let owned = birth && !tpl.per_evaluation && !is_accessor;
        let definite = owned && tpl.definite.contains(&a);
        let declared = owned && tpl.declared.contains(&a);
        let reserved =
            birth && (tpl.per_evaluation || tpl.reserved.contains(&a)) && !definite && !is_accessor;
        if birth && !definite && !declared && !reserved {
            slot_ok = false;
            fixed_layout = false;
        }
        // A reserved key keeps its slot, but an instance can be observed
        // while the slot still holds the hole: its presence is not the
        // complete birth layout's, so code compiled for that layout (the
        // `$pshape` clones) keeps its keys recheck.
        if reserved {
            fixed_layout = false;
        }
        // A key that may be absent while the instance is observable keeps
        // its birth slot only when no later key can be present by the time
        // it is added: the runtime then appends it after them, in
        // enumeration order (`later_late_key_present`), and its birth slot
        // stays the hole for life. A defined key comes after every later
        // key that only construction writes; any other such key can be
        // added at any time, after any later key that is ever written.
        let in_order = added || ordered[i] || {
            let def = defined(a);
            !any_key_written
                && written[i + 1..]
                    .iter()
                    .all(|&(post, ever)| if def { !post } else { !ever })
        };
        if !in_order {
            fixed_layout = false;
        }
        let slot = (birth && (definite || declared || reserved) && slot_ok && in_order)
            .then_some(i as u32);
        if tpl.per_evaluation && birth && !added {
            slot_ok = false;
        }
        let w = m.writers.get(&(tm, a)).cloned().unwrap_or_default();
        let readonly = escape.is_none()
            && hard.is_none()
            && w.post.is_empty()
            && any.post.is_empty()
            && !is_accessor;
        let mut kind = field_kind(m, &ts, a, definite);
        if escape.is_some() {
            kind.top = true;
        }
        let ta_stable = readonly && definite && ta_only_sealed(m, &ts, a);
        let ta_kind = if ta_stable {
            ta_one_kind(m, &ts, a)
        } else {
            None
        };
        let fns = if slot.is_some() && !is_accessor && escape.is_none() {
            field_bodies(m, &ts, a)
        } else {
            Vec::new()
        };
        let whole_life =
            m.array_fields && definite && !is_accessor && escape.is_none() && hard.is_none();
        let rep = if whole_life {
            // The field nodes of the tokens that have one, as `field_kind`
            // reads them.
            let nodes: Vec<NodeId> = ts
                .iter()
                .filter_map(|t| m.fields.get(&(*t, a)).copied())
                .collect();
            let ignored = if m.arr_field_sabotage {
                prim::NULL | prim::UNDEF
            } else {
                0
            };
            crate::bindings::arr_rep_beside(m, &nodes, ignored)
                .map_or(crate::bindings::Rep::Boxed, crate::bindings::Rep::Arr)
        } else {
            crate::bindings::Rep::Boxed
        };
        // I32 at rest (RFC §10.7, PR A): a whole-life field that holds only
        // Numbers has the Number representation of its joined nodes.
        let mut zero_tol = false;
        let rep = if rep == crate::bindings::Rep::Boxed
            && definite
            && !is_accessor
            && escape.is_none()
            && hard.is_none()
            && crate::demand::at_rest_demand()
        {
            let nodes: Vec<NodeId> = ts
                .iter()
                .filter_map(|t| m.fields.get(&(*t, a)).copied())
                .collect();
            zero_tol = nodes.iter().any(|&n| m.nodes[n as usize].zs_tol);
            crate::bindings::number_rep_of_nodes(m, &nodes)
        } else {
            rep
        };
        let literal_strings = !kind.top && !is_accessor && {
            let nodes: Vec<NodeId> = ts
                .iter()
                .filter_map(|t| m.fields.get(&(*t, a)).copied())
                .collect();
            crate::literal_strings::holds(m, &nodes)
        };
        fields.push(FieldFacts {
            rep,
            zero_tol,
            rest: false,
            literal_strings,
            fns,
            ta_stable,
            ta_kind,
            key: m.atoms[a as usize].clone(),
            private: m.is_private_atom(a),
            slot,
            definite,
            declared,
            reserved,
            data_field: !is_accessor,
            readonly,
            writers_ctor: w.ctor.len(),
            writers_post: w.post.len() + any.post.len(),
            kind,
        });
    }
    let (kind, module, class_id) = match &tpl.kind {
        TmplKind::Class {
            module,
            class_id,
            literal_shape,
            ..
        } => (
            if *literal_shape { "litclass" } else { "class" },
            Some(*module),
            Some(*class_id),
        ),
        TmplKind::Lit { module, .. } => ("lit", Some(*module), None),
        TmplKind::FnInst { func } => ("fninst", Some(m.funcs[*func as usize].module), None),
        _ => ("other", None, None),
    };
    TemplateFacts {
        id: tm,
        display: tpl.display.clone(),
        kind,
        module,
        class_id,
        escape,
        hard,
        chain: chain_problem,
        prefix_stable,
        fixed_layout,
        adds_keys: adds,
        birth_keys: tpl
            .birth_keys
            .iter()
            .map(|a| m.atoms[*a as usize].clone())
            .collect(),
        fields,
        statics: constant_statics(m, tm),
        proto_consts: constant_proto_members(m, tm),
        sabotaged: false,
        record: Err(crate::records::Rejected::off()),
        record_elems: Vec::new(),
        ancestors: {
            let mut v = Vec::new();
            let mut cur = tpl.parent;
            while let Some(p) = cur {
                if let TmplKind::Class { class_id, .. } = m.tmpls[p as usize].kind {
                    v.push(class_id);
                }
                cur = m.tmpls[p as usize].parent;
            }
            v
        },
    }
}

/// An accessor defined on a key the object's layout holds leaves the layout.
/// A literal's layout holds its birth keys and every key a program store
/// writes (an optional slot, the hole until the store runs); the runtime
/// gives an object that is defined an accessor under such a key its
/// overflow, and it leaves its layout for good (class id 0:
/// `layout_desc::prepare_hole_define`, `object_ops::define_property`), so no
/// layout claim of the template survives it. (An accessor on a birth key is
/// E5 HARD at the definition already: `Cons::DefineKey`.)
fn layout_exit(m: &Model, tm: TmplId, t: Tok) -> Option<Atom> {
    m.accessor_keys.get(&t)?.iter().copied().find(|&a| {
        m.writers
            .get(&(tm, a))
            .is_some_and(|w| !w.ctor.is_empty() || !w.post.is_empty())
    })
}

fn field_bodies(m: &Model, ts: &[Tok], a: Atom) -> Vec<crate::bindings::BodyRef> {
    let mut out = Vec::new();
    for &t in ts {
        let Some(&n) = m.fields.get(&(t, a)) else {
            continue;
        };
        let nd = &m.nodes[n as usize];
        if nd.pts.is_empty() && nd.prim == 0 {
            continue;
        }
        let b = crate::bindings::bodies_of(m, n);
        if b.is_empty() {
            return Vec::new();
        }
        out.extend(b);
    }
    out.sort();
    out.dedup();
    out
}

fn ta_only_sealed(m: &Model, ts: &[Tok], a: Atom) -> bool {
    let ns: Vec<NodeId> = ts
        .iter()
        .filter_map(|&t| m.fields.get(&(t, a)).copied())
        .collect();
    !ns.is_empty()
        && ns.iter().all(|&n| {
            let node = &m.nodes[n as usize];
            node.prim & !prim::UNDEF == 0 && {
                let toks = node.pts.to_sorted();
                !toks.is_empty() && toks.iter().all(|&x| ta_sealed_tok(m, x))
            }
        })
}

fn ta_sealed_tok(m: &Model, x: Tok) -> bool {
    matches!(m.toks[x as usize], TokKind::Obj(tm, _)
        if m.tmpls[tm as usize].kind.typed_storage())
        && !m.hard.contains_key(&x)
        && !m.escaped.contains_key(&x)
}

/// The single recorded element kind of the typed arrays in field `a` of `t`.
fn ta_one_kind(m: &Model, ts: &[Tok], a: Atom) -> Option<u8> {
    let mut kind = None;
    for &t in ts {
        let Some(&n) = m.fields.get(&(t, a)) else {
            continue;
        };
        for x in m.nodes[n as usize].pts.to_sorted() {
            let TokKind::Obj(tm, _) = m.toks[x as usize] else {
                return None;
            };
            let k = *m.ta_elem.get(&tm)?;
            if kind.is_some_and(|c| c != k) {
                return None;
            }
            kind = Some(k);
        }
    }
    kind
}

fn field_kind(m: &Model, ts: &[Tok], a: Atom, definite: bool) -> KindFact {
    let mut k = KindFact {
        prims: if definite { 0 } else { prim::UNDEF },
        objs: Vec::new(),
        funcs: 0,
        builtin_objs: 0,
        top: ts.iter().any(|t| m.escaped.contains_key(t)),
    };
    let mut objs = Vec::new();
    let mut seen: FxHashSet<Tok> = FxHashSet::default();
    for &t in ts {
        let Some(&n) = m.fields.get(&(t, a)) else {
            continue;
        };
        let node = &m.nodes[n as usize];
        k.prims |= node.prim;
        for x in node.pts.to_sorted() {
            if !seen.insert(x) {
                continue;
            }
            match &m.toks[x as usize] {
                TokKind::Top => k.top = true,
                // A closed function's parameter marker (decision 13): what
                // its callers pass is beside it; the marker only records
                // the uses for its contract (as for a receiver, `site_facts`).
                TokKind::Obj(..) if m.closed_marker(x) => {}
                // Any other marker stands for another unit's values with
                // nothing beside it (DECISIONS 119): it reads as ⊤.
                TokKind::Obj(..) if m.is_marker(x) => {
                    k.top = true;
                    k.prims |= prim::ALL;
                }
                TokKind::Obj(tm, _) if m.tmpls[*tm as usize].kind.is_fact_unit() => {
                    objs.push(m.tmpls[*tm as usize].display.clone())
                }
                TokKind::Func(_, _) | TokKind::Bound(_) | TokKind::BuiltinFn(_) => k.funcs += 1,
                _ => k.builtin_objs += 1,
            }
        }
    }
    objs.sort();
    objs.dedup();
    k.objs = objs;
    k
}

/// DEC220: an own-slot read depends on its receiver's objects. Another
/// birth sharing their layout may escape or be reconfigured independently.
/// Layout contracts and all chain/accessor obligations remain template-wide.
fn private_own_slot(m: &Model, tf: &TemplateFacts, t: Tok, key: &str, a: Atom) -> Option<u32> {
    if m.foreign.contains_key(&tf.id)
        || m.is_escaped(t)
        || m.hard.contains_key(&t)
        || layout_exit(m, tf.id, t).is_some()
        || m.objproto_accessor
        || tf.sabotaged
    {
        return None;
    }
    let tpl = &m.tmpls[tf.id as usize];
    if native_root(m, tf.id).is_some() {
        return None;
    }
    let star = m.atom_index.get(crate::STAR_KEY).copied();
    let computed =
        |t: Tok| star.is_some_and(|a| m.accessor_keys.get(&t).is_some_and(|ks| ks.contains(&a)));
    if star.is_some_and(|a| tpl.accessor_keys.contains(&a))
        || m.tmpl_toks(tf.id).into_iter().any(computed)
    {
        return None;
    }
    for p in chain_tokens(m, tf.id) {
        // A literal-shape prototype is a layout, not a runtime object
        // (DEC217(a)). Its fabricated constructor escape cannot let unknown
        // code change a private sibling's own slot. Its real ancestors and
        // every recorded accessor/HARD operation still have to be checked.
        let layout = matches!(m.toks[p as usize], TokKind::Proto(tm)
            if matches!(m.tmpls[tm as usize].kind,
                TmplKind::Class { literal_shape: true, .. }));
        if p == TOP
            || m.hard.contains_key(&p)
            || computed(p)
            || (!layout && !matches!(m.toks[p as usize], TokKind::ObjectProto) && m.is_escaped(p))
        {
            return None;
        }
    }
    tf.fields
        .iter()
        .find(|f| f.is(key, m.is_private_atom(a)) && f.definite && f.data_field)
        .and_then(|f| f.slot)
}

fn site_facts(g: &mut Gen, tfs: &[TemplateFacts]) -> Vec<SiteFacts> {
    let m = &g.m;
    let by_id: crate::pts::FxHashMap<TmplId, &TemplateFacts> =
        tfs.iter().map(|t| (t.id, t)).collect();
    let mut out = Vec::new();
    // One instance token per class: the receiver an entry guard admits.
    let mut class_tok: FxHashMap<TmplId, Tok> = FxHashMap::default();
    for (i, t) in m.toks.iter().enumerate() {
        if let TokKind::Obj(tm, c) = t {
            if *c == NO_CTX || !class_tok.contains_key(tm) {
                class_tok.insert(*tm, i as Tok);
            }
        }
    }
    // One fact per read site: records of the same site (a body walked
    // twice) unite their receivers.
    let mut first: FxHashMap<(u32, u32, u32, bool, bool), usize> = FxHashMap::default();
    let mut groups: Vec<(usize, Vec<u32>, Vec<FnIdx>)> = Vec::new();
    // The functions only the runtime has (a promise's resolvers, a promise
    // capability's executor). Every other function token is a body:
    // compiled here, or another unit's, held by its contract (a stand-in, a
    // foreign class's method).
    let runtime: FxHashSet<FnIdx> = m
        .resolvers
        .values()
        .chain(m.executors.values())
        .map(|&t| m.tok_fn(t))
        .collect();
    for (i, s) in g.read_sites.iter().enumerate() {
        let k = (s.module, s.byte_offset, s.key, s.is_call, s.value_call);
        match first.get(&k) {
            Some(&gi) => {
                groups[gi].1.push(s.recv);
                groups[gi].2.push(s.func);
            }
            None => {
                first.insert(k, groups.len());
                groups.push((i, vec![s.recv], vec![s.func]));
            }
        }
    }
    // A site in a body the solve runs more than once (a summary instance,
    // a for-in body's run per key) is one operation: its receiver holds
    // what the node of EVERY run holds, the shared body's and each
    // instance's copy (`Inst::fresh`). A run's copy may hold what the shared
    // body never observes (a for-in run's key, FVA 4.3b).
    let copies = element::run_copies(m);
    // The verdict of one site over the receiver nodes `recvs`.
    let verdict_of = |s: &crate::gen::ReadSite, recvs: &[u32]| {
        let all = element::with_copies(&copies, recvs);
        let set = receiver_set(m, &all);
        let toks = &set.toks;
        let prim = set.prims;
        let top_recv = set.top_recv;
        let key = &m.atoms[s.key as usize];
        let (mut receivers, mut top, mut verdict, mut fixed_shape, mut targets) =
            judge(m, &by_id, &runtime, s, &key, toks, prim, top_recv);
        // Dead needs every value of the sources: a node that left one out
        // (`dropped`) is not empty in the program.
        if verdict == Verdict::Dead && all.iter().any(|&r| m.nodes[r as usize].dropped) {
            verdict = Verdict::No("dead site (no receiver flows)".into());
        }
        // Throws, a claim naming the TypeError arm of a nullish receiver
        // (`nullish_arms`) and a member claim on a receiver set of
        // primitives only (every token a closed marker) need every value of
        // the sources too: a source that left a value out (`dropped`) holds
        // a set that is not the whole truth.
        let nullish = prim & (crate::model::prim::NULL | crate::model::prim::UNDEF) != 0;
        let only_prims = prim != 0 && toks.iter().all(|&t| m.closed_marker(t));
        if (verdict == Verdict::Throws
            || ((nullish || only_prims)
                && matches!(
                    verdict,
                    Verdict::NativeMember | Verdict::OwnLength | Verdict::OwnDataOrAbsent
                ))
            || (prim != 0 && matches!(verdict, Verdict::T0Mono | Verdict::T0Set)))
            && all.iter().any(|&r| m.nodes[r as usize].dropped)
        {
            verdict = Verdict::No("primitive receiver (a source left a value out)".into());
        }
        // Design §11.4: a site whose receiver is the `this` of a method of
        // class C runs, in the typed body behind the entry guard
        // `class_id(this) == C`, on C's instances only: E0 when it is T0
        // over C's layout (the generic body keeps the full site). A
        // parameter typed with a class type is guarded the same way; one
        // typed with an object type is decided after the census.
        let mut guard_keys = None;
        if let Verdict::No(_) = verdict {
            let guards: Option<Vec<&crate::gen::ParamGuard>> =
                recvs.iter().map(|r| g.param_guard.get(r)).collect();
            match guards.as_deref() {
                Some([crate::gen::ParamGuard::Keys(k), rest @ ..])
                    if rest
                        .iter()
                        .all(|x| matches!(x, crate::gen::ParamGuard::Keys(k2) if k2 == k)) =>
                {
                    guard_keys = Some(k.clone());
                }
                _ => {}
            }
            let guarded = recvs
                .iter()
                .map(|r| match g.param_guard.get(r) {
                    Some(crate::gen::ParamGuard::Class(tm)) => Some(*tm),
                    _ => m.this_class.get(r).copied(),
                })
                .collect::<Option<Vec<_>>>();
            if let Some(tm) = guarded
                .filter(|g| g.windows(2).all(|w| w[0] == w[1]))
                .and_then(|g| g.first().copied())
            {
                if let Some(&it) = class_tok.get(&tm) {
                    let (r2, _, v2, f2, t2) =
                        judge(m, &by_id, &runtime, s, &key, &[it], 0, top_recv);
                    if matches!(v2, Verdict::T0Mono | Verdict::T0Set) {
                        (receivers, top, verdict, fixed_shape, targets) =
                            (r2, true, Verdict::E0, f2, t2);
                    }
                }
            }
        }
        (
            receivers,
            top,
            verdict,
            fixed_shape,
            targets,
            guard_keys,
            set.values(m),
            prim,
        )
    };
    for (i, recvs, funcs) in &groups {
        let s = &g.read_sites[*i];
        let key = m.atoms[s.key as usize].clone();
        let (
            receivers,
            top,
            verdict,
            fixed_shape,
            targets,
            guard_keys,
            receiver_values,
            receiver_prims,
        ) = verdict_of(s, recvs);
        // Observational only (the facts dump): the verdict of each function
        // copy holding the site (an inlined body is a copy of its own), so a
        // dump compares a copy with the same copy of another run. Never read
        // by a proof or a digest.
        let diagnostic_copies = g.diagnostic.as_ref().map(|_| {
            let mut fs: Vec<FnIdx> = funcs.clone();
            fs.sort_unstable();
            fs.dedup();
            fs.into_iter()
                .map(|f| {
                    let rs: Vec<u32> = recvs
                        .iter()
                        .zip(funcs)
                        .filter(|(_, &x)| x == f)
                        .map(|(&r, _)| r)
                        .collect();
                    let fi = &m.funcs[f as usize];
                    let path = g.modules.get(fi.module as usize).map_or("", |(p, _)| *p);
                    (
                        format!("{path}:{}:{}", fi.func_id, fi.name),
                        verdict_of(s, &rs).2,
                    )
                })
                .collect::<Vec<_>>()
        });
        let mut target_fns: Vec<(u32, u32)> = targets
            .keys()
            .map(|&f| (m.funcs[f as usize].module, m.funcs[f as usize].func_id))
            .collect();
        target_fns.sort_unstable();
        target_fns.dedup();
        let mut t: Vec<String> = targets.into_values().collect();
        t.sort();
        t.dedup();
        let mut bodies: Vec<(u32, String)> = funcs
            .iter()
            .map(|&f| {
                let (_, id, name) = m.act_key(f);
                (id, name)
            })
            .collect();
        bodies.sort_unstable();
        bodies.dedup();
        out.push(SiteFacts {
            bodies,
            diagnostic_copies: diagnostic_copies.unwrap_or_default(),
            diagnostic_unit: g
                .diagnostic
                .as_ref()
                .map(|_| g.view.map_or("<whole>", |v| v.key.as_str()).to_owned()),
            module: s.module,
            byte_offset: s.byte_offset,
            key,
            is_call: s.is_call,
            receivers,
            receiver_values,
            receiver_prims,
            top,
            verdict,
            fixed_shape,
            targets: t,
            target_fns,
            guard_keys,
        });
    }
    out
}

/// The verdict of one site over receiver tokens `toks` (and primitive kinds
/// `prim`): receivers, ⊤, verdict, fixed shape, the bodies a call runs.
///
/// A call is proven when its lookup is the read rule's (every receiver
/// names the key by one definite data field of its layout, or resolves it
/// on its stable chain) and every value the lookups give, the callee set,
/// is a compiled body. Several bodies are fine: each runs in its own form.
/// A call through a value has the callee node's values as its set.
#[allow(clippy::too_many_arguments)]
/// The receivers of an `OwnLength` read: every token an Array exotic
/// ([`exact_array`]) or a closed marker (its callers' values are beside
/// it), every primitive a String, and at least one receiver.
/// A receiver set of primitives only (every token a closed marker) that
/// holds `undefined` and/or `null` beside the primitives `rest`: the
/// operation's own dispatch on the value's kind throws the TypeError for a
/// nullish value (ECMA-262 13.3.2.1, 7.1.18 ToObject) and runs `rest`'s
/// family member for the others. `Some((rest, arms))`: the primitive bits
/// left and the receivers naming the TypeError arms; `None` when no
/// nullish value flows or an object token sits beside it.
fn nullish_arms(m: &Model, toks: &[Tok], prim: u16) -> Option<(u16, Vec<String>)> {
    use crate::model::prim::{NULL, UNDEF};
    let nn = prim & (NULL | UNDEF);
    if nn == 0 || !toks.iter().all(|&t| m.closed_marker(t)) {
        return None;
    }
    let mut arms = Vec::new();
    if nn & UNDEF != 0 {
        arms.push("primitive:undefined (TypeError)".to_string());
    }
    if nn & NULL != 0 {
        arms.push("primitive:null (TypeError)".to_string());
    }
    Some((prim & !nn, arms))
}

fn own_length(m: &Model, toks: &[Tok], prim: u16) -> Option<Vec<String>> {
    let (prim, arms) = nullish_arms(m, toks, prim).unwrap_or((prim, Vec::new()));
    if prim != 0 && prim != crate::model::prim::STR {
        return None;
    }
    let mut receivers = arms;
    for &t in toks {
        if m.closed_marker(t) {
            continue;
        }
        if !exact_array(m, t) {
            return None;
        }
        receivers.push(m.tmpls[m.tmpl_of(t)? as usize].display.clone());
    }
    if prim != 0 {
        receivers.push("primitive:string".into());
    }
    receivers.sort();
    receivers.dedup();
    receivers
        .iter()
        .any(|r| !r.ends_with("(TypeError)"))
        .then_some(receivers)
}

/// Is every value of token `t` an Array exotic object (ECMA-262 10.4.2)?
/// An array site of this unit (a literal, a rest parameter, an ArrayCreate
/// result), an own-keys result (`keys`, `arraybox`), or another unit's
/// array its owner publishes as an Array (`ValueContract::arrays`,
/// `arrayboxes`; one it publishes as made, `made_arrays`, has ⊤ for its
/// constructor). Not the
/// `arguments` object (`TmplKind::Args`), not a container of primitives,
/// not a marker. An array made with ArraySpeciesCreate (10.4.2.3) is one
/// only while the constructor the algorithm takes (`Template::species`: the
/// receivers' `constructor` and its `@@species`, as the solve looked them
/// up) is `%Array%` or no object: `undefined` (or a `null` species) is an
/// ArrayCreate, any other primitive throws before an array is made.
pub(crate) fn exact_array(m: &Model, t: Tok) -> bool {
    if m.is_marker(t) {
        return false;
    }
    let TokKind::Obj(tm, _) = m.toks[t as usize] else {
        return false;
    };
    let tpl = &m.tmpls[tm as usize];
    match tpl.kind {
        TmplKind::Arr { .. } => {}
        TmplKind::BObj {
            what: "keys" | "arraybox",
            ..
        } => return true,
        _ => return false,
    }
    let Some(c) = tpl.species else {
        return true;
    };
    let nd = &m.nodes[c as usize];
    let array = m
        .tok_index
        .get(&TokKind::Builtin(crate::model::BuiltinNs::Array))
        .copied();
    nd.pts.to_sorted().into_iter().all(|v| Some(v) == array)
}

fn judge(
    m: &Model,
    by_id: &FxHashMap<TmplId, &TemplateFacts>,
    runtime: &FxHashSet<FnIdx>,
    s: &crate::gen::ReadSite,
    key: &str,
    toks: &[Tok],
    prim: u16,
    top_recv: NodeId,
) -> (Vec<String>, bool, Verdict, bool, FxHashMap<FnIdx, String>) {
    if s.value_call {
        let top = toks.contains(&TOP);
        if prim == 0 && toks.iter().all(|&t| m.closed_marker(t)) {
            return (Vec::new(), false, Verdict::Dead, true, FxHashMap::default());
        }
        if !m.unit_mode {
            if let Some(name) = intrinsic_callee(m, toks, prim) {
                if !m.realm_held {
                    return (vec![name], false, Verdict::IntrinsicCall, true, FxHashMap::default());
                }
                let why = "intrinsic callee: unknown code holds the realm and may replace the member";
                return (Vec::new(), false, Verdict::No(why.into()), true, FxHashMap::default());
            }
        }
        return match callee_set(m, runtime, toks, prim, top_recv) {
            Ok(t) if t.len() == 1 => (Vec::new(), top, Verdict::T0Mono, true, t),
            Ok(t) => (Vec::new(), top, Verdict::T0Set, true, t),
            Err(why) => (
                Vec::new(),
                top,
                Verdict::No(why),
                true,
                FxHashMap::default(),
            ),
        };
    }
    // An Array's and a String's `length` is a non-configurable own data
    // property (a String's is its UTF-16 count): no prototype or
    // definition can override it. Unlike T0 it names no layout slot or body.
    if !s.is_call && key == "length" {
        if let Some(receivers) = own_length(m, toks, prim) {
            return (
                receivers,
                false,
                Verdict::OwnLength,
                true,
                FxHashMap::default(),
            );
        }
    }
    // The intrinsic member of one builtin family. Like OwnLength it names
    // no layout slot or body. A write onto a builtin prototype that alone
    // keeps it from the site is the site's reason.
    let native_why = match native_member(m, s, toks, prim) {
        Ok(receivers) => {
            return (
                receivers,
                false,
                Verdict::NativeMember,
                true,
                FxHashMap::default(),
            )
        }
        Err(why) => why,
    };
    // A host object's member (console, the stdio streams): the runtime's
    // own native until a store reaches the key (`builtins::host_native`,
    // the predicate the solver's host calls and loads read).
    if let Some(r) = host_member(m, s, toks, prim) {
        return match r {
            Ok(receivers) => (
                receivers,
                false,
                Verdict::NativeMember,
                true,
                FxHashMap::default(),
            ),
            Err(why) => (
                Vec::new(),
                false,
                Verdict::No(why),
                true,
                FxHashMap::default(),
            ),
        };
    }
    if let Some(receivers) = ns_export(m, s, toks, prim) {
        return (receivers, false, Verdict::NsExport, true, FxHashMap::default());
    }
    let mut receivers = Vec::new();
    let mut top = false;
    let mut verdict: Option<Verdict> = None;
    let mut slots: Vec<Option<u32>> = Vec::new();
    // Receivers read by an own data lookup or `undefined` (OwnDataOrAbsent).
    let mut own_or_absent = 0usize;
    let mut fixed_shape = true;
    // A call: the nodes whose values the lookups give.
    let mut callee: Vec<NodeId> = Vec::new();
    // Reads of a class object's constant static (3.4).
    let mut statics = 0usize;
    for &t in toks {
        match &m.toks[t as usize] {
            TokKind::Top => {
                top = true;
                verdict.get_or_insert_with(|| {
                    if m.explain {
                        Verdict::No(format!(
                            "receiver from unmodeled code (TOP) <- {}",
                            m.top_origin(top_recv)
                        ))
                    } else {
                        Verdict::No("receiver from unmodeled code (TOP)".into())
                    }
                });
            }
            // A closed function's parameter marker (decision 13): what
            // its callers pass is the stand-ins beside it; the marker
            // only records the uses for its contract. A published method's
            // receiver marker with published receivers too (DECISIONS 107).
            TokKind::Obj(..) if m.closed_marker(t) => {}
            // Any other marker stands for another unit's values with
            // nothing beside it (DECISIONS 119): it reads as ⊤.
            TokKind::Obj(..) if m.is_marker(t) => {
                top = true;
                receivers.push(m.tok_name(t));
                verdict.get_or_insert(Verdict::No(
                    "receiver is another unit's value (marker)".into(),
                ));
            }
            TokKind::Obj(tm, _) => {
                let Some(tf) = by_id.get(tm) else {
                    receivers.push(m.tok_name(t));
                    // An array the program stores no named key on reads
                    // `undefined` under a key its chain lacks.
                    if !s.is_call && array_absent(m, t, key, s.key) {
                        own_or_absent += 1;
                        continue;
                    }
                    if !s.is_call && json_own_or_absent(m, t, key, s.key) {
                        own_or_absent += 1;
                        continue;
                    }
                    verdict.get_or_insert_with(|| {
                        Verdict::No(not_unit_why(m, t, key, s.key, s.is_call))
                    });
                    continue;
                };
                receivers.push(tf.display.clone());
                // Rule C: each evaluation of the class is its own private
                // brand; the template's slot is not this evaluation's.
                if m.tmpls[*tm as usize].per_evaluation && m.is_private_atom(s.key) {
                    verdict.get_or_insert(Verdict::No("per class evaluation".into()));
                    continue;
                }
                if !tf.prefix_stable {
                    if !s.is_call {
                        if let Some(slot) = private_own_slot(m, tf, t, key, s.key) {
                            slots.push(Some(slot));
                            // This proves one slot, not a fixed full layout.
                            fixed_shape = false;
                            continue;
                        }
                    }
                    let why = if let Some(e) = &tf.escape {
                        format!("escape: {e}")
                    } else if let Some(h) = &tf.hard {
                        format!("hard: {h}")
                    } else {
                        format!("chain: {}", tf.chain.clone().unwrap_or_default())
                    };
                    verdict.get_or_insert(Verdict::No(why));
                    continue;
                }
                if !tf.fixed_layout {
                    fixed_shape = false;
                }
                if s.is_call {
                    match lookup(m, tf, t, key, s.key) {
                        Ok(ns) => callee.extend(ns),
                        Err(why) => {
                            verdict.get_or_insert(Verdict::No(why));
                        }
                    }
                    continue;
                }
                let row = tf
                    .fields
                    .iter()
                    .find(|f| f.is(key, m.is_private_atom(s.key)));
                // No store names the key: a row only a read made, or none.
                let unwritten = row.is_none_or(|f| {
                    f.writers_ctor + f.writers_post == 0 && !tf.birth_keys.contains(&f.key)
                });
                match row {
                    Some(f)
                        if (f.definite || f.declared || f.reserved)
                            && f.data_field
                            && f.slot.is_some() =>
                    {
                        slots.push(f.slot)
                    }
                    Some(f) if !f.data_field => {
                        verdict.get_or_insert(Verdict::No("accessor key (E25)".into()));
                    }
                    // An absent key is one outcome, as one slot is: the
                    // read gives `undefined` (`None` in `slots`).
                    _ if unwritten && tf.fixed_layout && absent_key(m, t, key, s.key) => {
                        slots.push(None)
                    }
                    _ if own_data_or_absent(m, tf, t, key, s.key) => own_or_absent += 1,
                    Some(_) | None => {
                        // Not a definite birth field: a prototype member
                        // or a key added after birth.
                        verdict.get_or_insert(Verdict::No("key not a definite birth field".into()));
                    }
                }
            }
            // A class object: the key is one of its constant statics (3.4).
            // A static it inherits is not proven (the class objects' chain is
            // not modeled).
            TokKind::ClassObj(tm) => {
                receivers.push(m.tok_name(t));
                // Rule C: each evaluation is its own class object.
                if m.tmpls[*tm as usize].per_evaluation {
                    verdict.get_or_insert(Verdict::No("per class evaluation".into()));
                    continue;
                }
                let constant = by_id
                    .get(tm)
                    .is_some_and(|tf| tf.statics.iter().any(|k| k == key));
                let n = m.fields.get(&(t, s.key)).copied();
                match n {
                    Some(n) if constant && s.is_call => callee.push(n),
                    Some(_) if constant => statics += 1,
                    _ => {
                        verdict.get_or_insert(Verdict::No(
                            "not a constant static of the class (3.4)".into(),
                        ));
                    }
                }
            }
            // A program class's prototype object: the key is one of its own
            // constant members (15.7.14, `constant_proto_members`). A member
            // it inherits is not proven here.
            TokKind::Proto(tm) if by_id.contains_key(tm) => {
                receivers.push(m.tok_name(t));
                let constant = by_id
                    .get(tm)
                    .is_some_and(|tf| tf.proto_consts.iter().any(|k| k == key));
                let n = m.fields.get(&(t, s.key)).copied();
                match n {
                    Some(n) if constant && s.is_call => callee.push(n),
                    Some(_) if constant => statics += 1,
                    _ => {
                        verdict.get_or_insert(Verdict::No(
                            "not a constant own member of the class prototype (15.7.14)".into(),
                        ));
                    }
                }
            }
            other => {
                receivers.push(m.tok_name(t));
                let _ = other;
                verdict.get_or_insert(Verdict::No("receiver is a builtin/function value".into()));
            }
        }
    }
    // A closed marker names no receiver: its callers' values sit beside it.
    // With nothing beside it no receiver flows, and a proof names the
    // receivers it is over (the value-call rule, `callee_set`).
    let none = toks.iter().all(|&t| m.closed_marker(t));
    // The primitives beside the objects: the operation's own dispatch on
    // each value's kind (7.3.2 GetV, 7.1.18 ToObject). A nullish value
    // names its TypeError arm; a String, Number or Boolean reads its
    // family, where the key is absent (`family_absent`): the outcome of an
    // absent key (`undefined`), one more own-data-or-absent receiver or
    // the `None` slot. A call's callee on a primitive is not covered.
    if !none && prim != 0 && verdict.is_none() {
        match beside_prims(m, key, s.key, prim, s.is_call) {
            Some((r, absent)) => {
                receivers.extend(r);
                if absent && statics > 0 {
                    verdict = Some(Verdict::No(
                        "receivers mix class objects and primitives".into(),
                    ));
                } else if absent && own_or_absent > 0 {
                    own_or_absent += 1;
                } else if absent {
                    slots.push(None);
                }
            }
            None => {
                verdict = Some(Verdict::No(format!(
                    "primitive beside an object (key not absent on its family) {}",
                    crate::model::prim::name(prim)
                )))
            }
        }
    }
    let mut seen_r: FxHashSet<String> = FxHashSet::default();
    receivers.retain(|r| seen_r.insert(r.clone()));
    if none && prim == 0 {
        verdict.get_or_insert(Verdict::Dead);
    }
    if none && prim != 0 {
        use crate::model::prim::{NULL, UNDEF};
        if prim & !(NULL | UNDEF) == 0 {
            verdict.get_or_insert(Verdict::Throws);
        }
        if !s.is_call {
            if let Some(r) = prim_absent(m, key, s.key, toks, prim) {
                receivers.extend(r);
                verdict.get_or_insert(Verdict::OwnDataOrAbsent);
            }
        }
        verdict.get_or_insert(Verdict::No("primitive receiver".into()));
    }
    let mut targets = FxHashMap::default();
    let verdict = match verdict {
        Some(v) => v,
        None if s.is_call => {
            callee.sort_unstable();
            callee.dedup();
            let mut ts: Vec<Tok> = callee
                .iter()
                .flat_map(|&n| m.nodes[n as usize].pts.to_sorted())
                .collect();
            ts.sort_unstable();
            ts.dedup();
            let p = callee.iter().fold(0, |p, &n| p | m.nodes[n as usize].prim);
            let top_node = callee
                .iter()
                .copied()
                .find(|&n| m.nodes[n as usize].pts.contains_tok(TOP))
                .unwrap_or(top_recv);
            match callee_set(m, runtime, &ts, p, top_node) {
                Ok(t) => {
                    let one = t.len() == 1 && receivers.len() == 1;
                    targets = t;
                    if one {
                        Verdict::T0Mono
                    } else {
                        Verdict::T0Set
                    }
                }
                Err(why) => Verdict::No(why),
            }
        }
        None if own_or_absent > 0 && statics > 0 => {
            Verdict::No("receivers mix class objects and instances".into())
        }
        None if own_or_absent > 0 => Verdict::OwnDataOrAbsent,
        None if statics > 0 && !slots.is_empty() => {
            Verdict::No("receivers mix class objects and instances".into())
        }
        None if statics > 0 => {
            if receivers.len() == 1 {
                Verdict::T0Mono
            } else {
                Verdict::T0Set
            }
        }
        None => {
            let first = slots.first().copied().flatten();
            if slots.iter().all(|s| *s == first) {
                if receivers.len() == 1 {
                    Verdict::T0Mono
                } else {
                    Verdict::T0Set
                }
            } else {
                Verdict::No("slots differ across the receiver set".into())
            }
        }
    };
    let verdict = match (verdict, native_why) {
        (Verdict::No(_), Some(why)) => Verdict::No(why),
        (v, _) => v,
    };
    (receivers, top, verdict, fixed_shape, targets)
}

/// BIV (CHB §2): the receivers of a NativeMember site, or why there are
/// none. `Err(Some(..))` names the write onto a builtin prototype that alone
/// keeps an otherwise proven site (obligations 1-3 hold); `Err(None)`: an
/// obligation 1-3 fails, the site keeps its other reason.
///
/// 1. One family: every primitive a String and no object, or every object
///    an Array exotic object ([`exact_array`]), or every object a
///    collection of one kind, or every object a RegExp, or every object a
///    native Promise, or every primitive a Number and no object, or every
///    object a Buffer (a method `Buffer.prototype` owns); closed markers are skipped (their callers'
///    values are beside them), ⊤ and other markers are none.
/// 2. The family's intrinsic chain defines the key: a function for a call,
///    a function or a data member for a read (`builtins::native_described`).
/// 3. No receiver holds the key as an own property ([`native_own_free`]).
/// 4. No builtin prototype is written under the key or under any key: no
///    accessor on `Object.prototype` (E23), no redefinition of a builtin
///    prototype's key (defineProperty on one or through ⊤) and no hard
///    operation on them, no program write onto them, no store into ⊤
///    (which may be any of them). Per unit, other units' writes are
///    replayed into the same facts (`SharedWrite`).
fn native_member(
    m: &Model,
    s: &crate::gen::ReadSite,
    toks: &[Tok],
    prim: u16,
) -> Result<Vec<String>, Option<String>> {
    use crate::builtins::NativeFamily;
    let mut fam: Option<NativeFamily> = None;
    // A nullish value beside one primitive family: its TypeError arm.
    let (prim, mut receivers) = nullish_arms(m, toks, prim).unwrap_or((prim, Vec::new()));
    let join = |f: NativeFamily, fam: &mut Option<NativeFamily>| match fam {
        None => {
            *fam = Some(f);
            true
        }
        Some(g) => *g == f,
    };
    if prim != 0 {
        let (f, r) = match prim {
            crate::model::prim::STR => (NativeFamily::Str, "primitive:string"),
            crate::model::prim::NUM => (NativeFamily::Num, "primitive:number"),
            _ => return Err(None),
        };
        join(f, &mut fam);
        receivers.push(r.to_string());
    }
    for &t in toks {
        if m.closed_marker(t) {
            continue;
        }
        let f = if exact_array(m, t) {
            NativeFamily::Array
        } else {
            match m.toks[t as usize] {
                TokKind::Obj(tm, _) if !m.is_marker(t) => match m.tmpls[tm as usize].kind {
                    TmplKind::BObj {
                        what: w @ ("Map" | "Set" | "WeakMap" | "WeakSet"),
                        ..
                    } => NativeFamily::Collection(w),
                    TmplKind::BObj { what: "RegExp", .. } => NativeFamily::RegExp,
                    TmplKind::BObj {
                        what: "Promise", ..
                    } => NativeFamily::Promise,
                    // The per-unit solve misses a write onto `Buffer.prototype`
                    // through `Object.assign`: it proves no Buffer member.
                    TmplKind::BObj { what: "Buffer", .. } if !m.unit_mode => NativeFamily::Buffer,
                    _ => return Err(None),
                },
                _ => return Err(None),
            }
        };
        if !join(f, &mut fam) {
            return Err(None);
        }
        let Some(tm) = m.tmpl_of(t) else {
            return Err(None);
        };
        receivers.push(m.tmpls[tm as usize].display.clone());
    }
    let Some(fam) = fam else {
        return Err(None);
    };
    let a = s.key;
    if !crate::builtins::native_described(m, fam, &m.atoms[a as usize], s.is_call) {
        return Err(None);
    }
    for &t in toks {
        if !m.closed_marker(t) && !native_own_free(m, fam, t, a) {
            return Err(None);
        }
    }
    builtin_protos_unwritten(m, Some(fam), a).map_err(Some)?;
    receivers.sort();
    receivers.dedup();
    Ok(receivers)
}

/// A site whose receivers are all host objects (`builtins::host_of`):
/// `Ok` when each one's member under the key is its described native
/// member and no store reached it; `None` when a receiver is no host
/// object or a primitive flows.
fn host_member(
    m: &Model,
    s: &crate::gen::ReadSite,
    toks: &[Tok],
    prim: u16,
) -> Option<Result<Vec<String>, String>> {
    if prim != 0 || toks.is_empty() {
        return None;
    }
    let name = &m.atoms[s.key as usize];
    let mut receivers = Vec::new();
    let mut native = true;
    // A builtin namespace or constructor that is no host object (`Math`,
    // `Promise`, `Array`): the realm's own member, unwritten
    // (`builtins::ns_native`, BNSRECV).
    let mut realm_ns = false;
    for &t in toks {
        let TokKind::Builtin(ns) = m.toks[t as usize] else {
            return None;
        };
        match crate::builtins::host_of(ns, name, s.is_call) {
            Some(h) => {
                native &= crate::builtins::host_native(m, h, s.key, s.is_call);
                receivers.push(format!("host:{h:?}"));
            }
            None if crate::builtins::ns_native(m, t, ns, s.key, s.is_call) => {
                realm_ns = true;
                receivers.push(m.tok_name(t));
            }
            None => return None,
        }
    }
    if !native {
        return Some(Err("host member undescribed or written".into()));
    }
    // Interpreted code (DEC 150) names every global and may write the key;
    // no namespace object is a lock holder, so nothing refuses the write.
    if realm_ns && m.interp_runs {
        return Some(Err("namespace member: interpreted code may write the key".into()));
    }
    // Unknown code holds the realm (9.3, `realm_held`): a host object's
    // members are writable, and the solve does not replay unknown code's
    // replacement of one (`realm_hold_tok`), as for `Verdict::IntrinsicCall`.
    if m.realm_held {
        return Some(Err(
            "host member: unknown code holds the realm and may replace the member".into(),
        ));
    }
    receivers.sort();
    receivers.dedup();
    Some(Ok(receivers))
}

/// Obligation 4 of [`native_member`]: no builtin prototype is written
/// under key `a` or under any key, so the intrinsics' descriptions
/// (`members`) decide what their chains read under `a`. `Err`: the write
/// that may change it.
fn builtin_protos_unwritten(
    m: &Model,
    fam: Option<crate::builtins::NativeFamily>,
    a: Atom,
) -> Result<(), String> {
    let objproto = m.tok_index.get(&TokKind::ObjectProto).copied();
    // DEC 250: a final read (the runtime refuses every write that would
    // change it: `members::store_admitted` on each prototype the lookup
    // passes) is written by no store, the program's or one through ⊤.
    // `None`: a plain object's chain, which reaches `Object.prototype` only.
    let locked = match fam {
        Some(fam) => crate::builtins::native_locked(m, fam, a),
        None => crate::builtins::object_chain_locked(m, a),
    };
    // A redefinition is a store too: a defineProperty, an Annex B
    // `__defineGetter__`/`__defineSetter__` or an accessor on a builtin
    // prototype (`reshape`, `objproto_accessor`, a hard `Object.prototype`),
    // on it or through ⊤, of a key whose read is final throws (or returns
    // false) and installs nothing. Only an admitted one may change the read.
    if !locked
        && (m.objproto_accessor
            || m.reshaped_any
            || m.reshaped_keys.contains(&a)
            || objproto.is_some_and(|op| m.hard.contains_key(&op)))
    {
        return Err(
            "native member: a builtin prototype may be redefined (accessor, defineProperty)".into(),
        );
    }
    // Interpreted code (DEC 150's summary node: `new Function`, direct and
    // indirect eval, any solved call that may be the Function constructor)
    // holds every global, so every builtin prototype, and writes under any
    // key. A NativeMember site runs its member with no test (codegen's
    // `native_member_calls` takes no interp guard word), so only a final
    // read survives it.
    if fam.is_some() && !locked && m.interp_runs {
        return Err(
            "native member: interpreted code may write the key onto a builtin prototype".into(),
        );
    }
    // Unknown code holds the realm (9.3, `realm_held`): it may replace a
    // builtin prototype's member that the runtime lets a store replace, a
    // write the solve does not replay (`realm_hold_tok`). Only a final read
    // survives it, as for `Verdict::IntrinsicCall`.
    if fam.is_some() && !locked && m.realm_held {
        return Err(
            "native member: unknown code holds the realm and may replace the member".into(),
        );
    }
    let (through_top, own) = match objproto {
        _ if locked => (false, false),
        Some(op) => crate::builtins::native_written_by(m, op, a),
        None => (crate::builtins::native_written(m, TOP, a), false),
    };
    if own {
        return Err("native member: the program writes the key onto a builtin prototype".into());
    }
    if through_top {
        return Err(
            "native member: a store into ⊤ may write the key onto a builtin prototype".into(),
        );
    }
    Ok(())
}

/// Key `a` (`key`) is absent on receiver `t`, a closed fixed layout without
/// the key as a field, and on its whole prototype chain: nothing is stored
/// on `t` or on a program prototype of the chain under the key (no named
/// value, no computed store, no accessor, no `__proto__` relink, no hard
/// operation, no escape of the prototype), the chain ends in
/// `Object.prototype`, whose intrinsic description lacks the key, and no
/// builtin prototype is written under it ([`builtin_protos_unwritten`]).
/// [[Get]] then walks to the end of the chain: the read gives `undefined`.
/// A private name is never absent (its read throws without the brand).
fn absent_key(m: &Model, t: Tok, key: &str, a: Atom) -> bool {
    chain_absent(m, t, key, a, true)
}

/// The `absent_key` walk: `t`'s prototype chain holds no `a` and ends at
/// `%Object.prototype%`, which lacks it. `own`: `t` itself holds nothing
/// under `a` either.
fn chain_absent(m: &Model, t: Tok, key: &str, a: Atom, own: bool) -> bool {
    let holds_nothing = |n: Option<&NodeId>| {
        n.is_none_or(|&n| {
            let nd = &m.nodes[n as usize];
            !nd.dropped && nd.pts.is_empty() && nd.prim == 0
        })
    };
    let unwritten = |h: Tok| {
        holds_nothing(m.fields.get(&(h, a)))
            && holds_nothing(m.field_any.get(&h))
            && !m.super_relink.contains(&h)
            && m.accessor_keys.get(&h).is_none_or(|k| !k.contains(&a))
    };
    let Some(&op) = m.tok_index.get(&TokKind::ObjectProto) else {
        return false;
    };
    if m.is_private_atom(a) || (own && !unwritten(t)) {
        return false;
    }
    let mut seen: FxHashSet<Tok> = FxHashSet::default();
    let mut todo = vec![t];
    let mut ends = false;
    while let Some(h) = todo.pop() {
        // A known prototype set: no primitive (`null`) and no ⊤ in it.
        let Some(protos) = m.proto_node.get(&h).map(|&pn| &m.nodes[pn as usize]) else {
            return false;
        };
        if protos.prim != 0 || protos.pts.is_empty() {
            return false;
        }
        for p in protos.pts.to_sorted() {
            if !seen.insert(p) {
                continue;
            }
            if p == op {
                ends = true;
                continue;
            }
            let program_proto = matches!(m.toks[p as usize], TokKind::Proto(_))
                && unwritten(p)
                && !m.hard.contains_key(&p)
                && !m.is_escaped(p);
            if !program_proto {
                return false;
            }
            todo.push(p);
        }
    }
    ends && crate::builtins::object_proto_lacks(key) && builtin_protos_unwritten(m, None, a).is_ok()
}

/// [`Verdict::OwnDataOrAbsent`] for a `JSON.parse` object `t` (the
/// `json` site template, `builtins::json_value`) and key `a`: ParseJSON
/// (ECMA-262 25.5.1) evaluates the text as an ObjectLiteral, so the
/// object is ordinary, its [[Prototype]] `%Object.prototype%`
/// (OrdinaryObjectCreate) and every own key a data property
/// (CreateDataPropertyOrThrow, 13.2.5.5; JSON text is not subject to
/// Annex B.3.1, so `__proto__` too); the program's later stores and
/// definitions are [`own_data_or_absent_tok`]'s conditions.
fn json_own_or_absent(m: &Model, t: Tok, key: &str, a: Atom) -> bool {
    let TokKind::Obj(tm, _) = m.toks[t as usize] else {
        return false;
    };
    matches!(m.tmpls[tm as usize].kind, TmplKind::BObj { what: "json", .. })
        && !m.is_marker(t)
        && own_data_or_absent_tok(m, t, key, a)
}

/// The reason of a receiver token without layout facts that no
/// template-free rule admits: for a `JSON.parse` object the first failing
/// condition of [`own_data_or_absent_tok`].
fn not_unit_why(m: &Model, t: Tok, key: &str, a: Atom, call: bool) -> String {
    let base = "receiver is not a fact unit";
    let TokKind::Obj(tm, _) = m.toks[t as usize] else {
        return base.into();
    };
    match &m.tmpls[tm as usize].kind {
        TmplKind::BObj { what: "json", .. } if call => format!("{base} (json: a call)"),
        TmplKind::BObj { what: "json", .. } if m.is_marker(t) => format!("{base} (json: marker)"),
        TmplKind::BObj { what: "json", .. } => match own_data_or_absent_why(m, t, key, a) {
            Err(why) => format!("{base} (json: {why})"),
            Ok(()) => format!("{base} (json)"),
        },
        TmplKind::BObj { what, .. } => format!("{base} (builtin {what})"),
        _ => base.into(),
    }
}

/// The primitives beside the objects of a read or call (`judge`):
/// `undefined`/`null` name the operation's TypeError arm (6.2.5.5,
/// 7.1.18); for a read each String, Number or Boolean is `family_absent`
/// (the read gives `undefined`); a call's callee on one, a BigInt or a
/// Symbol is not covered. The receivers, and whether an absent outcome is
/// among them.
fn beside_prims(m: &Model, key: &str, a: Atom, prim: u16, call: bool) -> Option<(Vec<String>, bool)> {
    use crate::builtins::NativeFamily as F;
    use crate::model::prim::{BOOL, NULL, NUM, STR, UNDEF};
    if prim & !(STR | NUM | BOOL | NULL | UNDEF) != 0 {
        return None;
    }
    if call && prim & (STR | NUM | BOOL) != 0 {
        return None;
    }
    let mut r = Vec::new();
    if prim & UNDEF != 0 {
        r.push("primitive:undefined (TypeError)".to_string());
    }
    if prim & NULL != 0 {
        r.push("primitive:null (TypeError)".to_string());
    }
    for (bit, fam, name) in [
        (STR, F::Str, "primitive:string"),
        (NUM, F::Num, "primitive:number"),
        (BOOL, F::Bool, "primitive:boolean"),
    ] {
        if prim & bit != 0 {
            if !family_absent(m, fam, key, a) {
                return None;
            }
            r.push(name.to_string());
        }
    }
    let absent = prim & (STR | NUM | BOOL) != 0;
    Some((r, absent))
}

/// [`Verdict::OwnDataOrAbsent`] for receiver `t` (layout `tf`) and key `a`:
/// what [[Get]] finds is `t`'s own data property or nothing. `t`'s own
/// keys hold only data (no accessor on the token or its template), its
/// [[Prototype]] is never written (`super_relink`: a `__proto__` store or
/// a computed store whose key may be `__proto__` runs the
/// `Object.prototype.__proto__` setter, B.2.2.1.2), unknown code never
/// has it (no escape, no hard operation), and the chain lacks the key. The
/// optimistic JSON-key rule keeps tokens out of `super_relink` by
/// assumption: with it on, no claim.
fn own_data_or_absent(m: &Model, tf: &TemplateFacts, t: Tok, key: &str, a: Atom) -> bool {
    tf.prefix_stable && own_data_or_absent_tok(m, t, key, a)
}

/// The token conditions of [`own_data_or_absent`] (no layout needed).
fn own_data_or_absent_tok(m: &Model, t: Tok, key: &str, a: Atom) -> bool {
    own_data_or_absent_why(m, t, key, a).is_ok()
}

/// [`own_data_or_absent_tok`]'s conditions in order; `Err` names the
/// first that fails (the reason text of a refused site).
fn own_data_or_absent_why(m: &Model, t: Tok, key: &str, a: Atom) -> Result<(), String> {
    if m.optimistic_json_key {
        return Err("optimistic JSON-key rule on".into());
    }
    if m.super_relink.contains(&t) {
        return Err("__proto__ relink".into());
    }
    if m.is_escaped(t) {
        let why = m.escaped.get(&t).map(|r| m.reasons[*r as usize].clone());
        return Err(format!("escape: {}", why.unwrap_or_default()));
    }
    if let Some((k, r)) = m.hard.get(&t) {
        return Err(format!("hard: {} {}", k.code(), m.reasons[*r as usize]));
    }
    if let Some(k) = m.accessor_keys.get(&t).filter(|k| !k.is_empty()) {
        return Err(format!("accessor keys on the token ({})", k.len()));
    }
    if !m
        .tmpl_of(t)
        .is_some_and(|tm| m.tmpls[tm as usize].accessor_keys.is_empty())
    {
        return Err("accessor keys on the template".into());
    }
    if !chain_absent(m, t, key, a, false) {
        return Err("the chain has the key".into());
    }
    if !object_proto_unwritten(m, a) {
        return Err("a write of the key on Object.prototype".into());
    }
    Ok(())
}

/// [[Get]] of named key `a` (`key`) on array token `t` gives `undefined`.
/// `t` is an Array exotic object (`exact_array`, a `TmplKind::Arr` birth)
/// whose [[Prototype]] is `%Array.prototype%`: its own keys are its indices
/// and `length` (ECMA-262 10.4.2), any other own key one the program
/// stores (`fields`; under an unknown key `named_any`, whose values its
/// named keys read, apart from its elements in `field_any`; another
/// unit's named stores, `cell_arrs`; an accessor); no `__proto__`
/// relink, no hard operation and no escape gives unknown code the array. The key is
/// no index, not `length` and no private name; [[Get]] is OrdinaryGet
/// (10.1.8.1) and walks `%Array.prototype%` and `%Object.prototype%`,
/// whose descriptions lack it, and no write reaches either under it
/// ([`builtin_protos_unwritten`] for the Array family: `%Array.prototype%`
/// takes named adds, perry-abi lock.rs `add_locked`, so neither interpreted
/// code nor a held realm may run).
fn array_absent(m: &Model, t: Tok, key: &str, a: Atom) -> bool {
    let holds_nothing = |n: Option<&NodeId>| {
        n.is_none_or(|&n| {
            let nd = &m.nodes[n as usize];
            !nd.dropped && nd.pts.is_empty() && nd.prim == 0
        })
    };
    let TokKind::Obj(tm, _) = m.toks[t as usize] else {
        return false;
    };
    matches!(m.tmpls[tm as usize].kind, TmplKind::Arr { .. })
        && exact_array(m, t)
        && key != "length"
        && holds_nothing(m.fields.get(&(t, a)))
        && holds_nothing(if m.elems_apart(t) {
            m.named_any.get(&t)
        } else {
            m.field_any.get(&t)
        })
        && holds_nothing(m.cell_arrs.get(&t))
        && m.accessor_keys.get(&t).is_none_or(|k| k.is_empty())
        && m.tmpls[tm as usize].accessor_keys.is_empty()
        && !m.super_relink.contains(&t)
        && !m.hard.contains_key(&t)
        && !m.is_escaped(t)
        && family_absent(m, crate::builtins::NativeFamily::Array, key, a)
}

/// Key `a` (`key`) is absent on every value of family `fam` (an Array
/// exotic object `array_absent` admits, a String, Number or Boolean
/// primitive): no own key of such a value (an array's indices and
/// `length`, a String wrapper's indices and `length`, 10.4.2, 10.4.3; a
/// Number or Boolean wrapper owns none, 21.1.3, 20.3.3, so `length` is
/// absent on them), the family's intrinsic chain to
/// `%Object.prototype%` lacks it, and nothing writes it there (the
/// program, a store through ⊤, interpreted code, unknown code holding the
/// realm: `builtin_protos_unwritten`). OrdinaryGet (10.1.8.1), for a
/// primitive on GetV's ToObject wrapper (7.3.2), gives `undefined`.
fn family_absent(m: &Model, fam: crate::builtins::NativeFamily, key: &str, a: Atom) -> bool {
    (key != "length"
        || matches!(fam, crate::builtins::NativeFamily::Num | crate::builtins::NativeFamily::Bool))
        && key != "__proto__"
        && !key.starts_with(|c: char| c.is_ascii_digit() || c == '-')
        && !matches!(key, "Infinity" | "NaN")
        && !m.is_private_atom(a)
        && !m.optimistic_json_key
        && crate::builtins::family_chain_lacks(fam, key)
        && object_proto_unwritten(m, a)
        && builtin_protos_unwritten(m, Some(fam), a).is_ok()
}

/// Shape A: a named read whose receiver set holds only primitives (no
/// token but closed markers), each of the String, Number and Boolean
/// families with `family_absent`, `undefined`/`null` beside them naming
/// their TypeError arm (`nullish_arms`: the read's own dispatch on the
/// value's kind throws for them, 6.2.5.5, 7.1.18). The receivers.
fn prim_absent(m: &Model, key: &str, a: Atom, toks: &[Tok], prim: u16) -> Option<Vec<String>> {
    use crate::builtins::NativeFamily as F;
    use crate::model::prim::{BOOL, NUM, STR};
    if !toks.iter().all(|&t| m.closed_marker(t)) {
        return None;
    }
    let (rest, mut receivers) = nullish_arms(m, toks, prim).unwrap_or((prim, Vec::new()));
    if rest == 0 || rest & !(STR | NUM | BOOL) != 0 {
        return None;
    }
    for (bit, fam, name) in [
        (STR, F::Str, "primitive:string"),
        (NUM, F::Num, "primitive:number"),
        (BOOL, F::Bool, "primitive:boolean"),
    ] {
        if rest & bit == 0 {
            continue;
        }
        if !family_absent(m, fam, key, a) {
            return None;
        }
        receivers.push(name.to_string());
    }
    receivers.sort();
    Some(receivers)
}

/// `%Object.prototype%` holds nothing stored under `a` by the program: no
/// named store of `a`, no computed store onto it.
fn object_proto_unwritten(m: &Model, a: Atom) -> bool {
    let Some(&op) = m.tok_index.get(&TokKind::ObjectProto) else {
        return false;
    };
    let empty = |n: Option<&NodeId>| {
        n.is_none_or(|&n| {
            let nd = &m.nodes[n as usize];
            !nd.dropped && nd.pts.is_empty() && nd.prim == 0
        })
    };
    empty(m.fields.get(&(op, a))) && empty(m.field_any.get(&op))
}

/// [`Verdict::IntrinsicCall`]: the callee set is one `BuiltinFn` token of a
/// named native (closed markers stand for their callers' values beside
/// them), no ⊤, no primitive. Its name. The caller adds `!realm_held`.
fn intrinsic_callee(m: &Model, toks: &[Tok], prim: u16) -> Option<String> {
    if prim != 0 {
        return None;
    }
    let mut it = toks.iter().copied().filter(|&t| !m.closed_marker(t));
    let t = it.next()?;
    if it.next().is_some() {
        return None;
    }
    matches!(m.toks[t as usize], TokKind::BuiltinFn(Native::Member { .. })).then(|| m.tok_name(t))
}

/// [`Verdict::NsExport`]: every receiver is a module namespace token of
/// this program's modules (`ns_own` names what each module exports itself;
/// a unit's stand-in namespace has none), the key one of those names, no
/// primitive, no ⊤. A call reads the binding the same way, but the solve
/// lets an unproven call escape its namespace receiver to the callee
/// (DECISIONS 116): the read alone is claimed.
fn ns_export(m: &Model, s: &crate::gen::ReadSite, toks: &[Tok], prim: u16) -> Option<Vec<String>> {
    if s.is_call || prim != 0 || toks.is_empty() || m.is_private_atom(s.key) {
        return None;
    }
    toks.iter()
        .all(|&t| matches!(m.toks[t as usize], TokKind::ModNs(_)) && m.ns_own.contains(&(t, s.key)))
        .then(|| toks.iter().map(|&t| m.tok_name(t)).collect())
}

/// Obligation 3 of [`native_member`]: no object `t` denotes holds key `a`
/// as an own property. A primitive String holds no named key but `length`.
/// An Array: the solve's own value of the key on `t` holds nothing (its
/// field, else its named non-index keys' values `named_any`, else, per
/// unit, its shared cell's named keys): a named store, a store under a key
/// that may be no index, unknown code's writes (an escape, a hard
/// operation: ⊤ and every primitive), another unit's named store (the
/// cell's `#named` contract, DECISIONS 94) all put a value there. A
/// collection's entries are its field values too (`field_any`), so its
/// facts are the writes themselves: no named store of the key on its
/// template (`writers`), no store under a computed key or `__proto__`
/// (`super_relink`), no escape, no hard operation, no accessor. A RegExp's
/// facts are its writes in the same way (its own `lastIndex` is no method),
/// and so are a Buffer's (its elements are no named key).
fn native_own_free(m: &Model, fam: crate::builtins::NativeFamily, t: Tok, a: Atom) -> bool {
    use crate::builtins::NativeFamily;
    let empty = |n: Option<&NodeId>| {
        n.is_none_or(|&n| {
            let nd = &m.nodes[n as usize];
            !nd.dropped && nd.pts.is_empty() && nd.prim == 0
        })
    };
    match fam {
        // A primitive owns no property ToObject's wrapper would not get
        // from its prototype (a Number wrapper owns no string key).
        NativeFamily::Str | NativeFamily::Num | NativeFamily::Bool => true,
        // `__proto__` (or a computed key that may be it) stored into it.
        NativeFamily::Array if m.super_relink.contains(&t) => false,
        NativeFamily::Array => match m.fields.get(&(t, a)) {
            Some(n) => empty(Some(n)),
            None => empty(m.named_any.get(&t)) && empty(m.cell_arrs.get(&t)),
        },
        NativeFamily::Collection(_)
        | NativeFamily::RegExp
        | NativeFamily::Buffer
        | NativeFamily::Promise => {
            let Some(tm) = m.tmpl_of(t) else {
                return false;
            };
            m.writers
                .get(&(tm, a))
                .is_none_or(|w| w.ctor.is_empty() && w.post.is_empty())
                && !m.super_relink.contains(&t)
                && !m.is_escaped(t)
                && !m.hard.contains_key(&t)
                && m.accessor_keys.get(&t).is_none_or(|k| k.is_empty())
        }
    }
}

/// The bodies a callee set holds, or why it is not a set of bodies: it
/// holds ⊤, a builtin function value, a bound function, a class, a function
/// only the runtime has (a resolver), or a value that is no
/// function (calling a primitive throws).
fn callee_set(
    m: &Model,
    runtime: &FxHashSet<FnIdx>,
    toks: &[Tok],
    prim: u16,
    top_node: NodeId,
) -> Result<FxHashMap<FnIdx, String>, String> {
    if toks.contains(&TOP) {
        return Err(if m.explain {
            format!("callee set holds ⊤ <- {}", m.top_origin(top_node))
        } else {
            "callee set holds ⊤".into()
        });
    }
    let mut out = FxHashMap::default();
    for &t in toks {
        let why = match &m.toks[t as usize] {
            TokKind::Func(f, _) if !runtime.contains(f) => {
                out.insert(*f, body_name(m, *f));
                continue;
            }
            TokKind::Obj(..) if m.closed_marker(t) => continue,
            TokKind::BuiltinFn(_) | TokKind::Builtin(_) => {
                "callee set holds a builtin function value".into()
            }
            TokKind::Bound(_) => "callee set holds a bound function".into(),
            TokKind::ClassObj(_) => "callee set holds a class".into(),
            TokKind::Func(..) => "callee set holds a runtime function".into(),
            _ => "callee set holds a non-function value".into(),
        };
        return Err(why);
    }
    if prim != 0 {
        return Err("callee set holds a non-function value".into());
    }
    if out.is_empty() {
        return Err("dead site (no callee flows)".into());
    }
    Ok(out)
}

/// A body's name in a site record: a compiled body by its HIR id too (two
/// methods share a name), another unit's by its contract name.
fn body_name(m: &Model, f: FnIdx) -> String {
    let fi = &m.funcs[f as usize];
    if fi.func_id == u32::MAX {
        fi.name.clone()
    } else {
        format!("{}#{}", fi.name, fi.func_id)
    }
}

/// The read rule's lookup of `key` on receiver token `t` (layout `tf`): one
/// definite data birth field of the layout, or else, per holder, the first
/// prototype on the stable chain that holds the key (F5). The nodes whose
/// values the read gives; a holder that does not have the key and ends its
/// chain makes the read `undefined`, which no call admits.
fn lookup(
    m: &Model,
    tf: &TemplateFacts,
    t: Tok,
    key: &str,
    a: Atom,
) -> Result<Vec<NodeId>, String> {
    let computed = |h: Tok| {
        m.field_any
            .get(&h)
            .is_some_and(|&n| !m.nodes[n as usize].pts.is_empty())
    };
    let holds = |h: Tok| {
        m.fields.get(&(h, a)).copied().filter(|&n| {
            let nd = &m.nodes[n as usize];
            !nd.pts.is_empty() || nd.prim != 0
        })
    };
    if computed(t) {
        return Err("a computed key may hold it".into());
    }
    // A key the layout never writes (a read names it) is the chain's.
    match tf.fields.iter().find(|f| f.is(key, m.is_private_atom(a))) {
        Some(f) if f.definite && f.data_field && f.slot.is_some() => {
            return Ok(holds(t).into_iter().collect());
        }
        Some(f) if !f.data_field => return Err("accessor key (E25)".into()),
        Some(f) if f.writers_ctor + f.writers_post > 0 || tf.birth_keys.contains(&f.key) => {
            return Err("key not a definite birth field".into());
        }
        _ if holds(t).is_some() => return Err("key not a definite birth field".into()),
        _ => {}
    }
    let mut out = Vec::new();
    let mut seen: FxHashSet<Tok> = FxHashSet::default();
    let mut todo = vec![t];
    while let Some(h) = todo.pop() {
        let protos: Vec<Tok> = m
            .proto_node
            .get(&h)
            .map(|&pn| m.nodes[pn as usize].pts.to_sorted())
            .unwrap_or_default();
        if protos.is_empty() {
            return Err("method not found on the chain".into());
        }
        for p in protos {
            if !seen.insert(p) {
                continue;
            }
            if computed(p) {
                return Err("a computed key may hold it".into());
            }
            match holds(p) {
                Some(n) => out.push(n),
                None => todo.push(p),
            }
        }
    }
    Ok(out)
}
