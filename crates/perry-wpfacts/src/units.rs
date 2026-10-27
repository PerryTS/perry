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
    pub ctor_leaks: bool,
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
#[derive(Clone, Debug, PartialEq, Hash)]
pub(crate) struct ParamCall {
    pub param: u32,
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
        |x| (x.param, x.key.clone(), x.args.len()),
        |x, y| x.join(y),
    );
    a.sort_by(|x, y| (x.param, &x.key, x.args.len()).cmp(&(y.param, &y.key, y.args.len())));
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
    pub name: String,
    /// DECISIONS 165: a method of a published class (its owner's prototype
    /// holds it): a unit calling it publishes what it passes
    /// (`ProgramFacts::method_runs_out`), however it got the function.
    pub method: bool,
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

/// A published module-scope value binding as its readers see it (design
/// §11.2 "an object contract", generalized to every binding another
/// declaration unit reads, §12): the layouts it may hold (literal-shape
/// objects of the defining unit, instances of contract classes, layouts it
/// holds through other contracts), the contract classes and functions it
/// may hold, and whether it may hold an unknown value (⊤ to the reader).
/// The layouts' fields hold unknown values for the reader; a store or an
/// opening operation through them is the reader's census report.
#[derive(Clone, Debug, Default, PartialEq, Hash)]
pub(crate) struct ValueContract {
    pub layouts: Vec<ClassContract>,
    pub classes: Vec<ClassContract>,
    pub fns: Vec<FnContract>,
    pub unknown: bool,
    /// The runtime-owned objects it may hold (`Model::named_shared`), one
    /// object for the whole program: the reader holds them as well.
    pub shared: Vec<crate::model::Shared>,
    /// The primitive kinds it may hold (`model::prim` bits).
    pub prim: u16,
    /// The global builtins it may hold (design §17.16 (a)): a builtin is one
    /// runtime object for the whole program, the same token in every unit.
    pub builtins: Vec<crate::model::BuiltinNs>,
    /// The arrays of other units it may hold (DECISIONS 94), by template
    /// display: their elements are cell `arr:<display>`.
    pub arrays: Vec<String>,
    /// DECISIONS 184: the primitive method containers (`primbox`) it may
    /// hold, by name as `arrays`: their elements are cell `arr:<name>`; a
    /// reader's stand-in is a container of its own kind.
    pub boxes: Vec<String>,
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
        self.method |= o.method;
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

impl ClassContract {
    /// The join of two descriptions of one layout or class (by `display`;
    /// its name, class id, shape and native parent are its identity):
    /// every claim a reader may rely on holds in both. Facts weaken
    /// (`derive::TemplateFacts::join`); keys, methods and statics are the
    /// union, and a member the two describe differently is an unknown
    /// value; a field holds contract functions only where both say so;
    /// open keys and open statics only grow.
    pub(crate) fn join(&mut self, o: &ClassContract) {
        // Field functions: a field both describe holds the union of their
        // functions; a field one side's facts have without field functions
        // holds unknown values, so it has none in the join.
        let plain = |c: &ClassContract, k: &str| {
            c.facts.fields.iter().any(|f| f.key == k) && !c.field_fns.iter().any(|(q, _)| q == k)
        };
        let mut field_fns: Vec<(String, Vec<FnContract>)> = Vec::new();
        for (k, fs) in self.field_fns.iter().chain(o.field_fns.iter()) {
            if plain(self, k) || plain(o, k) {
                continue;
            }
            match field_fns.iter_mut().find(|(q, _)| q == k) {
                Some((_, v)) => join_keyed(v, fs, |f| f.name.clone(), FnContract::join),
                None => field_fns.push((k.clone(), fs.clone())),
            }
        }
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
        // Nested layouts (§18.7), as the field functions: a field both
        // describe holds the union of their layouts (primitives too if
        // either says so); a field one side's facts have without them holds
        // unknown values, so it has none in the join.
        let plain_l = |c: &ClassContract, k: &str| {
            c.facts.fields.iter().any(|f| f.key == k)
                && !c.field_layouts.iter().any(|(q, _, _)| q == k)
        };
        let mut field_layouts: Vec<(String, Vec<String>, bool)> = Vec::new();
        for (k, ds, p) in self.field_layouts.iter().chain(o.field_layouts.iter()) {
            if plain_l(self, k) || plain_l(o, k) {
                continue;
            }
            match field_layouts.iter_mut().find(|(q, _, _)| q == k) {
                Some((_, v, q)) => {
                    union_into(v, ds);
                    *q |= *p;
                }
                None => field_layouts.push((k.clone(), ds.clone(), *p)),
            }
        }
        field_layouts.sort();
        // Nested values (DECISIONS 94) and typed-array fields, as the
        // layouts: a field one side's facts have without them holds unknown
        // values; an element kind the two disagree on is none.
        let plain_v = |c: &ClassContract, k: &str| {
            c.facts.fields.iter().any(|f| f.key == k) && !c.field_values.iter().any(|(q, _)| q == k)
        };
        let mut field_values: Vec<(String, ValueContract)> = Vec::new();
        for (k, v) in self.field_values.iter().chain(o.field_values.iter()) {
            if plain_v(self, k) || plain_v(o, k) {
                continue;
            }
            match field_values.iter_mut().find(|(q, _)| q == k) {
                Some((_, x)) => x.join(v),
                None => field_values.push((k.clone(), v.clone())),
            }
        }
        field_values.sort_by(|x, y| x.0.cmp(&y.0));
        let plain_t = |c: &ClassContract, k: &str| {
            c.facts.fields.iter().any(|f| f.key == k) && !c.field_tas.iter().any(|(q, _, _)| q == k)
        };
        let mut field_tas: Vec<(String, Option<u8>, u16)> = Vec::new();
        for (k, e, p) in self.field_tas.iter().chain(o.field_tas.iter()) {
            if plain_t(self, k) || plain_t(o, k) {
                continue;
            }
            match field_tas.iter_mut().find(|(q, _, _)| q == k) {
                Some((_, f, q)) => {
                    if f != e {
                        *f = None;
                    }
                    *q |= *p;
                }
                None => field_tas.push((k.clone(), *e, *p)),
            }
        }
        field_tas.sort();
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
        self.definite.retain(|k| o.definite.contains(k));
        self.ctor_leaks |= o.ctor_leaks;
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
        union_into(&mut self.shared, &o.shared);
        self.shared.sort();
        self.prim |= o.prim;
        // Builtins and arrays (DECISIONS 94): by identity.
        union_into(&mut self.builtins, &o.builtins);
        union_into(&mut self.arrays, &o.arrays);
        union_into(&mut self.boxes, &o.boxes);
        self.builtins.sort();
        self.arrays.sort();
        self.boxes.sort();
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

/// One solve of the per-unit driver: a declaration unit of module `mi`
/// (design §12.6), its view, the solves it depends on (the units of its
/// module it names, every unit of each module it imports: what a census
/// round solves again), and the solves it is ordered after.
struct Solve<'a> {
    mi: usize,
    key: String,
    /// The one module the unit is cut from (its run-once view, N5, when
    /// it has one): borrowed, never copied per unit.
    module: &'a Module,
    view: crate::decl_units::UnitView<'a>,
    deps: Vec<usize>,
    /// Decision 21: the units of its module it names and, in each module it
    /// imports, the units owning the names it refers to (every unit for a
    /// namespace import or a name no unit exports). A unit reads another
    /// module's contracts only through those names, so this is the order
    /// the contracts must exist in; ordering by whole modules made cycles
    /// the names do not have, and a back edge's importer sees ⊤.
    after: Vec<usize>,
}

/// The unit of module range `t` exporting `name`; a re-export goes with
/// the first unit (`decl_units::views`).
/// The unit of module range `t` that exports `name` as an export of the
/// module: a unit exporting it to the module's own units only
/// (`UnitView::unit_exports`) is not the module's binding of that name
/// (`var _C = class _C {..}; export { _C }`: the class unit exports `_C` to
/// the binding's unit, which the module exports).
fn owner_of(out: &[Solve], t: &std::ops::Range<usize>, name: &str) -> Option<usize> {
    t.clone().find(|&si| {
        !out[si].view.unit_exports.contains(name)
            && out[si].view.exports.iter().any(|e| match e {
                Export::Named { exported, .. } | Export::ReExport { exported, .. } => {
                    exported == name
                }
                Export::NamespaceReExport { name: n, .. } => n == name,
                _ => false,
            })
    })
}

fn solves<'a>(
    sorted: &[(&'a str, &'a Module)],
    flats: &'a [Option<Module>],
    cfg: &FactsConfig,
    ex: &Exports,
) -> Vec<Solve<'a>> {
    // DECISIONS 129: what other modules can name of each module, from the
    // modules' own imports and re-exports (a unit's imports of its own
    // module's units are not another module's).
    let own: Vec<(&str, Vec<&Import>, &[Export])> = sorted
        .iter()
        .map(|(p, m)| (*p, m.imports.iter().collect(), m.exports.as_slice()))
        .collect();
    let reach = published_surface(&own, cfg, ex);
    let index: FxHashMap<&str, usize> = sorted
        .iter()
        .enumerate()
        .map(|(i, (p, _))| (*p, i))
        .collect();
    let mut out: Vec<Solve<'a>> = Vec::new();
    let mut range: Vec<std::ops::Range<usize>> = Vec::with_capacity(sorted.len());
    for (mi, (path, module)) in sorted.iter().enumerate() {
        let base = out.len();
        // A body that runs once at a module-scope statement is module scope
        // for the cut (N5, §12.5): the module's run-once view, one per
        // module, shared by all its units.
        let module: &'a Module = flats[mi].as_ref().unwrap_or(module);
        let named = |n: &str| reach.eval_scopes || reach.names(path, n);
        for (k, (view, deps)) in crate::decl_units::views(path, module, &named)
            .into_iter()
            .enumerate()
        {
            let deps: Vec<usize> = deps.into_iter().map(|d| base + d).collect();
            out.push(Solve {
                mi,
                key: crate::decl_units::unit_path(path, k),
                module,
                view: crate::decl_units::UnitView {
                    key: crate::decl_units::unit_path(path, k),
                    ..view
                },
                after: deps.clone(),
                deps,
            });
        }
        range.push(base..out.len());
    }
    let mut ns_reach: FxHashMap<usize, Vec<usize>> = FxHashMap::default();
    for si in 0..out.len() {
        let (path, module) = sorted[out[si].mi];
        let mut after: Vec<usize> = Vec::new();
        let mut deps: Vec<usize> = Vec::new();
        for imp in &module.imports {
            let Some(&t) = imp
                .resolved_path
                .as_deref()
                .filter(|t| *t != path)
                .and_then(|t| index.get(t))
            else {
                continue;
            };
            out[si].deps.extend(range[t].clone());
            // A re-export's names come from its source: the first unit,
            // which holds the module's re-exports, is ordered after all of it.
            let reexports = out[si].key.ends_with("#u0")
                && module.exports.iter().any(|e| match e {
                    Export::ReExport { source, .. }
                    | Export::ExportAll { source }
                    | Export::NamespaceReExport { source, .. } => *source == imp.source,
                    _ => false,
                });
            if reexports {
                after.extend(range[t].clone());
            }
            for sp in &imp.specifiers {
                let (local, name) = match sp {
                    ImportSpecifier::Named { imported, local } => (local, Some(imported.as_str())),
                    ImportSpecifier::Default { local } => (local, Some("default")),
                    ImportSpecifier::Namespace { local } => (local, None),
                };
                if !out[si].view.ext_names.contains(local) {
                    continue;
                }
                // A name another module re-exports is read where it is
                // declared (§18.14): ordered after, and depending on, the
                // declaring unit, not the re-exporting module (whose units
                // are ordered after all of their sources, a cycle when the
                // declaring module imports the re-exporter too).
                let (d, name) = match name.and_then(|n| ex.resolve(sorted[t].0, n)) {
                    Some(Resolved::Binding(p, n)) => (index[p], Some(n)),
                    Some(Resolved::Namespace(p)) => (index[p], None),
                    _ => (t, name),
                };
                if d != t {
                    deps.extend(range[d].clone());
                }
                // A namespace's keys are read where they are declared: its
                // unit is ordered after those modules too.
                if name.is_none() {
                    for &r in ns_reach.entry(d).or_insert_with(|| ex.reach(d)).iter() {
                        if r != out[si].mi {
                            deps.extend(range[r].clone());
                            after.extend(range[r].clone());
                        }
                    }
                }
                match name.and_then(|n| owner_of(&out, &range[d], n)) {
                    Some(w) => after.push(w),
                    None => after.extend(range[d].clone()),
                }
            }
        }
        // A re-export whose source the module does not import (resolved
        // by the driver): the first unit is ordered after all of the source
        // and depends on it, as for an imported one.
        if out[si].key.ends_with("#u0") {
            for e in &module.exports {
                let (Export::ReExport { source, .. }
                | Export::ExportAll { source }
                | Export::NamespaceReExport { source, .. }) = e
                else {
                    continue;
                };
                if module.imports.iter().any(|i| &i.source == source) {
                    continue;
                }
                let Some(&t) =
                    reexport_source(&cfg.export_sources, path, std::iter::empty(), source)
                        .as_deref()
                        .filter(|t| *t != path)
                        .and_then(|t| index.get(t))
                else {
                    continue;
                };
                out[si].deps.extend(range[t].clone());
                after.extend(range[t].clone());
            }
        }
        // Each once, in first-named order (the order's DFS follows it).
        let mut once: FxHashSet<usize> = FxHashSet::default();
        after.retain(|&w| once.insert(w));
        out[si].after.extend(after);
        deps.sort_unstable();
        deps.dedup();
        out[si].deps.extend(deps);
    }
    out
}

