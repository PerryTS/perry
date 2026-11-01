//! Per-unit driver pieces (decision 151, design §11): what a unit
//! publishes to the other units of the program.

use crate::pts::{FxHashMap, FxHashSet};
use crate::{FactsConfig, ProgramFacts, SiteFacts, TemplateFacts};
use perry_hir::{Export, Import, ImportSpecifier, Module};

/// A published class as its importers see it (design §11.2): its template
/// and the facts its defining unit proved, its prototype methods (key,
/// function name) along the chain inside that unit, and its native parent.
#[derive(Clone, Debug, PartialEq, Hash)]
pub(crate) struct ClassContract {
    pub display: String,
    pub name: String,
    pub class_id: u32,
    pub literal_shape: bool,
    pub facts: TemplateFacts,
    pub methods: Vec<(String, String)>,
    pub native_parent: Option<(String, String)>,
    /// Its parent class when that class is another unit's (DECISIONS 104):
    /// the parent's display, its contract published beside this one. The
    /// chain inside the defining unit is in `methods`; past it, a reader
    /// finds inherited methods on the parent's stand-in, as the defining
    /// unit does.
    pub parent: Option<String>,
    /// Fields that hold exactly these functions of the defining unit, each
    /// a function contract (a literal's methods: `errorUtil.errToObj`);
    /// every other field holds unknown values for the reader.
    pub field_fns: Vec<(String, Vec<FnContract>)>,
    /// Fields that hold only instances of these layouts of the defining
    /// unit (key, their displays, whether primitives too): nested
    /// contracts (design §18.7, `Gen::nested_fields`).
    pub field_layouts: Vec<(String, Vec<String>, bool)>,
    /// Fields that hold other units' stand-ins (builtins, unknown values,
    /// primitives) and nothing of the defining unit: what they hold, as a
    /// value contract (DECISIONS 88), the reader seeds them with.
    pub field_values: Vec<(String, ValueContract)>,
    /// Fields that hold only the defining unit's typed arrays, none escaped
    /// or hard (DECISIONS 126): key, their one element kind, the primitives
    /// beside them. The reader holds one stand-in typed array per class and
    /// key; its census reports `ta <key> of <display>` for what it does.
    pub field_tas: Vec<(String, Option<u8>, u16)>,
    /// The defining unit stores under computed keys: a reader may find any
    /// key (unknown values). Otherwise the keys are the facts' fields.
    pub open_keys: bool,
    /// The class object's own members along the chain inside the defining
    /// unit (key, the function when it is a static method; else an unknown
    /// value), and whether it may hold other keys (computed static stores,
    /// a native parent): then every static is unknown.
    pub statics: Vec<(String, Option<String>)>,
    pub statics_open: bool,
    /// The static methods with a function contract (key, contract): the
    /// defining unit checked their results (an instance of a contract
    /// class is given out, `Model::give_out`).
    pub static_fns: Vec<(String, FnContract)>,
    /// The data statics with a value contract (key, contract, DECISIONS
    /// 173): the defining unit checked what they hold after its solve.
    pub static_values: Vec<(String, ValueContract)>,
    /// A closed class (design §18.13, `decl_units::closed_bindings`): its
    /// defining module's path and binding name, and its constructor's
    /// parameter count. Every unit constructing it (by name or through an
    /// instance's `constructor`) publishes what it passes under that key
    /// (`ProgramFacts::ctor_args_out`).
    pub closed: Option<(String, String, u32)>,
    /// DECISIONS 157: `closed` is a published class (`UnitView::published`):
    /// a unit holding its stand-in forwards it as an open class's (the next
    /// unit holds its stand-in too, its constructions publish there).
    pub forwards: bool,
    /// DECISIONS 210: the layout of a constructor function's instances
    /// (`new <f>`, `TmplKind::FnInst`): `ctor_fn` is the function's own
    /// contract. A reader's `new` of the function constructs this layout
    /// (`Model::foreign_ctor`) and publishes what it passes
    /// (`ProgramFacts::ctor_runs_out`); the owner constructs it with the
    /// union of every unit's arguments (`Gen::published_ctors`).
    pub fn_inst: bool,
    /// The prototype methods with a function contract (key, contract), and
    /// the constructor's (DECISIONS 57): their receiver is a contract
    /// parameter, so another unit's subclass instance can run them.
    pub method_fns: Vec<(String, FnContract)>,
    pub ctor_fn: Option<FnContract>,
    /// Its instances' birth keys in birth order and the definite ones, and
    /// whether its constructor or one up its chain leaks `this`, as its own
    /// unit computed them (`compute_birth_keys`, `ctor_phase`): a subclass
    /// in another unit takes them as its layout prefix, as a subclass in
    /// the same unit does.
    pub birth_keys: Vec<String>,
    pub definite: Vec<String>,
    /// The birth keys reserved for a constructor store that are not
    /// definite (`Template::reserved`): known slots, present once stored.
    pub reserved: Vec<String>,
    pub ctor_leaks: bool,
    /// Its constructor, or one up its chain, may return an object other than
    /// `this` (`return expr`): a reader's `new` then yields that value, not
    /// the class's instance, which its own unit alone cannot name.
    pub ctor_returns: bool,
    /// The primitive kinds the published binding may hold beside the
    /// class (DECISIONS 115): a nested class whose members name it binds
    /// itself through a one-element cell that holds `undefined` until the
    /// class is evaluated (DECISIONS 50). A reader's import holds them as
    /// the defining unit's binding does; a layout reached any other way
    /// has none.
    pub binding_prim: u16,
}

/// A published function as its callers see it (design §11.2): per
/// parameter what the callee does with everything reachable from the
/// argument (`model::MARK_*`: read, touched by a protocol, or used beyond:
/// then it escapes), whether its body reads `arguments` (`variadic`: all
/// arguments escape) or has a rest parameter (`rest`: the arguments past
/// the parameters escape), and the layouts it returns (`None`: ⊤).
/// DECISIONS 118: one key a contract function stores into parameter
/// `dst`'s argument (`reach`: into what it reaches). The reader stores the
/// arguments of the parameters `from` under it, ⊤ when `top`, and `prim`.
/// DECISIONS 149: `dst` `u32::MAX` is the receiver (into what it reaches),
/// `key` `None` a computed key.
#[derive(Clone, Debug, PartialEq, Hash)]
pub(crate) struct StoreFlow {
    pub dst: u32,
    pub reach: bool,
    /// Into what the argument holds under this key (a keyed reach, design
    /// §18.4), not into all it reaches.
    pub via: Option<String>,
    pub key: Option<String>,
    pub from: Vec<u32>,
    /// DECISIONS 136: what parameter `j`'s argument holds under the key
    /// (and reaches from there), or reaches (`None`), is stored too.
    pub from_reach: Vec<(u32, Option<String>)>,
    pub top: bool,
    pub prim: u16,
    /// DECISIONS 176: the values of the defining unit it stores, described
    /// as a result is (DECISIONS 140).
    pub value: Option<Described>,
}

/// DECISIONS 140: a value a contract function hands the caller's unit, its
/// result or an argument of a call through a parameter, as the reader sees
/// it: the layouts it may be (`None`: ⊤), the parameters (`u32::MAX`: the
/// receiver) it may be or whose reach it may hold, what it holds through
/// one named key of one (then the keys read on it, DECISIONS 138), an
/// unknown value beside, the builtins, program objects and arrays
/// (DECISIONS 94), and its primitive kinds.
/// What a value holds through a parameter's reach (DECISIONS 138, 158):
/// the parameter (`u32::MAX`: the receiver), its first named key, the keys
/// the path repeats there (`Some`; `None`: any key), then each further
/// step's key (`None`: a computed key) with the keys repeated there. The
/// reader walks the same keys on its argument.
pub(crate) type ReachKey = (
    u32,
    String,
    Option<Vec<String>>,
    Vec<(Option<String>, Option<Vec<String>>)>,
);

#[derive(Clone, Debug, Default, PartialEq, Hash)]
pub(crate) struct Described {
    pub layouts: Option<Vec<String>>,
    pub params: Vec<u32>,
    pub reach: Vec<u32>,
    pub reach_keys: Vec<ReachKey>,
    pub unknown: bool,
    pub value: ValueContract,
    pub prim: u16,
}

impl Described {
    /// Every value either may be: a ⊤ (`None`) absorbs the layouts.
    pub(crate) fn join(&mut self, o: &Described) {
        match (&mut self.layouts, &o.layouts) {
            (Some(r), Some(s)) => {
                union_into(r, s);
                r.sort();
            }
            (r, _) => *r = None,
        }
        for (v, w) in [(&mut self.params, &o.params), (&mut self.reach, &o.reach)] {
            union_into(v, w);
            v.sort_unstable();
        }
        union_into(&mut self.reach_keys, &o.reach_keys);
        self.reach_keys.sort();
        self.unknown |= o.unknown;
        self.value.join(&o.value);
        self.prim |= o.prim;
    }
}

