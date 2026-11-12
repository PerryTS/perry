//! Complex constraints (`Cons`), attached to the node whose new tokens trigger them.

use super::*;

/// What an own-property test's gate node (`Cons::OwnGate`) holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OwnGateAt {
    /// The callee `H` of `H.call(o, k)` or the member `o.hasOwnProperty`:
    /// `Object.prototype.hasOwnProperty` / `propertyIsEnumerable` pass (and
    /// gate on their own `call` field: a written one runs instead of
    /// `Function.prototype.call`); any other token opens.
    OwnFn,
    /// A program function whose body derives the own-test predicate.
    Helper,
    /// Propagate a predicate refutation marker, never a program value.
    Refuted,
    /// The `Object` constructor of `Object.hasOwn(o, k)`: it gates on its
    /// `hasOwn` field (the program may replace the static); any other token
    /// opens.
    ObjectCtor,
    /// A value that holds no token unless the test ran other code (the
    /// test's result, a written `call` or `hasOwn` field): any token opens.
    Value,
}

/// Where a program observes the identity of what a node holds
/// (`Cons::Identity`; DESIGN-p4l4 §2.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IdentityRoute {
    /// `===` / `!==` between two values neither of which is a primitive
    /// literal (a comparison with one is a presence test).
    StrictEq,
    /// `==` / `!=`: object against object is identity.
    LooseEq,
    /// A `switch` discriminant and its case expressions (`===`).
    Switch,
    /// `Object.is`.
    ObjectIs,
    /// `Object.prototype.isPrototypeOf`: the receiver against each object on
    /// the argument's chain.
    IsPrototypeOf,
    /// A Map/Set/WeakMap/WeakSet key (SameValueZero, or identity).
    Collection,
    /// `structuredClone` keeps the sharing of what it copies.
    Clone,
    /// A realm declaration's `identity(slot)`.
    Native,
    /// `JSON.stringify` with a replacer that is no literal `null`,
    /// `undefined` or array: the replacer receives every value serialized,
    /// unseen by the model (DESIGN-p4l4 finding 14).
    JsonReplacer,
}

impl IdentityRoute {
    pub fn name(self) -> &'static str {
        match self {
            IdentityRoute::StrictEq => "===",
            IdentityRoute::LooseEq => "==",
            IdentityRoute::Switch => "switch",
            IdentityRoute::ObjectIs => "Object.is",
            IdentityRoute::IsPrototypeOf => "isPrototypeOf",
            IdentityRoute::Collection => "collection-key",
            IdentityRoute::Clone => "structuredClone",
            IdentityRoute::Native => "native",
            IdentityRoute::JsonReplacer => "JSON replacer",
        }
    }
}