/// Solves in dependency order (a solve after the ones it depends on), so a
/// contract is computed before a unit consumes it. In an import cycle the
/// back edge has no contract yet: that import is ⊤, as in P1.
fn dependency_order(solves: &[Solve]) -> Vec<usize> {
    let mut state = vec![0u8; solves.len()];
    let mut out = Vec::with_capacity(solves.len());
    for root in 0..solves.len() {
        if state[root] != 0 {
            continue;
        }
        // Iterative DFS, post-order.
        let mut stack: Vec<(usize, usize)> = vec![(root, 0)];
        state[root] = 1;
        while let Some(&mut (v, ref mut next)) = stack.last_mut() {
            if *next < solves[v].after.len() {
                let w = solves[v].after[*next];
                *next += 1;
                if state[w] == 0 {
                    state[w] = 1;
                    stack.push((w, 0));
                }
            } else {
                state[v] = 2;
                out.push(v);
                stack.pop();
            }
        }
    }
    out
}

/// The per-unit driver (decision 151, design §7, §11 and §12): one solve
/// per declaration unit in dependency order, each publishing contracts to
/// the units after it; then the census. A class some unit used beyond its
/// contract is broken: it is published as an escape, and the units that see
/// it again (its defining unit and every unit depending on that one,
/// transitively: contracts travel only along those edges) are solved again;
/// every other unit's inputs are unchanged, so its result is kept. What
/// the other units feed back only grows: broken, the argument contracts
/// (decision 13) and the store effects (decision 17), each last round's
/// value joined with what the latest solves report (D16): the entries
/// (templates, functions, keys, ⊤) and their descriptions are joins in
/// finite lattices (`ArgContract::join`), so every such input ascends and
/// the loop ends, and the result covers what every round's solves passed
/// and stored.