/// DECISIONS 140: the body calls parameter `param`'s argument with these
/// arguments (no receiver: `fn(a, b)`), so the caller's unit, which knows
/// the function it passed, runs that call; one entry per (parameter, key,
/// number of arguments), its arguments joined over the calls. With `key`
/// the body calls that method of the argument (`p.k(a, b)`, DECISIONS 144).
/// DECISIONS 236: `param` `u32::MAX` with `path` `Some(f)`: the method of
/// what the receiver holds under `f` (`this.f.k(a, b)`).
#[derive(Clone, Debug, PartialEq, Hash)]
pub(crate) struct ParamCall {
    pub param: u32,
    pub path: Option<String>,
    pub key: Option<String>,
    pub args: Vec<Described>,
}

impl ParamCall {
    pub(crate) fn join(&mut self, o: &ParamCall) {
        for (x, y) in self.args.iter_mut().zip(&o.args) {
            x.join(y);
        }
    }
}

/// Joins `b`'s calls into `a` by (parameter, key, number of arguments).
pub(crate) fn join_param_calls(a: &mut Vec<ParamCall>, b: &[ParamCall]) {
    join_keyed(
        a,
        b,
        |x| (x.param, x.path.clone(), x.key.clone(), x.args.len()),
        |x, y| x.join(y),
    );
    a.sort_by(|x, y| {
        (x.param, &x.path, &x.key, x.args.len()).cmp(&(y.param, &y.path, &y.key, y.args.len()))
    });
}

/// DECISIONS 152: the body hands parameter `param`'s argument to argument
/// `arg` of the constructor of closed class `class` (another unit's, by its
/// key, design §18.13): the caller's unit, which holds the value, passes it
/// there itself, so it reaches that constructor's argument contract as the
/// caller's own construction would.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct HandOverArg {
    pub param: u32,
    pub class: (String, String),
    pub arg: u32,
}

#[derive(Clone, Debug, PartialEq, Hash)]
pub(crate) struct FnContract {
    /// Emitted body identity, independent of this analysis unit's stand-in.
    /// The defining source path prevents equal HIR ids in different modules
    /// from becoming the same callable body. None for synthesized code.
    pub compiled_body: Option<(String, u32, bool)>,
    pub name: String,
    /// DECISIONS 165: a method of a published class (its owner's prototype
    /// holds it): a unit calling it publishes what it passes
    /// (`ProgramFacts::method_runs_out`), however it got the function.
    pub method: bool,
    /// DECISIONS 224: a function of its unit published to a known closed
    /// callee (DECISIONS 223): as for a method, a unit calling it publishes
    /// what it passes (`ProgramFacts::method_args_out`), however it got the
    /// function.
    pub callable: bool,
    /// DECISIONS 212: the published class (its display) whose instances a
    /// method runs on, when there is one: a unit holding the method as a
    /// value (returned, read from a field, an array element or the
    /// prototype) admits those as receivers and publishes them, as for a
    /// method call on the class's stand-in (DECISIONS 107). `None` with
    /// `method`: it admits no receiver (each one is a census report).
    pub this_class: Option<String>,
    /// Per parameter: what the callee does to the argument itself
    /// (`model::MARK_*`), and to what the argument reaches (its fields,
    /// transitively). Two sets: reading `p.n` and converting it is no
    /// conversion of `p`. `reach` is what the callee does to what it reads
    /// under a computed key; `reach_keys` what it does to what it reads
    /// under each named key `p.k` (design §18.4: `p.left[i] = v` writes
    /// into `p.left`, not into `p`'s prototype).
    pub params: Vec<u8>,
    pub reach: Vec<u8>,
    pub reach_keys: Vec<Vec<(String, u8)>>,
    pub variadic: bool,
    pub rest: bool,
    pub ret: Option<Vec<String>>,
    /// The parameters whose argument the result may be, and those whose
    /// reachable values (fields, transitively) it may hold: result
    /// transfers, the caller's own objects come back, nothing escapes.
    pub ret_params: Vec<u32>,
    pub ret_reach: Vec<u32>,
    /// What the result may hold through one named key of a parameter
    /// (`return p.k`, `u32::MAX`: the receiver): the reader loads that key
    /// of its argument, not everything the argument reaches (its
    /// prototype's methods, its class), as for `reach_keys` (§18.4).
    /// Then the keys the body read on that reach, repeatedly (`Some`), or
    /// any key (`None`, a computed-key read): `x = p.k; while (x.left) x =
    /// x.left; return x` is `p.k` then `left`* (DECISIONS 138).
    pub ret_reach_keys: Vec<ReachKey>,
    /// The result may also be an unknown value (⊤ to the caller).
    pub ret_unknown: bool,
    /// The global builtins, program objects and arrays it may return
    /// (DECISIONS 94, as a value contract holds them).
    pub ret_value: ValueContract,
    /// The primitive kinds the result may be (`model::prim`), when `ret`
    /// names its layouts; with no layouts (⊤) any.
    pub ret_prim: u16,
    /// Its receiver is not used: an arrow function, or a body that never
    /// reads `this` (§18.2).
    pub lexical_this: bool,
    /// What the body does to its receiver and to what that reaches
    /// (`model::MARK_*`; ESCAPE when it has no receiver marker), and the
    /// keys it stores into it (`None`: a computed key) (DECISIONS 57).
    pub this_use: u8,
    pub this_reach: u8,
    pub this_stores: Option<Vec<String>>,
    /// The keys the body calls on its receiver that only another unit's
    /// subclass defines (DECISIONS 97): a reader runs `this.<key>(⊤)` on
    /// each receiver it admits, the call the defining unit cannot see.
    pub this_calls: Vec<String>,
    /// Per parameter, the keys the body stores into the argument (`None`:
    /// a computed key); a stored value escapes (DECISIONS 84).
    pub param_stores: Vec<Option<Vec<String>>>,
    /// What the body stores under a named key into an argument or into
    /// what it reaches: the arguments of other parameters, or an unknown
    /// value, or primitives (DECISIONS 118).
    pub param_flows: Vec<StoreFlow>,
    /// The calls the body makes through a parameter (DECISIONS 140).
    pub param_calls: Vec<ParamCall>,
    /// The arguments it hands to a closed class's constructor (DECISIONS 152).
    pub param_handovers: Vec<HandOverArg>,
    /// The array cells (`arr:<name>`, DECISIONS 94) the body stores an
    /// argument into, per parameter (DECISIONS 162): a caller's unit stores
    /// its argument into that cell, as the whole program runs the store.
    pub param_cells: Vec<(u32, String)>,
    /// The binding name of a closed function (decision 19): a unit that
    /// calls it as a value publishes what it passes under this name.
    pub closed: Option<String>,
}

impl FnContract {
    /// The function named, not described (DECISIONS 106): its identity,
    /// and of its body only what makes no claim (every parameter, its
    /// receiver and its result escape or are unknown). A reader holding it
    /// calls the function through its owner's entry (`Gen::owner_fn`); with
    /// no entry it runs this, which is sound.
    pub(crate) fn named(&self) -> FnContract {
        use crate::model::MARK_ESCAPE;
        let n = self.params.len();
        FnContract {
            compiled_body: self.compiled_body.clone(),
            name: self.name.clone(),
            method: self.method,
            callable: self.callable,
            this_class: self.this_class.clone(),
            params: vec![MARK_ESCAPE; n],
            reach: vec![MARK_ESCAPE; n],
            reach_keys: vec![Vec::new(); n],
            variadic: self.variadic,
            rest: self.rest,
            ret: None,
            ret_params: Vec::new(),
            ret_reach: Vec::new(),
            ret_reach_keys: Vec::new(),
            ret_unknown: true,
            ret_value: Default::default(),
            ret_prim: crate::model::prim::ALL,
            lexical_this: self.lexical_this,
            this_use: MARK_ESCAPE,
            this_reach: MARK_ESCAPE,
            this_stores: None,
            this_calls: Vec::new(),
            param_stores: vec![None; n],
            param_flows: Vec::new(),
            param_calls: Vec::new(),
            param_handovers: Vec::new(),
            param_cells: Vec::new(),
            closed: self.closed.clone(),
        }
    }
}

