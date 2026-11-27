//! Derived facts F1–F6 read off the solved graph (design section 2), and
//! the per-site verdicts the precision report uses.

mod closed_methods;
pub(crate) mod dispatch;
mod element;
mod member_ops;
pub(crate) mod native_calls;
mod native_member;
use closed_methods::closed_methods;
pub(crate) use element::element_facts;
pub(crate) use element::receiver_set;
pub use element::{ElementFacts, ElementKey, ElementOp, ReceiverValue};
pub(crate) use member_ops::member_op_facts;
use native_member::host_member;
pub(crate) use native_member::native_member;

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
    /// RAYBOX15: the key is absent from every token of the template and
    /// from its whole prototype chain for the program's life: no birth key,
    /// no store names it (no `any` store may name it either), no accessor,
    /// no `[[Prototype]]` write, and the `absent_key` walk ends at
    /// `%Object.prototype%` without a holder, on a template with a fixed
    /// layout. OrdinaryGet (ECMA-262 10.1.8.1) of the key on an instance
    /// gives `undefined`: the outcome the read arm's `slots.push(None)`
    /// proves for a site, as a fact of the class.
    pub absent: bool,
    /// The field may hold a String, and every String any store of any unit
    /// puts there is the value of a program string literal
    /// (`literal_strings`, over every token of the template): never with an
    /// escaped template or a ⊤ kind.
    pub literal_strings: bool,
    /// Every value any store of any unit puts in the field is `true` or
    /// `false` (`booleans`, over every token of the template): never with
    /// an escaped template, a ⊤ kind or an accessor.
    pub booleans: bool,
    /// Every value any store puts in the field is `true`, `false`,
    /// `undefined` or `null` (`booleans::holds_or_nullish`), under the
    /// conditions of `booleans`.
    pub nullish_booleans: bool,
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
    /// Value records (`records`): the template's instances
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
    /// R9READ6: a read or call whose receivers mix kinds (program class
    /// instances, Arrays and typed arrays, String / Number / Boolean
    /// primitives), each decided by the proof its own kind has alone: an
    /// own data slot, a constant prototype member, an absent key
    /// (`undefined`), the intrinsic member of its builtin family
    /// (`native_arm`), one of a closed set of bodies, or, for a call, the
    /// TypeError of calling `undefined` where its chain lacks the key
    /// (7.3.14). The operation's own type dispatch (7.3.2 GetV, 7.1.18
    /// ToObject, 10.1.8.1 OrdinaryGet) tells the kinds apart at run time.
    /// Count-only: no consumer lowers it (codegen keeps the generic path,
    /// no quiet read, no check-mode claim).
    PerKind,
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
    /// itself (`ns_own`) or one `export *` resolves to (`resolve_export`,
    /// ResolveExport 16.2.1.6.3: never ambiguous): [[Get]] gives the export
    /// binding's current value, `GetBindingValue` of the module's binding
    /// (ECMA-262 10.4.6.8). No code writes a namespace ([[Set]] 10.4.6.9 and
    /// [[DefineOwnProperty]] 10.4.6.6 change nothing, [[Prototype]] is null
    /// and fixed, 10.4.6.1-2), unknown code holding it included: exact under
    /// a held realm. The claim is the binding, not a value: an `export let`
    /// its module reassigns is read live.
    NsExport,
    /// ACCSITE1 P2: a named read (the GetValue of a compound too) whose key,
    /// on every receiver, OrdinaryGet (ECMA-262 10.1.8.1) finds as ONE
    /// accessor property of a program prototype, the same getter function
    /// for all: no receiver can own the key (no birth key, no store, no
    /// computed store, no own accessor), no data property and no other
    /// definition of it sits on the chain before that holder, no prototype
    /// on the way is escaped or hard. The read is [[Call]] of the getter
    /// with `this` = the receiver (`targets`).
    AccessorGet,
    /// The same for a named store: OrdinarySet (10.1.9.2) finds the
    /// accessor and calls its one setter with the receiver (`targets`).
    AccessorSet,
    /// A named store whose receivers are program class instances that lack
    /// the key and whose chains lack it (`absent_key`): OrdinarySet creates
    /// an own data property (10.1.9.2 step 2.c-e). Unproven in round one
    /// (owner D1): no direct lowering exists.
    AddsKey,
    /// R9BIRTH4 (owner 2026-11-15): a named store whose receiver set is ONE
    /// program object, a class's prototype object or its class constructor
    /// (`known_object_store`), where OrdinarySet (ECMA-262 10.1.9.2) adds
    /// or overwrites an own data property of that object: no accessor of the
    /// key on it or its chain, no builtin member of the key on the chain,
    /// never frozen, sealed, non-extensible, given a defined key, relinked or
    /// held by unknown code. Counted proven in its own column ("known-object
    /// stores"); the emitted store stays the generic one.
    KnownObjectStore,
    /// PROXY3 (owner 2026-11-16: trap sites count as proven, in their own
    /// column): a read, call or named store whose receivers are all Proxy
    /// objects of `new Proxy( t, h )` that the solver modeled
    /// (`solve/proxy.rs`): the operation calls the trap read
    /// from the program handler `h` with `this = h` and the spec arguments
    /// (ECMA-262 10.5.8 [[Get]], 10.5.9 [[Set]]), or forwards to `t`, and
    /// both are in the solve. The emitted operation stays the generic one
    /// (the runtime's Proxy dispatch).
    ProxyTrap,
    No(String),
}

/// ACCSITE1 D2: the access a site record names; one record for every
/// member access, no separate store table.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum Access {
    /// A read, or a call (`is_call`).
    #[default]
    Read,
    /// A named store `o.k = v` or a compound's PutValue (13.15.2-3);
    /// `strict`: a failed [[Set]] throws (10.1.9, 13.15.2 PutValue step 5.d).
    /// `uint_value`: the stored value is a Number literal that is an integer
    /// in 0 ..= 2^32 - 1 (R9STORE6: `ArraySetLength` runs no user code).
    Set { strict: bool, uint_value: bool },
    /// A compound's GetValue (13.15.3 step 2), at the store's offset.
    UpdateGet,
}

impl Access {
    /// A store site or a compound's read: the "stores" column.
    pub fn is_store(self) -> bool {
        !matches!(self, Access::Read)
    }
}

pub use native_calls::IntrinsicCallee;

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
    pub access: Access,
    pub receivers: Vec<String>,
    /// The unguarded solved values, before a verdict's entry-guard refinement.
    pub receiver_values: Vec<ReceiverValue>,
    pub receiver_prims: u16,
    pub top: bool,
    pub verdict: Verdict,
    /// The shared method-call elision and its optional exact-route proof.
    pub(crate) method_call: Option<dispatch::MethodCallCertificate>,
    /// The audited named builtin route, including its runtime protocols.
    pub(crate) builtin_dispatch: bool,
    /// MATHB1: a call whose receiver (a member call) or callee (a value
    /// call) names a builtin namespace member: is the callee exactly the
    /// original builtin ([`namespace_callee`])? `None`: no namespace member.
    pub intrinsic_callee: Option<IntrinsicCallee>,
    pub fixed_shape: bool,
    /// A call site: the bodies its callee set holds (every body, sorted;
    /// empty when the set holds anything that is not a body).
    pub targets: Vec<String>,
    /// The HIR identity `(module, function id)` of each body in `targets`,
    /// sorted (`field_read_calls`).
    pub target_fns: Vec<(u32, u32)>,
    /// ONCHG2: a `T0Mono` / `T0Set` call whose callee set is only compiled
    /// closures and function declarations: each as a plain body codegen can
    /// call through the loaded function object (`bindings::BodyRef`), sorted.
    /// Empty when a body in `targets` is a method, a body without a HIR id
    /// or the set is no set of bodies.
    pub target_bodies: Vec<crate::bindings::BodyRef>,
    /// A `T0Mono` / `T0Set` call whose every receiver resolves the key to the
    /// one method its class (or an ancestor) declares under it: no own value,
    /// no write to a prototype on the way. The receiver's class alone then
    /// names the body (`closed_methods`).
    pub closed_methods: bool,
    /// `AccessorGet` / `AccessorSet`: the program class id whose prototype
    /// holds the one accessor function in `targets` (the class template its
    /// `this` belongs to). Codegen calls that class's accessor directly.
    /// A `T0Mono` / `T0Set` call whose callee set is one body: the class whose method it is (its
    /// `this` template's class); codegen calls that body directly with the
    /// receiver as `this` (ECMA-262 13.3.6.2, 7.3.14 Call).
    pub holder_class: Option<u32>,
    /// DECISIONS 225: the bodies holding the site, in its module (HIR
    /// function id, and for a synthetic context its name; `Model::act_key`).
    pub bodies: Vec<(u32, String)>,
}