pub(crate) fn run(sorted: &[(&str, &Module)], cfg: &FactsConfig) -> ProgramFacts {
    let ucfg = FactsConfig {
        unit: true,
        ..cfg.clone()
    };
    let flats: Vec<Option<Module>> = sorted
        .iter()
        .map(|(_, m)| crate::decl_units::run_once_flat(m))
        .collect();
    let exports = module_exports(sorted, &cfg.export_sources);
    let solves = solves(sorted, &flats, cfg, &exports);
    let viewed: Vec<(&str, Vec<&Import>, &[Export])> = solves
        .iter()
        .map(|s| {
            (
                sorted[s.mi].0,
                s.module
                    .imports
                    .iter()
                    .chain(s.view.imports.iter())
                    .collect(),
                s.view.exports.as_slice(),
            )
        })
        .collect();
    let mut input = published_surface(&viewed, cfg, &exports);
    input.exports = exports;
    // The classes another unit's class extends, through the import its
    // `extends` names (DECISIONS 57).
    for s in &solves {
        for &k in &s.view.classes {
            let Some(e) = s
                .module
                .classes
                .get(k)
                .and_then(|c| c.extends_name.as_ref())
            else {
                continue;
            };
            for imp in s.module.imports.iter().chain(s.view.imports.iter()) {
                let Some(t) = imp.resolved_path.as_deref() else {
                    continue;
                };
                for sp in &imp.specifiers {
                    let name = match sp {
                        ImportSpecifier::Named { imported, local } if local == e => {
                            imported.clone()
                        }
                        ImportSpecifier::Default { local } if local == e => "default".into(),
                        _ => continue,
                    };
                    input
                        .extended
                        .insert((crate::decl_units::module_of(t).to_string(), name));
                }
            }
        }
    }
    let order = dependency_order(&solves);
    let levels = dependency_levels(&solves, &order);
    let mut rounds = 0u32;
    let compares = std::cell::Cell::new(0u64);
    let mut round_work: Vec<u64> = Vec::new();
    let mut per_unit: Vec<Option<ProgramFacts>> = (0..solves.len()).map(|_| None).collect();
    let mut dirty = vec![true; solves.len()];
    // DECISIONS 170 (81 (b)): a unit reads its dependencies' class,
    // function and value contracts as they stand when its level solves:
    // those of an earlier level (the others are not published yet in the
    // round, `input.classes` starts empty). `exports`: each unit's, as a
    // number per latest solve; `exports_seen`: what a unit saw of them when
    // it was solved. An importer is solved again when that changed, not
    // because a dependency was solved: equal inputs repeat the same solve.
    let mut level_of = vec![0usize; solves.len()];
    for (l, level) in levels.iter().enumerate() {
        for &si in level {
            level_of[si] = l;
        }
    }
    let visible = |si: usize, exports: &[u64]| -> Vec<u64> {
        solves[si]
            .deps
            .iter()
            .filter(|&&d| level_of[d] < level_of[si])
            .map(|&d| exports[d])
            .collect()
    };
    let mut exports: Vec<u64> = vec![0; solves.len()];
    let mut exports_seen: Vec<Vec<u64>> = vec![Vec::new(); solves.len()];
    // The layouts whose entry is their owner's, not a forwarded copy
    // (DECISIONS 106).
    let mut owned_layouts: FxHashSet<String> = FxHashSet::default();
    // The function contracts each unit published at its latest solve
    // (DECISIONS 134).
    let mut fns_of: Vec<Vec<String>> = vec![Vec::new(); solves.len()];
    // The layouts each unit published as their owner at its latest solve.
    let mut layouts_of: Vec<Vec<String>> = vec![Vec::new(); solves.len()];
    // The unit whose forwarded copy an entry is (DECISIONS 134).
    let mut copy_of: FxHashMap<String, usize> = FxHashMap::default();
    // The shared objects' writes each round replayed (`Published::shared`),
    // and the one each unit's latest solve saw.
    let mut shared_rounds: Vec<std::collections::BTreeSet<crate::model::SharedWrite>> =
        vec![Default::default()];
    let mut shared_seen: Vec<usize> = vec![0; solves.len()];
    loop {
        rounds += 1;
        input.classes.clear();
        input.fns.clear();
        input.values.clear();
        // Layouts stay from the last round (DECISIONS 103): a contract a
        // cell carries names its layouts by display, and their publisher
        // may solve after the reader: each reader recorded what it read.
        let mut published_layouts: FxHashSet<String> = FxHashSet::default();
        let mut owned_now: FxHashSet<String> = FxHashSet::default();
        let mut published_fns: FxHashSet<String> = FxHashSet::default();
        let mut reports: FxHashSet<String> = FxHashSet::default();
        let mut work = 0u64;
        for level in &levels {
            // A unit over the bound keeps its result (`dependents`).
            for &si in level {
                if !dirty[si]
                    && per_unit[si].as_ref().is_some_and(|u| !u.stats.aborted)
                    && visible(si, &exports) != exports_seen[si]
                {
                    dirty[si] = true;
                }
            }
            let todo: Vec<usize> = level.iter().copied().filter(|&si| dirty[si]).collect();
            for &si in &todo {
                exports_seen[si] = visible(si, &exports);
            }
            let mut level_max = (0usize, 0u64, 0usize);
            for (si, mut u) in solve_level(&todo, &solves, sorted, &ucfg, &input) {
                work += u.stats.work;
                level_max.0 = level_max.0.max(u.stats.nodes);
                level_max.1 = level_max.1.max(u.stats.pts_entries);
                level_max.2 = level_max.2.max(u.sites.len());
                exports[si] = digest(&(&u.contracts_out, &u.fn_contracts_out, &u.values_out));
                // DECISIONS 197: a unit's write into a cell is its last
                // write joined with what its latest solve writes, as its
                // argument contracts are (D16): a cell only grows, and a
                // value one writer took from another's earlier write cannot
                // pass back and forth between them after its source stopped
                // writing it (tsc typescript.js#u4336/#u4339, period 2).
                if let Some(old) = per_unit[si].take() {
                    let at: FxHashMap<String, usize> = u
                        .cells_out
                        .iter()
                        .enumerate()
                        .map(|(i, (k, _))| (k.clone(), i))
                        .collect();
                    for (k, c) in old.cells_out {
                        match at.get(&k) {
                            Some(&i) => u.cells_out[i].1.join(&c),
                            None => u.cells_out.push((k, c)),
                        }
                    }
                }
                per_unit[si] = Some(u);
                shared_seen[si] = shared_rounds.len() - 1;
            }
            if cfg.report && !todo.is_empty() {
                eprintln!(
                    "perry: wpfacts census r{rounds} level {}/{} solved {} max nodes {} pts {} sites {} {}",
                    level_of[todo[0]],
                    levels.len(),
                    todo.len(),
                    level_max.0,
                    level_max.1,
                    level_max.2,
                    census(&per_unit, &input, &shared_rounds)
                );
            }
            // The level's contracts, published in `order` sequence: the
            // next level's one input.
            for &si in level {
                let s = &solves[si];
                let u = per_unit[si].as_ref().expect("every unit solved");
                // Published under the unit's path (the module's other units
                // import it) and, when it is an export of the module, under
                // the module's (other modules import it). A name exported to
                // the module's units only is no export of the module: under
                // the module's path it would stand for the module's binding
                // of that name, another unit's (`var _C = class _C {..};
                // export { _C }`: the module exports the binding, the class
                // `_C` goes to the binding's unit only).
                let module_export = |n: &String| !s.view.unit_exports.contains(n);
                for ((p, n), c) in &u.contracts_out {
                    input.classes.insert((s.key.clone(), n.clone()), c.clone());
                    if module_export(n) {
                        input.classes.insert((p.clone(), n.clone()), c.clone());
                    }
                }
                for ((p, n), c) in &u.fn_contracts_out {
                    input.fns.insert((s.key.clone(), n.clone()), c.clone());
                    if module_export(n) {
                        input.fns.insert((p.clone(), n.clone()), c.clone());
                    }
                }
                for ((p, n), c) in &u.values_out {
                    input.values.insert((s.key.clone(), n.clone()), c.clone());
                    if module_export(n) {
                        input.values.insert((p.clone(), n.clone()), c.clone());
                    }
                }
                // DECISIONS 134: a layout this unit owned at its last solve
                // and no longer publishes is gone now, unless another unit
                // births it too (DECISIONS 112).
                let now: Vec<String> = u
                    .layouts_out
                    .iter()
                    .filter(|c| !u.layouts_fwd.contains(&c.display))
                    .map(|c| c.display.clone())
                    .collect();
                for d in std::mem::replace(&mut layouts_of[si], now) {
                    if !layouts_of.iter().any(|v| v.contains(&d)) {
                        input.layouts.remove(&d);
                        owned_layouts.remove(&d);
                        copy_of.remove(&d);
                    }
                }
                for c in &u.layouts_out {
                    published_layouts.insert(c.display.clone());
                    if u.layouts_fwd.contains(&c.display) {
                        if !owned_layouts.contains(&c.display) {
                            input
                                .layouts
                                .insert(c.display.clone(), Digested::new(c.clone()));
                            copy_of.insert(c.display.clone(), si);
                        }
                        continue;
                    }
                    owned_now.insert(c.display.clone());
                    copy_of.remove(&c.display);
                    // Several units birth one literal shape (DECISIONS 112):
                    // its entry is the join of their contracts, ascending
                    // from the last round's owned entry, so a reader between
                    // two owners reads at the fixpoint what the last one
                    // leaves. A class has one defining unit: its latest solve.
                    let e = match input.layouts.get(&c.display).map(|e| &e.c) {
                        Some(p)
                            if c.literal_shape
                                && p.literal_shape
                                && owned_layouts.contains(&c.display) =>
                        {
                            let mut j = p.clone();
                            j.join(c);
                            j
                        }
                        _ => c.clone(),
                    };
                    owned_layouts.insert(c.display.clone());
                    input.layouts.insert(c.display.clone(), Digested::new(e));
                }
                // DECISIONS 134: a contract this unit published at its last
                // solve and no longer publishes is gone now, not at the end
                // of the round: a reader solving later in this round reads
                // what this solve left, and needs no round to see it go.
                let now: Vec<String> = u.own_fns_out.iter().map(|c| c.name.clone()).collect();
                for nm in std::mem::replace(&mut fns_of[si], now) {
                    if !u.own_fns_out.iter().any(|c| c.name == nm) {
                        input.own_fns.remove(&nm);
                    }
                }
                for c in &u.own_fns_out {
                    published_fns.insert(c.name.clone());
                    input
                        .own_fns
                        .insert(c.name.clone(), Digested::new(c.clone()));
                }
                reports.extend(u.reports.iter().cloned());
            }
        }
        round_work.push(work);
        // Decision 13: the argument contracts of closed functions: last
        // round's joined with what every unit's latest solve passes (D16).
        // Rebuilt from the latest solves alone, a contract can shrink: a
        // closed function whose argument is ⊤ makes a caller pass less,
        // the contract drops, and the cycle repeats (express, ee-first
        // `callback`, period 3). Joined, it only grows. A closed function
        // whose contract grew is solved again, with the units depending
        // on it; at the fixpoint every closed function's parameters hold
        // what its calls pass.
        let mut latest: FxHashMap<(String, String), ArgContract> = FxHashMap::default();
        let mut aborted = false;
        for (si, s) in solves.iter().enumerate() {
            let path = sorted[s.mi].0;
            let u = per_unit[si].as_ref().expect("every unit solved");
            if u.stats.aborted {
                aborted = true;
                for nm in &s.view.closed_refs {
                    latest
                        .entry((path.to_string(), nm.clone()))
                        .or_default()
                        .top = true;
                }
                continue;
            }
            for (nm, ac) in &u.args_out {
                latest
                    .entry((path.to_string(), nm.clone()))
                    .or_default()
                    .join(ac);
            }
            // A closed class's constructions, from any module (§18.13).
            for (k, ac) in &u.ctor_args_out {
                latest.entry(k.clone()).or_default().join(ac);
            }
        }
        // DECISIONS 157: an aborted unit may hold any published class (it
        // need not name it: a namespace, a value), and its constructions
        // publish nothing.
        if aborted {
            for s in &solves {
                let path = sorted[s.mi].0;
                for nm in &s.view.published {
                    latest
                        .entry((path.to_string(), nm.clone()))
                        .or_default()
                        .top = true;
                }
            }
        }
        let mut args = input.args.clone();
        for (k, ac) in &latest {
            args.entry(k.clone()).or_default().join(ac);
        }
        let mut seeds: Vec<usize> = Vec::new();
        for (si, s) in solves.iter().enumerate() {
            let path = sorted[s.mi].0;
            let changed = s.view.closed.keys().any(|nm| {
                let k = (path.to_string(), nm.clone());
                args.get(&k) != input.args.get(&k)
            });
            if changed {
                seeds.push(si);
            }
        }
        input.args = args;
        // DECISIONS 107: the receivers of published methods: last round's
        // joined with every unit's latest solve (D16), so they only grow.
        // The owners of a method whose join changed are solved again.
        let mut this_args = input.this_args.clone();
        let mut this_top = input.this_args_top;
        for u in per_unit.iter().flatten() {
            if u.stats.aborted {
                this_top = true;
                continue;
            }
            for (nm, vc) in &u.this_args_out {
                this_args.entry(nm.clone()).or_default().join(vc);
            }
        }
        let mut keys: Vec<&String> = this_args.keys().chain(input.this_args.keys()).collect();
        keys.sort_unstable();
        keys.dedup();
        let recv_changed: Vec<String> = keys
            .into_iter()
            .filter(|k| {
                this_top != input.this_args_top
                    || digest(&this_args.get(*k)) != digest(&input.this_args.get(*k))
            })
            .map(|k| format!("fn:{k}"))
            .collect();
        if !recv_changed.is_empty() {
            for (si, u) in per_unit.iter().enumerate() {
                let u = u.as_ref().expect("every unit solved");
                if !u.stats.aborted && u.fns_defined.iter().any(|d| recv_changed.contains(d)) {
                    seeds.push(si);
                }
            }
        }
        input.this_args = this_args;
        input.this_args_top = this_top;
        // DECISIONS 165: the published methods some unit runs, joined the
        // same way; the owners of a newly run one solve again.
        let mut runs_changed: Vec<String> = Vec::new();
        for u in per_unit.iter().flatten() {
            for nm in &u.method_runs_out {
                if input.method_runs.insert(nm.clone()) {
                    runs_changed.push(format!("fn:{nm}"));
                }
            }
        }
        if !runs_changed.is_empty() {
            for (si, u) in per_unit.iter().enumerate() {
                let u = u.as_ref().expect("every unit solved");
                if !u.stats.aborted && u.fns_defined.iter().any(|d| runs_changed.contains(d)) {
                    seeds.push(si);
                }
            }
        }
        // DECISIONS 148: what the running units pass to published methods,
        // last round's joined with every unit's latest solve and compared
        // as values; the owners of a method whose join changed solve again.
        let mut margs = input.method_args.clone();
        for u in per_unit.iter().flatten() {
            for (nm, ac) in &u.method_args_out {
                margs.entry(nm.clone()).or_default().join(ac);
            }
        }
        let margs_changed: Vec<String> = margs
            .iter()
            .filter(|(k, v)| input.method_args.get(*k) != Some(*v))
            .map(|(k, _)| format!("fn:{k}"))
            .collect();
        if !margs_changed.is_empty() {
            for (si, u) in per_unit.iter().enumerate() {
                let u = u.as_ref().expect("every unit solved");
                if !u.stats.aborted && u.fns_defined.iter().any(|d| margs_changed.contains(d)) {
                    seeds.push(si);
                }
            }
        }
        input.method_args = margs;
        // Decision 17: the store effects: last round's joined with what
        // every unit's latest solve stores (D16), so they only grow too.
        // The owners of a layout whose effect keys grew perform them,
        // solved again with their dependents.
        let mut stores = input.stores.clone();
        for u in per_unit.iter().flatten() {
            for (d, k, c) in &u.stores_out {
                let v = stores.entry(d.clone()).or_default();
                match v.iter_mut().find(|(k2, _)| k2 == k) {
                    Some((_, e)) => e.join(c),
                    None => v.push((k.clone(), c.clone())),
                }
            }
        }
        for v in stores.values_mut() {
            v.sort_by(|x, y| x.0.cmp(&y.0));
        }
        // Compared as values: a layout whose keys differ either way (joined,
        // they can only have grown).
        let mut changed: Vec<String> = stores
            .keys()
            .chain(input.stores.keys())
            .filter(|d| stores.get(*d) != input.stores.get(*d))
            .cloned()
            .collect();
        changed.sort();
        changed.dedup();
        if !changed.is_empty() {
            let owner = owners(&per_unit);
            for d in &changed {
                match owner.get(d) {
                    Some(sis) => seeds.extend(sis.iter().copied()),
                    None => seeds.extend(0..solves.len()),
                }
            }
        }
        input.stores = stores;
        // Design §17.16 (b): the cells, from every unit's writes so far
        // (DECISIONS 197). The units reading a cell whose writes changed are
        // solved again.
        let mut cells: FxHashMap<String, Vec<CellWrite>> = FxHashMap::default();
        for (si, s) in solves.iter().enumerate() {
            let u = per_unit[si].as_ref().expect("every unit solved");
            for (k, c) in &u.cells_out {
                let v = cells.entry(k.clone()).or_default();
                let d = digest(c);
                let i = match v.iter().position(|w| w.digest == d && w.c == *c) {
                    Some(i) => i,
                    None => {
                        v.push(CellWrite::new(c.clone(), d));
                        v.len() - 1
                    }
                };
                v[i].writers.push(s.key.clone());
            }
        }
        for v in cells.values_mut() {
            for w in v.iter_mut() {
                w.writers.sort_unstable();
            }
            v.sort_by(|x, y| x.writers.cmp(&y.writers));
        }
        let cells_top = per_unit.iter().flatten().any(|u| u.stats.aborted);
        // A unit is solved again when what it READS of a cell changed: the
        // other writers' contracts (its own write is in its own solve,
        // DECISIONS 89), an unknown one or a unit over the bound being ⊤
        // that absorbs every other unknown. A reader forwarding the ⊤ it
        // was seeded with changes no other reader's view.
        // DECISIONS 169: the view is the writes' digests (taken once per
        // round, `CellWrite`), not their contracts cloned and sorted per
        // reader: AWFY rounds 2-4 viewed 180K cell reads, 18.8 s of 26.6.
        let view = |cells: &FxHashMap<String, Vec<CellWrite>>, top: bool, me: &str, k: &str| {
            let others: Vec<&CellWrite> = cells
                .get(k)
                .into_iter()
                .flatten()
                .filter(|w| !(w.writers.len() == 1 && w.writers[0] == me))
                .collect();
            let pure = |c: &ValueContract| {
                c.unknown
                    && c.layouts.is_empty()
                    && c.classes.is_empty()
                    && c.fns.is_empty()
                    && c.builtins.is_empty()
                    && c.arrays.is_empty()
                    && c.boxes.is_empty()
            };
            let top = top || others.iter().any(|w| pure(&w.c));
            let mut v: Vec<u64> = others
                .into_iter()
                .filter(|w| !pure(&w.c))
                .map(|w| if top { w.top } else { w.digest })
                .collect();
            v.sort_unstable();
            v.dedup();
            (top, v)
        };
        // A layout no unit published this round is gone; a unit that read
        // a layout which changed during the round read it before its
        // publisher solved, or the old one: solved again (DECISIONS 103).
        input.layouts.retain(|k, _| published_layouts.contains(k));
        copy_of.retain(|k, _| input.layouts.contains_key(k));
        owned_layouts.retain(|k| owned_now.contains(k));
        input.own_fns.retain(|k, _| published_fns.contains(k));
        // DECISIONS 114: each contract is digested once per round, as its
        // readers recorded it (`Gen::owner_fn`, `Gen::other_layout`): a
        // read compares two numbers, so the round does not grow with the
        // reads (awfy_Richards: 58544 reads of 39 layouts, each digest the
        // Debug text of a whole contract).
        let gone = digest(&None::<&ClassContract>);
        let fns_now: FxHashMap<&str, u64> = input
            .own_fns
            .iter()
            .map(|(k, e)| (k.as_str(), e.digest))
            .collect();
        let layouts_now: FxHashMap<&str, u64> = input
            .layouts
            .iter()
            .map(|(k, e)| (k.as_str(), e.digest))
            .collect();
        compares.set(compares.get() + (fns_now.len() + layouts_now.len()) as u64);
        let fn_changed =
            |(k, h): &(String, u64)| fns_now.get(k.as_str()).copied().unwrap_or(gone) != *h;
        for (si, u) in per_unit.iter().enumerate() {
            let u = u.as_ref().expect("every unit solved");
            // A unit over the bound keeps its result (`dependents`): what it
            // read changes nothing it publishes.
            if !u.stats.aborted && u.fns_read.iter().any(fn_changed) {
                seeds.push(si);
            }
        }
        let layout_changed =
            |(k, h): &(String, u64)| layouts_now.get(k.as_str()).copied().unwrap_or(gone) != *h;
        for (si, u) in per_unit.iter().enumerate() {
            let u = u.as_ref().expect("every unit solved");
            // A unit over the bound keeps its result (`dependents`): what it
            // read changes nothing it publishes.
            // DECISIONS 134: a unit's own forwarded copy is built from
            // what it read through the other channels, each tracked: it
            // does not solve its publisher again.
            let own_copy = |(k, _): &&(String, u64)| copy_of.get(k.as_str()) == Some(&si);
            if !u.stats.aborted
                && u.layouts_read
                    .iter()
                    .filter(|r| !own_copy(r))
                    .any(layout_changed)
            {
                seeds.push(si);
            }
        }
        // A cell whose writes and whose ⊤ are as last round reads alike in
        // every unit: only the changed ones are viewed per reader
        // (DECISIONS 114).
        let mut keys: Vec<&String> = cells.keys().chain(input.cells.keys()).collect();
        keys.sort_unstable();
        keys.dedup();
        compares.set(compares.get() + keys.len() as u64);
        let changed: FxHashSet<&str> = keys
            .into_iter()
            .filter(|k| input.cells.get(*k) != cells.get(*k))
            .map(|k| k.as_str())
            .collect();
        let top_changed = input.cells_top != cells_top;
        for (si, u) in per_unit.iter().enumerate() {
            let u = u.as_ref().expect("every unit solved");
            let me = solves[si].key.as_str();
            if u.cells_read.iter().any(|k| {
                (top_changed || changed.contains(k.as_str())) && {
                    compares.set(compares.get() + 1);
                    view(&input.cells, input.cells_top, me, k) != view(&cells, cells_top, me, k)
                }
            }) {
                seeds.push(si);
            }
        }
        drop(changed);
        input.cells = cells;
        input.cells_top = cells_top;
        // The write effects: last round's joined with what every unit's
        // latest solve stores into other units' bindings (an aborted unit:
        // any value into each it writes), so they only grow (D16, as the
        // argument contracts). The unit owning a binding whose writes grew
        // holds them, solved again with its dependents.
        let mut writes = input.writes.clone();
        for u in per_unit.iter().flatten() {
            for (k, vc) in &u.writes_out {
                writes.entry(k.clone()).or_default().join(vc);
            }
        }
        let mut grown: Vec<&String> = writes
            .iter()
            .filter(|(k, v)| input.writes.get(*k) != Some(*v))
            .map(|((p, _), _)| p)
            .collect();
        grown.sort();
        grown.dedup();
        for p in grown {
            seeds.extend(solves.iter().position(|s| &s.key == p));
        }
        input.writes = writes;
        // The shared objects' writes (`model::SharedWrite`): last round's
        // joined with every unit's latest, so they only grow (D16). When
        // they grew, the units that can see a new one are solved again.
        let mut shared = input.shared.clone();
        for u in per_unit.iter().flatten() {
            shared.extend(u.shared_out.iter().cloned());
        }
        shared_closure(&mut shared);
        if shared != input.shared {
            // A unit solved again only for a write it has not taken (one it
            // replayed, or one it performs itself with the replay's effect)
            // and can see (`model::SharedReads`).
            for (si, u) in per_unit.iter().enumerate() {
                let seen = &shared_rounds[shared_seen[si]];
                let u = u.as_ref().expect("every unit solved");
                if shared.iter().any(|w| {
                    !seen.contains(w)
                        && !(replay_is_own(w) && u.shared_out.contains(w))
                        && !u.stats.aborted
                        && u.shared_reads.sees(w)
                }) {
                    seeds.push(si);
                }
            }
            shared_rounds.push(shared.clone());
        }
        input.shared = shared;
        if cfg.report {
            eprintln!(
                "perry: wpfacts census r{rounds} end seeds {} {}",
                seeds.len(),
                census(&per_unit, &input, &shared_rounds)
            );
        }
        let fresh: Vec<String> = reports
            .into_iter()
            .filter(|d| !input.broken.contains(d))
            .collect();
        if !fresh.is_empty() || !seeds.is_empty() {
            dirty = if fresh.is_empty() {
                vec![false; solves.len()]
            } else {
                resolve_again(&solves, &per_unit, &fresh)
            };
            // DECISIONS 170: a seeded unit is solved again; its importers
            // follow when its outputs change (the level loop above). A
            // newly broken class still re-solves its owner's importers
            // (`resolve_again`): they read the broken set itself.
            for si in seeds {
                if per_unit[si].as_ref().is_some_and(|u| !u.stats.aborted) {
                    dirty[si] = true;
                }
            }
            input.broken.extend(fresh);
            continue;
        }
        let mut pf = ProgramFacts {
            module_paths: sorted.iter().map(|(p, _)| p.to_string()).collect(),
            dynamic_code_reach: cfg.dynamic_code_reach.clone(),
            sabotage: cfg.sabotage.clone(),
            bindings: vec![Default::default(); sorted.len()],
            ..Default::default()
        };
        if cfg.unit_io {
            pf.unit_io = unit_io(sorted, &solves, &order, &per_unit, &input);
        }
        // The cells' writes at the fixpoint, each once per unit writing it
        // (each unit holds its own), for the tests to read their size.
        #[cfg(test)]
        for (k, ws) in &input.cells {
            for w in ws {
                pf.cells_out
                    .extend(w.writers.iter().map(|_| (k.clone(), w.c.clone())));
            }
        }
        for (si, u) in per_unit.into_iter().enumerate() {
            let u = u.expect("every unit solved");
            // Its layouts, for the tests to read the contracts' form
            // (DECISIONS 208).
            #[cfg(test)]
            pf.layouts_out.extend(u.layouts_out.iter().cloned());
            merge_unit(&mut pf, solves[si].mi, &solves[si].key, &solves[si].view, u);
        }
        for (path, b) in pf.module_paths.iter().zip(&mut pf.bindings) {
            unique_bindings(&mut b.locals, path, "LocalId");
            unique_bindings(&mut b.this_of, path, "this-of FuncId");
        }
        pf.stats.modules = sorted.len();
        typed_param_verdicts(&mut pf);
        pf.rounds = rounds;
        // The cost of the run is every round's.
        pf.stats.work = round_work.iter().sum();
        pf.round_work = round_work;
        pf.compares = compares.get();
        pf.broken = {
            let mut b: Vec<String> = input.broken.iter().cloned().collect();
            b.sort();
            b
        };
        // The program's writes of the shared objects: every unit's, joined.
        pf.shared_out = input.shared.iter().cloned().collect();
        return pf;
    }
}