/// DECISIONS 106 for what a forwarded copy of another unit's function
/// holds: every function described inside it whose owner publishes an entry
/// (`owned`) is named (`FnContract::named`), as the reader takes that entry
/// by name in any case. Copied whole, a description held the copies its
/// owner's description held, which held it again: two units whose functions
/// pass each other's closures (zod's `emitOwnKeys` and the record check it
/// calls back) nested one more level every round, the contracts grew 4x
/// per two rounds and the fixpoint never came. Named, a description holds
/// only its own unit's functions and one level of copies: its size is
/// bounded by the program's, whatever the rounds.
pub(crate) fn name_owned_in_fn(fc: &mut FnContract, owned: &dyn Fn(&str) -> bool) {
    name_owned_in_value(&mut fc.ret_value, owned);
    for pc in &mut fc.param_calls {
        for a in &mut pc.args {
            name_owned_in_value(&mut a.value, owned);
        }
    }
    for fl in &mut fc.param_flows {
        if let Some(d) = &mut fl.value {
            name_owned_in_value(&mut d.value, owned);
        }
    }
}

fn name_owned_fns(fns: &mut [FnContract], owned: &dyn Fn(&str) -> bool) {
    for f in fns {
        name_owned(f, owned);
    }
}

fn name_owned(f: &mut FnContract, owned: &dyn Fn(&str) -> bool) {
    if owned(&f.name) {
        *f = f.named();
    } else {
        name_owned_in_fn(f, owned);
    }
}

fn name_owned_in_value(vc: &mut ValueContract, owned: &dyn Fn(&str) -> bool) {
    name_owned_fns(&mut vc.fns, owned);
    for c in vc.layouts.iter_mut().chain(vc.classes.iter_mut()) {
        name_owned_in_class(c, owned);
    }
}

fn name_owned_in_class(c: &mut ClassContract, owned: &dyn Fn(&str) -> bool) {
    for (_, fs) in &mut c.field_fns {
        name_owned_fns(fs, owned);
    }
    for (_, vc) in c.field_values.iter_mut().chain(c.static_values.iter_mut()) {
        name_owned_in_value(vc, owned);
    }
    for (_, f) in c.static_fns.iter_mut().chain(c.method_fns.iter_mut()) {
        name_owned(f, owned);
    }
    if let Some(f) = &mut c.ctor_fn {
        name_owned(f, owned);
    }
}