/// Complex constraints, attached to the node whose new tokens trigger them.
#[derive(Clone, Copy, Debug)]
pub enum Cons {
    /// The program observes the identity of every object the node holds
    /// (`route`). No flow and no effect on the solve: a fact the value-record
    /// derive reads (DESIGN-p4l4 R4). Generated only under
    /// `PERRY_FACTS_VALUE_RECORDS=1` (`Model::value_records`).
    Identity { route: IdentityRoute },
    /// The node is the value a contract function `f` stores under `key`
    /// into its parameter `dst`'s argument (`reach`: into what it reaches)
    /// (DECISIONS 118; `dst` `u32::MAX`: into what its receiver reaches,
    /// and `Key::Any` a computed key, DECISIONS 149): a marker of one of
    /// `f`'s parameters is that argument, which the reader stores; any
    /// other value escapes.
    StoreOut {
        f: FnIdx,
        dst: u32,
        reach: bool,
        /// A keyed reach (design §18.4): what the argument holds under it.
        via: Option<Atom>,
        key: Key,
        reason: u32,
        /// The receiver's reach marker stored into (`dst` `u32::MAX`).
        at: Tok,
        /// What the store met: `at`, or (a receiver-rooted store, DECISIONS
        /// 149b) another function's marker or ⊤.
        to: Tok,
    },
    /// dst ⊇ read(read(t, `prototype`), key) for every t in the node
    /// (`E.prototype.key`): a builtin namespace whose prototype's own
    /// member `key` is final reads the native it names
    /// (`Model::proto_key`).
    ProtoKey { key: Atom, dst: NodeId },
    /// dst ⊇ read(t, key) for every t in the node.
    Load { key: Key, dst: NodeId },
    /// Values of own properties only (enumerability over-approximated).
    OwnValues { dst: NodeId },
    /// A store under an index of each Array in the node: it may leave a
    /// hole (`Model::mark_hole`); an append never does.
    IndexHole,
    /// A store under an index of each Array in the node, at a site whose
    /// key gen proved at most the Array's length there
    /// (`Model::inbounds_sites`): it leaves no hole in an Array whose length
    /// never shrinks (`Model::index_kept`), and may leave one in any other.
    IndexKept,
    /// `registerPathModule`: dst ⊇ a CommonJS record's live `exports` (the
    /// wrapper's record literal carries `__perry_cjs_record` at birth), or
    /// the published value itself for a raw publication.
    CjsPublish { dst: NodeId },
    /// write(t, key) ⊇ src for every t in the node.
    Store { key: Key, src: NodeId, site: SiteId },
    /// The node is a callee.
    Call { call: CallId },
    /// The node is a callee whose `this` is fixed to one receiver token
    /// (a method found on that receiver or its chain).
    CallThis { call: CallId, this: Tok },
    /// The node is a `new` callee.
    New { call: CallId },
    /// The node holds the constructor a built-in makes its result with
    /// (the `this` of `Array.from`/`Array.of`, an array's species): a
    /// program constructor (a class or function of the program's code) is
    /// constructed by `call`. The realm's own constructors, `%Array%` among
    /// them, make the built-in's own array, which the built-in's rule makes.
    NewProgram { call: CallId },
    /// The target of Reflect.construct; allocation is selected by new_target.
    ReflectNew { call: CallId, new_target: NodeId },
    /// New-target values paired with one target, rebuilt from ReflectNew.
    ReflectNewTarget { call: CallId, target: Tok },
    /// `new Name(...)` of a builtin constructor named in the source: runs
    /// once per call (the holder holds one marker token), so an instance
    /// that renames the call constructs its own object.
    NewNamed { name: Atom, call: CallId },
    /// The node is a method-call receiver (`recv.key(...)`).
    MCall { key: Key, call: CallId },
    /// The node is `ProtoNode(cur)`: continue a lookup for `origin`.
    Chain {
        origin: Tok,
        key: Key,
        act: ChainAct,
    },
    /// Every token escapes (goes to ⊤), with the reason.
    TopSink { reason: u32 },
    /// An operation on the node runs in throw region `region` (DESIGN SEEDE
    /// section 8): once ⊤ is here, unknown code may run (a getter, a trap,
    /// ToPrimitive, a callee) and throw anything into the region.
    Raise { region: NodeId },
    /// A realm field already retains its canonical value. Storing it again
    /// exposes no new reach; every other alternative takes the usual sink.
    RuntimeStore { reason: u32, retained: Tok },
    /// A parameter of a closed class's stand-in constructor (design §18.13,
    /// DECISIONS 109): an instance of another unit's contract (a stand-in)
    /// goes to the defining unit's constructor through the argument
    /// contract, which describes it there as the same stand-in, so that
    /// unit's census covers what the constructor does with it; an array
    /// leaves by its element cell (DECISIONS 94, 149e: the parameter is
    /// checked as a published value, `Gen::check_value`, and the argument
    /// contract names it); any other token escapes, with the reason.
    /// DECISIONS 152: a parameter marker of this unit's function (its
    /// callers' value) is handed over by that function's contract
    /// (`FnContract::param_handovers`): the callers' units pass what they
    /// hold to argument `arg` of the constructor `ctor` themselves.
    HandOver { reason: u32, ctor: Tok, arg: u32 },
    /// DECISIONS 149d: what a receiver-rooted store of function `f` puts
    /// into an object of this unit: every value but `f`'s parameter and
    /// argument markers, which are `f`'s receiver flow (`Cons::StoreOut`).
    OwnOnly { f: FnIdx, dst: NodeId },
    /// SHC6 h4: what a store into a published method's receiver marker
    /// puts into this unit's own instances (`store_into_own_instances`):
    /// every value, a marker (a caller's value, relative to a caller that
    /// may run the method unseen) as ⊤.
    CallerValue { dst: NodeId, reason: u32 },
    /// The `this` of a contract stand-in of another unit's class `tmpl`
    /// (design §11.2): its instances are inside the contract; any other
    /// receiver escapes (that unit runs the method on it unseen).
    /// `this` of another unit's method: only that class's instances (and,
    /// for a static method, its class object); any other receiver escapes.
    /// `this` (`Model::this_contracts`, `u32::MAX`: none) is the method's
    /// receiver contract (DECISIONS 57): a token of a subclass of `tmpl` is
    /// admitted with the published effects on it.
    ContractThis {
        tmpl: TmplId,
        reason: u32,
        statics: bool,
        this: u32,
    },
    /// `this` of the override token of (`tmpl`, `key`) (DECISIONS 97): a
    /// method's receiver marker records the call (`Model::this_calls`);
    /// `tmpl`'s receiver token passes (it stands for the class's own
    /// instances, which no subclass key reaches); any other receiver
    /// escapes with `reason`, as for an unknown callee.
    OverrideThis {
        tmpl: TmplId,
        key: Atom,
        reason: u32,
    },
    /// Once anything (a token or a primitive) reaches the node, `dst` holds
    /// ⊤ and everything `from` holds: the result of a call that exists only
    /// when an argument reaches it.
    /// (`reason` `None`: only `from`, no ⊤; `from` = `dst`: only ⊤.)
    TopIfAny {
        dst: NodeId,
        from: NodeId,
        reason: Option<u32>,
    },
    /// The specifier of `require` call `call` (R-SPEC): each string
    /// constant that reaches the node resolves as that literal does
    /// (`builtins::require_by_spec`); a value no constant spells (any
    /// string, a token, ⊤) resolves as anything. Other primitives resolve
    /// to nothing: the loader throws on a specifier that is no string.
    RequireSpec { call: CallId },
    /// Once ⊤ reaches the node, `dst` holds ⊤ (all kinds) for `reason`; a
    /// data value or a marker leaves it inert (an instance's ⊤ arrives as
    /// ⊤). The result of a rule that is unknown only when the node is: an
    /// accessor walk over a holder whose any-key node holds ⊤.
    TopIfTop { dst: NodeId, reason: u32 },
    /// `from` flows into `dst` once the node may hold a value `test`
    /// admits (`guard`): a conditional's branch, a logical operator's right
    /// operand, a destructuring default reach the result only when their
    /// test can take the value that selects them. A node that never holds
    /// such a value is a test that never selects the branch.
    Guard { test: u8, from: NodeId, dst: NodeId },
    /// The node's values flow into `dst` without `undefined`: the chain of
    /// a read whose own data key the object surely holds. The lookup stops
    /// at the object, so the chain's end (a key no link defines reads
    /// `undefined`) is never reached; its values stay, a superset.
    Defined { dst: NodeId },
    /// The left operand of `l ?? r`: its values but `null` and `undefined`
    /// flow into `dst` (the result is `l` only when it is neither). A rule
    /// of the program text, so a summary replays it per call; `Defined` is
    /// the solver's, decided for the values one solve saw.
    NotNullish { dst: NodeId },
    /// Rule G2 (property truthiness): `x` read where `x.key` tested truthy
    /// (`if (x && x.key)`, ECMA-262 7.1.2, 13.13, 14.6, 13.14). Its values
    /// flow into `dst` but the ones the test excludes: `null` and
    /// `undefined` (reading `key` on them throws), and an object token whose
    /// read of `key` (own field, prototype chain, unknown-key field,
    /// accessor; `Model::load_from`) holds only `undefined`/`null`. One
    /// predicate (`guard::TRUTHY` on that read): any object, function,
    /// marker, ⊤ or primitive kind other than `undefined`/`null` in the read
    /// passes the token, re-checked as the read grows (monotone). ⊤ and
    /// markers in `x` always pass. A rule of the program text, like
    /// `NotNullish`.
    FieldTruthy { key: Key, dst: NodeId },
    /// The left operand of `l && r`: its falsy part flows into `dst` (the
    /// result is `l` only when it is falsy, and every object is truthy):
    /// its primitive kinds that have a falsy value, ⊤ as those kinds only,
    /// and its parameter or argument markers (a caller's value). No object
    /// or function token. A rule of the program text, like `NotNullish`.
    Falsy { dst: NodeId },
    /// The argument of a defaulted parameter: its values but `undefined`
    /// flow into the parameter's local `dst` (the default replaces
    /// `undefined`, a `Guard` on the argument). A rule of the program text,
    /// like `NotNullish`.
    NotUndefined { dst: NodeId },
    /// Every object token reaches a HARD operation.
    Hard { kind: Hard, reason: u32 },
    /// DECISIONS 242 (contract half): another unit's function reads `key`
    /// on the argument this node holds (`FnContract::reach_keys`): a miss
    /// site without a value (`Model::note_chain_miss` on every token).
    Miss { key: Key },
    /// `delete o.key`: E7 HARD on every object token; on ⊤ (which may be a
    /// builtin prototype) a write of `key` only.
    DeleteKey { key: Atom, reason: u32 },
    /// `Object.defineProperty(o, key, desc)` with a constant key (FVA 4.3a).
    /// An accessor descriptor (`data` false) on a literal layout that does
    /// not hold `key` as data: the key becomes an accessor key (the getter
    /// and setter are installed by `Accessor`) and the layout gains it. On a
    /// prototype that has no method `key`, either descriptor: `key` is a key
    /// of the prototype that is no plain data slot (an accessor, or a value
    /// whose attributes are the descriptor's), as a class getter is; the
    /// value is stored by the caller. Any other token (a key it holds as
    /// data, a class instance, ⊤, a data descriptor on a literal): E5 HARD.
    DefineKey { key: Atom, data: bool, reason: u32 },
    /// `Object.defineProperties(o, map)`: the node holds the maps. A literal
    /// with exactly its birth keys defines each of them by the descriptor
    /// under it (`DefineDesc`); any other map defines its own values
    /// (`descs`) under a computed key: E5 HARD on `o`, the descriptors'
    /// values (`value`, a load of `descs`) stored under any key at `site`.
    DefineAll {
        o: NodeId,
        descs: NodeId,
        value: NodeId,
        site: SiteId,
        reason: u32,
    },
    /// Internal: the node holds the descriptors `defineProperties` defines
    /// `key` of `o` by. An accessor descriptor literal is a `DefineKey`
    /// accessor; any other descriptor is a data definition (its `value`
    /// stored under `key` at `site`); either installs its get/set.
    DefineDesc {
        o: NodeId,
        key: Atom,
        site: SiteId,
        reason: u32,
    },
    /// Every token gains keys after birth (computed store …).
    AddsKeys,
    /// Builtin protocol touch: what `proto` reads on every token.
    Implicit { reason: u32, proto: Protocol },
    /// InstanceofOperator (13.10.2) with the node as its right-hand side:
    /// `GetMethod(C, @@hasInstance)` on every token, called with `arg`, the
    /// left-hand side value (`Call(instOfHandler, C, « V »)`).
    HasInstance { reason: u32, arg: NodeId },
    /// The node is the target of `bound`, a bound function the runtime may
    /// call with anything (`Protocol::Any`): the call is the target's
    /// [[Call]] (10.4.1.1). A class constructor throws a TypeError before
    /// its body runs (10.2.1 step 2); a bound target forwards the call;
    /// any other target is called with anything: `bound` escapes.
    BoundCall { reason: u32, bound: Tok },
    /// The node holds `Object.create` prototype arguments (E10).
    CreateProto { reason: u32 },
    /// The node is `ProtoNode(of)`: dst ⊇ the values of `key` on each
    /// prototype (and further up), shared by every receiver of `of`.
    /// `builtin` false: the builtin prototypes on the chain add nothing (an
    /// own key shadows them, DECISIONS 234).
    ChainUnion {
        key: Key,
        dst: NodeId,
        of: Tok,
        builtin: bool,
    },
    /// The node holds a value given as the prototype of an object (its
    /// `ProtoNode` is `protos`): a builtin prototype there may be any of
    /// them (`ObjectProto` collapses them), whose members the chain cannot
    /// name: the chain also holds an unknown prototype.
    ProtoGiven { protos: NodeId },
    /// The node holds a value given as the [[Prototype]] of class object
    /// `of` (or of ⊤): `of`'s `super()` runs a class held here as its
    /// parent, on an instance of `of`.
    ParentGiven { of: Tok },
    /// The node is `ProtoNode(t)`: dst ⊇ every prototype, transitively.
    ProtoClosure { dst: NodeId },
    /// The node is the prototype closure of receiver `origin`: builtin
    /// prototype methods (Object.prototype, native parents) run for it.
    ProtoMethods { origin: Tok, key: Key, call: CallId },
    /// The node is the prototype closure of `origin`: accessors of every
    /// holder on it run for a computed-key lookup with `origin` as `this`.
    AnyAccessors { origin: Tok, act: ChainAct },
    /// The node is a copy source's unnamed-key field (`field_any`): once it
    /// holds a value the source has an own key the model does not name, a
    /// [[Set]] of some key on `target` (Object.assign, `spread_into`).
    UnnamedSet { target: Tok },
    /// The node is the prototype closure of `origin`: an unknown (⊤)
    /// prototype may hold a setter, so the stored value and `origin` escape.
    StoreTopProto { origin: Tok, src: NodeId },
    /// The node is `this`: continue a `super.key` lookup at `start` with
    /// each `this` token as the receiver.
    Super { start: Tok, key: Key, act: ChainAct },
    /// The node holds the prototypes of a `super` access's home object
    /// (its [[GetPrototypeOf]] at the access): continue the lookup at each
    /// (`Cons::Super`), with each token of `recv` as the receiver.
    SuperFrom {
        recv: NodeId,
        key: Key,
        act: ChainAct,
    },
    /// The node is `this` of a constructor of `class`: run the parent
    /// constructor chain for each instance (`super(...)`).
    SuperCtor { class: TmplId, call: CallId },
    /// A live constructor-prototype candidate runs on the same derived receiver.
    SuperCtorParent {
        class: TmplId,
        call: CallId,
        origin: Tok,
    },
    /// The node holds method closures installed on `recv` (literal methods):
    /// their `this` is `recv`.
    ThisFrom { recv: NodeId },
    /// A literal's own content: every token here gets `src` in field `key`
    /// (no store: a birth, as generation's direct field edges are). A
    /// summary's form of a literal born in its body.
    BirthField { key: Key, src: NodeId },
    /// The node holds getter/setter functions installed on the objects of
    /// `holder` under `key`.
    Accessor {
        holder: NodeId,
        key: Atom,
        setter: bool,
    },
    /// Internal: install accessor `f` on every token of the node.
    /// `f`: the accessor's callable token (a known body or opaque callback).
    /// `from`: the node that held `f` (`NO_NODE`: none), the source the
    /// census attributes an opaque accessor's ⊤ to.
    AccessorInstall {
        key: Atom,
        f: Tok,
        setter: bool,
        from: NodeId,
    },
    /// A direct Annex B runtime entry, dispatched per receiver token.
    AnnexBDefine { call: CallId, setter: bool },
    /// Reflect an accessor descriptor after ToPropertyKey.
    AnnexBLookup { call: CallId, setter: bool },
    /// The node holds objects whose prototype set includes `src`'s values.
    ProtoFrom { src: NodeId },
    /// dst ⊇ prototypes of the node's objects (`Object.getPrototypeOf`), as
    /// values (`Model::proto_value`).
    ProtoOf { dst: NodeId },
    /// The node is `ProtoNode(t)`: dst ⊇ its prototypes as the runtime has
    /// them (DECISIONS 217): a literal's class prototype is its layout, no
    /// object; that layout's own prototypes stand in its place.
    ProtoValues { dst: NodeId },
    /// The node is the prototype closure of receiver `origin`, read or
    /// called under a key that names no intrinsic member and not
    /// `__proto__` (a number; DECISIONS 217): every program prototype's
    /// fields under any key, what a store into ⊤ wrote on an intrinsic one.
    PlainKey { origin: Tok, act: ChainAct },
    /// dst ⊇ the `next` method of each iterator in the node.
    IterNext { dst: NodeId },
    /// The node holds the source of `Array.from`: a program object whose
    /// `@@iterator` may be nullish is read as an array-like (ECMA-262
    /// 23.1.2.1 step 5: `length`, then each index), its elements into
    /// `dst`. Iteration is `Gen::iterate`'s.
    FromArrayLike { dst: NodeId },
    /// The node holds `Reflect.construct` new-targets (E11).
    NewTargetHard { reason: u32 },
    /// The node is the evaluated `extends` value of class `tm`, linked by
    /// name to a built-in parent whose summary `super()` runs: any value
    /// other than `expected`, the global object's declared constructor
    /// (⊤, a program-written global, `null`), runs as the parent
    /// constructor with `this` = the instance: it escapes, and the instance
    /// escapes with hard E11.
    NativeParent {
        tm: TmplId,
        expected: Tok,
        reason: u32,
    },
    /// The node is the object of `o[idx]` with a computed key: dst ⊇ its
    /// elements/fields; the full keyed lookup (prototype chain, builtin
    /// members) runs only if `idx` can hold a non-number key (`IdxKey`).
    IdxLoad {
        idx: NodeId,
        dst: NodeId,
        shape: KeyShapeId,
        /// `o[k]` in `for (k in o)`, the loop's own key on the binding it
        /// iterates (`Gen::enum_read`): the key is one of `o`'s enumerable
        /// string keys, so on a built-in object (`Model::enum_free`) the
        /// read is what the program wrote there, never a built-in member.
        enumerable: bool,
    },
    /// The node is the key of `o[k]` for object token `obj`: a string,
    /// symbol or object key runs the full lookup. It is never taken to be
    /// `"__proto__"` (DECISIONS 217: no program reaches a prototype through
    /// an unknown string key; only the named forms read it).
    IdxKey {
        obj: Tok,
        dst: NodeId,
        shape: KeyShapeId,
    },
    /// The past-the-end `undefined` of a computed-key read `o[key]` of
    /// program Array token `obj` (DESIGN-p4l1 §2.8, PR-C1). Watches both
    /// inputs: it sits on the key node and on `arr_len(obj)`. `dst` ⊇
    /// `undefined` once the read may be at or past the length: the length
    /// is ⊤, or the key may be anything but an integer Number in
    /// `[0, arr_len(obj).lo)`. Below the length the read is an element or a
    /// hole, which the read's own edges carry (`all_fields`, `hole`).
    ElemUndef { obj: Tok, key: NodeId, dst: NodeId },
    /// `IdxLoad` of a read that runs only after an own-property test of the
    /// same object and key (`gen/own_guard.rs`): a program object's own
    /// fields under the key (`IdxOwnKey`), never its prototype chain; any
    /// other value reads as `IdxLoad`. `OwnGate` adds the ordinary `IdxLoad`
    /// once the test may be something else.
    IdxOwnLoad {
        idx: NodeId,
        dst: NodeId,
        shape: KeyShapeId,
        /// As `IdxLoad`'s.
        enumerable: bool,
    },
    /// The node is the key of an own-only read of object token `obj`
    /// (`IdxOwnLoad`): a primitive key reads `obj`'s own fields it may name;
    /// a token (an object, whose ToPropertyKey runs program code at the test
    /// and again at the read, or ⊤) runs the full lookup (`IdxKey`'s).
    IdxOwnKey {
        obj: Tok,
        dst: NodeId,
        shape: KeyShapeId,
    },
    /// The body-derived own-test predicate: nonmatching callee values or
    /// replaceable builtin fields refute `dst` outside the value lattice. The same
    /// predicate serves direct guards and helper-body evidence.
    OwnTest { dst: NodeId, at: OwnGateAt },
    /// A refutation marker restores the ordinary read `obj[idx]`.
    OwnGate {
        obj: NodeId,
        idx: NodeId,
        dst: NodeId,
        shape: KeyShapeId,
        enumerable: bool,
    },
    /// The node is `ProtoNode(from)` on the chain of receiver `of` of an
    /// enumerable-key read (`IdxLoad` `enumerable`): dst ⊇ what the program
    /// wrote under `key` on each prototype, further up while the prototype
    /// has no enumerable built-in member (`Model::enum_link`, which needs
    /// `from` for the collapsed `ObjectProto`); any other prototype reads
    /// `key` in full.
    EnumChain {
        key: Key,
        dst: NodeId,
        of: Tok,
        from: Tok,
    },
    /// The node is the receiver of `o[idx](...)` with a computed key: its
    /// objects' own and program members under a number key; the full
    /// method lookup only if `idx` can hold another key (`IdxKeyCall`).
    IdxMCall {
        idx: NodeId,
        call: CallId,
        shape: KeyShapeId,
    },
    /// The node is the key of `o[k](...)` for receiver token `obj`: a
    /// string, symbol or object key calls `Key::Any`'s methods.
    /// `obj` ⊤ with `prim`: the receiver's primitives of those kinds.
    IdxKeyCall {
        obj: Tok,
        prim: u16,
        call: CallId,
        shape: KeyShapeId,
    },
    /// The node is the value of a for-in body (FVA 4.3b, `gen/for_in.rs`):
    /// each body token runs per value of `obj` (`ForInKeys`), its runs bound
    /// through the template call `call` (one argument: the key).
    ForInBody { obj: NodeId, call: CallId },
    /// The node is the object for-in body `ft` iterates: a literal whose keys
    /// are exactly its birth keys runs the body once per key, that run's key
    /// parameter holding the one key (`Model::key_nodes`); any other value
    /// runs it once through `call`, with every key.
    ForInKeys { ft: Tok, call: CallId },
    /// The node is the key of `defineProperty(o, k, desc)` in a for-in body:
    /// a run's one key defines it as a constant key does (`DefineKey`);
    /// any other key is E5 HARD.
    DefineKeyed {
        o: NodeId,
        reason: u32,
        accessor: bool,
    },
    /// The node is the key of a store into `o` in a for-in body: a run's one
    /// key stores under that key; any other under the keys the loop's
    /// subject enumerates when `subj` is that subject's node (ENUM3,
    /// `Cons::EnumKeyStore`), else under `Key::Any`.
    StoreKeyed {
        o: NodeId,
        src: NodeId,
        site: SiteId,
        subj: NodeId,
    },
    /// ENUM3: on the subject `s` of `for (k in s)` (`from: None`) or on the
    /// prototype node of its chain link `from`: `o[k] = src` stores under
    /// each String key the for-in may yield (ECMA-262 14.7.5.9
    /// EnumerateObjectProperties, 14.7.5.10: the enumerable String keys of
    /// `s` and its chain), the names written on each link the ENUM1/ENUM2
    /// read rule covers (`Model::enum_key_store`), growing as fields
    /// appear; under any String key for any other value.
    EnumKeyStore {
        o: NodeId,
        src: NodeId,
        site: SiteId,
        from: Option<Tok>,
    },
    /// ENUM3: on a link's any-key field (or what a store into ⊤ wrote on a
    /// built-in link): once it holds a value, a key no name lists may be
    /// enumerated, so `o[k] = src` stores under any String key.
    EnumKeyAny {
        o: NodeId,
        src: NodeId,
        site: SiteId,
    },
    /// The node is the key of a computed store `o[key] = src`: a key that
    /// may be a Number stores at an index (the element key), one that may be
    /// anything else (a string, a symbol, an object's ToPrimitive result)
    /// stores under `Key::Any`.
    StoreIndexed {
        o: NodeId,
        src: NodeId,
        site: SiteId,
        shape: KeyShapeId,
    },
    /// The node is the key of a definition whose descriptor's get (set) is
    /// `f`, on the objects of `holder`: a run's one key installs it under
    /// that key, any other under the computed-key star.
    AccessorKeyed {
        holder: NodeId,
        f: NodeId,
        setter: bool,
    },
    /// The node is a ToNumeric operand (`-`, `*`, `++`, unary `-`, ...):
    /// dst ⊇ the numeric kinds its values turn into (Number; BigInt for a
    /// BigInt; both for an object, whose ToPrimitive may return either).
    Arith { dst: NodeId },
    /// The node is an operand of a numeric operator whose result `dst`
    /// takes its integer interval from this rule (`range::eval`), the
    /// operand's interval beside `other`'s (`NO_NODE`: a unary operator).
    /// The kinds of `dst` stay `Arith`'s / `Plus`'s.
    Range {
        op: RangeOp,
        other: NodeId,
        dst: NodeId,
        /// The node is the right operand (`eval`'s second argument).
        right: bool,
    },
    /// The node holds Arrays an index store writes, with its index in
    /// `[lo, hi]`: each keeps its birth length only when that interval lies
    /// inside it (`Model::arr_len_index`).
    LenIndex { lo: i64, hi: i64 },
    /// The node is token `t`'s length node (`Model::arr_len`), and `t` is
    /// permuted in place (`reverse`, `copyWithin`): once the length is not
    /// a point it is ⊤ (`Model::arr_len_permuted`).
    LenPermuted { t: Tok },
    /// The node is an operand of `+`: `str` ⊇ String when the value may be
    /// a String (or an object, whose ToPrimitive may return one); `num` ⊇
    /// the numeric kinds of its other values (Number for a Number, Boolean,
    /// `null` or `undefined`; BigInt for a BigInt; both for an object).
    /// ECMA-262 13.15.3: after ToPrimitive a String operand concatenates,
    /// so the generation rule lets `num` reach the result only through a
    /// [`Cons::Guard`] with test [`guard::NOT_STR`] on the other operand.
    /// Rule M3: `other` is the other operand and `right` says this node is
    /// the right one; `str`'s string constants and known prefixes are the
    /// pair's (`Model::plus_strs`).
    Plus {
        str: NodeId,
        num: NodeId,
        other: NodeId,
        right: bool,
    },
    /// The node is the only argument of `Array(x)` / `new Array(x)`: a Number
    /// is the length (the array has holes, no element), any other value is
    /// the one element (ECMA-262 Array). `dst` ⊇ its tokens and its
    /// non-number primitives.
    ArrayLenArg { dst: NodeId },
    /// The node holds the source of a typed array `ta`: a buffer or view
    /// there means `ta` shares (and so exposes) that buffer (B1).
    ShareBuffer { ta: Tok },
    /// The node holds the source of view `view`'s [[ViewedArrayBuffer]]
    /// (`VIEWED_BUFFER`). `share`: a view of the same memory (`subarray`,
    /// Buffer `slice`): the source view's buffer. Otherwise a constructor
    /// argument: an ArrayBuffer is the buffer (23.2.5.1.3, 25.3.2.1), any
    /// other value makes `fresh` (`None`: the birth throws).
    ViewedBuffer { view: Tok, fresh: Option<Tok>, share: bool },
    /// The node holds the methods a lookup found on receiver `origin`:
    /// each runs with `origin` as `this` (the `this` half of `CallThis`,
    /// shared by every call site that looks the same key up on `origin`).
    ThisTo { origin: Tok },
    /// The node is call `call`'s callee relay (the union of the method
    /// nodes its receivers found): arguments and result bind once per
    /// (call, function) (the argument half of `CallThis`).
    CallArgs { call: CallId },
    /// The node is the result of run `run` (`Node::frame`), bound to
    /// `call`: the call's result takes each value, except that the family
    /// of the run's argument marker for a parameter is the caller's own
    /// argument there (DECISIONS 44 (b), `Model::ret_into`).
    RetInto { call: CallId, run: u32 },
    /// The node is argument `i` of `call`, bound to run `run`: each marker
    /// it gains is what the run's result returns of that parameter's marker
    /// family (DECISIONS 44 (b), `Model::arg_into`).
    ArgInto { call: CallId, run: u32, i: u32 },
    /// The node is the source of an object spread / `Object.assign` into
    /// the values of `into`: each source object copies its own keys, key
    /// by key, into each target (`Model::spread_into`), never through one
    /// any-key bucket.
    Spread { into: NodeId, site: SiteId },
    /// The node holds the targets of a spread from source token `src`; `from` is the node of the
    /// spread's source value (the census attributes a spread of ⊤ to it).
    SpreadFrom {
        src: Tok,
        site: SiteId,
        from: NodeId,
    },
    /// The node is a receiver of summarized function token `ft` at `call`
    /// (or of no call: `NO_NODE`): each value binds the instance of its
    /// layout (`Model::bind_layout`).
    ThisInst { call: CallId, ft: Tok },
    /// The node is the receiver of `call` into named native `f`
    /// (`f.call(x)`): each value runs the native with it as `this`
    /// (`Model::native_this`).
    NativeThis { call: CallId, f: Tok },
    /// The node is a method node some call sites bound through: a builtin
    /// namespace value in it runs per (call, receiver) (`callee_sites`).
    BuiltinCallee,
    /// The node holds prototypes on a named-key accessor lookup's chain:
    /// `pass` ⊇ the holders the lookup passes through (and continues
    /// above), `stop` ⊇ the prototypes where a declared member ends it.
    /// Built once per (start, key), shared by every receiver below it.
    HolderStep {
        key: Key,
        pass: NodeId,
        stop: NodeId,
    },
    /// The node is a lookup's pass-holder set: accessors there run for
    /// `origin` (and later ones replay through `visits`); ⊤ there is an
    /// unknown prototype.
    VisitHolders {
        origin: Tok,
        key: Key,
        act: ChainAct,
    },
    /// The node is a lookup's stop-holder set: accessors there run.
    VisitStop {
        origin: Tok,
        key: Key,
        act: ChainAct,
    },
}