/// The driver's held state for the census line (`FactsConfig::report`):
/// what the latest solves keep and what the round publishes, with the RSS.
fn census(
    per_unit: &[Option<ProgramFacts>],
    input: &Published,
    shared_rounds: &[std::collections::BTreeSet<crate::model::SharedWrite>],
) -> String {
    let us = per_unit.iter().flatten();
    let sum = |f: &dyn Fn(&ProgramFacts) -> usize| us.clone().map(f).sum::<usize>();
    let status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    let mib = |k: &str| {
        status
            .lines()
            .find(|l| l.starts_with(k))
            .and_then(|l| l.split_whitespace().nth(1))
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(0)
            >> 10
    };
    let handed = input
        .shared
        .iter()
        .filter(|w| matches!(w, crate::model::SharedWrite::Handed(_)))
        .count();
    let reads = |u: &ProgramFacts| {
        u.shared_reads.keys.len()
            + u.shared_reads
                .objects
                .values()
                .map(|k| k.as_ref().map_or(1, |k| k.len()))
                .sum::<usize>()
    };
    format!(
        "| rss {} hwm {} MiB | units {} sites {} tmpls {} toks {} shared_out {} cells_out {} cells_read {} layouts_out {} layouts_read {} fns_read {} contracts_out {} values_out {} shared_reads {} | in layouts {} classes {} fns {} values {} own_fns {} cells {} shared {} handed {} writes {} stores {} args {} | shared_rounds {} entries {}",
        mib("VmRSS:"),
        mib("VmHWM:"),
        us.clone().count(),
        sum(&|u| u.sites.len()),
        sum(&|u| u.templates.len()),
        sum(&|u| u.tok_names.len()),
        sum(&|u| u.shared_out.len()),
        sum(&|u| u.cells_out.len()),
        sum(&|u| u.cells_read.len()),
        sum(&|u| u.layouts_out.len()),
        sum(&|u| u.layouts_read.len()),
        sum(&|u| u.fns_read.len()),
        sum(&|u| u.contracts_out.len()),
        sum(&|u| u.values_out.len()),
        sum(&reads),
        input.layouts.len(),
        input.classes.len(),
        input.fns.len(),
        input.values.len(),
        input.own_fns.len(),
        input.cells.len(),
        input.shared.len(),
        handed,
        input.writes.len(),
        input.stores.values().map(|v| v.len()).sum::<usize>(),
        input.args.len(),
        shared_rounds.len(),
        shared_rounds.iter().map(|s| s.len()).sum::<usize>(),
    )
}