/// A published module-scope value binding as its readers see it (design
/// §11.2 "an object contract", generalized to every binding another
/// declaration unit reads, §12): the layouts it may hold (literal-shape
/// objects of the defining unit, instances of contract classes, layouts it
/// holds through other contracts), the contract classes and functions it
/// may hold, and whether it may hold an unknown value (⊤ to the reader).
/// The layouts' fields hold unknown values for the reader; a store or an
/// opening operation through them is the reader's census report.
/// Dump-only: why a value contract is `unknown`, the first reason and the
/// unit that published it (stamped when the dump is on). It takes no part in
/// equality or hashing, so rounds and fingerprints are those of a run without
/// it.
#[derive(Clone, Default)]
pub(crate) struct Why(pub Option<(&'static str, Option<std::sync::Arc<str>>)>);

impl PartialEq for Why {
    fn eq(&self, _: &Why) -> bool {
        true
    }
}

impl std::hash::Hash for Why {
    fn hash<H: std::hash::Hasher>(&self, _: &mut H) {}
}

impl std::fmt::Debug for Why {
    fn fmt(&self, _: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        Ok(())
    }
}

impl Why {
    /// The smaller of the two (reason, unit), so a join does not depend on
    /// the order the units are folded in.
    fn join(&mut self, o: &Why) {
        let key = |w: &Why| w.0.as_ref().map(|(r, u)| (*r, u.clone()));
        match (key(self), key(o)) {
            (None, Some(_)) => self.0 = o.0.clone(),
            (Some(a), Some(b)) if b < a => self.0 = o.0.clone(),
            _ => {}
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Hash)]
pub(crate) struct ValueContract {
    pub layouts: Vec<ClassContract>,
    pub classes: Vec<ClassContract>,
    pub fns: Vec<FnContract>,
    pub unknown: bool,
    /// Why it is unknown (dump only, see `Why`).
    pub why: Why,
    /// The runtime-owned objects it may hold (`Model::named_shared`), one
    /// object for the whole program: the reader holds them as well.
    pub shared: Vec<crate::model::Shared>,
    /// The primitive kinds it may hold (`model::prim` bits).
    pub prim: u16,
    /// The global builtins it may hold (design §17.16 (a)): a builtin is one
    /// runtime object for the whole program, the same token in every unit.
    pub builtins: Vec<crate::model::BuiltinNs>,
    /// The arrays of other units it may hold (DECISIONS 94), by template
    /// display: their elements are cell `arr:<display>`. Each is an Array
    /// exotic object (ECMA-262 10.4.2), as its publisher proves
    /// (`derive::exact_array`).
    pub arrays: Vec<String>,
    /// ARU D6 (CWN 2026-11-01): the arrays made through a constructor
    /// (ArraySpeciesCreate) that their publisher does not prove to be
    /// Arrays, named and celled as `arrays`: a reader's stand-in of one is
    /// no Array for it. A name in both lists is one of these.
    pub made_arrays: Vec<String>,
    /// DECISIONS 184: the primitive method containers (`primbox`) it may
    /// hold, by name as `arrays`: their elements are cell `arr:<name>`; a
    /// reader's stand-in is a container of its own kind.
    pub boxes: Vec<String>,
    /// ARU D3/D6: the containers among them that are Arrays (`arraybox`:
    /// own-keys results), named and celled as `boxes`.
    pub arrayboxes: Vec<String>,
    pub indexed_owners: Vec<crate::bindings::IndexedOwner>,
}

/// Keeps the prefix `a` and `b` share.
fn common_prefix(a: &mut Vec<String>, b: &[String]) {
    let n = a.iter().zip(b).take_while(|(x, y)| x == y).count();
    a.truncate(n);
}

/// What the calls of a closed function pass (decision 13,
/// `decl_units::closed_bindings`): per parameter the value contract of the
/// arguments, and whether some call may pass anything (`top`: a unit calls
/// it through ⊤ or a broken contract, or claims nothing over the bound).
/// One unit publishes its calls' (`ProgramFacts::args_out`); the callee
/// takes the union over every unit.
#[derive(Clone, Debug, Default, PartialEq, Hash)]
pub(crate) struct ArgContract {
    pub params: Vec<ValueContract>,
    pub top: bool,
}

/// Joins `b` into `a` as sets: what `b` holds that `a` lacks is appended,
/// so `a ⊔ a = a` (the driver compares last round's value with the join).
fn union_into<T: Clone + PartialEq>(a: &mut Vec<T>, b: &[T]) {
    for x in b {
        if !a.contains(x) {
            a.push(x.clone());
        }
    }
}

/// Joins entries keyed by `key`: an entry both hold is joined by `join`,
/// one only `b` holds is appended.
fn join_keyed<T: Clone, K: PartialEq>(
    a: &mut Vec<T>,
    b: &[T],
    key: impl Fn(&T) -> K,
    join: impl Fn(&mut T, &T),
) {
    for y in b {
        match a.iter_mut().find(|x| key(x) == key(y)) {
            Some(x) => join(x, y),
            None => a.push(y.clone()),
        }
    }
}

impl FnContract {
    /// The join of two descriptions of one function (by `name`): every
    /// claim the caller may rely on holds in both. The marks per
    /// parameter, variadic, the borrowed and returned parameters and an
    /// unknown result only grow; the result layouts grow, and a ⊤ result
    /// (`None`) absorbs them; the receiver is unused only if both say so.
    pub(crate) fn join(&mut self, o: &FnContract) {
        if self.compiled_body != o.compiled_body {
            self.compiled_body = None;
        }
        self.method |= o.method;
        self.callable |= o.callable;
        if self.this_class != o.this_class {
            self.this_class = None;
        }
        for (v, w) in [(&mut self.params, &o.params), (&mut self.reach, &o.reach)] {
            if v.len() < w.len() {
                v.resize(w.len(), 0);
            }
            for (x, y) in v.iter_mut().zip(w) {
                *x |= *y;
            }
        }
        // Per named key (§18.4): the marks grow, a key either reads is read.
        if self.reach_keys.len() < o.reach_keys.len() {
            self.reach_keys.resize(o.reach_keys.len(), Vec::new());
        }
        for (v, w) in self.reach_keys.iter_mut().zip(&o.reach_keys) {
            for (k, u) in w {
                match v.iter_mut().find(|(q, _)| q == k) {
                    Some((_, x)) => *x |= *u,
                    None => v.push((k.clone(), *u)),
                }
            }
            v.sort();
        }
        self.variadic |= o.variadic;
        self.rest |= o.rest;
        self.ret_prim |= o.ret_prim;
        match (&mut self.ret, &o.ret) {
            (Some(r), Some(s)) => union_into(r, s),
            (r, _) => *r = None,
        }
        union_into(&mut self.ret_params, &o.ret_params);
        union_into(&mut self.ret_reach, &o.ret_reach);
        self.ret_unknown |= o.ret_unknown;
        union_into(&mut self.ret_reach_keys, &o.ret_reach_keys);
        self.ret_reach_keys.sort();
        self.ret_value.join(&o.ret_value);
        self.lexical_this &= o.lexical_this;
        // The receiver (DECISIONS 57, 97): the marks and the calls grow; a
        // computed-key store (`None`) absorbs the named keys.
        self.this_use |= o.this_use;
        self.this_reach |= o.this_reach;
        join_stores(&mut self.this_stores, &o.this_stores);
        union_into(&mut self.this_calls, &o.this_calls);
        self.this_calls.sort();
        if self.param_stores.len() < o.param_stores.len() {
            self.param_stores
                .resize(o.param_stores.len(), Some(Vec::new()));
        }
        for (x, y) in self.param_stores.iter_mut().zip(&o.param_stores) {
            join_stores(x, y);
        }
        // DECISIONS 118: a store flow per (parameter, key): its sources,
        // ⊤ and primitives grow.
        for y in &o.param_flows {
            let same = |x: &StoreFlow| {
                (x.dst, x.reach, &x.via, &x.key) == (y.dst, y.reach, &y.via, &y.key)
            };
            match self.param_flows.iter_mut().find(|x| same(x)) {
                Some(x) => {
                    union_into(&mut x.from, &y.from);
                    x.from.sort_unstable();
                    for r in &y.from_reach {
                        if !x.from_reach.contains(r) {
                            x.from_reach.push(r.clone());
                        }
                    }
                    x.from_reach.sort();
                    x.top |= y.top;
                    x.prim |= y.prim;
                    match (&mut x.value, &y.value) {
                        (Some(a), Some(b)) => a.join(b),
                        (a @ None, Some(b)) => *a = Some(b.clone()),
                        _ => {}
                    }
                }
                None => self.param_flows.push(y.clone()),
            }
        }
        self.param_flows.sort_by(|x, y| {
            (x.dst, x.reach, &x.via, &x.key).cmp(&(y.dst, y.reach, &y.via, &y.key))
        });
        join_param_calls(&mut self.param_calls, &o.param_calls);
        union_into(&mut self.param_handovers, &o.param_handovers);
        self.param_handovers.sort();
        union_into(&mut self.param_cells, &o.param_cells);
        self.param_cells.sort();
        if self.closed.is_none() {
            self.closed = o.closed.clone();
        }
    }
}

/// The keys stored into a value: `None` (a computed key) absorbs.
fn join_stores(a: &mut Option<Vec<String>>, b: &Option<Vec<String>>) {
    match (a.as_mut(), b) {
        (Some(x), Some(y)) => {
            union_into(x, y);
            x.sort();
        }
        _ => *a = None,
    }
}

/// A static member of a class contract as its reader builds it
/// (`Gen::stand_in_class`): a function contract, a stub of the named
/// method, or an unknown value.
#[derive(Clone, PartialEq)]
enum StaticMember {
    Fn(FnContract),
    Stub(String),
    Value(ValueContract),
    Top,
}

/// What a reader builds of field `k` of layout `c` (`Gen::stand_in_class`):
/// its typed arrays, its nested layouts and a value contract, each it
/// has (a join may give several, `ClassContract::join`); else its field
/// functions; else its facts. Empty: `c` has no such field.
#[derive(Debug, PartialEq)]
pub(crate) enum FieldView<'c> {
    Ta(Option<u8>, u16),
    Lay(Vec<String>, bool),
    Val(&'c ValueContract),
    Fns(&'c Vec<FnContract>),
    Plain(&'c crate::derive::FieldFacts),
}

pub(crate) fn field_views<'c>(c: &'c ClassContract, k: &str) -> Vec<FieldView<'c>> {
    let mut v = Vec::new();
    if let Some((_, e, p)) = c.field_tas.iter().find(|(q, _, _)| q == k) {
        v.push(FieldView::Ta(*e, *p));
    }
    if let Some((_, ds, p)) = c.field_layouts.iter().find(|(q, _, _)| q == k) {
        v.push(FieldView::Lay(ds.clone(), *p));
    }
    if let Some((_, x)) = c.field_values.iter().find(|(q, _)| q == k) {
        v.push(FieldView::Val(x));
    }
    if !v.is_empty() {
        return v;
    }
    if let Some((_, fs)) = c.field_fns.iter().find(|(q, _)| q == k) {
        return vec![FieldView::Fns(fs)];
    }
    c.facts
        .fields
        .iter()
        .find(|f| f.is(k, false))
        .map(FieldView::Plain)
        .into_iter()
        .collect()
}

/// A class's field holding only its own instances (and primitives,
/// DECISIONS 110): the reader's instance of its stand-in.
fn own_instances(c: &ClassContract, f: &crate::derive::FieldFacts) -> bool {
    let k = &f.kind;
    !c.literal_shape
        && f.data_field
        && !k.top
        && k.funcs == 0
        && k.builtin_objs == 0
        && !k.objs.is_empty()
        && k.objs.iter().all(|d| *d == c.display)
}

/// A key the defining unit's layout never holds: no birth key, no store
/// into it, its kind only the `undefined` of a key not yet present
/// (`derive::field_kind`): a read names it (`this.status()` on the
/// layout's own instance), nothing writes it. It has no own value: a read
/// of it is the chain's, as in the defining unit (`derive::lookup`).
pub(crate) fn unheld(c: &ClassContract, f: &crate::derive::FieldFacts) -> bool {
    use crate::model::prim;
    let k = &f.kind;
    f.data_field
        && !f.definite
        && !f.declared
        && !f.reserved
        && f.writers_ctor + f.writers_post == 0
        && !c.birth_keys.contains(&f.key)
        && !k.top
        && k.objs.is_empty()
        && k.funcs == 0
        && k.builtin_objs == 0
        && k.prims == prim::UNDEF
}

/// A field described by its facts alone, as a value contract: the
/// primitives a data field holds (DECISIONS 74; `undefined` before a field
/// not definite exists), else an unknown value (its owner sank it).
fn plain_value(c: &ClassContract, f: &crate::derive::FieldFacts) -> ValueContract {
    use crate::model::prim;
    if unheld(c, f) {
        return ValueContract::default();
    }
    let k = &f.kind;
    if f.data_field
        && !k.top
        && k.objs.is_empty()
        && k.funcs == 0
        && k.builtin_objs == 0
        && k.prims != 0
    {
        let undef = if f.definite { 0 } else { prim::UNDEF };
        return ValueContract {
            prim: k.prims | undef,
            ..Default::default()
        };
    }
    ValueContract {
        unknown: true,
        prim: prim::ALL,
        ..Default::default()
    }
}

impl ClassContract {
    /// The join of two descriptions of one layout or class (by `display`;
    /// its name, class id, shape and native parent are its identity):
    /// every claim a reader may rely on holds in both. Facts weaken
    /// (`derive::TemplateFacts::join`); keys, methods and statics are the
    /// union, and a member the two describe differently is an unknown
    /// value; a field holds contract functions only where both say so;
    /// open keys and open statics only grow.
    pub(crate) fn join(&mut self, o: &ClassContract) {
        // Fields: each side's description of a field is what its reader
        // builds of it (`field_view`, `Gen::stand_in_class`). A field both
        // describe the same way keeps that description, joined: the union
        // of their field functions or nested layouts (§18.7, primitives too
        // if either says so), the join of their value contracts (DECISIONS
        // 88), their typed arrays (DECISIONS 126; an element kind the two
        // disagree on is none). A field they describe differently holds what
        // each says: the reader builds every description given for it, field
        // functions and plain facts as a value contract (`plain_value`). The
        // join makes no unknown value of a field: what an owner described
        // it did not sink, so a reader finding ⊤ there would store past its
        // owner (a store into another unit's literal read back through a
        // result: the reader's `r.v = ..` never reached `Box.v`).
        let mut keys: Vec<&String> = Vec::new();
        for k in [self, o].into_iter().flat_map(|c| {
            c.facts
                .fields
                .iter()
                .map(|f| &f.key)
                .chain(c.field_fns.iter().map(|(k, _)| k))
                .chain(c.field_layouts.iter().map(|(k, _, _)| k))
                .chain(c.field_values.iter().map(|(k, _)| k))
                .chain(c.field_tas.iter().map(|(k, _, _)| k))
        }) {
            if !keys.contains(&k) {
                keys.push(k);
            }
        }
        let mut field_fns: Vec<(String, Vec<FnContract>)> = Vec::new();
        let mut field_layouts: Vec<(String, Vec<String>, bool)> = Vec::new();
        let mut field_values: Vec<(String, ValueContract)> = Vec::new();
        let mut field_tas: Vec<(String, Option<u8>, u16)> = Vec::new();
        for k in keys {
            let views: Vec<(&ClassContract, FieldView<'_>)> = [&*self, o]
                .into_iter()
                .flat_map(|c| field_views(c, k).into_iter().map(move |v| (c, v)))
                .collect();
            if views.iter().all(|(_, v)| matches!(v, FieldView::Plain(_))) {
                continue;
            }
            let fns_only = views.iter().all(|(_, v)| matches!(v, FieldView::Fns(_)));
            for (c, v) in views {
                let v = match v {
                    FieldView::Plain(f) if own_instances(c, f) => {
                        FieldView::Lay(vec![c.display.clone()], f.kind.prims != 0)
                    }
                    v => v,
                };
                match v {
                    FieldView::Fns(fs) if fns_only => {
                        match field_fns.iter_mut().find(|(q, _)| q == k) {
                            Some((_, x)) => join_keyed(x, fs, |f| f.name.clone(), FnContract::join),
                            None => field_fns.push((k.clone(), fs.clone())),
                        }
                    }
                    FieldView::Lay(ds, p) => {
                        match field_layouts.iter_mut().find(|(q, _, _)| q == k) {
                            Some((_, x, q)) => {
                                union_into(x, &ds);
                                *q |= p;
                            }
                            None => field_layouts.push((k.clone(), ds, p)),
                        }
                    }
                    FieldView::Ta(e, p) => match field_tas.iter_mut().find(|(q, _, _)| q == k) {
                        Some((_, f, q)) => {
                            if *f != e {
                                *f = None;
                            }
                            *q |= p;
                        }
                        None => field_tas.push((k.clone(), e, p)),
                    },
                    v => {
                        let vc = match v {
                            FieldView::Val(vc) => vc.clone(),
                            FieldView::Fns(fs) => ValueContract {
                                fns: fs.clone(),
                                ..Default::default()
                            },
                            FieldView::Plain(f) => plain_value(c, f),
                            _ => unreachable!("handled above"),
                        };
                        match field_values.iter_mut().find(|(q, _)| q == k) {
                            Some((_, x)) => x.join(&vc),
                            None => field_values.push((k.clone(), vc)),
                        }
                    }
                }
            }
        }
        field_layouts.sort();
        field_values.sort_by(|x, y| x.0.cmp(&y.0));
        field_tas.sort();
        // Statics, as the reader builds them.
        let member = |c: &ClassContract, k: &str| -> Option<StaticMember> {
            let (_, m) = c.statics.iter().find(|(q, _)| q == k)?;
            Some(match c.static_fns.iter().find(|(q, _)| q == k) {
                Some((_, fc)) => StaticMember::Fn(fc.clone()),
                None if c.static_values.iter().any(|(q, _)| q == k) => {
                    let (_, vc) = c.static_values.iter().find(|(q, _)| q == k).unwrap();
                    StaticMember::Value(vc.clone())
                }
                None => match m {
                    Some(f) => StaticMember::Stub(f.clone()),
                    None => StaticMember::Top,
                },
            })
        };
        let mut statics: Vec<(String, Option<String>)> = Vec::new();
        let mut static_fns: Vec<(String, FnContract)> = Vec::new();
        let mut static_values: Vec<(String, ValueContract)> = Vec::new();
        for (k, _) in self.statics.iter().chain(o.statics.iter()) {
            if statics.iter().any(|(q, _)| q == k) {
                continue;
            }
            let m = match (member(self, k), member(o, k)) {
                (Some(x), None) | (None, Some(x)) => x,
                (Some(StaticMember::Fn(mut f)), Some(StaticMember::Fn(g))) if f.name == g.name => {
                    f.join(&g);
                    StaticMember::Fn(f)
                }
                (Some(StaticMember::Value(mut v)), Some(StaticMember::Value(w))) => {
                    v.join(&w);
                    StaticMember::Value(v)
                }
                (Some(x), Some(y)) if x == y => x,
                _ => StaticMember::Top,
            };
            let slot = self
                .statics
                .iter()
                .chain(o.statics.iter())
                .find(|(q, _)| q == k)
                .and_then(|(_, m)| m.clone());
            match m {
                StaticMember::Fn(f) => {
                    statics.push((k.clone(), slot));
                    static_fns.push((k.clone(), f));
                }
                StaticMember::Stub(f) => statics.push((k.clone(), Some(f))),
                StaticMember::Value(v) => {
                    statics.push((k.clone(), None));
                    static_values.push((k.clone(), v));
                }
                StaticMember::Top => statics.push((k.clone(), None)),
            }
        }
        // The methods with a receiver contract and the constructor's
        // (DECISIONS 57): one function joined; two different ones, the
        // reader's stub.
        let mut method_fns: Vec<(String, FnContract)> = Vec::new();
        for (k, f) in self.method_fns.iter().chain(o.method_fns.iter()) {
            match method_fns.iter().position(|(q, _)| q == k) {
                Some(i) if method_fns[i].1.name == f.name => method_fns[i].1.join(f),
                Some(i) => method_fns[i].1.name.clear(),
                None => method_fns.push((k.clone(), f.clone())),
            }
        }
        method_fns.retain(|(_, f)| !f.name.is_empty());
        method_fns.sort_by(|x, y| x.0.cmp(&y.0));
        match (&mut self.ctor_fn, &o.ctor_fn) {
            (Some(f), Some(g)) if f.name == g.name => f.join(g),
            (f, _) => *f = None,
        }
        if self.parent != o.parent {
            self.parent = None;
        }
        // The layout prefix a subclass in another unit takes: the common
        // birth keys, definite in both; a leak in either leaks.
        common_prefix(&mut self.birth_keys, &o.birth_keys);
        // A known slot whose presence one of them does not know.
        let known = |c: &Self, k: &String| c.definite.contains(k) || c.reserved.contains(k);
        let reserved: Vec<String> = self
            .definite
            .iter()
            .chain(self.reserved.iter())
            .filter(|k| known(o, k) && !(self.definite.contains(k) && o.definite.contains(k)))
            .cloned()
            .collect();
        self.definite.retain(|k| o.definite.contains(k));
        self.reserved = reserved;
        self.ctor_leaks |= o.ctor_leaks;
        self.ctor_returns |= o.ctor_returns;
        self.binding_prim |= o.binding_prim;
        self.field_values = field_values;
        self.field_tas = field_tas;
        self.method_fns = method_fns;
        // A closed class (§18.13) only where both say so, the same binding.
        if self.closed != o.closed || self.forwards != o.forwards {
            self.closed = None;
            self.forwards = false;
        }
        self.facts.join(&o.facts);
        union_into(&mut self.methods, &o.methods);
        self.field_fns = field_fns;
        self.field_layouts = field_layouts;
        self.open_keys |= o.open_keys;
        self.statics = statics;
        self.statics_open |= o.statics_open;
        self.static_fns = static_fns;
        self.static_values = static_values;
    }
}

impl ValueContract {
    /// Unknown, for this reason unless it already has one.
    pub(crate) fn unknown_because(&mut self, why: &'static str) {
        self.unknown = true;
        if self.why.0.is_none() {
            self.why.0 = Some((why, None));
        }
    }

    /// A contract that is unknown for this reason.
    pub(crate) fn unknown_for(why: &'static str) -> ValueContract {
        let mut vc = ValueContract {
            prim: crate::model::prim::ALL,
            ..Default::default()
        };
        vc.unknown_because(why);
        vc
    }

    /// Names the publishing unit on a reason that has none yet.
    pub(crate) fn stamp(&mut self, unit: &std::sync::Arc<str>) {
        if let Some((_, u @ None)) = &mut self.why.0 {
            *u = Some(unit.clone());
        }
    }

    /// The join of two value contracts: the layouts, classes and
    /// functions (each by identity, its description joined), an unknown
    /// value and the primitive kinds only grow.
    pub(crate) fn join(&mut self, o: &ValueContract) {
        join_keyed(
            &mut self.layouts,
            &o.layouts,
            |c| c.display.clone(),
            ClassContract::join,
        );
        join_keyed(
            &mut self.classes,
            &o.classes,
            |c| c.display.clone(),
            ClassContract::join,
        );
        join_keyed(&mut self.fns, &o.fns, |f| f.name.clone(), FnContract::join);
        self.unknown |= o.unknown;
        self.why.join(&o.why);
        union_into(&mut self.shared, &o.shared);
        self.shared.sort();
        self.prim |= o.prim;
        // Builtins and arrays (DECISIONS 94): by identity.
        union_into(&mut self.builtins, &o.builtins);
        union_into(&mut self.arrays, &o.arrays);
        union_into(&mut self.made_arrays, &o.made_arrays);
        union_into(&mut self.boxes, &o.boxes);
        union_into(&mut self.arrayboxes, &o.arrayboxes);
        union_into(&mut self.indexed_owners, &o.indexed_owners);
        self.indexed_owners.sort();
        self.builtins.sort();
        self.arrays.sort();
        self.made_arrays.sort();
        self.boxes.sort();
        self.arrayboxes.sort();
        self.layouts.sort_by(|x, y| x.display.cmp(&y.display));
        self.classes.sort_by(|x, y| x.display.cmp(&y.display));
        self.fns.sort_by(|x, y| x.name.cmp(&y.name));
    }
}

impl ArgContract {
    /// The lattice join (D16): `top`, the parameters and each parameter's
    /// value contract (`ValueContract::join`, descriptions included) only
    /// grow. Every component is a join in a finite lattice, so the driver's
    /// argument contracts, last round's joined with what the latest solves
    /// pass, ascend and stop changing.
    fn join(&mut self, o: &ArgContract) {
        self.top |= o.top;
        if self.params.len() < o.params.len() {
            self.params.resize(o.params.len(), ValueContract::default());
        }
        for (a, b) in self.params.iter_mut().zip(&o.params) {
            a.join(b);
        }
    }
}

/// What the other units can name of each module (§11.1): the export names
/// some module imports or re-exports by name, or the whole namespace where
/// a module imports it as a namespace, re-exports all of it, or a dynamic
/// `import()` can reach it. Syntactic and linear in the import edges.
#[derive(Clone, Debug, Default)]
pub(crate) struct Published {
    whole: FxHashSet<String>,
    /// DECISIONS 188: the CommonJS support module (the module every wrapped
    /// module imports as `perry:cjs`): its `cache` and `main` are the
    /// program's `require.cache` and main record.
    pub(crate) cjs_support: Option<String>,
    /// Modules whose namespace object itself reaches other units as a value
    /// no contract describes: a dynamic `import()` or an `export * as`
    /// re-export. A static namespace import is described (§18.2).
    ns_object: FxHashSet<String>,
    names: FxHashMap<String, FxHashSet<String>>,
    /// Interpreted code can import any module: everything is published.
    all: bool,
    /// A direct eval can name a module's bindings: its code may call any
    /// function of that module (no function is closed, decision 13).
    eval_scopes: bool,
    /// Templates (by display) the census found used beyond their contract:
    /// published as escapes.
    pub(crate) broken: FxHashSet<String>,
    /// The class contracts of the units solved so far, by (defining module
    /// path, export name).
    pub(crate) classes: FxHashMap<(String, String), ClassContract>,
    /// Their function contracts, by (defining module path, export name).
    pub(crate) fns: FxHashMap<(String, String), FnContract>,
    /// Their value contracts, by (defining module path, export name).
    pub(crate) values: FxHashMap<(String, String), ValueContract>,
    /// The layouts contract functions return, by template display.
    pub(crate) layouts: FxHashMap<String, Digested<ClassContract>>,
    /// The function contracts by name, each its defining unit's
    /// (DECISIONS 106), kept across rounds as `layouts`.
    pub(crate) own_fns: FxHashMap<String, Digested<FnContract>>,
    /// DECISIONS 107: per published method (contract name), the receivers
    /// the units running it published, joined; `this_args_top`: a unit
    /// over the bound may run any of them on anything.
    pub(crate) this_args: FxHashMap<String, ValueContract>,
    pub(crate) this_args_top: bool,
    /// DECISIONS 165: the published methods (contract names) some unit
    /// runs, joined (an aborted unit may run any: `this_args_top`).
    pub(crate) method_runs: FxHashSet<String>,
    /// DECISIONS 148: per published method some unit runs (contract name),
    /// what the running units' calls pass, joined
    /// (`ProgramFacts::method_args_out`).
    pub(crate) method_args: FxHashMap<String, ArgContract>,
    /// DECISIONS 210: per contract function some unit constructs (`new` of
    /// its stand-in; contract name), what the constructing units pass,
    /// joined (`ProgramFacts::ctor_runs_out`): its owner constructs it with
    /// their union on its own `new <f>` layout and publishes that layout.
    pub(crate) ctor_runs: FxHashMap<String, ArgContract>,
    /// The argument contracts of closed functions (decision 13), by
    /// (module path, name): the union over every unit's calls.
    pub(crate) args: FxHashMap<(String, String), ArgContract>,
    /// Store effects (decision 17): per literal layout or class display,
    /// the existing keys other units store into and the value contract of
    /// what they store (union over every unit; DECISIONS 73 is its `prim`,
    /// design §17.16 (c)).
    pub(crate) stores: FxHashMap<String, Vec<(String, ValueContract)>>,
    /// Write effects: per module-scope binding (its unit's path, its name)
    /// the value contract of what other units store into it (union over
    /// every unit, `Gen::local_write`).
    pub(crate) writes: FxHashMap<(String, String), ValueContract>,
    /// What the units do to the objects every unit shares (union over every
    /// unit, `model::SharedWrite`): each unit's solve replays them.
    pub(crate) shared: std::collections::BTreeSet<crate::model::SharedWrite>,
    /// Each module's namespace object (design §18.2): its keys, where each
    /// resolves through re-exports (§18.14), and whether it may hold any key
    /// (an `export *` from outside the program).
    pub(crate) exports: Exports,
    /// The classes (defining module path, export name) a class of another
    /// unit names in its `extends` (DECISIONS 57): their constructor is a
    /// contract too, run on any receiver.
    pub(crate) extended: FxHashSet<(String, String)>,
    /// Cells (design §17.16 (b)): a runtime location every unit names the
    /// same way (`cjs:<path>`, the registry union `cjs:*`, a computed path
    /// `cjs:?`), the value contracts written, each with the units writing
    /// it (their keys, sorted). A reader joins the other units' writes.
    /// Grouped by contract (DECISIONS 113): every unit may write one cell
    /// (`global:*`), and a reader takes each distinct write once.
    pub(crate) cells: FxHashMap<String, Vec<CellWrite>>,
    /// A unit solve went over the bound: what it writes is unknown, so
    /// every cell may hold an unknown value.
    pub(crate) cells_top: bool,
}

impl Published {
    pub(crate) fn whole(&self, path: &str) -> bool {
        self.all || self.whole.contains(path)
    }

    pub(crate) fn fn_contract(&self, path: &str, name: &str) -> Option<&FnContract> {
        self.fns.get(&(path.to_string(), name.to_string()))
    }

    pub(crate) fn value_contract(&self, path: &str, name: &str) -> Option<&ValueContract> {
        self.values.get(&(path.to_string(), name.to_string()))
    }

    pub(crate) fn layout(&self, display: &str) -> Option<&ClassContract> {
        self.layouts.get(display).map(|e| &e.c)
    }

    /// What a reader of layout `display` records (DECISIONS 114): its
    /// digest, or the digest of no entry.
    pub(crate) fn layout_seen(&self, display: &str) -> u64 {
        self.layouts
            .get(display)
            .map_or_else(|| digest(&None::<&ClassContract>), |e| e.digest)
    }

    pub(crate) fn own_fn(&self, name: &str) -> Option<&FnContract> {
        self.own_fns.get(name).map(|e| &e.c)
    }

    /// What a reader of function contract `name` records, as `layout_seen`.
    pub(crate) fn own_fn_seen(&self, name: &str) -> u64 {
        self.own_fns
            .get(name)
            .map_or_else(|| digest(&None::<&FnContract>), |e| e.digest)
    }

    pub(crate) fn class_contract(&self, path: &str, name: &str) -> Option<&ClassContract> {
        self.classes.get(&(path.to_string(), name.to_string()))
    }

    /// The closed class `(path, name)` (`ClassContract::closed`): its
    /// contract under the module's path, or, when the class is no export of
    /// the module (published under its unit's path alone, `unit_exports`),
    /// the one whose `closed` names it.
    pub(crate) fn closed_class(&self, path: &str, name: &str) -> Option<&ClassContract> {
        self.class_contract(path, name).or_else(|| {
            self.classes.values().find(|c| {
                c.closed
                    .as_ref()
                    .is_some_and(|(p, n, _)| p == path && n == name)
            })
        })
    }

    pub(crate) fn arg_contract(&self, path: &str, name: &str) -> Option<&ArgContract> {
        self.args.get(&(path.to_string(), name.to_string()))
    }

    /// A direct eval may name module bindings: interpreted code (which
    /// otherwise reaches only exports and globals) may call a module's
    /// functions by name. The reach does not say which module's, so none
    /// is closed.
    /// What other units write into binding `name` of unit `path`.
    pub(crate) fn binding_write(&self, path: &str, name: &str) -> Option<&ValueContract> {
        self.writes.get(&(path.to_string(), name.to_string()))
    }

    pub(crate) fn stored_keys(&self, display: &str) -> &[(String, ValueContract)] {
        self.stores.get(display).map_or(&[], |v| v.as_slice())
    }

    pub(crate) fn eval_scopes(&self) -> bool {
        self.eval_scopes
    }

    /// Module `path`'s namespace object reaches other units undescribed.
    /// Interpreted code (decision 150) holds what escaped, the globals and
    /// what is thrown, as in the whole-program solve: no namespace object
    /// unless a direct eval can name a module's bindings (DECISIONS 65).
    pub(crate) fn ns_object(&self, path: &str) -> bool {
        self.eval_scopes || self.ns_object.contains(path)
    }

    pub(crate) fn ns_keys(&self, path: &str) -> Option<&ModExports> {
        self.exports.by_path.get(path)
    }

    /// Where `import { name } from path` leads (§18.14).
    pub(crate) fn resolve(&self, path: &str, name: &str) -> Option<Resolved<'_>> {
        self.exports.resolve(path, name)
    }

    pub(crate) fn broken(&self, display: &str) -> bool {
        self.broken.contains(display)
    }

    pub(crate) fn extended(&self, path: &str, name: &str) -> bool {
        self.extended
            .contains(&(path.to_string(), name.to_string()))
    }

    pub(crate) fn names(&self, path: &str, name: &str) -> bool {
        self.whole(path) || self.names.get(path).is_some_and(|n| n.contains(name))
    }
}

/// The module an `export … from source` of module `path` reads: the
/// driver's resolution (`FactsConfig::export_sources`), else an import of
/// the same source, as `Gen::resolve_source`. A re-export declares no import
/// of its source (`export * as X from "./self"`), so the imports alone miss
/// it, and the surface, the namespace keys and the order must not.
fn reexport_source<'i>(
    es: &crate::ExportSources,
    path: &str,
    imports: impl IntoIterator<Item = &'i Import>,
    source: &str,
) -> Option<String> {
    if let Some(p) = es.get(path).and_then(|m| m.get(source)) {
        return Some(p.clone());
    }
    imports
        .into_iter()
        .find(|i| i.source == source)
        .and_then(|i| i.resolved_path.clone())
}

pub(crate) fn published_surface(
    sorted: &[(&str, Vec<&Import>, &[Export])],
    cfg: &FactsConfig,
    ex: &Exports,
) -> Published {
    let mut p = Published {
        all: cfg.dynamic_code.is_some()
            || cfg.dynamic_code_reach.globals
            || !cfg.dynamic_code_reach.sites.is_empty(),
        eval_scopes: !cfg.dynamic_code_reach.lexical_modules.is_empty(),
        ..Default::default()
    };
    for (path, imports, exports) in sorted {
        let resolve = |source: &str| {
            reexport_source(&cfg.export_sources, path, imports.iter().copied(), source)
        };
        for imp in imports.iter().copied() {
            if imp.source == "perry:cjs" {
                if let Some(t) = &imp.resolved_path {
                    p.cjs_support = Some(t.clone());
                }
            }
            let Some(t) = imp.resolved_path.as_ref().filter(|t| t.as_str() != *path) else {
                continue;
            };
            // Another declaration unit of the same module: its names are
            // the module's (§12.3).
            let t = &crate::decl_units::module_of(t).to_string();
            if imp.type_only || imp.runtime_erased {
                continue;
            }
            // `import()` hands over the namespace object itself, also where
            // the driver folds it into a static import of the same module.
            if imp.is_dynamic || imp.is_dynamic_target {
                p.whole.insert(t.clone());
                p.ns_object.insert(t.clone());
            }
            for s in &imp.specifiers {
                match s {
                    ImportSpecifier::Named { imported, .. } => {
                        p.names
                            .entry(t.clone())
                            .or_default()
                            .insert(imported.clone());
                    }
                    ImportSpecifier::Default { .. } => {
                        p.names
                            .entry(t.clone())
                            .or_default()
                            .insert("default".into());
                    }
                    ImportSpecifier::Namespace { .. } => {
                        p.whole.insert(t.clone());
                    }
                }
            }
        }
        for e in exports.iter() {
            match e {
                Export::ReExport {
                    source, imported, ..
                } => {
                    if let Some(t) = resolve(source) {
                        p.names.entry(t).or_default().insert(imported.clone());
                    }
                }
                Export::ExportAll { source } => {
                    if let Some(t) = resolve(source) {
                        p.whole.insert(t);
                    }
                }
                Export::NamespaceReExport { source, .. } => {
                    if let Some(t) = resolve(source) {
                        p.whole.insert(t.clone());
                        p.ns_object.insert(t);
                    }
                }
                Export::Named { .. } => {}
            }
        }
    }
    // A name a module re-exports is named where it is declared (§18.14):
    // published there, as the importer reads its contract there; a
    // re-exported namespace is whole there.
    let mut add: Vec<(String, String)> = Vec::new();
    let mut whole: Vec<String> = p.whole.iter().cloned().collect();
    for (t, names) in &p.names {
        for n in names {
            match ex.resolve(t, n) {
                Some(Resolved::Binding(d, m)) if d != t || m != n => {
                    add.push((d.to_string(), m.to_string()))
                }
                Some(Resolved::Namespace(d)) => whole.push(d.to_string()),
                _ => {}
            }
        }
    }
    let mut seen: FxHashSet<String> = FxHashSet::default();
    while let Some(t) = whole.pop() {
        if !seen.insert(t.clone()) {
            continue;
        }
        p.whole.insert(t.clone());
        let Some(me) = ex.by_path.get(&t) else {
            continue;
        };
        for (k, target) in &me.keys {
            match target {
                ExportTarget::Binding(d, m) => {
                    let d = &ex.paths[*d as usize];
                    if *d != t || m != k {
                        add.push((d.clone(), m.clone()));
                    }
                }
                ExportTarget::Namespace(d) => whole.push(ex.paths[*d as usize].clone()),
                ExportTarget::Unknown => {}
            }
        }
    }
    for (d, m) in add {
        p.names.entry(d).or_default().insert(m);
    }
    p
}

/// Where an export name of a module leads (ECMAScript ResolveExport, design
/// §18.14): a binding some module declares, a module's namespace object
/// (`export * as n from`, `import * as n; export { n }`), or unknown.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum ExportTarget {
    /// Binding `.1` that module `.0` (an index into `Exports::paths`) declares.
    Binding(u32, String),
    /// The namespace object of module `.0`.
    Namespace(u32),
    /// A source outside the program, or a name two `export *` sources bind
    /// differently (the namespace leaves it out, a named import of it is a
    /// link error): nothing is known of it.
    Unknown,
}

/// A module's namespace (design §18.2): its export names, each with where
/// it resolves, sorted by name; open when an `export *` source in reach is
/// outside the program (any other key may be there).
#[derive(Clone, Debug, Default)]
pub(crate) struct ModExports {
    pub(crate) keys: Vec<(String, ExportTarget)>,
    pub(crate) open: bool,
}

impl ModExports {
    pub(crate) fn get(&self, k: &str) -> Option<&ExportTarget> {
        self.keys
            .binary_search_by(|(x, _)| x.as_str().cmp(k))
            .ok()
            .map(|i| &self.keys[i].1)
    }

    pub(crate) fn has(&self, k: &str) -> bool {
        self.get(k).is_some()
    }
}

/// Every module's namespace, by path; `paths` names the modules targets
/// point at.
#[derive(Clone, Debug, Default)]
pub(crate) struct Exports {
    pub(crate) paths: Vec<String>,
    pub(crate) by_path: FxHashMap<String, ModExports>,
}

/// What an import of a name from a module binds (`Published::resolve`).
#[derive(Debug, PartialEq)]
pub(crate) enum Resolved<'a> {
    /// Binding `.1` declared by module `.0`: its contract is keyed so.
    Binding(&'a str, &'a str),
    /// The namespace object of module `.0`.
    Namespace(&'a str),
    Unknown,
}

impl Exports {
    /// The other modules declaring a binding module `m`'s namespace holds,
    /// through the namespaces it holds too (`m` itself left out).
    pub(crate) fn reach(&self, m: usize) -> Vec<usize> {
        let mut out: Vec<usize> = Vec::new();
        let mut seen: FxHashSet<usize> = FxHashSet::default();
        let mut todo = vec![m];
        while let Some(x) = todo.pop() {
            if !seen.insert(x) {
                continue;
            }
            let Some(me) = self.by_path.get(&self.paths[x]) else {
                continue;
            };
            for (_, t) in &me.keys {
                match *t {
                    ExportTarget::Binding(d, _) if d as usize != m => out.push(d as usize),
                    ExportTarget::Namespace(d) => todo.push(d as usize),
                    _ => {}
                }
            }
        }
        out.sort_unstable();
        out.dedup();
        out
    }

    /// Where `import { name } from path` leads: `None` when `path` has no
    /// such key (no export the declarations name: its contract, if any, is
    /// keyed by `path` itself).
    pub(crate) fn resolve<'s>(&'s self, path: &str, name: &str) -> Option<Resolved<'s>> {
        Some(match self.by_path.get(path)?.get(name)? {
            ExportTarget::Binding(m, n) => Resolved::Binding(&self.paths[*m as usize], n),
            ExportTarget::Namespace(m) => Resolved::Namespace(&self.paths[*m as usize]),
            ExportTarget::Unknown => Resolved::Unknown,
        })
    }
}