/// DESIGN-native-dispatch §2: the dispatcher obligation of a named member
/// call: Get-then-Call of `key` on every receiver the solve gives the site,
/// unguarded (an entry guard's refinement is no linker proof).
#[derive(Clone, Copy, Debug)]
pub struct DispatchObligation<'a> {
    pub key: &'a str,
    pub receivers: &'a [ReceiverValue],
    pub prims: u16,
    /// A HIR-specialized member op: its variant (`None`: a named site).
    pub op: Option<&'a str>,
    /// A namespace member op: the namespace, a direct call.
    pub namespace: Option<&'a str>,
    /// The op's fallback edge into the dispatcher can be taken
    /// ([`crate::MemberOpFacts::redispatch`]); named calls require a
    /// certificate for the actual lowering, including every failed guard.
    pub redispatch: bool,
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
        self.booleans &= o.booleans;
        self.nullish_booleans &= o.nullish_booleans;
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
        // R9BIRTH7: a settled top-level `X.k = v` defines the own data
        // property before any code can read `X` (`gen::settled`), as a
        // class-body static field does.
        let settled = m
            .settled_static
            .get(&(tm, a))
            .is_some_and(|w| window_own(m, w));
        if m.static_fields.contains(&(tm, a)) || !stored || settled {
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
        || !matches!(
            tpl.kind,
            TmplKind::Class {
                literal_shape: false,
                ..
            }
        )
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
    // Rule 3 (`gen::proto_first`, RAYBOX3): a key that a static block opening
    // the class body stores on the prototype (`C.prototype.K = lit`). The
    // static elements run in ClassDefinitionEvaluation (15.7.14) before the
    // outer binding is initialized; the inner one is reachable only from code
    // of the class body, and none of it runs before the store (the scan
    // admits only earlier such stores and literal static fields). So the key
    // is an own data property of the prototype before any instance or any
    // other code can reach the prototype. It stays one while every writer of
    // the key on the prototype is such a store (`proto_writers`), no builtin
    // prototype is reshaped under the key, and the prototype checks above
    // hold (no escape, no HARD operation such as delete or defineProperty, no
    // computed-key store, no accessor): the solver's `proto_first_holds` at
    // its own link.
    if !m.reshaped_any {
        let mut firsts: Vec<Atom> = m
            .proto_first_keys
            .iter()
            .filter(|&&(c, _)| c == tm)
            .map(|&(_, a)| a)
            .collect();
        firsts.sort_unstable();
        for a in firsts {
            if m.reshaped_keys.contains(&a)
                || m.is_private_atom(a)
                || tpl.accessor_keys.contains(&a)
                || accessors.is_some_and(|ks| ks.contains(&a))
            {
                continue;
            }
            if only_proto_first_writers(m, tm, a)
                && m.fields.get(&(proto, a)).is_some_and(|&n| holds(n))
            {
                out.push(m.atoms[a as usize].clone());
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

/// Rule 3: every writer of `a` on the prototype of `tm` is a static-block
/// store opening the class body (`gen::proto_first`).
fn only_proto_first_writers(m: &Model, tm: TmplId, a: Atom) -> bool {
    m.proto_writers.get(&(tm, a)).is_none_or(|w| {
        w.ctor.is_empty()
            && w.post
                .iter()
                .all(|&s| m.stores[s as usize].proto_first == Some(tm))
    })
}

/// The static prototype chain tokens of a template (plus every prototype
/// object a `new f()` / literal `__proto__` can name).
/// R9BIRTH3 rule M, against the solved program: for each method `k` a
/// constructor on `tm`'s chain (class `Q`) follows (`Model::rule_m`), every
/// instance of `tm` has `tm`'s prototype as its [[Prototype]] (no
/// `Reflect.construct` with another new target, 10.2.2 / 10.1.13), no
/// prototype of a class from `tm` up to below `Q` holds a value or accessor
/// under `k` or an unknown key, and `Q`'s prototype holds exactly one value
/// under `k` (its method: nothing stored another there) and no accessor.
/// Else the followed call may run other code: the chain is not stable.
fn rule_m_problem(m: &Model, tm: TmplId, ts: &[Tok]) -> Option<String> {
    let proto = |q: TmplId| m.tok_index.get(&TokKind::Proto(q)).copied();
    let pts_len = |n: Option<&NodeId>| n.map_or(0, |&n| m.nodes[n as usize].pts.to_sorted().len());
    let mut below: Vec<TmplId> = Vec::new();
    let mut cur = Some(tm);
    let mut checked_instances = false;
    while let Some(q) = cur {
        // R9STORE3 rule S: every instance of `tm` has `tm`'s prototype, and
        // from it up to the class `D` that declares the followed setter of
        // `a` no prototype holds a value, an accessor or an unknown key under
        // `a`; `D`'s prototype holds exactly one setter there and no value:
        // the [[Set]] of every instance calls that setter (10.1.9.2).
        if let Some(ss) = m.rule_s.get(&q) {
            let own = proto(tm);
            for t in ts {
                if let Some(&pn) = m.proto_node.get(t) {
                    let ps = m.nodes[pn as usize].pts.to_sorted();
                    if own.is_none() || ps.iter().any(|&p| Some(p) != own) {
                        return Some("rule S: an instance's prototype is not its class's".into());
                    }
                }
            }
            for &(a, d) in ss {
                let Some(d) = d else {
                    return Some("rule S: setter owner not resolved".into());
                };
                let mut x = Some(tm);
                loop {
                    let Some(xq) = x else {
                        return Some("rule S: setter owner not on the chain".into());
                    };
                    let Some(p) = proto(xq) else {
                        return Some("rule S: no prototype token".into());
                    };
                    let value =
                        pts_len(m.fields.get(&(p, a))) != 0 || pts_len(m.field_any.get(&p)) != 0;
                    if xq == d {
                        if value || m.accessors.get(&(p, a)).is_none_or(|(_, s)| s.len() != 1) {
                            return Some(format!(
                                "rule S: prototype member {} not the setter alone",
                                m.atoms[a as usize]
                            ));
                        }
                        break;
                    }
                    if value || m.accessors.contains_key(&(p, a)) {
                        return Some(format!(
                            "rule S: a prototype below holds {}",
                            m.atoms[a as usize]
                        ));
                    }
                    x = m.tmpls[xq as usize].parent;
                }
            }
        }
        if let Some(names) = m.rule_m.get(&q) {
            if !checked_instances {
                checked_instances = true;
                let own = proto(tm);
                for t in ts {
                    if let Some(&pn) = m.proto_node.get(t) {
                        let ps = m.nodes[pn as usize].pts.to_sorted();
                        if own.is_none() || ps.iter().any(|&p| Some(p) != own) {
                            return Some(
                                "rule M: an instance's prototype is not its class's".into(),
                            );
                        }
                    }
                }
            }
            for &a in names {
                for &d in &below {
                    if let Some(p) = proto(d) {
                        if pts_len(m.fields.get(&(p, a))) != 0
                            || m.accessors.contains_key(&(p, a))
                            || pts_len(m.field_any.get(&p)) != 0
                        {
                            return Some(format!(
                                "rule M: a prototype below holds {}",
                                m.atoms[a as usize]
                            ));
                        }
                    }
                }
                let Some(p) = proto(q) else {
                    return Some("rule M: no prototype token".into());
                };
                if pts_len(m.fields.get(&(p, a))) != 1
                    || m.accessors.contains_key(&(p, a))
                    || pts_len(m.field_any.get(&p)) != 0
                {
                    return Some(format!(
                        "rule M: prototype member {} not the method alone",
                        m.atoms[a as usize]
                    ));
                }
            }
        }
        below.push(q);
        cur = m.tmpls[q as usize].parent;
    }
    None
}

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
    // Only code that may write a token beyond a builtin contract changes
    // its layout; a contract writes only what it states (Phase B).
    let escape = ts.iter().find_map(|&t| m.writer_escape_why(t));
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
            if let Some(r) = m.writer_escape_why(p) {
                chain_problem
                    .get_or_insert_with(|| format!("prototype {} escapes: {r}", m.tok_name(p)));
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
    if chain_problem.is_none() {
        chain_problem = rule_m_problem(m, tm, &ts);
    }
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
        // R9STORE3: a key rule I keeps present from birth on (`inert_sure`:
        // the construction phase stores it before `this` is observed, its
        // chain reads run no code) is definite like a `CtorPhase::definite` key.
        let def_a = tpl.definite.contains(&a) || m.inert_sure(tm, a);
        let added = birth && def_a && !is_accessor;
        let owned = birth && !tpl.per_evaluation && !is_accessor;
        let definite = owned && def_a;
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
        // R9BIRTH1 rule A: a computed-key store after birth writes key `a`
        // only when its key may be `a` (ToPropertyKey of a value of the
        // key's shape, ECMA-262 13.15.2 / 10.1.9.2: OrdinarySet creates or
        // changes only property P); a key of unknown content (shape 0) may
        // be any key.
        let any_post = any
            .post
            .iter()
            .filter(|&&(w, k)| m.any_store_may_name(tm, w, k, a))
            .count();
        let readonly = escape.is_none()
            && hard.is_none()
            && w.post.is_empty()
            && any_post == 0
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
        // I32 at rest (`internals/representation.md` §7, PR A): a whole-life field that holds only
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
        let field_nodes = || -> Vec<NodeId> {
            ts.iter()
                .filter_map(|t| m.fields.get(&(*t, a)).copied())
                .collect()
        };
        let literal_strings =
            !kind.top && !is_accessor && crate::literal_strings::holds(m, &field_nodes());
        let booleans = !kind.top
            && definite
            && !is_accessor
            && escape.is_none()
            && hard.is_none()
            && crate::booleans::holds(m, &field_nodes());
        let nullish_booleans = !kind.top
            && definite
            && !is_accessor
            && escape.is_none()
            && hard.is_none()
            && crate::booleans::holds_or_nullish(m, &field_nodes());
        let absent = !birth
            && !is_accessor
            && escape.is_none()
            && hard.is_none()
            && w.ctor.is_empty()
            && w.post.is_empty()
            && any_post == 0
            && !ts.is_empty()
            && class_absent(m, &tpl.kind, &ts, a);
        fields.push(FieldFacts {
            rep,
            zero_tol,
            rest: false,
            absent,
            literal_strings,
            booleans,
            nullish_booleans,
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
            writers_post: w.post.len() + any_post,
            kind,
        });
    }
    // RAYBOX15: an absent row needs the whole layout fixed for life.
    if !fixed_layout {
        for f in &mut fields {
            f.absent = false;
        }
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
    // ONCHANGE3 rule A: the born-with keys, before any site reads them.
    let born = crate::born_with::derive(g, tfs);
    g.m.born_with = born;
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
    let mut first: FxHashMap<(u32, u32, u32, bool, bool, Access), usize> = FxHashMap::default();
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
    let all: Vec<&crate::gen::ReadSite> = g.read_sites.iter().chain(g.store_sites.iter()).collect();
    for (i, s) in all.iter().enumerate() {
        let k = (
            s.module,
            s.byte_offset,
            s.key,
            s.is_call,
            s.value_call,
            s.access,
        );
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
            || (prim != 0
                && matches!(
                    verdict,
                    Verdict::T0Mono | Verdict::T0Set | Verdict::KnownObjectStore | Verdict::PerKind
                )))
            && all.iter().any(|&r| m.nodes[r as usize].dropped)
        {
            verdict = Verdict::No("primitive receiver (a source left a value out)".into());
        }
        // Design §11.4: a site whose receiver is the `this` of a method of
        // class C runs, in the typed body behind the entry guard
        // `class_id(this) == C`, on C's instances only: E0 when it is T0
        // over C's layout (the generic body keeps the full site). A
        // parameter typed with a class type is guarded the same way.
        if let Verdict::No(_) = verdict {
            let guarded = recvs
                .iter()
                .map(|r| {
                    // DEADARM: a read inside an `if` arm is the arm's gated
                    // copy of the binding (`Gen::region_read`,
                    // `Model::name_of`); the entry guard is the binding's.
                    let r = match m.name_of.get(r) {
                        Some(b) if m.labels[*r as usize] == "live-read" => b,
                        _ => r,
                    };
                    match g.param_guard.get(r) {
                        Some(crate::gen::ParamGuard::Class(tm)) => Some(*tm),
                        _ => m.this_class.get(r).copied(),
                    }
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
                        if m.stop_top.is_some() {
                            // TOPSTOP2: what the unguarded set held (the
                            // site's `top=1` says only "E0 by the guard").
                            let held: Vec<String> =
                                toks.iter().take(6).map(|&t| m.tok_words(t)).collect();
                            eprintln!(
                                "TOPSTOP e0-guard: {} .{} unguarded top={} {:?}; unguarded receivers [{}] prims {prim:#x}",
                                m.pos_text((s.module as u32, s.byte_offset as u32)),
                                key,
                                top,
                                verdict,
                                held.join("; ")
                            );
                        }
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
            set.values(m),
            prim,
        )
    };
    let mut ledger_refused: Vec<crate::hold_ledger::Refused> = Vec::new();
    let mut ledger_totals = [[0usize; 3]; 2];
    for (i, recvs, funcs) in &groups {
        let s = all[*i];
        let key = m.atoms[s.key as usize].clone();
        let (receivers, top, verdict, fixed_shape, targets, receiver_values, receiver_prims) =
            verdict_of(s, recvs);
        if m.ledger.is_some() {
            let store = s.access.is_store();
            let path = g.modules.get(s.module as usize).map_or("", |(p, _)| *p);
            let part = crate::hold_ledger::part(path);
            ledger_totals[store as usize][part] += 1;
            if matches!(verdict, Verdict::No(_)) || (store && matches!(verdict, Verdict::AddsKey)) {
                let set = receiver_set(m, &element::with_copies(&copies, recvs));
                let crate::hold_ledger::Blame {
                    roots,
                    full,
                    causes,
                } = crate::hold_ledger::blame(m, &set.toks, set.top_recv);
                // A site no hold explains is named by its own first refusal
                // (its verdict), so the ledger says why it stays.
                let why = roots.is_empty().then(|| {
                    let names: Vec<String> =
                        set.toks.iter().take(6).map(|&t| m.tok_name(t)).collect();
                    let origin = if set.toks.contains(&crate::model::TOP) {
                        m.top_origin(set.top_recv)
                    } else {
                        String::new()
                    };
                    crate::hold_ledger::Unattributed {
                        why: match &verdict {
                            Verdict::No(r) => r.clone(),
                            _ => "store adds a key".to_owned(),
                        },
                        detail: format!(
                            "{path}\t{}\t{key}\t{}\t{}\t{origin}",
                            s.byte_offset,
                            if store { "store" } else { "read" },
                            names.join(" "),
                        ),
                    }
                });
                ledger_refused.push(crate::hold_ledger::Refused {
                    site: format!("{path}\t{}\t{key}\t{}", s.byte_offset, store as u8),
                    roots,
                    full,
                    causes,
                    part,
                    store,
                    why,
                });
            }
        }
        let intrinsic_callee = if s.is_call && !matches!(s.access, Access::Set { .. }) {
            let all = element::with_copies(&copies, recvs);
            let set = receiver_set(m, &all);
            let dropped = all.iter().any(|&r| m.nodes[r as usize].dropped);
            namespace_callee(m, s, &set.toks, set.prims, dropped)
        } else {
            None
        };
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
        // ONCHANGE2: a call proven to run one body (T0Mono, or T0Set over
        // several receivers whose callee set is that one body) names the
        // class whose method that body is, so codegen calls it directly.
        let holder_class = match (&verdict, targets.len()) {
            (Verdict::AccessorGet | Verdict::AccessorSet, 1)
            | (Verdict::T0Mono | Verdict::T0Set, 1)
                if s.is_call == matches!(verdict, Verdict::T0Mono | Verdict::T0Set) =>
            {
                targets.keys().next().and_then(|&f| {
                    let tm = *m.this_class.get(&m.funcs[f as usize].this)?;
                    match &m.tmpls[tm as usize].kind {
                        crate::model::TmplKind::Class { class_id, .. } => Some(*class_id),
                        _ => None,
                    }
                })
            }
            _ => None,
        };
        let mut target_fns: Vec<(u32, u32)> = targets
            .keys()
            .map(|&f| (m.funcs[f as usize].module, m.funcs[f as usize].func_id))
            .collect();
        target_fns.sort_unstable();
        target_fns.dedup();
        // ONCHG2: the callee set as bodies called through the function object
        // (13.3.6.2 EvaluateCall: the value the member read gives is the
        // callee; 10.2.1 [[Call]] of an ECMAScript function object runs its
        // body with that object's environment): all or nothing.
        let target_bodies = if matches!(verdict, Verdict::T0Mono | Verdict::T0Set) && s.is_call {
            crate::bindings::bodies_of_fns(m, targets.keys().copied())
        } else {
            Vec::new()
        };
        let closed_methods = s.is_call
            && matches!(verdict, Verdict::T0Mono | Verdict::T0Set)
            && closed_methods(
                g,
                &by_id,
                &receiver_set(m, &element::with_copies(&copies, recvs)).toks,
                &key,
                s.key,
            );
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
            method_call: None,
            builtin_dispatch: false,
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
            access: s.access,
            receivers,
            receiver_values,
            receiver_prims,
            top,
            verdict,
            intrinsic_callee,
            fixed_shape,
            targets: t,
            target_fns,
            target_bodies,
            closed_methods,
            holder_class,
        });
    }
    if m.ledger.is_some() {
        crate::hold_ledger::write(m, &ledger_refused, ledger_totals);
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

/// R9BIRTH7: a `length` read whose receivers are each an Array exotic
/// object or a String ([`own_length`]'s: an own, non-configurable data
/// property, 10.4.2, 10.4.3) or a typed array, at least one of them a
/// typed array: an integer-indexed exotic object's [[GetOwnProperty]] of a
/// non-numeric key is ordinary (10.4.5.1), so it owns `length` only when
/// the program defines it (its writes: [`native_own_free`]; a store does
/// not create it, the chain's accessor has no setter, 10.1.9.2); its
/// [[Get]] then reaches `get %TypedArray%.prototype.length` (23.2.3.21)
/// unless a builtin prototype is written ([`builtin_protos_unwritten`]).
/// Each receiver's read is its builtin's: a NativeMember over two
/// families, as OwnLength is over Arrays and Strings.
fn indexed_length(
    m: &Model,
    s: &crate::gen::ReadSite,
    toks: &[Tok],
    prim: u16,
) -> Option<Vec<String>> {
    use crate::builtins::NativeFamily;
    let (prim, mut receivers) = nullish_arms(m, toks, prim).unwrap_or((prim, Vec::new()));
    if prim != 0 && prim != crate::model::prim::STR {
        if why_indexed() {
            eprintln!(
                "WHY_INDEXED length @{} prims {}",
                s.byte_offset,
                crate::model::prim::name(prim)
            );
        }
        return None;
    }
    // Diagnostic (off: one flag read): a receiver refused before the typed
    // array check, with its template kind and ArraySpeciesCreate species.
    let refused = |t: Tok| {
        if why_indexed() {
            let (kind, species) = match m.toks[t as usize] {
                TokKind::Obj(tm, _) => {
                    let tpl = &m.tmpls[tm as usize];
                    let sp = tpl.species.map(|c| {
                        m.nodes[c as usize]
                            .pts
                            .to_sorted()
                            .into_iter()
                            .map(|v| m.tok_name(v))
                            .collect::<Vec<_>>()
                    });
                    (
                        format!("{:?}", tpl.kind)
                            .chars()
                            .take(60)
                            .collect::<String>(),
                        sp,
                    )
                }
                _ => ("not an object".to_string(), None),
            };
            eprintln!(
                "WHY_INDEXED length @{} refused {} marker={} kind={kind} species={species:?}",
                s.byte_offset,
                m.tok_name(t),
                m.is_marker(t)
            );
        }
    };
    let mut typed = false;
    for &t in toks {
        if m.closed_marker(t) {
            continue;
        }
        if !exact_array(m, t) {
            let TokKind::Obj(tm, _) = m.toks[t as usize] else {
                refused(t);
                return None;
            };
            let is_typed = matches!(
                m.tmpls[tm as usize].kind,
                TmplKind::BObj {
                    what: "TypedArray",
                    ..
                }
            );
            if !is_typed
                || m.is_marker(t)
                || !native_own_free(m, NativeFamily::TypedArray, t, s.key)
            {
                if !is_typed {
                    refused(t);
                }
                if is_typed && why_indexed() {
                    let a = s.key;
                    eprintln!(
                        "WHY_INDEXED {} marker={} escaped={:?} hard={:?} writers={} relink={} acc={}",
                        m.tok_name(t),
                        m.is_marker(t),
                        m.escaped.get(&t).map(|r| m.reasons[*r as usize].clone()),
                        m.hard.get(&t).map(|(k, r)| format!("{} {}", k.code(), m.reasons[*r as usize])),
                        m.writers.get(&(tm, a)).is_some_and(|w| !w.ctor.is_empty() || !w.post.is_empty()),
                        m.super_relink.contains(&t),
                        m.accessor_keys.get(&t).is_some_and(|k| !k.is_empty()),
                    );
                }
                return None;
            }
            typed = true;
        }
        receivers.push(m.tmpls[m.tmpl_of(t)? as usize].display.clone());
    }
    if !typed
        || !crate::builtins::native_described(m, NativeFamily::TypedArray, "length", false)
        || builtin_protos_unwritten(m, Some(NativeFamily::TypedArray), s.key).is_err()
    {
        if typed && why_indexed() {
            eprintln!(
                "WHY_INDEXED protos {:?}",
                builtin_protos_unwritten(m, Some(NativeFamily::TypedArray), s.key)
            );
        }
        return None;
    }
    if prim != 0 {
        receivers.push("primitive:string".into());
    }
    receivers.sort();
    receivers.dedup();
    Some(receivers)
}

/// `PERRY_FACTS_WHY_INDEXED=1`: one stderr line per typed-array receiver
/// [`indexed_length`] refuses, naming the failing condition.
pub(crate) fn why_indexed() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("PERRY_FACTS_WHY_INDEXED").is_some())
}

/// R9BIRTH7: `arguments.length` on an arguments object `t` (10.4.4.6,
/// 10.4.4.7: CreateMappedArgumentsObject / CreateUnmappedArgumentsObject
/// define `length` as an own data property, writable and configurable).
/// It stays an own data property unless the program deletes or redefines
/// it: no hard operation on `t` (delete, defineProperty, a prototype or
/// extensibility change), no accessor on it or its template, no data
/// descriptor definition of the key, and `t` never reaches unknown code
/// (no escape). A store keeps it data (10.1.9.2 OrdinarySetWithOwnDescriptor:
/// the existing writable data property takes the value). The value is
/// whatever was last stored (the solve's field) or the argument count.
fn args_own_length(m: &Model, t: Tok, a: Atom) -> bool {
    let TokKind::Obj(tm, _) = m.toks[t as usize] else {
        return false;
    };
    let tpl = &m.tmpls[tm as usize];
    matches!(tpl.kind, TmplKind::Args { .. })
        && !m.is_marker(t)
        && !m.is_escaped(t)
        && !m.hard.contains_key(&t)
        && m.accessor_keys.get(&t).is_none_or(|k| k.is_empty())
        && tpl.accessor_keys.is_empty()
        && !m.defined_keys.get(&tm).is_some_and(|ks| ks.contains(&a))
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
    if let Access::Set { strict, .. } = s.access {
        return judge_store(m, by_id, s, key, toks, prim, strict);
    }
    if s.value_call {
        let top = toks.contains(&TOP);
        // S8 (`ReadSite::global_get`): an `undefined` / `null` callee of
        // the [[Get]] is the throw arm (ReferenceError when absent,
        // 9.1.1.2.6; TypeError, 7.3.14): nothing is called (CWN
        // 2026-11-16: an always-throwing arm counts as proven). The other
        // values are the callee set.
        let throws = s.global_get && prim & (prim::UNDEF | prim::NULL) != 0;
        let prim = if s.global_get {
            prim & !(prim::UNDEF | prim::NULL)
        } else {
            prim
        };
        if throws && prim == 0 && !top && toks.iter().all(|&t| m.closed_marker(t)) {
            let arms = vec!["ReferenceError (absent global)".to_string()];
            return (arms, false, Verdict::PerKind, true, FxHashMap::default());
        }
        if prim == 0 && toks.iter().all(|&t| m.closed_marker(t)) {
            return (Vec::new(), false, Verdict::Dead, true, FxHashMap::default());
        }
        if !m.unit_mode {
            if let Some(name) = intrinsic_callee(m, toks, prim) {
                if !m.realm_held || !intrinsic_replaceable(m, toks) {
                    return (
                        vec![name],
                        false,
                        Verdict::IntrinsicCall,
                        true,
                        FxHashMap::default(),
                    );
                }
                let why =
                    "intrinsic callee: unknown code holds the realm and may replace the member";
                return (
                    Vec::new(),
                    false,
                    Verdict::No(why.into()),
                    true,
                    FxHashMap::default(),
                );
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
        if let Some(receivers) = indexed_length(m, s, toks, prim) {
            return (
                receivers,
                false,
                Verdict::NativeMember,
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
    // R9READ10: a nullish value beside the builtin namespaces is the
    // read's TypeError arm (13.3.2.1, 7.3.2 GetV: ToObject(undefined /
    // null) throws, 7.1.18); the namespaces' arm stays the realm's member.
    // The site is then decided per kind (PerKind: no claim, generic read).
    let nn = prim & (crate::model::prim::NULL | crate::model::prim::UNDEF);
    if let Some(r) = host_member(m, s, toks, prim & !nn) {
        return match r {
            Ok(mut receivers) if nn != 0 => {
                if nn & crate::model::prim::UNDEF != 0 {
                    receivers.push("primitive:undefined (TypeError)".into());
                }
                if nn & crate::model::prim::NULL != 0 {
                    receivers.push("primitive:null (TypeError)".into());
                }
                (
                    receivers,
                    false,
                    Verdict::PerKind,
                    true,
                    FxHashMap::default(),
                )
            }
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
        return (
            receivers,
            false,
            Verdict::NsExport,
            true,
            FxHashMap::default(),
        );
    }
    let mut receivers = Vec::new();
    let mut top = false;
    let mut verdict: Option<Verdict> = None;
    let mut slots: Vec<Option<u32>> = Vec::new();
    // Receivers read by an own data lookup or `undefined` (OwnDataOrAbsent).
    let mut own_or_absent = 0usize;
    // A slot in `slots` that is not a definite own data field: one a
    // private-name lookup reads (not [[Get]]), or a declared / reserved
    // slot that may hold the hole.
    let mut not_definite_slot = false;
    let mut fixed_shape = true;
    // Rule A (ONCHANGE3): a call receiver read by its born-with own slot,
    // and one read on the chain. Both together stay generic (owner
    // 2026-11-15: mixed sites are unproven).
    let (mut born_own, mut chain_read) = (false, false);
    // A call: the nodes whose values the lookups give.
    let mut callee: Vec<NodeId> = Vec::new();
    // Reads of a class object's constant static (3.4).
    let mut statics = 0usize;
    // ACCSITE1 P2: the getter each receiver's lookup ends at.
    let mut acc: Vec<Tok> = Vec::new();
    // R9READ6: receivers decided by their builtin family's intrinsic member
    // (`native_arm`), and call receivers whose chain lacks the key (the
    // TypeError arm of calling `undefined`).
    let (mut native_arms, mut throw_arms) = (0usize, 0usize);
    // PROXY3: receivers that are modeled Proxy objects (`Verdict::ProxyTrap`).
    let mut traps = 0usize;
    for &t in toks {
        match &m.toks[t as usize] {
            TokKind::Proxy(_) => {
                receivers.push(m.tok_name(t));
                traps += 1;
            }
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
                    if !s.is_call && key == "length" && args_own_length(m, t, s.key) {
                        own_or_absent += 1;
                        continue;
                    }
                    // An array the program stores no named key on reads
                    // `undefined` under a key its chain lacks.
                    if !s.is_call && array_absent(m, t, key, s.key) {
                        own_or_absent += 1;
                        continue;
                    }
                    // An array whose own keys are data (a stored named key,
                    // an index, `length`) and whose chain lacks the key past
                    // it: an own data value or `undefined`.
                    if !s.is_call && json_chain_array(m, t, key, s.key) {
                        own_or_absent += 1;
                        continue;
                    }
                    if !s.is_call && json_own_or_absent(m, t, key, s.key) {
                        own_or_absent += 1;
                        if m.json_relink.contains(&t)
                            && json_relink_closed(m, t, key, s.key) == Ok(true)
                        {
                            receivers
                                .push("json: [[Prototype]] chain ends at null (undefined)".into());
                        }
                        continue;
                    }
                    match native_arm(m, s, t, key) {
                        Some(true) => native_arms += 1,
                        Some(false) => throw_arms += 1,
                        None => {
                            verdict.get_or_insert_with(|| {
                                Verdict::No(not_unit_why(m, t, key, s.key, s.is_call))
                            });
                        }
                    }
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
                            not_definite_slot = true;
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
                    match lookup_holders(m, tf, t, key, s.key) {
                        Ok(hs) => {
                            for (h, n) in hs {
                                if h == t && m.born_with.contains(&(t, s.key)) {
                                    born_own = true;
                                } else if h != t {
                                    chain_read = true;
                                }
                                callee.push(n);
                            }
                        }
                        // R9READ6: no holder on the chain and none can
                        // appear (the `absent_key` walk: own field node, computed stores,
                        // accessors, the chain): GetValue gives `undefined`, Call
                        // throws (7.3.14): the receiver's TypeError arm.
                        Err(_) if instance_absent(m, tf, t, key, s.key) => throw_arms += 1,
                        // R9READ11: a key the instance gains by a store that
                        // may not run (a conditional store in a method, a
                        // copy after birth) and that its chain lacks
                        // (`own_data_or_absent`: no accessor, escape, hard
                        // operation or descriptor define on it, nothing on
                        // the chain or on Object.prototype; the computed-key
                        // refusal above came first): OrdinaryGet (10.1.8.1)
                        // reads the own data property, whose values are the
                        // field's (every store of the key on the token), or
                        // `undefined`, and the call throws a TypeError
                        // (13.3.6.2, 7.3.14): the field's callees plus the
                        // TypeError arm; the site stays generic.
                        Err(why)
                            if why == "key not a definite birth field"
                                && own_data_or_absent(m, tf, t, key, s.key) =>
                        {
                            if let Some(&n) = m.fields.get(&(t, s.key)) {
                                callee.push(n);
                            }
                            throw_arms += 1;
                        }
                        Err(why) => {
                            // Diagnostic (off: one flag read): the refused arm.
                            if why_indexed() {
                                eprintln!(
                                    "WHY_ARM @{} {} .{key} call: {why} unwritten={} absent_key={}",
                                    s.byte_offset,
                                    m.tok_name(t),
                                    key_unwritten(m, tf, key, s.key),
                                    absent_key(m, t, key, s.key)
                                );
                            }
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
                let unwritten = key_unwritten(m, tf, key, s.key);
                match row {
                    Some(f)
                        if (f.definite || f.declared || f.reserved)
                            && f.data_field
                            && f.slot.is_some() =>
                    {
                        // A declared or reserved slot may hold the hole
                        // (the key absent: [[Get]] goes on up the chain).
                        not_definite_slot |= !f.definite;
                        slots.push(f.slot)
                    }
                    Some(f) if !f.data_field => match accessor_one(m, t, s.key, false) {
                        // An own data property shadows the getter (10.1.8.1).
                        Ok(AccOne::Fn(_)) if may_own_data_key(m, *tm, s.key) => {
                            verdict.get_or_insert(Verdict::No(
                                "read: an instance may own the key".into(),
                            ));
                        }
                        Ok(AccOne::Fn(x)) => acc.push(x),
                        _ => {
                            verdict.get_or_insert(Verdict::No("accessor key (E25)".into()));
                        }
                    },
                    // An absent key is one outcome, as one slot is: the
                    // read gives `undefined` (`None` in `slots`).
                    // (`fixed_layout`: the `None` slot names the full layout.)
                    _ if tf.fixed_layout && instance_absent(m, tf, t, key, s.key) => {
                        slots.push(None)
                    }
                    _ if own_data_or_absent(m, tf, t, key, s.key) => own_or_absent += 1,
                    // Rule B3: a key the object never holds as its own (no
                    // store names it, no computed key may hold it: the
                    // `lookup` walk) is read where calls find it: OrdinaryGet
                    // (ECMA-262 10.1.8.1) climbs the stable chain to the first
                    // holder. Proven when every holder is a program class
                    // prototype whose own member is constant (15.7.14), the
                    // same read as one on the prototype itself. The layout
                    // need not be fixed (R9BIRTH5): OrdinaryGet stops at the
                    // first object that has the key itself, so keys the
                    // instance gains after birth that are not this key do not
                    // move the holder; `unwritten` and the walk (no named,
                    // computed or assign/define store may give the instance or
                    // a link below the holder the key) keep it the holder.
                    _ if unwritten
                        && lookup_holders(m, tf, t, key, s.key).is_ok_and(|hs| {
                            !hs.is_empty()
                                && hs.iter().all(|&(h, _)| match &m.toks[h as usize] {
                                    // R9BIRTH6: or a key a settled top-level
                                    // store added (`present_from_start`): the
                                    // field holds every value stored there.
                                    TokKind::Proto(tm) => {
                                        by_id.get(tm).is_some_and(|pf| {
                                            pf.proto_consts.iter().any(|k| k == key)
                                        }) || (m.settled_proto.contains_key(&(*tm, s.key))
                                            && present_from_start(m, h, s.key)
                                            && !m
                                                .accessor_keys
                                                .get(&h)
                                                .is_some_and(|ks| ks.contains(&s.key)))
                                    }
                                    _ => false,
                                })
                        }) =>
                    {
                        statics += 1
                    }
                    // An inherited accessor (ACCSITE1 S5), when no instance
                    // owns the key as data (10.1.8.1 step 1 finds that first).
                    Some(_) | None
                        if !may_own_data_key(m, *tm, s.key)
                            && matches!(accessor_one(m, t, s.key, false), Ok(AccOne::Fn(_))) =>
                    {
                        if let Ok(AccOne::Fn(x)) = accessor_one(m, t, s.key, false) {
                            acc.push(x);
                        }
                    }
                    Some(_) | None => {
                        // Diagnostic (off: one flag read): the refused arm.
                        if why_indexed() {
                            eprintln!(
                                "WHY_ARM @{} {} .{key} read: row={:?} unwritten={unwritten} fixed={} absent_key={} own_or_absent={}",
                                s.byte_offset,
                                m.tok_name(t),
                                row.map(|f| (f.writers_ctor, f.writers_post, f.definite, f.declared, f.data_field, tf.birth_keys.contains(&f.key))),
                                tf.fixed_layout,
                                absent_key(m, t, key, s.key),
                                own_data_or_absent(m, tf, t, key, s.key)
                            );
                            for w in m
                                .writers
                                .get(&(*tm, s.key))
                                .iter()
                                .flat_map(|w| w.post.iter())
                            {
                                eprintln!(
                                    "WHY_ARM_W @{} named post {}",
                                    s.byte_offset,
                                    m.stores.get(*w as usize).map_or("?", |x| x.desc.as_str())
                                );
                            }
                            for (w, k) in m.any_writers.get(tm).iter().flat_map(|w| w.post.iter()) {
                                if m.any_store_may_name(*tm, *w, *k, s.key) {
                                    eprintln!("WHY_ARM_W @{} any post {} fn={} m={} loop={:?} key={:?} shape={:?}", s.byte_offset, m.stores.get(*w as usize).map_or("?", |x| x.desc.as_str()), m.stores.get(*w as usize).and_then(|x| m.funcs.get(x.func as usize)).map_or("?", |f| f.name.as_str()), m.stores.get(*w as usize).map_or(0, |x| x.module), m.stores.get(*w as usize).map(|x| x.in_loop), k, match k { crate::model::Key::Any(i) => m.key_shapes.get(*i as usize).map(|x| (x.globs.clone(), x.free.len())), _ => None });
                                }
                            }
                        }
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
            // R9READ10: the own `length` of a builtin function value, a
            // Number no program operation reached (`builtin_fn_own_length`,
            // 20.2.4.1): the intrinsic member of its kind; no user code runs.
            TokKind::Builtin(_) | TokKind::BuiltinFn(_)
                if !s.is_call && crate::builtins::builtin_fn_own_length(m, t, s.key) =>
            {
                receivers.push(m.tok_name(t));
                native_arms += 1;
            }
            // R9READ11: a call of a `Function.prototype` method (`F.call(
            // x )`) on a builtin method value: the realm's method of
            // %Function.prototype% (10.3.4: its [[Prototype]]; 20.2.3.3
            // `call`), no own key, no write of the key onto a builtin
            // prototype (where the model's builtin prototypes collapse) or
            // through ⊤. The callee is that builtin; the site stays generic.
            TokKind::BuiltinFn(_)
                if s.is_call
                    && crate::builtins::builtin_fn_proto_method(m, t, s.key)
                    && object_proto_unwritten(m, s.key)
                    && builtin_protos_unwritten(m, None, s.key).is_ok() =>
            {
                receivers.push(m.tok_name(t));
                native_arms += 1;
            }
            // R9READ11: a read of a member of `Array.prototype` taken as a
            // value (`Array.prototype.slice`: `builtins::builtin_proto_value`
            // gives its `NativeProto` token): %Array.prototype% is an Array
            // exotic object (23.1.3) holding the realm's Array methods as own
            // data; the member is the described one when no receiver holds
            // a program-made own key (`native_own_free`) and no builtin
            // prototype is written under the key (`builtin_protos_unwritten`,
            // where a write through the value lands: `native_proto_alias`).
            // A read only: the site stays generic (PerKind, no claim).
            TokKind::NativeProto(_, Some(crate::model::BuiltinNs::Array))
                if !s.is_call
                    && !m.unit_mode
                    && !m.realm_held
                    && crate::builtins::native_described(
                        m,
                        crate::builtins::NativeFamily::Array,
                        &m.atoms[s.key as usize],
                        false,
                    )
                    && native_own_free(m, crate::builtins::NativeFamily::Array, t, s.key)
                    && builtin_protos_unwritten(
                        m,
                        Some(crate::builtins::NativeFamily::Array),
                        s.key,
                    )
                    .is_ok() =>
            {
                receivers.push(m.tok_name(t));
                native_arms += 1;
            }
            // R9READ9: a read of a key a builtin function value and its
            // chain lack (`builtins::builtin_value_absent`), with no write of
            // the key onto `Object.prototype` (where the model's builtin
            // prototypes collapse) or through ⊤: `undefined` (10.1.8.1).
            //
            // R9READ10: for a call `o.k( … )` that `undefined` is the
            // callee and the call throws a TypeError (13.3.6.2
            // EvaluateCall: IsCallable(undefined) is false, 7.3.14): the
            // receiver's TypeError arm, as for an instance whose chain lacks
            // the key (`instance_absent`); the other arms' callees are
            // checked below and the site stays generic (CWN 2026-11-16: an
            // always-TypeError arm counts as proven).
            TokKind::Builtin(_) | TokKind::BuiltinFn(_)
                if crate::builtins::builtin_value_absent(m, t, s.key)
                    && object_proto_unwritten(m, s.key)
                    && builtin_protos_unwritten(m, None, s.key).is_ok() =>
            {
                receivers.push(m.tok_name(t));
                if s.is_call {
                    throw_arms += 1;
                } else {
                    own_or_absent += 1;
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
    // the `None` slot. A call on one of them reads `undefined` there and
    // throws: its TypeError arm.
    if !none && prim != 0 && verdict.is_none() {
        match beside_prims(m, key, s.key, prim, s.is_call) {
            Ok((r, absent)) => {
                receivers.extend(r);
                // R9READ6: beside constant chain members the primitive's
                // absent outcome is one more decided arm (PerKind below).
                if absent && own_or_absent > 0 {
                    own_or_absent += 1;
                } else if absent {
                    slots.push(None);
                }
            }
            Err(w) => {
                verdict = Some(Verdict::No(format!(
                    "primitive beside an object (key not absent on its family) {}: {w}",
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
    if born_own && chain_read {
        verdict.get_or_insert(Verdict::No(
            "mixed: born-with own slot and chain receivers".into(),
        ));
    }
    // PROXY3: only modeled Proxy objects: the trap verdict. Beside any other
    // receiver or a primitive the site is not decided by one rule.
    //
    // PROXY9 (owner 2026-11-17): beside `undefined` / `null` only, the site
    // is still the trap verdict: there GetV's ToObject throws the TypeError
    // (7.3.2, 7.1.18), the always-throws arm `beside_prims` named above. The
    // emitted operation stays the generic one, which throws it as node does.
    if traps > 0 {
        use crate::model::prim::{NULL, UNDEF};
        verdict.get_or_insert(if traps == toks.len() && prim & !(NULL | UNDEF) == 0 {
            Verdict::ProxyTrap
        } else {
            Verdict::No("mixed: Proxy and other receivers".into())
        });
    }
    let mut targets = FxHashMap::default();
    let verdict = match verdict {
        Some(v) => v,
        // R9READ6: a call where some receiver runs its family's intrinsic
        // or throws: the bodies the others reach must still be a closed
        // set; the site stays generic (no `method_calls` target).
        None if s.is_call && native_arms + throw_arms > 0 => {
            callee.sort_unstable();
            callee.dedup();
            let mut ts: Vec<Tok> = callee
                .iter()
                .flat_map(|&n| m.nodes[n as usize].pts.to_sorted())
                .collect();
            ts.sort_unstable();
            ts.dedup();
            let p = callee.iter().fold(0, |p, &n| p | m.nodes[n as usize].prim);
            // R9READ11: beside a TypeError arm an `undefined` / `null`
            // callee is that arm (7.3.14 Call: IsCallable false throws).
            let p = if throw_arms > 0 {
                p & !(crate::model::prim::UNDEF | crate::model::prim::NULL)
            } else {
                p
            };
            let top_node = callee
                .iter()
                .copied()
                .find(|&n| m.nodes[n as usize].pts.contains_tok(TOP))
                .unwrap_or(top_recv);
            if callee.is_empty() {
                Verdict::PerKind
            } else {
                match callee_set(m, runtime, &ts, p, top_node) {
                    Ok(_) => Verdict::PerKind,
                    Err(why) => Verdict::No(why),
                }
            }
        }
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
        None if !acc.is_empty() => accessor_verdict(
            m,
            &mut acc,
            slots.len() + own_or_absent + statics + native_arms,
            &mut targets,
            false,
        ),
        // R9READ6: each receiver's [[Get]] decided by its own kind: an own
        // data slot or `undefined`, a constant member on its chain, the
        // intrinsic member of its family.
        None if native_arms > 0 => Verdict::PerKind,
        None if own_or_absent > 0 && statics > 0 => Verdict::PerKind,
        None if own_or_absent > 0 => Verdict::OwnDataOrAbsent,
        None if statics > 0 && !slots.is_empty() => Verdict::PerKind,
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
            } else if !not_definite_slot && !m.is_private_atom(s.key) && !m.optimistic_json_key {
                // Each entry is one receiver's [[Get]] outcome (10.1.8.1):
                // `Some` a definite own data field (found first, whatever
                // the chain holds), `None` a key absent from the object and
                // its whole chain (`undefined`). Different slots, or a slot
                // beside an absent key, are still an own data value or
                // `undefined` on every receiver: the `own_or_absent` outcome.
                Verdict::OwnDataOrAbsent
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

/// An instance of template `tm` may own `a` as a data property: a class
/// field (DefineField, 7.3.33: a birth key), a birth descriptor, or a later
/// data definition (rule D2, `defined_keys`). OrdinaryGet (ECMA-262 10.1.8.1
/// step 1) and OrdinarySet (10.1.9.2 step 1) find that own property first, so
/// an inherited accessor of `a` never runs for it: `accessor_one`'s
/// precondition, checked by every caller that takes its accessor arm.
fn may_own_data_key(m: &Model, tm: TmplId, a: Atom) -> bool {
    m.tmpls[tm as usize].birth_keys.contains(&a)
        || m.birth_attrs.contains_key(&(tm, a))
        || m.defined_keys.get(&tm).is_some_and(|ks| ks.contains(&a))
}

/// ACCSITE1: where a named lookup of `a` from receiver `t` ends.
enum AccOne {
    /// The one accessor function (getter for a read, setter for a store).
    Fn(Tok),
    /// A store whose accessor has a getter and no setter: [[Set]] returns
    /// false (10.1.9.2 step 7), PutValue throws in strict code.
    GetterOnly,
    /// No holder on the chain.
    Absent,
}

/// ACCSITE1 P2: the OrdinaryGet / OrdinarySet walk (ECMA-262 10.1.8.1,
/// 10.1.9.2) of named key `a` from receiver `t`, when it ends at one
/// accessor property. `t` must not own the key in any form (no field node
/// with values, no computed store, no own accessor); each prototype on the
/// way neither holds the key as data nor may gain it (computed store,
/// writer escape, hard operation), and the chain cannot end at `null` before the
/// holder. Every holder found names the same getter and setter lists.
fn accessor_one(m: &Model, t: Tok, a: Atom, set: bool) -> Result<AccOne, String> {
    let has = |n: NodeId| {
        let nd = &m.nodes[n as usize];
        !nd.pts.is_empty() || nd.prim != 0
    };
    let holds = |h: Tok| m.fields.get(&(h, a)).is_some_and(|&n| has(n));
    let computed = |h: Tok| m.field_any.get(&h).is_some_and(|&n| has(n));
    // The receiver is a prefix-stable instance (the callers' rule: no hard
    // operation, no escape), so no [[DefineOwnProperty]] gives it the key;
    // a [[Set]] of the key (a named or a computed store) finds the accessor
    // on the chain and never creates an own property (10.1.9.2 step 3). The
    // field node the solve also writes is that arm's over-approximation
    // (ACCSITE1 D3), not an own property. An own accessor defines it.
    if m.accessors.contains_key(&(t, a)) {
        return Err("the receiver may own the key".into());
    }
    let mut found: Option<&(Vec<Tok>, Vec<Tok>)> = None;
    let mut ended = false;
    let mut seen: FxHashSet<Tok> = FxHashSet::default();
    let mut todo = vec![t];
    // A prototype gains the key by a computed store, a hard operation or
    // code that may write it under any key (`writer_escape_why`, as the
    // template's chain check reads it): code handed it by a builtin contract
    // writes only what the contract states, which the solve models (Phase B,
    // owner 2026-11-23). `Object.prototype` escapes along the chain of every
    // handed value, and unknown code never writes a member of an intrinsic
    // it reaches along a handed value's chain (owner premise 2026-11-19,
    // the closed world): only the program's writes (`computed`, `holds`,
    // its accessors) and a hard operation give it the key. Code beside the
    // program (`foreign_code`) may write it.
    let objproto = m.tok_index.get(&TokKind::ObjectProto).copied();
    let may_gain = |p: Tok| {
        computed(p)
            || m.hard.contains_key(&p)
            || ((Some(p) != objproto || m.foreign_code()) && m.writer_escape_why(p).is_some())
    };
    while let Some(h) = todo.pop() {
        let protos: Vec<Tok> = match m.proto_node.get(&h) {
            Some(&pn) if m.nodes[pn as usize].prim != 0 => {
                return Err("the chain may end at a null prototype".into())
            }
            Some(&pn) => m.nodes[pn as usize].pts.to_sorted(),
            None => Vec::new(),
        };
        if protos.is_empty() {
            ended = true;
            continue;
        }
        for p in protos {
            if !seen.insert(p) {
                continue;
            }
            if p == TOP || m.is_marker(p) {
                return Err("a prototype from unmodeled code".into());
            }
            if may_gain(p) {
                return Err("a prototype on the chain may gain the key".into());
            }
            match m.accessors.get(&(p, a)) {
                Some(acc) => {
                    if holds(p) {
                        return Err("data and accessor definitions of the key".into());
                    }
                    match found {
                        None => found = Some(acc),
                        Some(f) if f == acc => {}
                        Some(_) => return Err("different accessors on the chains".into()),
                    }
                }
                None if holds(p) => return Err("a data property on the chain".into()),
                None => todo.push(p),
            }
        }
    }
    let Some((g, s)) = found else {
        return Ok(AccOne::Absent);
    };
    if ended {
        return Err("a chain may end before the accessor".into());
    }
    let list = if set { s } else { g };
    match list.as_slice() {
        [x] if m.tok_fn(*x) != NO_FN => Ok(AccOne::Fn(*x)),
        [] if set && !g.is_empty() => Ok(AccOne::GetterOnly),
        _ => Err("not exactly one accessor function".into()),
    }
}

/// The accessor verdict over the receivers' accessor functions `acc`;
/// `others`: receivers that resolved the key otherwise.
fn accessor_verdict(
    m: &Model,
    acc: &mut Vec<Tok>,
    others: usize,
    targets: &mut FxHashMap<FnIdx, String>,
    set: bool,
) -> Verdict {
    acc.sort_unstable();
    acc.dedup();
    if others > 0 {
        return Verdict::No("receivers mix an accessor with data or absent keys".into());
    }
    if acc.len() != 1 {
        return Verdict::No("receivers resolve the key to different accessors".into());
    }
    let f = m.tok_fn(acc[0]);
    targets.insert(f, body_name(m, f));
    if set {
        Verdict::AccessorSet
    } else {
        Verdict::AccessorGet
    }
}

/// ACCSITE1 P1/P2: the verdict of a named store site `o.k = v` (ECMA-262
/// 13.15.2 PutValue, 10.1.9.2 OrdinarySet). Per receiver (a program class
/// instance, prefix-stable): an own definite writable data field at a slot
/// (step 2: the own data descriptor is written); else the one accessor the
/// chain ends at (`accessor_one`: the setter is called; no setter: strict
/// code throws); else a key absent from receiver and chain (`absent_key`):
/// the store adds an own key. One kind over all receivers.
#[allow(clippy::too_many_arguments)]
fn judge_store(
    m: &Model,
    by_id: &FxHashMap<TmplId, &TemplateFacts>,
    s: &crate::gen::ReadSite,
    key: &str,
    toks: &[Tok],
    prim: u16,
    strict: bool,
) -> (Vec<String>, bool, Verdict, bool, FxHashMap<FnIdx, String>) {
    let mut receivers: Vec<String> = Vec::new();
    let mut top = false;
    let mut fixed_shape = true;
    let mut why: Option<String> = None;
    let mut slots: Vec<u32> = Vec::new();
    let mut acc: Vec<Tok> = Vec::new();
    let (mut throws, mut adds) = (0usize, 0usize);
    // PROXY3: receivers that are modeled Proxy objects.
    let mut traps = 0usize;
    // R9STORE4 rule A: receivers where the store adds or overwrites an own
    // data property with no user code (`instance_sets`).
    let mut sets = 0usize;
    let private = m.is_private_atom(s.key);
    // R9BIRTH4: one known object (a class's prototype or constructor) as
    // the whole receiver set.
    if let ([t], 0) = (toks, prim) {
        if matches!(
            m.toks[*t as usize],
            TokKind::Proto(_) | TokKind::ClassObj(_)
        ) {
            let verdict = match known_object_store(m, *t, key, s.key) {
                Ok(()) => Verdict::KnownObjectStore,
                Err(w) => Verdict::No(format!("store: {w}")),
            };
            return (
                vec![m.tok_name(*t)],
                false,
                verdict,
                true,
                FxHashMap::default(),
            );
        }
    }
    for &t in toks {
        match &m.toks[t as usize] {
            TokKind::Top => {
                top = true;
                why.get_or_insert("store: receiver from unmodeled code (TOP)".into());
            }
            TokKind::Obj(..) if m.closed_marker(t) => {}
            TokKind::Obj(tm, _) if !m.is_marker(t) && by_id.contains_key(tm) => {
                let tf = by_id[tm];
                receivers.push(tf.display.clone());
                if m.tmpls[*tm as usize].per_evaluation && private {
                    why.get_or_insert("store: per class evaluation".into());
                    continue;
                }
                if !tf.prefix_stable {
                    why.get_or_insert("store: receiver layout is not prefix-stable".into());
                    continue;
                }
                if !tf.fixed_layout {
                    fixed_shape = false;
                }
                let row = tf.fields.iter().find(|f| f.is(key, private));
                match row {
                    // A key a data descriptor defines (at birth, BD-A, or
                    // later, D2) may be non-writable: 10.1.9.2 step 2.a.
                    Some(f)
                        if f.data_field
                            && (m.birth_attrs.contains_key(&(*tm, s.key))
                                || m.defined_keys
                                    .get(tm)
                                    .is_some_and(|ks| ks.contains(&s.key))) =>
                    {
                        why.get_or_insert("store: a defined key (its attributes decide)".into());
                    }
                    Some(f) if f.data_field && f.definite && f.slot.is_some() => {
                        slots.push(f.slot.unwrap_or_default())
                    }
                    _ => match accessor_one(m, t, s.key, true) {
                        // R9STORE6: the setter is the FIRST holder for every
                        // instance only if none owns the key as data: a class
                        // field (DefineField, 7.3.33: a birth key), a birth
                        // descriptor or a later data definition (rule D2,
                        // `defined_keys`) makes an own property that 10.1.9.2
                        // step 1 finds first. `accessor_one`'s precondition.
                        Ok(AccOne::Fn(_)) if may_own_data_key(m, *tm, s.key) => {
                            why.get_or_insert("store: an instance may own the key".into());
                        }
                        Ok(AccOne::Fn(x)) => acc.push(x),
                        Ok(AccOne::GetterOnly) if strict => throws += 1,
                        Ok(AccOne::GetterOnly) => {
                            why.get_or_insert("store: getter-only accessor in sloppy code".into());
                        }
                        Ok(AccOne::Absent) if row.is_none() && absent_key(m, t, key, s.key) => {
                            adds += 1
                        }
                        Ok(AccOne::Absent) if instance_sets(m, t, key, s.key) => sets += 1,
                        Ok(AccOne::Absent) => {
                            why.get_or_insert("store: key not a definite birth field".into());
                        }
                        // R9STORE6 item 2: the first holder is a writable data
                        // property of a program prototype (a class method).
                        Err(_) if proto_data_sets(m, t, key, s.key) => sets += 1,
                        Err(w) => {
                            why.get_or_insert(format!("store: {w}"));
                        }
                    },
                }
            }
            // R9STORE6 item 3: `arr.length = n` onto an Array.
            _ if key == "length"
                && matches!(
                    s.access,
                    Access::Set {
                        uint_value: true,
                        ..
                    }
                )
                && array_length_sets(m, t, s.key) =>
            {
                receivers.push(m.tok_name(t));
                sets += 1;
            }
            // PROXY3: a modeled Proxy object: its `set` trap or the forward
            // (10.5.9; `Verdict::ProxyTrap`).
            TokKind::Proxy(_) => {
                receivers.push(m.tok_name(t));
                traps += 1;
            }
            // BIRTH1 rule GS: a program-owned key of the global object.
            _ if global_user_sets(m, t, key, s.key) => {
                receivers.push(m.tok_name(t));
                sets += 1;
            }
            // R9STORE5 (i): a builtin function value or an Array.
            _ if builtin_store_sets(m, t, key, s.key) => {
                receivers.push(m.tok_name(t));
                sets += 1;
            }
            _ => {
                receivers.push(m.tok_name(t));
                why.get_or_insert("store: receiver is not a program class instance".into());
            }
        }
    }
    let mut seen_r: FxHashSet<String> = FxHashSet::default();
    receivers.retain(|r| seen_r.insert(r.clone()));
    // R9STORE3 rule N: `undefined` / `null` beside object receivers. PutValue
    // (ECMA-262 6.2.5.6 step 3.a) takes ToObject of the base first: a nullish
    // base throws the TypeError (7.1.18) before any [[Set]], after the value
    // was evaluated (13.15.2); the objects store as their own verdict says.
    // So the store is the object arm's verdict plus its TypeError arm, the
    // read side's `nullish_arms` form; proven only when the object arm is a
    // proven slot store (codegen: the receiver test sends a nullish value to
    // the complete [[Set]], CLS D3). Any other primitive is not this rule.
    // R9STORE5 (ii): a Number, String, Boolean, BigInt or Symbol base in
    // strict code: [[Set]] on its wrapper with the primitive as Receiver
    // (6.2.5.6 step 3.a-c) returns false at 10.1.9.2 step 2.b (a data
    // property) or at step 3 (an accessor without a setter); PutValue throws
    // the TypeError (step 3.e). Only a setter runs code: the one builtin
    // setter on these chains is `Object.prototype.__proto__` (B.2.2.1.2),
    // refused with any program write onto a builtin prototype under the key
    // (`builtin_chain_plain`). A TypeError arm as rule N's.
    let prim_obj = prim & !(crate::model::prim::NULL | crate::model::prim::UNDEF);
    let prim_throws = prim_obj == 0 || (strict && builtin_chain_plain(m, key, s.key));
    let nullish_beside = prim != 0 && prim_throws && !toks.iter().all(|&t| m.closed_marker(t));
    // R9STORE4 rule T: only `undefined` / `null` reach the base (no object,
    // every token a closed marker): PutValue's ToObject (6.2.5.6 step 3.a,
    // 7.1.18) throws the TypeError after the value was evaluated (13.15.2),
    // on every run: `Verdict::Throws` (the caller demotes it when a source
    // left a value out). The emitted store stays the generic one.
    let only_nullish = prim != 0 && prim_throws && toks.iter().all(|&t| m.closed_marker(t));
    if prim != 0 && !nullish_beside && !only_nullish {
        why.get_or_insert("store: primitive receiver".into());
    }
    let mut targets = FxHashMap::default();
    let kinds = [
        !slots.is_empty(),
        !acc.is_empty(),
        throws > 0,
        adds > 0,
        sets > 0,
    ]
    .iter()
    .filter(|&&b| b)
    .count();
    let one_setter = {
        let mut a = acc.clone();
        a.sort_unstable();
        a.dedup();
        a.len() == 1
    };
    let verdict = match why {
        Some(w) => Verdict::No(w),
        // PROXY9 (owner 2026-11-17): beside `undefined` / `null` only, still
        // the trap verdict: PutValue's ToObject throws the TypeError there
        // (6.2.5.6 step 3.a, 7.1.18), the always-throws arm of rule T.
        None if traps > 0
            && traps == toks.len()
            && prim & !(crate::model::prim::NULL | crate::model::prim::UNDEF) == 0 =>
        {
            Verdict::ProxyTrap
        }
        None if traps > 0 => Verdict::No("store: Proxy beside other receivers".into()),
        None if only_nullish => {
            receivers.extend(prim_throw_arms(prim));
            Verdict::Throws
        }
        None if sets > 0 && acc.is_empty() && throws == 0 => Verdict::KnownObjectStore,
        None if kinds == 0 => Verdict::Dead,
        // R9STORE6 item 1 (CWN decision 2026-11-16: a per-kind store verdict
        // counts as proven): receivers whose chain ends at ONE known setter
        // (`accessor_one`: 10.1.9.2 step 3 / 7, Call(setter, Receiver, << V >>))
        // beside receivers whose store adds or sets an own data property
        // (rules A / R9STORE5, step 2.c-e). Every arm exact; no slot claim and
        // no setter claim (`targets` stays empty): codegen keeps the generic
        // store, which performs each arm's [[Set]].
        None if one_setter && throws == 0 && slots.is_empty() && (sets > 0 || adds > 0) => {
            Verdict::KnownObjectStore
        }
        None if kinds > 1 => Verdict::No("store: receivers mix store kinds".into()),
        None if !slots.is_empty() => {
            if slots.iter().all(|&x| x == slots[0]) {
                if receivers.len() == 1 {
                    Verdict::T0Mono
                } else {
                    Verdict::T0Set
                }
            } else {
                Verdict::No("store: slots differ across the receiver set".into())
            }
        }
        None if !acc.is_empty() => accessor_verdict(m, &mut acc, 0, &mut targets, true),
        None if throws > 0 => Verdict::Throws,
        None => Verdict::AddsKey,
    };
    let verdict = match verdict {
        // A slot store's receiver test sends only a nullish base to the
        // complete [[Set]] (rule N); another primitive arm composes with the
        // generic store a KnownObjectStore keeps. COVER3: a store proven to
        // call one setter takes the same receiver test (`undefined` / `null`
        // only): the nullish base goes to the complete [[Set]], whose
        // ToObject throws the TypeError after the value was evaluated
        // (6.2.5.6 step 3.a, 13.15.2); an object calls the setter.
        v @ (Verdict::T0Mono
        | Verdict::T0Set
        | Verdict::KnownObjectStore
        | Verdict::ProxyTrap
        | Verdict::AccessorSet)
            if nullish_beside && (prim_obj == 0 || matches!(v, Verdict::KnownObjectStore)) =>
        {
            receivers.extend(prim_throw_arms(prim));
            v
        }
        Verdict::No(w) => Verdict::No(w),
        _ if nullish_beside => Verdict::No("store: primitive receiver".into()),
        v => v,
    };
    // R9STORE1: a store the rules above leave undecided (an add, or a key
    // that is no definite birth field) onto ONE object literal allocation,
    // whose effect `literal_store` decides exactly. A slot store stays one.
    let verdict = match (verdict, toks, prim) {
        (Verdict::AddsKey | Verdict::No(_), [t], 0) if literal_store(m, *t, key, s.key).is_ok() => {
            Verdict::KnownObjectStore
        }
        (v, _, _) => v,
    };
    (receivers, top, verdict, fixed_shape, targets)
}

/// The TypeError arms of a store's primitive receivers, by kind.
fn prim_throw_arms(prim: u16) -> Vec<String> {
    use crate::model::prim::*;
    [
        (NUM, "number"),
        (STR, "string"),
        (TRUE | FALSE, "boolean"),
        (BIGINT, "bigint"),
        (SYMBOL, "symbol"),
        (UNDEF, "undefined"),
        (NULL, "null"),
    ]
    .iter()
    .filter(|(b, _)| prim & b != 0)
    .map(|(_, n)| format!("primitive:{n} (TypeError)"))
    .collect()
}

/// R9STORE5: no builtin prototype holds an accessor or a non-writable
/// property under `key` that the description does not list: no program
/// write or definition reaches a builtin prototype under it
/// (`builtin_protos_unwritten`, `object_proto_unwritten`), no unknown code
/// holds the realm, no interpreted code runs; not `__proto__` (the
/// `Object.prototype` accessor, B.2.2.1) and not a private name.
fn builtin_chain_plain(m: &Model, key: &str, a: Atom) -> bool {
    !m.unit_mode
        && !m.realm_held
        && !m.interp_runs
        && !m.is_private_atom(a)
        && key != "__proto__"
        && builtin_protos_unwritten(m, None, a).is_ok()
        && object_proto_unwritten(m, a)
}

/// R9STORE5 (i): a named store of `key` onto `t` adds or overwrites an own
/// data property with no user code: a builtin function value
/// (`builtins::builtin_value_sets`) or an Array (`exact_array`) under a key
/// that is no array index and not `length`: an Array's [[Set]] is
/// OrdinarySet and its [[DefineOwnProperty]] is OrdinaryDefineOwnProperty
/// for such a key (10.4.2.1 step 3; the chain `Array.prototype` /
/// `Object.prototype` has no setter for it, `builtin_chain_plain`), on an
/// array never HARD, escaped or relinked and with no own accessor.
fn builtin_store_sets(m: &Model, t: Tok, key: &str, a: Atom) -> bool {
    let array = || {
        exact_array(m, t)
            && key != "length"
            && !crate::builtins::members::is_index_name(key)
            && !m.is_escaped(t)
            && !m.hard.contains_key(&t)
            && !m.super_relink.contains(&t)
            && !m.accessors.contains_key(&(t, a))
            && m.accessor_keys.get(&t).is_none_or(|k| !k.contains(&a))
    };
    builtin_chain_plain(m, key, a) && (crate::builtins::builtin_value_sets(m, t, a) || array())
}

/// BIRTH1 rule GS: a named store of `key` (atom `a`) onto the global
/// object `t` (`globalThis.k = v`, `g["k"] = v` with `g` the global object)
/// where `key` is a program-owned global (`global_user_key`: neither the
/// realm's `%Global%` declarations nor `Object.prototype` list it) adds or
/// overwrites an own data property and runs no user code. The global
/// object is an ordinary extensible object (ECMA-262 19, 9.1.1.4); 10.1.9.2
/// OrdinarySetWithOwnDescriptor: its own `key`, if any, is a writable data
/// property, since only program stores make it (the solver models each as
/// the global's field, `solve/store.rs`, while `native_written` is false:
/// no store into ⊤ and no unknown writer names the key; in whole-program
/// mode an escaped global records no any-key write, owner decision
/// 2026-11-18, closed world 11-14) and no accessor, definition, freeze,
/// seal or preventExtensions reaches it (`accessors`, `accessor_keys`,
/// `hard`); else the chain `%Object.prototype%` lacks the key
/// and no program write or definition reaches a builtin prototype under it
/// (`object_proto_unwritten`, `builtin_protos_unwritten`), so step 2.e
/// CreateDataProperty creates it. Not `__proto__`, not a private name, no
/// interpreted code, not unit mode (another unit's writes are unseen).
/// A key the realm declares counts when it is a writable data global the
/// runtime cannot call (`declared_data_global`, owner decision 2026-11-19).
fn global_user_sets(m: &Model, t: Tok, key: &str, a: Atom) -> bool {
    let star = m.atom_index.get(crate::STAR_KEY).copied();
    crate::builtins::global_object(m, t)
        && !m.unit_mode
        && !m.interp_runs
        && !m.is_private_atom(a)
        && key != "__proto__"
        && if crate::builtins::global_user_key(m, key) {
            !crate::builtins::native_written(m, t, a)
        } else {
            // A declared key has no R1 field (`builtin_field`): the
            // program's own [[Set]] is recorded as a plain data write onto
            // the global object, the value `global_value_written` keeps in
            // the global's field. Every other writer still counts there
            // (unknown code through ⊤ once the global object reached it, a
            // write under any key, an accessor, a given prototype, the
            // realm replaced); a definition or an integrity operation
            // makes the global object hard (below).
            crate::builtins::declared_data_global(m, t, key)
                && !crate::builtins::global_value_written(m, t, a)
        }
        && !m.hard.contains_key(&t)
        && !m.super_relink.contains(&t)
        && !m.accessors.contains_key(&(t, a))
        && m.accessor_keys
            .get(&t)
            .is_none_or(|k| !k.iter().any(|&k| k == a || Some(k) == star))
        && object_proto_unwritten(m, a)
        && builtin_protos_unwritten(m, None, a).is_ok()
}

/// R9STORE6 item 3: `t.length = n`, `t` an Array exotic object
/// (`exact_array`), `n` a Number literal integer in 0 ..= 2^32 - 1
/// (`Access::Set::uint_value`). 10.1.9.2 OrdinarySet: `length` is an own
/// data property of every Array (10.4.2.2 ArrayCreate step 6), so step 2.d
/// calls the array's [[DefineOwnProperty]] (10.4.2.1 step 1) and
/// ArraySetLength (10.4.2.4): ToUint32(n) = ToNumber(n) (no RangeError, no
/// user code: a primitive); `length` stays writable and every element
/// configurable unless a definition or an integrity operation reaches the
/// array: `Object.defineProperty` / freeze / seal / preventExtensions mark
/// it HARD (E5 / E8), so step 17-19 deletes the elements >= n and sets the
/// length; nothing throws. Not escaped (unknown code could define), no own
/// accessor (none can exist on an Array's `length`).
fn array_length_sets(m: &Model, t: Tok, a: Atom) -> bool {
    exact_array(m, t)
        && !m.is_escaped(t)
        && !m.hard.contains_key(&t)
        && !m.accessors.contains_key(&(t, a))
        && m.accessor_keys.get(&t).is_none_or(|k| !k.contains(&a))
}

/// R9STORE4 rule A (CWN decision 2026-11-16): a named store of `key`
/// (atom `a`) onto `t`, an instance of a prefix-stable program class (the
/// caller's row: no hard operation on any instance (E5 defineProperty, E8
/// freeze / seal / preventExtensions, E9 Proxy), no escape, no native
/// parent, no defined key) whose chain `accessor_one` found free of an
/// accessor or a data property under the key, adds or overwrites an own
/// data property of `t` and runs no user code: ECMA-262 10.1.9.2
/// OrdinarySetWithOwnDescriptor: an own `key` is a writable data property
/// (only a definition makes one otherwise, refused above) and is set in
/// place (step 2.c-d); else the chain, program prototypes only ending at
/// %Object.prototype% which lacks the key and which no program write or
/// definition reaches under it (`chain_absent`), holds no property of the
/// key, so step 2.e CreateDataProperty creates it on the extensible `t`.
/// Not relinked (`__proto__` store, B.2.2.1.2) and not a private name.
/// With a nullish receiver beside it (rule N) the generic store the
/// verdict keeps throws the TypeError of 6.2.5.6 step 3.a.
fn instance_sets(m: &Model, t: Tok, key: &str, a: Atom) -> bool {
    matches!(m.toks[t as usize], TokKind::Obj(..))
        && !m.is_marker(t)
        && !m.is_private_atom(a)
        && !m.is_escaped(t)
        && !m.hard.contains_key(&t)
        && !m.super_relink.contains(&t)
        && !m.accessors.contains_key(&(t, a))
        && m.accessor_keys.get(&t).is_none_or(|k| !k.contains(&a))
        && chain_absent(m, t, key, a, false)
}

/// R9STORE6 item 2: a named store of `key` (atom `a`) onto `t`, a program
/// class instance with the own conditions of [`instance_sets`], whose chain
/// holds the key as a DATA property of a program prototype (a class method:
/// DefineMethodProperty, ECMA-262 15.7.14 / 10.2.8, [[Writable]] true; or a
/// plain store onto the prototype). 10.1.9.2 OrdinarySetWithOwnDescriptor:
/// the walk (step 1.c, the parent's [[Set]]) stops at the first holder; its
/// descriptor is a writable data property (step 2.a passes: only a
/// definition makes one non-writable, and `Object.defineProperty` of a
/// member key marks the prototype HARD, solve/apply.rs ~915; freeze /
/// seal too), so step 2.c-e sets the Receiver's own writable data
/// property or creates it on the extensible `t`. No user code runs. Each
/// prototype on the way: a program prototype with no accessor and no
/// accessor definition under the key, no computed store, not HARD, not
/// escaped, not relinked; a branch that holds no data property reaches
/// %Object.prototype%, which lacks the key and which no program write
/// reaches under it (as `chain_absent`).
fn proto_data_sets(m: &Model, t: Tok, key: &str, a: Atom) -> bool {
    let own = matches!(m.toks[t as usize], TokKind::Obj(..))
        && !m.is_marker(t)
        && !m.is_private_atom(a)
        && !m.is_escaped(t)
        && !m.hard.contains_key(&t)
        && !m.super_relink.contains(&t)
        && !m.accessors.contains_key(&(t, a))
        && m.accessor_keys.get(&t).is_none_or(|k| !k.contains(&a));
    let Some(&op) = m.tok_index.get(&TokKind::ObjectProto) else {
        return false;
    };
    if !own {
        return false;
    }
    let has = |n: Option<&NodeId>| {
        n.is_some_and(|&n| {
            let nd = &m.nodes[n as usize];
            nd.dropped || !nd.pts.is_empty() || nd.prim != 0
        })
    };
    let (mut held, mut ends) = (false, false);
    let mut seen: FxHashSet<Tok> = FxHashSet::default();
    let mut todo = vec![t];
    while let Some(h) = todo.pop() {
        let Some(protos) = m.proto_node.get(&h).map(|&pn| &m.nodes[pn as usize]) else {
            return false;
        };
        if protos.prim != 0 || protos.dropped || protos.pts.is_empty() {
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
            let plain = matches!(m.toks[p as usize], TokKind::Proto(_))
                && !m.hard.contains_key(&p)
                && !m.is_escaped(p)
                && !m.super_relink.contains(&p)
                && !m.accessors.contains_key(&(p, a))
                && m.accessor_keys.get(&p).is_none_or(|k| !k.contains(&a))
                && !has(m.field_any.get(&p));
            if !plain {
                return false;
            }
            if has(m.fields.get(&(p, a))) {
                held = true;
            } else {
                todo.push(p);
            }
        }
    }
    held && (!ends
        || (crate::builtins::object_proto_lacks(key)
            && builtin_protos_unwritten(m, None, a).is_ok()))
}

/// R9STORE1: may a named store of `key` (atom `a`) onto `t`, one object
/// literal allocation site (`TmplKind::Lit` or a literal-shape class),
/// be decided exactly as "adds or overwrites an own data property of the
/// object" (ECMA-262 13.15.2 PutValue, 10.1.9.2 OrdinarySet, 10.1.9.2
/// step 3.e CreateDataProperty / step 2.c the value of a writable own data
/// property)? The literal is born by OrdinaryObjectCreate(%Object.prototype%)
/// (13.2.5.5) with writable data properties (CreateDataPropertyOrThrow),
/// and its own accessors only from `get` / `set` entries. Own: no accessor
/// of the key and no accessor definition under it, no computed store onto
/// it (a computed `__proto__` [[Set]] relinks it, B.3.1), never HARD (no
/// freeze / seal / preventExtensions, defineProperty, delete, relink or
/// Proxy target: E5-E9), not escaped (no unknown code holds it) and no
/// marker. Chain: exactly %Object.prototype% (a `__proto__: x` entry sets
/// another), which lacks the key (`__proto__` included: refused) and which
/// no program write or definition reaches under it. Each run of the site
/// is one object; the verdict holds for every run, under any branch.
fn literal_store(m: &Model, t: Tok, key: &str, a: Atom) -> Result<(), String> {
    let TokKind::Obj(tm, _) = m.toks[t as usize] else {
        return Err("not an object literal".into());
    };
    let literal = match &m.tmpls[tm as usize].kind {
        TmplKind::Lit { enum_image, .. } => !enum_image,
        TmplKind::Class { literal_shape, .. } => *literal_shape,
        _ => false,
    };
    if !literal || m.is_marker(t) || m.closed_marker(t) {
        return Err("not an object literal".into());
    }
    if m.is_private_atom(a) {
        return Err("a private name".into());
    }
    if m.is_escaped(t) {
        return Err("a known object unknown code may hold".into());
    }
    if m.hard.contains_key(&t) || m.super_relink.contains(&t) {
        return Err("a known object with a hard operation".into());
    }
    if m.accessors.contains_key(&(t, a)) || m.accessor_keys.get(&t).is_some_and(|k| k.contains(&a))
    {
        return Err("an accessor of the key on the known object".into());
    }
    if m.field_any.get(&t).is_some_and(|&n| {
        let nd = &m.nodes[n as usize];
        nd.dropped || !nd.pts.is_empty() || nd.prim != 0
    }) {
        return Err("a computed store onto the object".into());
    }
    // The chain: %Object.prototype%, directly (a `Lit`) or through the
    // literal-shape class's own prototype object, the lowering's stand-in
    // for the literal's [[Prototype]] link: one the program never writes,
    // defines on, relinks or hands to unknown code.
    let Some(&op) = m.tok_index.get(&TokKind::ObjectProto) else {
        return Err("no %Object.prototype%".into());
    };
    let protos = |h: Tok| {
        m.proto_node.get(&h).and_then(|&pn| {
            let nd = &m.nodes[pn as usize];
            (nd.prim == 0 && !nd.dropped).then(|| nd.pts.to_sorted())
        })
    };
    let empty = |n: Option<&NodeId>| {
        n.is_none_or(|&n| {
            let nd = &m.nodes[n as usize];
            !nd.dropped && nd.pts.is_empty() && nd.prim == 0
        })
    };
    let chain_ok = match protos(t).as_deref() {
        Some([p]) if *p == op => true,
        Some([p]) => {
            m.toks[*p as usize] == TokKind::Proto(tm)
                && !m.is_marker(*p)
                && !m.is_escaped(*p)
                && !m.hard.contains_key(p)
                && !m.super_relink.contains(p)
                && empty(m.fields.get(&(*p, a)))
                && empty(m.field_any.get(p))
                && !m.accessors.contains_key(&(*p, a))
                && m.accessor_keys.get(p).is_none_or(|k| !k.contains(&a))
                && protos(*p).as_deref() == Some(&[op][..])
        }
        _ => false,
    };
    if !chain_ok {
        return Err("a prototype other than %Object.prototype%".into());
    }
    if !crate::builtins::object_proto_lacks(key) {
        return Err("the key is a builtin prototype member".into());
    }
    builtin_protos_unwritten(m, None, a)?;
    Ok(())
}

/// R9BIRTH4: may a named store of `key` (atom `a`) onto `t`, a class's
/// prototype object or class constructor, be decided exactly as "adds or
/// overwrites an own data property of `t`" (ECMA-262 10.1.9.2
/// OrdinarySetWithOwnDescriptor)? `t` is one object: its class is
/// evaluated once (`per_evaluation` classes mint one per evaluation).
/// Own: no accessor of the key (a class getter / setter, static or not);
/// an own data property of it is writable (methods and fields are created
/// writable, 10.2.8 / 7.3.6; only a definition makes one non-writable,
/// which marks the object HARD). A class constructor's own `prototype`,
/// `length` and `name` are non-writable (15.7.14, 10.2.9-10): they are
/// %Function.prototype% members by name, refused below. Chain: every
/// prototype a program class's prototype / constructor, with no accessor
/// of the key, not HARD, not escaped, not relinked, ending at
/// %Object.prototype% (a constructor: at %Function.prototype%, which
/// `ObjectProto` stands for too); the key is no member of the builtin
/// prototype the chain ends at (`__proto__`, Function's `caller` /
/// `arguments` accessors included), and no program write or definition
/// reaches a builtin prototype under it (`builtin_protos_unwritten`).
/// Extensible: `t` is never frozen, sealed or made non-extensible (E8),
/// defined on (E5, or an accessor key of a prototype), deleted from (E7),
/// relinked (E6) or a Proxy target (E9): any of those is HARD. Not
/// escaped: no unknown code holds it.
fn known_object_store(m: &Model, t: Tok, key: &str, a: Atom) -> Result<(), String> {
    let is_ctor = match m.toks[t as usize] {
        TokKind::ClassObj(tm) | TokKind::Proto(tm) => {
            if m.tmpls[tm as usize].per_evaluation {
                return Err("a class evaluated more than once".into());
            }
            matches!(m.toks[t as usize], TokKind::ClassObj(_))
        }
        _ => return Err("receiver is not a program class instance".into()),
    };
    if m.is_private_atom(a) {
        return Err("a private name".into());
    }
    let lacks = if is_ctor {
        crate::builtins::function_chain_lacks(key)
    } else {
        crate::builtins::object_proto_lacks(key)
    };
    if !lacks {
        return Err("the key is a builtin prototype member".into());
    }
    let mut seen: FxHashSet<Tok> = FxHashSet::default();
    let mut todo = vec![t];
    let mut ends = false;
    while let Some(h) = todo.pop() {
        if !seen.insert(h) {
            continue;
        }
        if h != t && !matches!(m.toks[h as usize], TokKind::Proto(_) | TokKind::ClassObj(_)) {
            return Err("a prototype from unmodeled code".into());
        }
        if m.is_marker(h) || m.is_escaped(h) {
            return Err("a known object unknown code may hold".into());
        }
        if m.hard.contains_key(&h) || m.super_relink.contains(&h) {
            return Err("a known object with a hard operation".into());
        }
        // A definition onto a prototype under a key it holds no method of
        // is an accessor key there (`Cons::DefineKey`), as a getter is.
        if m.accessors.contains_key(&(h, a))
            || m.accessor_keys.get(&h).is_some_and(|k| k.contains(&a))
        {
            return Err("an accessor of the key on the known object or its chain".into());
        }
        let Some(&pn) = m.proto_node.get(&h) else {
            return Err("an unknown prototype chain".into());
        };
        let nd = &m.nodes[pn as usize];
        if nd.prim != 0 || nd.pts.is_empty() {
            return Err("a chain that may end at null".into());
        }
        for p in nd.pts.to_sorted() {
            if p == TOP {
                return Err("a prototype from unmodeled code".into());
            }
            if m.toks[p as usize] == TokKind::ObjectProto {
                ends = true;
            } else {
                todo.push(p);
            }
        }
    }
    if !ends {
        return Err("a chain that may not end at a builtin prototype".into());
    }
    builtin_protos_unwritten(m, None, a)?;
    Ok(())
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
    // PSTORE (owner 2026-11-24, the E20 realm hold dropped): a program
    // store onto a prototype on the family's chain is a member of THAT
    // intrinsic, which every lookup on a value of the family passes
    // (`builtins::intrinsic_miss_on`). A realm prototype taken as a value
    // (`Storage.prototype`, `Response.prototype`: `builtins::realm_proto_of`)
    // is one object of its own, its writes are recorded under its identity
    // alone (`solve::native_proto_alias`), not under `ObjectProto`: ask
    // each prototype of the realm's declared chain (`native_chain`) here.
    if let Some(fam) = fam.filter(|_| !locked) {
        for name in crate::builtins::native_chain(fam.chain_holder()) {
            let Some(&np) = m.tok_index.get(&TokKind::native_proto(&name)) else {
                continue;
            };
            let (top, own) = crate::builtins::native_written_by(m, np, a);
            if own {
                return Err(format!(
                    "native member: the program writes the key onto {name}.prototype"
                ));
            }
            if top {
                return Err(format!(
                    "native member: a store into ⊤ may write the key onto {name}.prototype"
                ));
            }
        }
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

/// No store of the program names key `a` on instances of `tf`: its row
/// is one only a read made, or there is none.
fn key_unwritten(m: &Model, tf: &TemplateFacts, key: &str, a: Atom) -> bool {
    tf.fields
        .iter()
        .find(|f| f.is(key, m.is_private_atom(a)))
        .is_none_or(|f| f.writers_ctor + f.writers_post == 0 && !tf.birth_keys.contains(&f.key))
}

/// Key `a` is absent on class instance `t` (template `tf`) and its whole
/// chain for the instance's life: no store names it (`key_unwritten`) and
/// the `absent_key` walk holds. [[Get]] gives `undefined` (10.1.8.1); the
/// one proof of a read's absent arm and of a call's TypeError arm (7.3.14).
fn instance_absent(m: &Model, tf: &TemplateFacts, t: Tok, key: &str, a: Atom) -> bool {
    key_unwritten(m, tf, key, a) && absent_key(m, t, key, a)
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
/// `h` holds nothing under `a` (no field value, no `any`-key value), has no
/// `[[Prototype]]` write and no accessor under `a`.
fn tok_unwritten(m: &Model, h: Tok, a: Atom) -> bool {
    let holds_nothing = |n: Option<&NodeId>| {
        n.is_none_or(|&n| {
            let nd = &m.nodes[n as usize];
            !nd.dropped && nd.pts.is_empty() && nd.prim == 0
        })
    };
    holds_nothing(m.fields.get(&(h, a)))
        && holds_nothing(m.field_any.get(&h))
        && !m.super_relink.contains(&h)
        && m.accessor_keys.get(&h).is_none_or(|k| !k.contains(&a))
}

/// RAYBOX15 (`FieldFacts::absent`): every token of a class template lacks
/// `a` as its own and along its whole chain. A token the solve never asked
/// for its `[[Prototype]]` (no `proto_node`) still has the chain the class
/// seeds at birth (`Model::proto_node`: `Proto(constructor_origin)`, the same
/// for every token of the template; a `[[Prototype]]` write goes through
/// that node), so it is judged on its own keys plus a walked token's chain,
/// which holds the seed and more.
fn class_absent(m: &Model, kind: &TmplKind, ts: &[Tok], a: Atom) -> bool {
    if !matches!(kind, TmplKind::Class { .. }) {
        return false;
    }
    let key = &m.atoms[a as usize];
    let (walked, seeded): (Vec<Tok>, Vec<Tok>) =
        ts.iter().partition(|t| m.proto_node.contains_key(*t));
    !walked.is_empty()
        && walked.iter().all(|&t| absent_key(m, t, key, a))
        && seeded.iter().all(|&t| tok_unwritten(m, t, a))
}

fn absent_key(m: &Model, t: Tok, key: &str, a: Atom) -> bool {
    chain_absent(m, t, key, a, true)
}

/// The `absent_key` walk: `t`'s prototype chain holds no `a` and ends at
/// `%Object.prototype%`, which lacks it. `own`: `t` itself holds nothing
/// under `a` either.
fn chain_absent(m: &Model, t: Tok, key: &str, a: Atom, own: bool) -> bool {
    let _holds_nothing = |n: Option<&NodeId>| {
        n.is_none_or(|&n| {
            let nd = &m.nodes[n as usize];
            !nd.dropped && nd.pts.is_empty() && nd.prim == 0
        })
    };
    let unwritten = |h: Tok| tok_unwritten(m, h, a);
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
    matches!(
        m.tmpls[tm as usize].kind,
        TmplKind::BObj { what: "json", .. }
    ) && !m.is_marker(t)
        && own_data_or_absent_tok(m, t, key, a)
}

/// The reason of a receiver token without layout facts that no
/// template-free rule admits: for a `JSON.parse` object the first failing
/// condition of [`own_data_or_absent_tok`].
/// R9READ6: receiver `t`, an Array exotic object (`exact_array`) or a typed
/// array, decided by its family alone, as `native_member` decides a
/// one-family set: `Some(true)` when it reads its family's intrinsic member
/// (described, no own key under `a`: `native_own_free`, no builtin
/// prototype written: `builtin_protos_unwritten`); `Some(false)` for a call
/// on an Array whose chain lacks the key (`array_absent`): GetV gives
/// `undefined` and Call throws a TypeError (7.3.14). `None`: undecided.
/// One receiver of a mixed read or call (`judge`) decided by its native
/// family: `Some(true)` when the single-kind proof of that receiver alone
/// holds (the same functions: `own_length` / `indexed_length` for a
/// `length` read, `native_member`), so a per-kind verdict never accepts a
/// receiver its own kind's rule refuses; `Some(false)` for a call on an
/// Array whose chain lacks the key (`array_absent`: Call of `undefined`
/// throws, 7.3.14).
fn native_arm(m: &Model, s: &crate::gen::ReadSite, t: Tok, key: &str) -> Option<bool> {
    let one = [t];
    let call = s.is_call;
    let own = !call
        && key == "length"
        && (own_length(m, &one, 0).is_some() || indexed_length(m, s, &one, 0).is_some());
    let member = native_member(m, s, &one, 0);
    if why_indexed() {
        eprintln!(
            "WHY_NARM {} .{key} call={call} own_length={own} native_member={:?}",
            m.tok_name(t),
            member.as_ref().map(|_| ())
        );
    }
    if own || member.is_ok() {
        return Some(true);
    }
    if call && exact_array(m, t) && array_absent(m, t, key, s.key) {
        return Some(false);
    }
    None
}

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
/// 7.1.18); each String, Number or Boolean is `family_absent` (GetV reads
/// `undefined`: 7.3.2, 10.1.8.1): for a read that is an absent outcome,
/// for a call it is the TypeError arm (Call of `undefined`, 7.3.14; the
/// runtime's primitive dispatch, `native_call_method_tower`, throws
/// `js_throw_type_error_not_a_function` when the family's prototype has no
/// callable under the key). A BigInt or a Symbol is not covered (no
/// family here). The receivers, and whether an absent outcome is among
/// them.
fn beside_prims(
    m: &Model,
    key: &str,
    a: Atom,
    prim: u16,
    call: bool,
) -> Result<(Vec<String>, bool), String> {
    use crate::builtins::NativeFamily as F;
    use crate::model::prim::{BOOL, NULL, NUM, STR, UNDEF};
    if prim & !(STR | NUM | BOOL | NULL | UNDEF) != 0 {
        return Err("a primitive of no String/Number/Boolean family".into());
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
            if let Err(w) = family_absent_why(m, fam, key, a) {
                return Err(w);
            }
            r.push(if call {
                format!("{name} (TypeError)")
            } else {
                name.to_string()
            });
        }
    }
    let absent = !call && prim & (STR | NUM | BOOL) != 0;
    Ok((r, absent))
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
    let relinked = m.super_relink.contains(&t);
    if relinked && !m.json_relink.contains(&t) {
        return Err("__proto__ relink".into());
    }
    if let Some((k, r)) = m.hard.get(&t) {
        return Err(format!("hard: {} {}", k.code(), m.reasons[*r as usize]));
    }
    if let Some(k) = m.accessor_keys.get(&t).filter(|k| !k.is_empty()) {
        return Err(format!("accessor keys on the token ({})", k.len()));
    }
    if m.tmpl_of(t)
        .and_then(|tm| m.defined_keys.get(&tm))
        .is_some_and(|ks| ks.contains(&a))
    {
        return Err("the key was defined by a data descriptor (its attributes)".into());
    }
    if !m
        .tmpl_of(t)
        .is_some_and(|tm| m.tmpls[tm as usize].accessor_keys.is_empty())
    {
        return Err("accessor keys on the template".into());
    }
    if relinked {
        json_relink_closed(m, t, key, a)?;
    } else if !chain_absent(m, t, key, a, false) {
        return Err("the chain has the key".into());
    }
    if !object_proto_unwritten(m, a) {
        return Err("a write of the key on Object.prototype".into());
    }
    Ok(())
}

/// JSON-RELINK-CLOSED (ECMA-262 10.1.2 OrdinarySetPrototypeOf, 10.1.8.1
/// OrdinaryGet, 25.5.1 JSON.parse, B.2.2.1.2 the `__proto__` setter): the
/// chain of `JSON.parse` object `t` after `__proto__` stores
/// (`json_relink`, modeled by `given_prototype`) holds only `JSON.parse`
/// objects under the same conditions (their own data is data; relinked
/// ones are walked), arrays on which the key is absent (`array_absent`) or
/// whose own keys are data and whose `%Array.prototype%` lacks the key
/// (`json_chain_array`),
/// `%Object.prototype%` and null; the end lacks the key. A named read then
/// gives a data value of a chain object or `undefined`. `Ok(true)`: null
/// may end the chain (the null-ended arm).
fn json_relink_closed(m: &Model, t: Tok, key: &str, a: Atom) -> Result<bool, String> {
    let op = m.tok_index.get(&TokKind::ObjectProto).copied();
    let mut seen: FxHashSet<Tok> = FxHashSet::default();
    let mut todo = vec![t];
    let mut null = false;
    seen.insert(t);
    while let Some(h) = todo.pop() {
        let Some(protos) = m.proto_node.get(&h).map(|&pn| &m.nodes[pn as usize]) else {
            return Err("relink: no prototype set".into());
        };
        if protos.dropped || protos.prim & !crate::model::prim::NULL != 0 {
            return Err("relink: the prototype set is not whole".into());
        }
        null |= protos.prim & crate::model::prim::NULL != 0;
        for p in protos.pts.to_sorted() {
            if Some(p) == op || !seen.insert(p) {
                continue;
            }
            if p == TOP {
                return Err("relink: ⊤ may be the prototype".into());
            }
            if json_own_or_absent_obj(m, p) {
                if m.super_relink.contains(&p) {
                    if !m.json_relink.contains(&p) {
                        return Err("relink: a chain object is relinked otherwise".into());
                    }
                    todo.push(p);
                } else if !m.proto_node.get(&p).is_none_or(|&pn| {
                    m.nodes[pn as usize]
                        .pts
                        .to_sorted()
                        .iter()
                        .all(|&q| Some(q) == op)
                        && m.nodes[pn as usize].prim == 0
                }) {
                    return Err("relink: a chain object's prototype is not Object.prototype".into());
                }
                continue;
            }
            if array_absent(m, p, key, a) || json_chain_array(m, p, key, a) {
                continue;
            }
            return Err(format!(
                "relink: prototype {} may hold the key",
                m.tok_name(p)
            ));
        }
    }
    if !(crate::builtins::object_proto_lacks(key) && builtin_protos_unwritten(m, None, a).is_ok())
        || m.is_private_atom(a)
    {
        return Err("relink: the chain's end may hold the key".into());
    }
    Ok(null)
}

/// An Array exotic object `p` on a relinked `JSON.parse` object's chain
/// whose own keys the program may store (the `t[k] = v` that relinks may
/// write onto `p` too): the read under `key` ends on `p` with a data value
/// or continues at `%Array.prototype%`. Its own keys are data: its indices
/// and `length` (ECMA-262 10.4.2), a named key a store adds
/// (`array_named_property_set`, perry-runtime array/indexing_keyed.rs
/// `js_array_set_string_key`), no accessor (token or template), no hard
/// operation, no escape. A store under `__proto__` onto an Array adds an
/// own named data key in the runtime (the same funnel; `[[Prototype]]`
/// stays `%Array.prototype%`, which `arr.__proto__` reads:
/// get_field_by_name_tail.rs `array_proto_slot`), and a relink by
/// `Object.setPrototypeOf` is hard. `length` is `p`'s own data. Any other
/// key: `%Array.prototype%`'s chain lacks it and nothing writes it there
/// (`family_absent` for the Array family; its own `__proto__`, index and
/// private keys refuse).
fn json_chain_array(m: &Model, p: Tok, key: &str, a: Atom) -> bool {
    let TokKind::Obj(tm, _) = m.toks[p as usize] else {
        return false;
    };
    matches!(m.tmpls[tm as usize].kind, TmplKind::Arr { .. })
        && exact_array(m, p)
        && m.accessor_keys.get(&p).is_none_or(|k| k.is_empty())
        && m.tmpls[tm as usize].accessor_keys.is_empty()
        && !m.hard.contains_key(&p)
        && !m.is_escaped(p)
        && !m.is_private_atom(a)
        && (key == "length" || family_absent(m, crate::builtins::NativeFamily::Array, key, a))
}

/// A `JSON.parse` object whose own keys are data and which unknown code
/// does not reach (the token conditions of [`own_data_or_absent_why`]
/// before the chain).
fn json_own_or_absent_obj(m: &Model, t: Tok) -> bool {
    let TokKind::Obj(tm, _) = m.toks[t as usize] else {
        return false;
    };
    matches!(
        m.tmpls[tm as usize].kind,
        TmplKind::BObj { what: "json", .. }
    ) && !m.is_marker(t)
        && !m.is_escaped(t)
        && !m.hard.contains_key(&t)
        && m.accessor_keys.get(&t).is_none_or(|k| k.is_empty())
        && m.tmpls[tm as usize].accessor_keys.is_empty()
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
    family_absent_why(m, fam, key, a).is_ok()
}

/// [`family_absent`]'s conditions in order; `Err` names the first that
/// fails (the reason text of a refused site).
fn family_absent_why(
    m: &Model,
    fam: crate::builtins::NativeFamily,
    key: &str,
    a: Atom,
) -> Result<(), String> {
    use crate::builtins::NativeFamily as F;
    if (key == "length" && !matches!(fam, F::Num | F::Bool))
        || key == "__proto__"
        || key.starts_with(|c: char| c.is_ascii_digit() || c == '-')
        || matches!(key, "Infinity" | "NaN")
        || m.is_private_atom(a)
    {
        return Err("the family may hold the key".into());
    }
    if m.optimistic_json_key {
        return Err("optimistic JSON-key rule on".into());
    }
    if !crate::builtins::family_chain_lacks(fam, key) {
        return Err("the family's chain holds the key".into());
    }
    if !object_proto_unwritten(m, a) {
        return Err("Object.prototype may be written under the key".into());
    }
    builtin_protos_unwritten(m, Some(fam), a)
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
/// May unknown code that holds the realm (`realm_held`) replace the
/// intrinsic callee in `toks` (`intrinsic_callee`)? A built-in prototype's
/// member, yes. A namespace member (`Math.max`), no: under the closed world
/// (owner 2026-11-14) that code is the runtime's natives, which replace no
/// member of a namespace (owner decision 2026-11-18, R-GLOBAL step 3); only
/// interpreted code or an unseen module does.
fn intrinsic_replaceable(m: &Model, toks: &[Tok]) -> bool {
    if m.interp_runs || m.unseen_module_runs {
        return true;
    }
    !toks.iter().all(|&t| {
        m.closed_marker(t)
            || matches!(
                m.toks[t as usize],
                TokKind::BuiltinFn(Native::Member { identity, .. })
                    if identity
                        .rsplit_once('.')
                        .is_some_and(|(n, _)| crate::builtins::ns_by_name(n).is_some())
            )
    })
}

fn intrinsic_callee(m: &Model, toks: &[Tok], prim: u16) -> Option<String> {
    if prim != 0 {
        return None;
    }
    let mut it = toks.iter().copied().filter(|&t| !m.closed_marker(t));
    let t = it.next()?;
    if it.next().is_some() {
        return None;
    }
    matches!(
        m.toks[t as usize],
        TokKind::BuiltinFn(Native::Member { .. })
    )
    .then(|| m.tok_name(t))
}

/// MATHB1 rule R1: is the callee of call site `s` exactly the realm's
/// original builtin namespace member `N.m`?
///
/// The site names `N.m` when it is a member call `recv.m(…)` whose receiver
/// set holds a builtin namespace token `N` (the direct form is the HIR pair
/// `NamespacePatched(N) ? N.m(…) : <intrinsic>`, whose generic arm is this
/// ordinary member call), or a value call whose callee set holds the
/// builtin function value `N.m` (`const c = N.m; c(…)`). It is exact iff:
/// - the set is that one token (no other token, no ⊤, no primitive) and no
///   source left a value out (`dropped`);
/// - `m` is a member the realm declares on `N` (`members::ns_member`);
/// - no write may reach the member: no `native_writes` entry for (N, m),
///   (N, every key), (⊤, m) or (⊤, every key): every [[Set]] (a store
///   through any reference, a computed store whose key may be `m`, Reflect.set,
///   Object.assign), [[DefineOwnProperty]] (defineProperty /
///   defineProperties / Reflect.defineProperty, accessors), [[Delete]], a
///   Proxy's target, and a value given to unknown code records one there
///   (`note_native_write` / `note_native_data_write`); no program field of
///   N holds a stored value under `m` or a computed key (`fields`,
///   `field_any`);
/// - the global binding `N` is not replaced: no `native_writes` entry on the
///   global object (`globalThis` or the HIR's global sentinel) under the
///   key `N` or every key (`globalThis.N = v`, a sloppy `N = v`,
///   `globalThis[k] = v`);
/// - the whole program is solved (no unit mode: units use the facts-off
///   rule) and no unknown code holds the realm (`realm_held`).
///   CLOSED-WORLD PREMISE (owner 2026-11-14): code the solve does not see
///   (eval, `new Function`, an open native addon) is exactly what
///   `realm_held` names; nothing else can write `N.m`.
///
/// Lexical shadowing (a local, parameter, import or class named `N`) is the
/// HIR's: the pair and the global read are emitted only for an unshadowed
/// `N` (`shadows_unqualified_global`).
pub(crate) fn namespace_callee(
    m: &Model,
    s: &crate::gen::ReadSite,
    toks: &[Tok],
    prim: u16,
    dropped: bool,
) -> Option<IntrinsicCallee> {
    use crate::model::BuiltinNs;
    let named: Vec<Tok> = toks
        .iter()
        .copied()
        .filter(|&t| !m.closed_marker(t))
        .collect();
    // The namespace and member the site names.
    let (ns, member): (BuiltinNs, String) = if s.value_call {
        let mut ids = named.iter().filter_map(|&t| match m.toks[t as usize] {
            TokKind::BuiltinFn(Native::Member { identity, .. }) => {
                let (n, k) = identity.rsplit_once('.')?;
                Some((crate::builtins::ns_by_name(n)?, k.to_string()))
            }
            _ => None,
        });
        ids.next()?
    } else {
        let ns = named.iter().find_map(|&t| match m.toks[t as usize] {
            TokKind::Builtin(ns) if !crate::builtins::global_object(m, t) => Some(ns),
            _ => None,
        })?;
        (ns, m.atoms[s.key as usize].clone())
    };
    let name = format!("{ns:?}.{member}");
    let no = |why: &str| Some(IntrinsicCallee::No(format!("{name}: {why}")));
    let ns_tok = m.tok_index.get(&TokKind::Builtin(ns)).copied();
    let one = named.len() == 1
        && match m.toks[named[0] as usize] {
            TokKind::BuiltinFn(_) => s.value_call,
            _ => Some(named[0]) == ns_tok,
        };
    if !one || prim != 0 || toks.contains(&TOP) {
        let mut held: Vec<String> = named.iter().map(|&t| m.tok_name(t)).collect();
        if toks.contains(&TOP) {
            held.push("⊤".into());
        }
        if prim != 0 {
            held.push(format!("primitives {prim:#x}"));
        }
        return no(&format!(
            "the set holds more than the one namespace member: {}",
            held.join(", ")
        ));
    }
    if dropped {
        return no("a source left a value out");
    }
    if m.unit_mode {
        return no("units mode: the facts-off rule decides");
    }
    // CLOSED-WORLD PREMISE (owner 2026-11-14): unknown code that holds the
    // realm is the runtime's natives, which replace no namespace member
    // (owner decision 2026-11-18, R-GLOBAL step 3); interpreted code and an
    // unseen module may.
    if m.realm_held && (m.interp_runs || m.unseen_module_runs) {
        return no("interpreted code holds the realm and may replace the member");
    }
    if !crate::builtins::members::ns_declares(ns, &member) {
        return no("not a member the realm declares on the namespace");
    }
    let a = m.atom_index.get(&member).copied();
    let written = |t: Tok| {
        m.native_writes.contains_key(&(t, None))
            || a.is_some_and(|a| m.native_writes.contains_key(&(t, Some(a))))
    };
    if written(TOP) {
        return no("a write on an unknown receiver may reach the member");
    }
    if let Some(t) = ns_tok {
        if written(t) {
            return no("a write on the namespace may reach the member");
        }
        let empty = |n: Option<&NodeId>| {
            n.is_none_or(|&n| {
                let nd = &m.nodes[n as usize];
                !nd.dropped && nd.pts.is_empty() && nd.prim == 0
            })
        };
        if !empty(a.and_then(|a| m.fields.get(&(t, a)))) || !empty(m.field_any.get(&t)) {
            return no("a store on the namespace may reach the member");
        }
    }
    for (&(t, k), _) in m.native_writes.iter() {
        if t == TOP || !crate::builtins::global_object(m, t) {
            continue;
        }
        if k.is_none_or(|k| crate::builtins::ns_by_name(&m.atoms[k as usize]) == Some(ns)) {
            return no("the global binding of the namespace may be replaced");
        }
    }
    Some(IntrinsicCallee::Exact(name))
}

/// [`Verdict::NsExport`]: every receiver is a module namespace token of
/// this program's modules (`ns_own` names what each module exports itself;
/// a unit's stand-in namespace has none), the key one each resolves to
/// exactly one binding (`resolve_export`), no
/// primitive, no ⊤. A call reads the binding the same way, but the solve
/// lets an unproven call escape its namespace receiver to the callee
/// (DECISIONS 116): the read alone is claimed.
fn ns_export(m: &Model, s: &crate::gen::ReadSite, toks: &[Tok], prim: u16) -> Option<Vec<String>> {
    if s.is_call || prim != 0 || toks.is_empty() || m.is_private_atom(s.key) {
        return None;
    }
    toks.iter()
        .all(|&t| {
            matches!(m.toks[t as usize], TokKind::ModNs(_))
                && matches!(resolve_export(m, t, s.key, &mut Vec::new()), Ok(Some(_)))
        })
        .then(|| toks.iter().map(|&t| m.tok_name(t)).collect())
}

/// ResolveExport (ECMA-262 16.2.1.6.3) of name `a` on the module of
/// namespace `ns`: `Ok(Some(b))` one binding, named by the namespace whose
/// module exports `a` itself (`ns_own`: a local or indirect export, or
/// `export * as`); `Ok(None)` none (a cycle: `(ns, a)` already in the
/// resolve set; `default`, which no `export *` provides; no star source
/// has it); `Err(())` ambiguous (two star sources resolve to different
/// bindings) or unknown (a star source outside the program). Own exports
/// shadow star ones; star sources are followed recursively. The name never
/// changes along a star edge, so the resolve set is the namespaces seen.
/// An indirect export is its own module's binding here, so the same
/// binding reached once through it and once directly counts as two
/// (refused, never wrongly proven).
fn resolve_export(m: &Model, ns: Tok, a: Atom, seen: &mut Vec<Tok>) -> Result<Option<Tok>, ()> {
    if seen.contains(&ns) {
        return Ok(None);
    }
    seen.push(ns);
    if m.ns_own.contains(&(ns, a)) {
        return Ok(Some(ns));
    }
    if &*m.atoms[a as usize] == "default" {
        return Ok(None);
    }
    let mut found = None;
    for &src in m.star_from.get(&ns).map_or(&[][..], |v| v.as_slice()) {
        if !matches!(m.toks[src as usize], TokKind::ModNs(_)) {
            return Err(());
        }
        if let Some(b) = resolve_export(m, src, a, seen)? {
            if found.is_some_and(|f| f != b) {
                return Err(());
            }
            found = Some(b);
        }
    }
    Ok(found)
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
        // A declared fresh object (`NativeFamily::Declared`): what the solve
        // stored on it holds nothing under the key: its field, its any-key
        // values, each shaped store whose shape admits the key (a Web
        // Storage item: any String but Storage.prototype's own names); no
        // accessor, no relink, no escape, no hard operation.
        NativeFamily::Declared(_) => {
            empty(m.fields.get(&(t, a)))
                && empty(m.field_any.get(&t))
                && empty(m.named_any.get(&t))
                && m.shaped_of.get(&t).is_none_or(|v| {
                    v.iter().all(|&(sh, n, store)| {
                        !store || !m.key_may_name(Key::Any(sh), a) || empty(Some(&n))
                    })
                })
                && !m.super_relink.contains(&t)
                && !m.is_escaped(t)
                && !m.hard.contains_key(&t)
                && m.accessor_keys.get(&t).is_none_or(|k| !k.contains(&a))
        }
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
        | NativeFamily::Promise
        | NativeFamily::TypedArray => {
            let Some(tm) = m.tmpl_of(t) else {
                return false;
            };
            // R9BIRTH8: a store never creates an own key that the chain
            // holds as an accessor without a setter: [[Set]] of a typed
            // array under a non-numeric key is OrdinarySet (10.4.5.5), which
            // reaches the accessor and returns false (10.1.9.2 step 7: a
            // TypeError in strict code). `%TypedArray%.prototype`'s `buffer`,
            // `byteLength`, `byteOffset` and `length` are such (23.2.3.2,
            // 23.2.3.3, 23.2.3.4, 23.2.3.21) while no builtin prototype is
            // written (`builtin_protos_unwritten`, checked by the caller).
            let getter_only = matches!(fam, NativeFamily::TypedArray)
                && matches!(
                    &*m.atoms[a as usize],
                    "buffer" | "byteLength" | "byteOffset" | "length"
                );
            (getter_only
                || m.writers
                    .get(&(tm, a))
                    .is_none_or(|w| w.ctor.is_empty() && w.post.is_empty()))
                && !m.super_relink.contains(&t)
                && !m.is_escaped(t)
                && m.hard.get(&t).is_none_or(|&(k, _)| {
                    // R9BIRTH8: B1 (a `.buffer` read, a view over a shared
                    // buffer) lets other holders write the view's bytes
                    // only: an integer-indexed exotic object's buffer
                    // backs its canonical numeric keys (10.4.5.2-5,
                    // 10.4.5.15/16), never an ordinary key, so no own
                    // named key appears (`mark_hard_as`: a later mark of
                    // another kind replaces B1).
                    k == crate::model::Hard::B1Exposed && matches!(fam, NativeFamily::TypedArray)
                })
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
pub(crate) fn lookup(
    m: &Model,
    tf: &TemplateFacts,
    t: Tok,
    key: &str,
    a: Atom,
) -> Result<Vec<NodeId>, String> {
    lookup_holders(m, tf, t, key, a).map(|hs| hs.into_iter().map(|(_, n)| n).collect())
}

/// Rule U (ONCHANGE1): instance token `t` of template `tm` is a birth site
/// (`Obj(tm, ctx)`, not the template-wide token) that no named store of key
/// `a` reaches, and no store reaches the template-wide token either. Its
/// objects never get an own property `a` (ECMA-262 10.1.9: [[Set]] and
/// [[DefineOwnProperty]] are the only adds; an any-key store is refused by
/// the `computed` test of the walk): a read of `a` on them is the chain's.
fn unwritten_here(m: &Model, tm: TmplId, t: Tok, a: Atom) -> bool {
    if !matches!(m.toks[t as usize], TokKind::Obj(x, c) if x == tm && c != crate::model::NO_CTX) {
        return false;
    }
    m.tok_writers.get(&(tm, a)).is_none_or(|ws| {
        !ws.contains(&t)
            && !ws.iter().any(
                |&w| matches!(m.toks[w as usize], TokKind::Obj(_, c) if c == crate::model::NO_CTX),
            )
    })
}

/// The F5 chain walk of [`lookup`], each value node with the object that
/// holds the key (`t` itself for a definite own data field).
fn lookup_holders(
    m: &Model,
    tf: &TemplateFacts,
    t: Tok,
    key: &str,
    a: Atom,
) -> Result<Vec<(Tok, NodeId)>, String> {
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
    // Diagnostic (off: one env read): `PERRY_FACTS_WHY_LOOKUP=<key>` prints
    // the lookup clause inputs for that key.
    static WHY_LOOKUP: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
    if WHY_LOOKUP
        .get_or_init(|| std::env::var("PERRY_FACTS_WHY_LOOKUP").ok())
        .as_deref()
        == Some(key)
    {
        let row = tf.fields.iter().find(|f| f.is(key, m.is_private_atom(a)));
        let h = m.fields.get(&(t, a)).copied();
        eprintln!(
            "WHY_LOOKUP {} .{key} t={t} row={:?} birth={} field={:?} prim={:#x} pts={}",
            tf.display,
            row.map(|f| (
                f.definite,
                f.data_field,
                f.slot,
                f.writers_ctor,
                f.writers_post
            )),
            tf.birth_keys.iter().any(|k| k == key),
            h,
            h.map_or(0, |n| m.nodes[n as usize].prim),
            h.map_or(0, |n| m.nodes[n as usize].pts.len()),
        );
        eprintln!(
            "WHY_LOOKUP settled={:?}",
            m.settled_proto
                .iter()
                .filter(|((_, x), _)| *x == a)
                .collect::<Vec<_>>()
        );
    }
    // A key the layout never writes (a read names it) is the chain's.
    match tf.fields.iter().find(|f| f.is(key, m.is_private_atom(a))) {
        Some(f) if f.definite && f.data_field && f.slot.is_some() => {
            return Ok(holds(t).into_iter().map(|n| (t, n)).collect());
        }
        Some(f) if !f.data_field => return Err("accessor key (E25)".into()),
        // Rule A (ONCHANGE3, `born_with.rs`): every object of this birth
        // site holds an own data property `a` before it can be observed:
        // OrdinaryGet (10.1.8.1) finds it first; its values are the stores
        // of `a` that reach the token.
        _ if m.born_with.contains(&(t, a)) && holds(t).is_some() => {
            return Ok(holds(t).into_iter().map(|n| (t, n)).collect());
        }
        // Rule U (ONCHANGE1): post-birth stores of the key reach other birth
        // sites only: this one holds no own property of it (`unwritten_here`),
        // so OrdinaryGet (10.1.8.1) climbs the chain below.
        Some(f)
            if (f.writers_ctor + f.writers_post > 0 || tf.birth_keys.contains(&f.key))
                && !(f.writers_ctor == 0
                    && !tf.birth_keys.contains(&f.key)
                    && unwritten_here(m, tf.id, t, a)) =>
        {
            return Err("key not a definite birth field".into());
        }
        _ if holds(t).is_some() => return Err("key not a definite birth field".into()),
        _ => {}
    }
    let mut out = Vec::new();
    let mut seen: FxHashSet<Tok> = FxHashSet::default();
    let mut todo = vec![t];
    while let Some(h) = todo.pop() {
        // A prototype set that may hold `null` (`x.__proto__ = null`,
        // `Object.setPrototypeOf(x, null)`, `Object.create(null)`: 10.1.2.1)
        // may end the chain at `h`, which lacks the key: the read may be
        // `undefined` (10.1.8.1) and the call may throw (13.3.6).
        if m.proto_node
            .get(&h)
            .is_some_and(|&pn| m.nodes[pn as usize].prim != 0)
        {
            return Err("the chain may end at a null prototype".into());
        }
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
                Some(n) if present_from_start(m, p, a) => out.push((p, n)),
                Some(_) => {
                    if WHY_LOOKUP.get().is_some_and(|w| w.as_deref() == Some(key)) {
                        eprintln!("WHY_LOOKUP R9BIRTH6 holder {:?} not present from the start: hard={} esc={} top={} own={} gf={}", m.toks[p as usize], m.hard.contains_key(&p), m.is_escaped(p), m.global_in_top, m.global_written_own, m.global_fields.iter().any(|(k, _)| *k == a));
                    }
                    return Err("the key may be read before its store has run (R9BIRTH6)".into());
                }
                None => todo.push(p),
            }
        }
    }
    Ok(out)
}

/// R9READ5: a settled window (`gen::settled`) ran no code: its globals are
/// the realm's own data properties, and each store an inert construction
/// made (`this.k = v`, `Y.prototype.k = v`) is an OrdinarySet (10.1.9.2)
/// along the chain of a fresh `Y` (`Y.prototype`, then `Object.prototype`:
/// Y has no heritage, 10.1.13) that runs code only through an accessor
/// under `k` and fails only on a non-writable `k`: on `Y.prototype` no
/// HARD operation (defineProperty, freeze, delete), no unknown code
/// (escape), no [[Prototype]] write (`super_relink`) and no accessor under
/// a stored key (`accessor_keys`; the class body's own `gen` refused); on
/// `Object.prototype` no write under a stored key but plain stores
/// (`native_nonplain_written`: a plain store makes writable data).
/// CLOSED-WORLD PREMISE (owner 2026-11-14): every accessor definition is in
/// the program.
fn window_own(m: &Model, w: &SettledWin) -> bool {
    let op = m.tok_index.get(&TokKind::ObjectProto).copied();
    window_globals_own(m, &w.globals)
        && w.classes.iter().all(|&tm| {
            m.tok_index.get(&TokKind::Proto(tm)).is_some_and(|&p| {
                !m.hard.contains_key(&p)
                    && !m.is_escaped(p)
                    && !m.super_relink.contains(&p)
                    && !m
                        .accessor_keys
                        .get(&p)
                        .is_some_and(|ks| w.keys.iter().any(|k| ks.contains(k)))
            })
        })
        && (w.keys.is_empty()
            || op.is_some_and(|op| {
                w.keys
                    .iter()
                    .all(|&k| !crate::builtins::native_nonplain_written(m, op, k))
            }))
}

/// The globals a settled window read (`gen::settled`) are the realm's own
/// data properties: no ⊤ global, no program write of a global, and no
/// program value under any of the names.
fn window_globals_own(m: &Model, gs: &[Atom]) -> bool {
    gs.is_empty()
        || (!m.global_in_top
            && !m.global_written_own
            && gs.iter().all(|g| {
                m.global_fields
                    .iter()
                    .filter(|(k, _)| k == g)
                    .all(|&(_, n)| {
                        let nd = &m.nodes[n as usize];
                        nd.pts.to_sorted().into_iter().all(|t| {
                            t != TOP
                                && m.tmpl_of(t).is_none()
                                && !matches!(m.toks[t as usize], TokKind::Func(..))
                        })
                    })
            }))
}

/// R9BIRTH6: holder `p`'s own key `a` exists at every read or call that
/// can reach `p` on a chain: a class-body member is defined while the class
/// is created (15.7.14), before anything can reach the prototype; a key a
/// store adds is present only after that store ran, so it must be a
/// settled store (`gen::settled`: it completes before any code can reach
/// the prototype) on a prototype no `delete` / `defineProperty` (HARD) or
/// unknown code (escape) changes, and every global it read on the way must
/// be the realm's own data property: unknown code cannot reach the global
/// object and every value the solve sees under that global name is no
/// program value (a program getter or store would put its own there). A literal object holds its birth keys from
/// its evaluation on. Else the read may find no `a` there: `undefined`, or
/// a TypeError for a call (13.3.6.2): never one holder's value.
fn present_from_start(m: &Model, p: Tok, a: Atom) -> bool {
    match m.toks[p as usize] {
        TokKind::Proto(tm) => {
            let tpl = &m.tmpls[tm as usize];
            if tpl.proto_members.contains(&a) {
                return true;
            }
            // Rule 3 (RAYBOX3, `gen::proto_first`): a static block opening
            // the class body stored the key; static elements run in
            // ClassDefinitionEvaluation (15.7.14) before the outer binding is
            // initialized, so before any read can reach the prototype. Only
            // such stores write it; no HARD operation or unknown code removes it.
            if m.proto_first_keys.contains(&(tm, a))
                && !m.reshaped_any
                && !m.reshaped_keys.contains(&a)
                && only_proto_first_writers(m, tm, a)
                && !m.hard.contains_key(&p)
                && !m.is_escaped(p)
            {
                return true;
            }
            let Some(gs) = m.settled_proto.get(&(tm, a)) else {
                return false;
            };
            !m.hard.contains_key(&p) && !m.is_escaped(p) && window_own(m, gs)
        }
        TokKind::Obj(tm, _) => {
            let tpl = &m.tmpls[tm as usize];
            matches!(
                tpl.kind,
                TmplKind::Class {
                    literal_shape: true,
                    ..
                } | TmplKind::Lit { .. }
            ) && tpl.birth_keys.contains(&a)
        }
        _ => false,
    }
}