/// Does replaying `w` (`Model::replay_shared`) do exactly what the unit
/// performing `w` itself already did? A store or accessor on ⊤, an
/// unmodeled reach and a handover run the same rules either way; a store
/// or accessor on one object puts the other unit's unknown value there.
fn replay_is_own(w: &crate::model::SharedWrite) -> bool {
    use crate::model::{Shared, SharedWrite};
    match w {
        SharedWrite::Store(s, _) | SharedWrite::Accessor(s, _) => *s == Shared::Top,
        SharedWrite::Unmodeled(_) | SharedWrite::Handed(_) => true,
    }
}

/// A runtime-owned object one unit hands to unknown code (`SharedWrite::Handed`)
/// is ⊤ in the other units' code: each store and accessor any unit performs on
/// ⊤ is one on that object, as the whole-program solve sees it on the
/// object's own token. A store under a computed key covers every key of its
/// object: the stores under one key beside it are dropped, so a write that
/// says nothing new solves no unit again.
fn shared_closure(shared: &mut std::collections::BTreeSet<crate::model::SharedWrite>) {
    use crate::model::{Shared, SharedWrite};
    let handed: Vec<Shared> = shared
        .iter()
        .filter_map(|w| match w {
            SharedWrite::Handed(s) => Some(s.clone()),
            _ => None,
        })
        .collect();
    let on_top: Vec<SharedWrite> = shared
        .iter()
        .filter(|w| {
            matches!(
                w,
                SharedWrite::Store(Shared::Top, _) | SharedWrite::Accessor(Shared::Top, _)
            )
        })
        .cloned()
        .collect();
    for s in &handed {
        for w in &on_top {
            shared.insert(match w {
                SharedWrite::Store(_, k) => SharedWrite::Store(s.clone(), k.clone()),
                SharedWrite::Accessor(_, k) => SharedWrite::Accessor(s.clone(), k.clone()),
                _ => unreachable!("only stores and accessors on ⊤"),
            });
        }
    }
    let computed: Vec<Shared> = shared
        .iter()
        .filter_map(|w| match w {
            SharedWrite::Store(s, None) => Some(s.clone()),
            _ => None,
        })
        .collect();
    shared.retain(|w| !matches!(w, SharedWrite::Store(s, Some(_)) if computed.contains(s)));
}