/// One module's own export entries, as ResolveExport reads them.
enum Own<'m> {
    /// Declared by the module under its export name.
    Here,
    /// An indirect export: name `.1` of module `.0` (`export { a } from`,
    /// `import { a }; export { a }`); `None`: a module outside the program.
    Indirect(Option<usize>, &'m str),
    /// The namespace of module `.0` (`None`: outside the program).
    Ns(Option<usize>),
}

struct Resolver<'m> {
    own: Vec<FxHashMap<&'m str, Own<'m>>>,
    stars: Vec<Vec<Option<usize>>>,
    /// Resolutions reached without cutting a cycle: the same from every
    /// resolve set.
    memo: Vec<FxHashMap<&'m str, Option<ExportTarget>>>,
}

impl<'m> Resolver<'m> {
    /// ResolveExport(m, name) (ECMA-262 16.2.1.6.3): `None` when not
    /// found; `.1` when a cycle was cut on the way (the result then
    /// depends on the resolve set and is not memoized).
    fn resolve(
        &mut self,
        m: usize,
        name: &'m str,
        set: &mut Vec<(usize, &'m str)>,
    ) -> (Option<ExportTarget>, bool) {
        if let Some(r) = self.memo[m].get(name) {
            return (r.clone(), false);
        }
        if set.contains(&(m, name)) {
            return (None, true);
        }
        set.push((m, name));
        let mut cyc = false;
        let r = match self.own[m].get(name) {
            Some(Own::Here) => Some(ExportTarget::Binding(m as u32, name.to_string())),
            Some(Own::Ns(Some(t))) => Some(ExportTarget::Namespace(*t as u32)),
            Some(Own::Ns(None) | Own::Indirect(None, _)) => Some(ExportTarget::Unknown),
            Some(&Own::Indirect(Some(t), imported)) => {
                let (r, c) = self.resolve(t, imported, set);
                cyc |= c;
                // An indirect export that resolves nowhere is a link error.
                Some(r.unwrap_or(ExportTarget::Unknown))
            }
            // `export *` never re-exports `default`.
            None if name == "default" => None,
            None => {
                let mut acc: Option<ExportTarget> = None;
                for i in 0..self.stars[m].len() {
                    let r = match self.stars[m][i] {
                        None => Some(ExportTarget::Unknown),
                        Some(s) => {
                            let (r, c) = self.resolve(s, name, set);
                            cyc |= c;
                            r
                        }
                    };
                    acc = match (acc, r) {
                        (a, None) => a,
                        (None, b) => b,
                        (Some(a), Some(b)) if a == b => Some(a),
                        // Two different bindings (ambiguous), or a source
                        // outside the program that may bind it too.
                        _ => Some(ExportTarget::Unknown),
                    };
                }
                acc
            }
        };
        set.pop();
        if !cyc {
            self.memo[m].insert(name, r.clone());
        }
        (r, cyc)
    }
}

/// Each module's namespace (design §18.2, §18.14): its export names
/// (GetExportedNames: its own, then every `export *` source's but
/// `default`, a revisited module ending the walk) and where each resolves
/// (ResolveExport), from the HIR's import and export declarations.
/// Compiler-internal: nothing of it is emitted.
pub(crate) fn module_exports(sorted: &[(&str, &Module)], es: &crate::ExportSources) -> Exports {
    let index: FxHashMap<&str, usize> = sorted
        .iter()
        .enumerate()
        .map(|(i, (p, _))| (*p, i))
        .collect();
    let mut own: Vec<FxHashMap<&str, Own>> = Vec::with_capacity(sorted.len());
    let mut stars: Vec<Vec<Option<usize>>> = Vec::with_capacity(sorted.len());
    for (path, m) in sorted {
        let module_of = |p: Option<String>| p.and_then(|t| index.get(t.as_str()).copied());
        // The module's import bindings: an export of one is indirect.
        let mut imported: FxHashMap<&str, Own> = FxHashMap::default();
        for imp in m.imports.iter().filter(|i| !i.type_only) {
            let t = || module_of(imp.resolved_path.clone());
            for sp in &imp.specifiers {
                let (local, o) = match sp {
                    ImportSpecifier::Named { imported, local } => {
                        (local, Own::Indirect(t(), imported.as_str()))
                    }
                    ImportSpecifier::Default { local } => (local, Own::Indirect(t(), "default")),
                    ImportSpecifier::Namespace { local } => (local, Own::Ns(t())),
                };
                imported.insert(local.as_str(), o);
            }
        }
        let mut o: FxHashMap<&str, Own> = FxHashMap::default();
        let mut st: Vec<Option<usize>> = Vec::new();
        for e in &m.exports {
            let src = |source: &str| module_of(reexport_source(es, path, &m.imports, source));
            match e {
                Export::Named { local, exported } => {
                    let v = match imported.get(local.as_str()) {
                        Some(&Own::Indirect(t, n)) => Own::Indirect(t, n),
                        Some(&Own::Ns(t)) => Own::Ns(t),
                        _ => Own::Here,
                    };
                    o.insert(exported.as_str(), v);
                }
                Export::ReExport {
                    source,
                    imported,
                    exported,
                } => {
                    o.insert(
                        exported.as_str(),
                        Own::Indirect(src(source), imported.as_str()),
                    );
                }
                Export::NamespaceReExport { source, name } => {
                    o.insert(name.as_str(), Own::Ns(src(source)));
                }
                Export::ExportAll { source } => st.push(src(source)),
            }
        }
        let decls = m
            .exported_functions
            .iter()
            .map(|(n, _)| n)
            .chain(
                m.functions
                    .iter()
                    .filter(|f| f.is_exported)
                    .map(|f| &f.name),
            )
            .chain(m.classes.iter().filter(|c| c.is_exported).map(|c| &c.name))
            .chain(m.enums.iter().filter(|e| e.is_exported).map(|e| &e.name))
            .chain(m.exported_objects.iter());
        for n in decls {
            o.entry(n.as_str()).or_insert(Own::Here);
        }
        own.push(o);
        stars.push(st);
    }
    let mut r = Resolver {
        own,
        stars,
        memo: (0..sorted.len()).map(|_| FxHashMap::default()).collect(),
    };
    let mut by_path = FxHashMap::default();
    for (i, (path, _)) in sorted.iter().enumerate() {
        // GetExportedNames.
        let mut names: FxHashSet<&str> = r.own[i].keys().copied().collect();
        let mut open = false;
        let mut seen = vec![i];
        let mut todo: Vec<Option<usize>> = r.stars[i].clone();
        while let Some(s) = todo.pop() {
            let Some(s) = s else {
                open = true;
                continue;
            };
            if seen.contains(&s) {
                continue;
            }
            seen.push(s);
            names.extend(r.own[s].keys().copied().filter(|k| *k != "default"));
            todo.extend(r.stars[s].iter().copied());
        }
        let mut keys: Vec<(String, ExportTarget)> = names
            .into_iter()
            .filter_map(|k| {
                let t = r.resolve(i, k, &mut Vec::new()).0?;
                Some((k.to_string(), t))
            })
            .collect();
        keys.sort_unstable_by(|a, b| a.0.cmp(&b.0));
        by_path.insert(path.to_string(), ModExports { keys, open });
    }
    Exports {
        paths: sorted.iter().map(|(p, _)| p.to_string()).collect(),
        by_path,
    }
}

#[path = "units/driver.rs"]
mod driver;
pub(crate) use driver::digest;
pub(crate) use driver::run;
#[cfg(test)]
use driver::unique_bindings;
pub(crate) use driver::CellWrite;
pub(crate) use driver::Digested;

#[cfg(test)]
mod binding_collision_tests {
    #[test]
    #[should_panic(expected = "duplicate LocalId 7")]
    fn duplicate_local_is_an_internal_compiler_error() {
        super::unique_bindings(&mut [(7, ()), (7, ())], "clones.ts", "LocalId");
    }

    #[test]
    #[should_panic(expected = "duplicate this-of FuncId 9")]
    fn duplicate_this_is_an_internal_compiler_error() {
        super::unique_bindings(&mut [(9, ()), (9, ())], "clones.ts", "this-of FuncId");
    }
}