impl Cons {
    /// The rule reads the key node's string constants (`Node::strs`): a
    /// pop where only they grew runs it (`Model::solve_fixpoint`).
    pub fn carries_strs(&self) -> bool {
        matches!(
            self,
            Cons::StoreIndexed { .. }
                | Cons::RequireSpec { .. }
                | Cons::Defined { .. }
                | Cons::NotNullish { .. }
                | Cons::FieldTruthy { .. }
                | Cons::NotUndefined { .. }
        )
    }

    /// The rule delivers Numbers of its host with the host's interval
    /// (`model::range`): it re-runs when only that interval grows, so the
    /// interval it delivered keeps covering the host's.
    pub fn carries_range(&self) -> bool {
        matches!(
            self,
            Cons::Range { .. }
                | Cons::LenPermuted { .. }
                | Cons::ElemUndef { .. }
                | Cons::StoreIndexed { .. }
                | Cons::RetInto { .. }
                | Cons::Defined { .. }
                | Cons::NotNullish { .. }
                | Cons::FieldTruthy { .. }
                | Cons::NotUndefined { .. }
                | Cons::Falsy { .. }
        )
    }

    pub fn kind_name(&self) -> &'static str {
        match self {
            Cons::Identity { .. } => "identity",
            Cons::Load { .. } => "load",
            Cons::ProtoKey { .. } => "protokey",
            Cons::OwnValues { .. } => "ownvalues",
            Cons::IndexHole => "indexhole",
            Cons::IndexKept => "indexkept",
            Cons::CjsPublish { .. } => "cjspublish",
            Cons::Store { .. } => "store",
            Cons::Call { .. } => "call",
            Cons::CallThis { .. } => "callthis",
            Cons::New { .. } => "new",
            Cons::NewProgram { .. } => "newprogram",
            Cons::FromArrayLike { .. } => "fromarraylike",
            Cons::ReflectNew { .. } => "reflectnew",
            Cons::ReflectNewTarget { .. } => "reflectnewtarget",
            Cons::NewNamed { .. } => "newnamed",
            Cons::MCall { .. } => "mcall",
            Cons::Chain { .. } => "chain",
            Cons::TopSink { .. } | Cons::RuntimeStore { .. } => "top",
            Cons::Raise { .. } => "raise",
            Cons::HandOver { .. } => "handover",
            Cons::OwnOnly { .. } => "ownonly",
            Cons::CallerValue { .. } => "callervalue",
            Cons::StoreOut { .. } => "storeout",
            Cons::ContractThis { .. } => "contractthis",
            Cons::OverrideThis { .. } => "overridethis",
            Cons::TopIfAny { .. } => "topifany",
            Cons::RequireSpec { .. } => "requirespec",
            Cons::TopIfTop { .. } => "topiftop",
            Cons::Guard { .. } => "guard",
            Cons::Defined { .. } => "defined",
            Cons::NotNullish { .. } => "notnullish",
            Cons::FieldTruthy { .. } => "fieldtruthy",
            Cons::Falsy { .. } => "falsy",
            Cons::NotUndefined { .. } => "notundefined",
            Cons::Hard { .. } => "hard",
            Cons::Miss { .. } => "miss",
            Cons::DeleteKey { .. } => "deletekey",
            Cons::AddsKeys => "addskeys",
            Cons::DefineKey { .. } => "definekey",
            Cons::DefineAll { .. } => "defineall",
            Cons::DefineDesc { .. } => "definedesc",
            Cons::Implicit { .. } => "implicit",
            Cons::HasInstance { .. } => "hasinstance",
            Cons::BoundCall { .. } => "boundcall",
            Cons::CreateProto { .. } => "createproto",
            Cons::ChainUnion { .. } => "chainunion",
            Cons::EnumChain { .. } => "enumchain",
            Cons::ProtoGiven { .. } => "protogiven",
            Cons::ParentGiven { .. } => "parentgiven",
            Cons::ProtoClosure { .. } => "protoclosure",
            Cons::ProtoMethods { .. } => "protomethods",
            Cons::AnyAccessors { .. } => "anyaccessors",
            Cons::UnnamedSet { .. } => "unnamedset",
            Cons::StoreTopProto { .. } => "storetopproto",
            Cons::Super { .. } => "super",
            Cons::SuperFrom { .. } => "super-from",
            Cons::SuperCtor { .. } => "superctor",
            Cons::SuperCtorParent { .. } => "superctorparent",
            Cons::ThisFrom { .. } => "thisfrom",
            Cons::BirthField { .. } => "birthfield",
            Cons::Accessor { .. } => "accessor",
            Cons::AccessorInstall { .. } => "accinstall",
            Cons::AnnexBDefine { .. } => "annexbdefine",
            Cons::AnnexBLookup { .. } => "annexblookup",
            Cons::ProtoFrom { .. } => "protofrom",
            Cons::ProtoOf { .. } => "protoof",
            Cons::ProtoValues { .. } => "protovalues",
            Cons::PlainKey { .. } => "plainkey",
            Cons::IterNext { .. } => "iternext",
            Cons::NewTargetHard { .. } => "newtargethard",
            Cons::NativeParent { .. } => "nativeparent",
            Cons::Arith { .. } => "arith",
            Cons::Range { .. } => "range",
            Cons::LenIndex { .. } => "lenindex",
            Cons::LenPermuted { .. } => "lenpermuted",
            Cons::Plus { .. } => "plus",
            Cons::ArrayLenArg { .. } => "arraylenarg",
            Cons::IdxLoad { .. } => "idxload",
            Cons::IdxKey { .. } => "idxkey",
            Cons::ElemUndef { .. } => "elemundef",
            Cons::IdxOwnLoad { .. } => "idxownload",
            Cons::IdxOwnKey { .. } => "idxownkey",
            Cons::OwnGate { .. } => "owngate",
            Cons::OwnTest { .. } => "owntest",
            Cons::IdxMCall { .. } => "idxmcall",
            Cons::IdxKeyCall { .. } => "idxkeycall",
            Cons::ForInBody { .. } => "forinbody",
            Cons::ForInKeys { .. } => "forinkeys",
            Cons::DefineKeyed { .. } => "definekeyed",
            Cons::StoreKeyed { .. } => "storekeyed",
            Cons::EnumKeyStore { .. } => "enumkeystore",
            Cons::EnumKeyAny { .. } => "enumkeyany",
            Cons::StoreIndexed { .. } => "storeindexed",
            Cons::AccessorKeyed { .. } => "accessorkeyed",
            Cons::ShareBuffer { .. } => "sharebuffer",
            Cons::ViewedBuffer { .. } => "viewedbuffer",
            Cons::ThisTo { .. } => "thisto",
            Cons::CallArgs { .. } => "callargs",
            Cons::RetInto { .. } => "retinto",
            Cons::ArgInto { .. } => "arginto",
            Cons::Spread { .. } => "spread",
            Cons::SpreadFrom { .. } => "spreadfrom",
            Cons::ThisInst { .. } => "thisinst",
            Cons::NativeThis { .. } => "nativethis",
            Cons::BuiltinCallee => "builtincallee",
            Cons::HolderStep { .. } => "holderstep",
            Cons::VisitHolders { .. } => "visitholders",
            Cons::VisitStop { .. } => "visitstop",
        }
    }
}