/// Decision 32: the units by dependency level, each level in `order`
/// sequence. A unit is one level above the highest unit it is ordered
/// after; a back edge of a cycle (to a unit `order` puts later) is ⊤ for
/// it, as in the walk, and does not count. A unit reads other units only
/// through its `after` names (decision 21), all at lower levels, so a
/// level's units are independent of each other.
fn dependency_levels(solves: &[Solve], order: &[usize]) -> Vec<Vec<usize>> {
    let mut pos = vec![0usize; solves.len()];
    for (p, &si) in order.iter().enumerate() {
        pos[si] = p;
    }
    let mut level = vec![0usize; solves.len()];
    let mut out: Vec<Vec<usize>> = Vec::new();
    for &si in order {
        let l = solves[si]
            .after
            .iter()
            .filter(|&&w| pos[w] < pos[si])
            .map(|&w| level[w] + 1)
            .max()
            .unwrap_or(0);
        level[si] = l;
        if out.len() <= l {
            out.resize_with(l + 1, Vec::new);
        }
        out[l].push(si);
    }
    out
}

/// Decision 32: one level's dirty units, solved against the one frozen
/// input on up to `jobs` threads. Results go back by unit, so the thread
/// count changes the wall and nothing else.
fn solve_level(
    todo: &[usize],
    solves: &[Solve],
    sorted: &[(&str, &Module)],
    ucfg: &FactsConfig,
    input: &Published,
) -> Vec<(usize, ProgramFacts)> {
    let solve = |si: usize| {
        let s = &solves[si];
        let list = [(sorted[s.mi].0.to_string(), s.module)];
        (
            si,
            crate::compute_in(&list, ucfg, Some(input), Some(&s.view)),
        )
    };
    let jobs = ucfg.jobs.clamp(1, todo.len().max(1));
    if jobs == 1 {
        return todo.iter().map(|&si| solve(si)).collect();
    }
    let next = std::sync::atomic::AtomicUsize::new(0);
    std::thread::scope(|sc| {
        let workers: Vec<_> = (0..jobs)
            .map(|_| {
                // The calling thread's recursion depth, not a spawned
                // thread's default 2 MiB.
                std::thread::Builder::new()
                    .stack_size(256 << 20)
                    .spawn_scoped(sc, || {
                        let mut mine = Vec::new();
                        while let Some(&si) =
                            todo.get(next.fetch_add(1, std::sync::atomic::Ordering::Relaxed))
                        {
                            mine.push(solve(si));
                        }
                        mine
                    })
                    .expect("spawn a unit solver")
            })
            .collect();
        workers
            .into_iter()
            .flat_map(|w| w.join().expect("a unit solve panicked"))
            .collect()
    })
}

/// `FactsConfig::unit_io` (design §13.3): per unit, a digest of every input
/// it reads (the contracts of the units it is ordered after, its closed
/// functions' argument contracts, the store effects on and the census
/// breaks of what it owns or reads) and of its facts, plus one line per
/// published output. Two dumps of an edited program name the units an edit
/// reaches and the contract that carried it.
fn unit_io(
    sorted: &[(&str, &Module)],
    solves: &[Solve],
    order: &[usize],
    per_unit: &[Option<ProgramFacts>],
    input: &Published,
) -> Vec<String> {
    use crate::dump::{fnv, FNV_OFFSET};
    let mut pos = vec![0usize; solves.len()];
    for (p, &si) in order.iter().enumerate() {
        pos[si] = p;
    }
    let mut outs: Vec<Vec<(String, u64)>> = Vec::with_capacity(solves.len());
    let mut owned: Vec<Vec<String>> = Vec::with_capacity(solves.len());
    for u in per_unit {
        let u = u.as_ref().expect("every unit solved");
        let mut o: Vec<(String, u64)> = Vec::new();
        for ((_, n), c) in &u.contracts_out {
            o.push((format!("class:{n}"), digest(c)));
        }
        for ((_, n), c) in &u.fn_contracts_out {
            o.push((format!("fn:{n}"), digest(c)));
        }
        for ((_, n), c) in &u.values_out {
            o.push((format!("value:{n}"), digest(c)));
        }
        for c in &u.layouts_out {
            o.push((format!("layout:{}", c.display), digest(c)));
        }
        for (n, c) in &u.args_out {
            o.push((format!("args:{n}"), digest(c)));
        }
        for ((p, n), c) in &u.ctor_args_out {
            o.push((format!("args:{p}:{n}"), digest(c)));
        }
        for (n, c) in &u.this_args_out {
            o.push((format!("this:{n}"), digest(c)));
        }
        for n in &u.method_runs_out {
            o.push((format!("runs:{n}"), 0));
        }
        for (n, c) in &u.method_args_out {
            o.push((format!("margs:{n}"), digest(c)));
        }
        for (d, k, kinds) in &u.stores_out {
            o.push((format!("store:{d}.{k}"), digest(kinds)));
        }
        for (k, c) in &u.cells_out {
            o.push((format!("cell:{k}"), digest(c)));
        }
        for ((p, n), vc) in &u.writes_out {
            o.push((format!("write:{p}.{n}"), digest(vc)));
        }
        for d in &u.reports {
            o.push((format!("report:{d}"), 0));
        }
        o.sort();
        o.dedup();
        outs.push(o);
        let mut w: Vec<String> = u
            .templates
            .iter()
            .filter(|t| t.module == Some(0))
            .map(|t| t.display.clone())
            .collect();
        w.extend(u.fns_defined.iter().cloned());
        owned.push(w);
    }
    let mut lines = Vec::new();
    for (si, s) in solves.iter().enumerate() {
        let u = per_unit[si].as_ref().expect("every unit solved");
        let path = sorted[s.mi].0;
        let mut after: Vec<usize> = s
            .after
            .iter()
            .copied()
            .filter(|&w| pos[w] < pos[si])
            .collect();
        after.sort();
        after.dedup();
        let mut h = FNV_OFFSET;
        for &w in &after {
            fnv(&mut h, &solves[w].key);
            for (n, x) in &outs[w] {
                fnv(&mut h, &format!("{n}={x:016x}"));
            }
        }
        let mut closed: Vec<&String> = s.view.closed.keys().collect();
        closed.sort();
        let closed_names: Vec<&str> = closed.iter().map(|n| n.as_str()).collect();
        for nm in closed {
            fnv(
                &mut h,
                &format!(
                    "arg {nm} {:?}",
                    input.args.get(&(path.to_string(), nm.clone()))
                ),
            );
        }
        let mut seen: Vec<&String> = owned[si]
            .iter()
            .chain(after.iter().flat_map(|&w| owned[w].iter()))
            .collect();
        seen.sort();
        seen.dedup();
        for d in seen {
            fnv(
                &mut h,
                &format!(
                    "store {d} {:?} broken {}",
                    input.stores.get(d),
                    input.broken.contains(d)
                ),
            );
        }
        let mut f = FNV_OFFSET;
        for t in u.templates.iter().filter(|t| t.module == Some(0)) {
            for l in crate::dump::template_lines(t) {
                fnv(&mut f, &l);
            }
        }
        for st in &u.sites {
            fnv(&mut f, &format!("{st:?}"));
        }
        let st = &u.stats;
        // Its declarations by name (the first few): where an edit lands.
        let mut decls: Vec<String> = s
            .view
            .classes
            .iter()
            .map(|&k| format!("class:{}", s.module.classes[k].name))
            .collect();
        decls.extend(
            s.view
                .funcs
                .iter()
                .map(|&k| s.module.functions[k].name.clone()),
        );
        let more = decls.len().saturating_sub(6);
        decls.truncate(6);
        if more > 0 {
            decls.push(format!("+{more}"));
        }
        let keys = |v: &[usize]| {
            v.iter()
                .map(|&w| solves[w].key.as_str())
                .collect::<Vec<_>>()
                .join(",")
        };
        let mut deps = s.deps.clone();
        deps.sort();
        deps.dedup();
        lines.push(format!(
            "# uio {} work={} ms={:.1} in={h:016x} facts={f:016x} after={} deps={} closed={} decls={}",
            s.key,
            st.work,
            st.gen_ms + st.solve_ms + st.derive_ms,
            keys(&after),
            keys(&deps),
            closed_names.join(","),
            decls.join(",")
        ));
        for (n, x) in &outs[si] {
            lines.push(format!("# uout {} {n} {x:016x}", s.key));
        }
        // The cells it read (DECISIONS 171: the ones its solve consumed).
        let mut reads: Vec<&String> = u.cells_read.iter().collect();
        reads.sort();
        reads.dedup();
        for k in reads {
            lines.push(format!("# uread {} cell:{k}", s.key));
        }
    }
    lines
}

/// The units a newly broken contract changes (design §11.3): the unit
/// defining it (a template it owns, or a function it publishes) and every
/// unit depending on that one, transitively. A unit over the bound claims
/// and publishes nothing whatever its inputs, so it keeps its result and
/// passes no change on: solving it again would only pay the bound again. A
/// display no unit defines makes every unit dirty.
fn resolve_again(
    solves: &[Solve],
    per_unit: &[Option<ProgramFacts>],
    fresh: &[String],
) -> Vec<bool> {
    let n = solves.len();
    let owner = owners(per_unit);
    let mut work: Vec<usize> = Vec::new();
    for d in fresh {
        // An override report changes the overridden class's unit
        // (DECISIONS 59).
        // So does a typed-array field report (DECISIONS 126).
        let d = d
            .strip_prefix("override ")
            .or_else(|| d.strip_prefix("ta "))
            .and_then(|r| r.split_once(" of "))
            .map_or(d.as_str(), |(_, c)| c);
        match owner.get(d) {
            Some(sis) => work.extend(sis.iter().copied()),
            // No unit owns it: every unit again, but a unit over the bound
            // keeps its result, as in `dependents` (it claims nothing
            // whatever its inputs; tsc: the 200M-work unit was solved again
            // for one unowned display).
            None => {
                return (0..n)
                    .map(|si| !per_unit[si].as_ref().is_some_and(|u| u.stats.aborted))
                    .collect()
            }
        }
    }
    dependents(solves, per_unit, work)
}

/// A published contract and its digest (DECISIONS 114), taken once when
/// it is published: its readers record the number (`Published::layout_seen`)
/// and the round compares numbers, not contracts.
#[derive(Clone, Debug)]
pub(crate) struct Digested<T> {
    pub c: T,
    pub digest: u64,
}

impl<T: std::hash::Hash> Digested<T> {
    pub(crate) fn new(c: T) -> Self {
        let digest = digest(&Some(&c));
        Digested { c, digest }
    }
}

/// One distinct write of a cell (design §17.16 (b)): its contract and the
/// units writing it (their keys, sorted). DECISIONS 169: its digests are
/// taken once, when the round groups the writes (`digest`; `top`: with
/// the unknown value and the primitives dropped, as a reader whose view is
/// ⊤ sees it), so a reader's view of the cell compares numbers, as a
/// layout read does (DECISIONS 166).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct CellWrite {
    pub digest: u64,
    pub top: u64,
    pub writers: Vec<String>,
    pub c: ValueContract,
}

impl CellWrite {
    fn new(c: ValueContract, digest: u64) -> Self {
        let mut t = c.clone();
        t.unknown = false;
        t.prim = 0;
        CellWrite {
            digest,
            top: self::digest(&t),
            writers: Vec::new(),
            c,
        }
    }
}

/// What a reader saw of an entry read by name (a layout, a function
/// contract, DECISIONS 103/106): it is solved again when the entry differs.
pub(crate) fn digest<T: std::hash::Hash>(v: &T) -> u64 {
    use std::hash::Hasher;
    let mut h = std::collections::hash_map::DefaultHasher::new();
    v.hash(&mut h);
    h.finish()
}

/// The units defining each display. A display may have several owners:
/// every unit holding a copy of one literal shape (`decl_units`).
fn owners(per_unit: &[Option<ProgramFacts>]) -> FxHashMap<String, Vec<usize>> {
    let mut owner: FxHashMap<String, Vec<usize>> = FxHashMap::default();
    for (si, u) in per_unit.iter().enumerate() {
        let Some(u) = u else { continue };
        for t in u.templates.iter().filter(|t| t.module == Some(0)) {
            owner.entry(t.display.clone()).or_default().push(si);
        }
        for d in &u.fns_defined {
            owner.entry(d.clone()).or_default().push(si);
        }
    }
    owner
}

/// Units `work` and every unit depending on one of them, transitively;
/// a unit over the bound keeps its result (`resolve_again`).
fn dependents(
    solves: &[Solve],
    per_unit: &[Option<ProgramFacts>],
    mut work: Vec<usize>,
) -> Vec<bool> {
    let n = solves.len();
    let mut importers: Vec<Vec<usize>> = vec![Vec::new(); n];
    for (si, s) in solves.iter().enumerate() {
        for &d in &s.deps {
            importers[d].push(si);
        }
    }
    let mut dirty = vec![false; n];
    while let Some(si) = work.pop() {
        if per_unit[si].as_ref().is_some_and(|u| u.stats.aborted) {
            continue;
        }
        if !std::mem::replace(&mut dirty[si], true) {
            work.extend(importers[si].iter().copied());
        }
    }
    dirty
}

/// A unit's bodies by program module: its solve's module 0 is the unit's
/// own module (`mi`); a body of any other module is no claim here.
fn program_bodies(bodies: &mut Vec<crate::bindings::BodyRef>, mi: usize) {
    if bodies.iter().any(|b| b.module != 0) {
        bodies.clear();
    }
    for b in bodies.iter_mut() {
        b.module = mi as u32;
    }
}

fn merge_unit(
    pf: &mut ProgramFacts,
    mi: usize,
    path: &str,
    view: &crate::decl_units::UnitView,
    u: ProgramFacts,
) {
    for mut t in u.templates {
        if t.module != Some(0) {
            continue;
        }
        t.module = Some(mi as u32);
        for f in t.fields.iter_mut() {
            program_bodies(&mut f.fns, mi);
        }
        t.id = pf.templates.len() as crate::model::TmplId;
        pf.templates.push(t);
    }
    pf.sites.extend(u.sites.into_iter().map(|s| SiteFacts {
        module: mi as u32,
        ..s
    }));
    if let Some(mut b) = u.bindings.into_iter().next() {
        for (_, f) in b.locals.iter_mut().chain(b.this_of.iter_mut()) {
            program_bodies(&mut f.bodies, mi);
        }
        let into = &mut pf.bindings[mi];
        into.locals.extend(
            b.locals
                .into_iter()
                .filter(|(id, _)| view.binding_locals.contains(id)),
        );
        into.this_of.extend(
            b.this_of
                .into_iter()
                .filter(|(id, _)| view.binding_funcs.contains(id)),
        );
    }
    pf.super_unrelinked.extend(u.super_unrelinked);
    pf.interp_guard = pf.interp_guard.take().or(u.interp_guard);
    pf.fnproto_bind_written = pf.fnproto_bind_written.take().or(u.fnproto_bind_written);
    let (a, s) = (&mut pf.stats, &u.stats);
    a.templates += s.templates;
    a.fact_units += s.fact_units;
    a.nodes += s.nodes;
    a.constraints += s.constraints;
    a.tokens += s.tokens;
    a.functions += s.functions;
    a.gen_ms += s.gen_ms;
    a.solve_ms += s.solve_ms;
    a.derive_ms += s.derive_ms;
    a.pops += s.pops;
    a.instances += s.instances;
    a.shared_instances += s.shared_instances;
    a.composed += s.composed;
    a.contexts += s.contexts;
    a.work += s.work;
    a.top_calls += s.top_calls;
    a.pts_entries += s.pts_entries;
    pf.units.push((path.to_string(), u.stats));
}

/// Post-census verdicts for sites on a parameter whose TS type is an
/// object type (design §11.4): the type picks the candidates, every class
/// template of the program whose definite data fields carry the type's
/// keys; the entry guard is `class_id(param) ∈ candidates`. The site is E0
/// when every candidate is prefix-stable and holds the key in one slot.
fn typed_param_verdicts(pf: &mut ProgramFacts) {
    let mut cand_of: FxHashMap<Vec<String>, Vec<usize>> = FxHashMap::default();
    for s in pf.sites.iter_mut() {
        let Some(keys) = s.guard_keys.take() else {
            continue;
        };
        if !matches!(s.verdict, crate::Verdict::No(_)) || s.is_call {
            continue;
        }
        let tfs = &pf.templates;
        let cands = cand_of.entry(keys.clone()).or_insert_with(|| {
            (0..tfs.len())
                .filter(|&i| {
                    let t = &tfs[i];
                    t.class_id.is_some()
                        && keys.iter().all(|k| {
                            t.fields
                                .iter()
                                .any(|f| &f.key == k && f.definite && f.data_field)
                        })
                })
                .collect()
        });
        if cands.is_empty() {
            continue;
        }
        let slot = |t: &TemplateFacts| {
            t.fields
                .iter()
                .find(|f| f.key == s.key && f.definite && f.data_field)
                .and_then(|f| f.slot)
        };
        let first = slot(&tfs[cands[0]]);
        if first.is_some()
            && cands
                .iter()
                .all(|&i| tfs[i].prefix_stable && slot(&tfs[i]) == first)
        {
            s.receivers = cands.iter().map(|&i| tfs[i].display.clone()).collect();
            s.verdict = crate::Verdict::E0;
        }
    }
}

/// Facts must name one lexical binding. A second declaration with the same
/// id is a compiler bug, not a choice of which declaration's proof to keep.
fn unique_bindings<T>(values: &mut [(u32, T)], module: &str, kind: &str) {
    values.sort_by_key(|(id, _)| *id);
    for pair in values.windows(2) {
        assert_ne!(
            pair[0].0, pair[1].0,
            "per-unit facts: duplicate {kind} {} in {module}; cloned bodies must have distinct binding identities",
            pair[0].0,
        );
    }
}

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
