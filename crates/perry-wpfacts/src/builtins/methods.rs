//! Method calls on primitives, own objects and prototypes: the summaries a call through a key runs.

use super::*;
use crate::summary::{Deferred, NativeRecv};

/// An intrinsic prototype on `origin`'s chain read or called under a key
/// that names none of its members (a number, DECISIONS 217): only a number
/// key a store into ⊤ wrote there, now or later in the solve
/// (`native_written` of `NUM_KEY`), as a program prototype gives its
/// numeric fields (`Model::num_fields`). None where the chain refuses the
/// add (`locked_chain_read` of `NUM_KEY`, DEC 250): the key is absent.
pub fn plain_key(m: &mut Model, origin: Tok, act: ChainAct) {
    let num = m.atom(NUM_KEY);
    if locked_chain_read(m, origin, num) {
        return;
    }
    match act {
        ChainAct::Load(dst) => {
            if !intrinsic_miss(m, num, NativeMiss::Load(dst)) {
                opaque_member(m, dst);
            }
        }
        ChainAct::Call(call) => {
            if !intrinsic_miss(m, num, NativeMiss::Call(origin, call)) {
                let prev = m.cur_this.replace(origin);
                unknown_method(m, call, "argument of a written intrinsic key");
                m.cur_this = prev;
            }
        }
        ChainAct::Store(_) | ChainAct::Lookup { .. } => {}
    }
}

/// `p[n](...)` on primitives of kinds `prim` under a number key: an index
/// of a string is a one-character string, of anything else absent; only a
/// number key a store into ⊤ wrote on the prototype is a method (DECISIONS 217).
pub fn prim_plain_key(m: &mut Model, prim: u16, call: CallId) {
    if prim & (prim::NULL | prim::UNDEF) == prim {
        return;
    }
    let num = m.atom(NUM_KEY);
    if !intrinsic_miss(m, num, NativeMiss::PrimCall(prim, call)) {
        unknown_prim_method(m, call, "argument of a written primitive method");
    }
}

pub fn tok_store(m: &mut Model, t: Tok, _key: Key, src: NodeId) {
    match m.toks[t as usize] {
        // Object.prototype members are reachable from every object the
        // runtime handles: unknown code may call them with any receiver.
        TokKind::ObjectProto
        | TokKind::Builtin(_)
        | TokKind::BuiltinFn(_)
        | TokKind::ModNs(_)
        | TokKind::NativeProto(_) => {
            // E20: stored where the runtime / unmodeled code reads it.
            if m.explain && !m.shared_replay {
                let kn = match _key {
                    Key::Named(a) => m.atoms[a as usize].clone(),
                    Key::Any(_) => "[computed]".into(),
                };
                eprintln!(
                    "perry: wpfacts explain: E20 store on {} key {kn}",
                    m.tok_name(t)
                );
            }
            let r = m.reason("E20 stored on a builtin/global object");
            let retained = if !m.unit_mode
                && matches!(m.toks[t as usize], TokKind::Builtin(BuiltinNs::GlobalThis))
            {
                match _key {
                    Key::Named(a) => m
                        .native_property(t, &m.atoms[a as usize])
                        .and_then(|p| p.native),
                    _ => None,
                }
                .and_then(|identity| {
                    let value = m.tok(TokKind::BuiltinFn(Native::Member { identity }));
                    match m.toks[value as usize] {
                        TokKind::Builtin(_)
                            if !m
                                .native_shape(value)
                                .is_none_or(|s| s.global_identity.matches(identity)) =>
                        {
                            None
                        }
                        _ => Some(value),
                    }
                })
            } else {
                None
            };
            if let Some(retained) = retained {
                m.cons(
                    src,
                    Cons::RuntimeStore {
                        reason: r,
                        retained,
                    },
                );
            } else {
                m.sink_node(src, r);
            }
            // `process.argv = v`: later reads of the property see `v`.
            if matches!(m.toks[t as usize], TokKind::Builtin(BuiltinNs::Process)) {
                for i in 0..m.process_arrays.len() {
                    let (k, a, _) = m.process_arrays[i];
                    if match _key {
                        Key::Named(n) => n == k,
                        Key::Any(_) => true,
                    } {
                        m.edge(src, a);
                    }
                }
            }
        }
        _ => {}
    }
}

// ------------------------------------------------------------- methods

/// Before summaries exist, a native method run waits (`Deferred::Native`).
fn native_waits(m: &mut Model, r: NativeRecv, key: Key, call: CallId) -> bool {
    if m.sum.ready {
        return false;
    }
    m.sum.deferred.push(Deferred::Native(r, key, call));
    true
}

/// The native method run a `Deferred::Native` names.
pub(crate) fn native_method_at(m: &mut Model, r: NativeRecv, key: Key, call: CallId) {
    match r {
        NativeRecv::Own(t) => own_method(m, t, key, call),
        NativeRecv::Proto(p, origin) => proto_method(m, p, origin, key, call),
        NativeRecv::Prim(prim) => prim_method(m, prim, key, call),
    }
}

pub fn prim_method(m: &mut Model, prim: u16, key: Key, call: CallId) {
    if native_waits(m, NativeRecv::Prim(prim), key, call) {
        return;
    }
    if let Key::Named(a) = key {
        if matches!(
            members::prim_member(prim, &m.atoms[a as usize]),
            Some(Mem::Fn)
        ) {
            m.diagnostic_builtin(call, |m| {
                format!("primitive({prim}).{}", m.atoms[a as usize])
            });
        }
    }

    if prim & (prim::NULL | prim::UNDEF) == prim {
        return;
    }
    let Key::Named(a) = key else {
        // A computed key may name any method of the chain.
        return prim_method_generic(m, call);
    };
    let name = m.atoms[a as usize].clone();
    // A key a store into ⊤ may have written onto the chain: the call runs
    // what was stored (a described method may be replaced too), unless the
    // read is final on every kind of the mask (DEC 250).
    let locked = members::prim_families(prim).all(|f| members::locked_read(f, None, &name));
    if !locked && !intrinsic_miss(m, a, NativeMiss::PrimCall(prim, call)) {
        unknown_prim_method(
            m,
            call,
            &format!("argument of a written primitive method {name}"),
        );
    }
    match members::prim_member(prim, &name) {
        // `Object.prototype`'s methods on a primitive (no own chain here).
        Some(Mem::Fn) if name == "valueOf" => ret_prim(m, call, prim),
        Some(Mem::Fn) if name == "toString" || name == "toLocaleString" => {
            implicit_args(m, call, "ToString radix", Protocol::ToPrimitive);
            ret_prim(m, call, prim::STR);
        }
        Some(Mem::Fn)
            if matches!(
                name.as_str(),
                "hasOwnProperty" | "propertyIsEnumerable" | "isPrototypeOf"
            ) =>
        {
            implicit_args(m, call, "ToPropertyKey", Protocol::ToPrimitive);
            ret_prim(m, call, prim::BOOL);
        }
        // Defines on the wrapper, which is discarded; nothing of the
        // program runs.
        Some(Mem::Fn) if name == "__defineGetter__" || name == "__defineSetter__" => {}
        Some(Mem::Fn) if name == "__lookupGetter__" || name == "__lookupSetter__" => {
            annexb_lookup(m, None, call, name == "__lookupSetter__");
        }
        Some(Mem::Fn) if string_methods::string_method(m, call, &name) => {}
        Some(Mem::Fn) => prim_method_generic(m, call),
        // A data member is no function: TypeError.
        Some(_) => {}
        // Absent: a TypeError, unless written (above).
        None => {}
    }
}

/// A method of a primitive's chain the model does not describe one by one:
/// callbacks are called by the runtime with primitives (replace), object
/// arguments are coerced, the result is a primitive or a container of them.
pub(super) fn prim_method_generic(m: &mut Model, call: CallId) {
    implicit_args(m, call, "argument of a primitive method", Protocol::Any);
    ret_prim(m, call, prim::ALL);
    let pb = site_tmpl(m, call, "primbox");
    ret_tok(m, call, pb);
}

/// `name` called on `t`, a builtin object of family `fam`, where the model
/// describes no such method: a member of the family's own intrinsic
/// prototype runs as an unknown method (`why`: the arguments and the
/// receiver escape, the result is ⊤); a member of `Object.prototype` runs
/// through the chain (`proto_method`); a data member is no function; a name
/// no intrinsic of the chain defines is absent (a TypeError), unless it may
/// have been written onto a builtin prototype.
pub(super) fn undescribed_method(
    m: &mut Model,
    fam: Family,
    t: Tok,
    name: &str,
    call: CallId,
    why: &str,
) {
    match members::own(fam, name) {
        Some(Mem::Fn) if fam == Family::Wrapper => prim_method_generic(m, call),
        Some(Mem::Fn) => unknown_method(m, call, why),
        Some(_) => {}
        None if members::member(Family::Object, name).is_some() => {}
        None => {
            let a = m.atom(name);
            if !intrinsic_unwritten(m, fam, None, a, NativeMiss::Call(t, call)) {
                unknown_method(m, call, why);
            }
        }
    }
}

/// Methods on builtin tokens (the receiver's own builtin behaviour).
pub fn own_method(m: &mut Model, t: Tok, key: Key, call: CallId) {
    if native_waits(m, NativeRecv::Own(t), key, call) {
        return;
    }
    let prev = m.cur_this.replace(t);
    own_method_inner(m, t, key, call);
    m.cur_this = prev;
}

pub(super) fn own_method_inner(m: &mut Model, t: Tok, key: Key, call: CallId) {
    let name = match key {
        Key::Named(a) => Some(m.atoms[a as usize].clone()),
        Key::Any(_) => None,
    };
    // A builtin function's `constructor` is the Function constructor (E14);
    // a computed key may name it.
    if let TokKind::Builtin(ns) = m.toks[t as usize] {
        if members::ns_chain(ns) == Family::Function && m.key_may_name_str(key, "constructor") {
            return call_builtin_ns(m, BuiltinNs::Function, call);
        }
    }
    // A well-known-symbol method is a named member of the intrinsic chain
    // where the chain is described (a function value, an array, a described
    // builtin object: absent unless the description names it, as the
    // iteration protocol of the builtin iterables is; a builtin namespace
    // whose own keys and chain are described, `Object[Symbol.iterator]`
    // included); on anything else it is a computed-key method.
    let described = match &m.toks[t as usize] {
        TokKind::Func(..) | TokKind::Bound(_) | TokKind::ClassObj(_) => true,
        TokKind::Builtin(ns) => name
            .as_deref()
            .is_some_and(|n| n.starts_with(WK_PREFIX) && members::ns_member(*ns, n).is_some()),
        TokKind::Obj(tm, _) => match &m.tmpls[*tm as usize].kind {
            TmplKind::Arr { .. } => true,
            TmplKind::BObj { what, .. } => {
                members::bobj_family(what) != Family::Opaque
                    || matches!(
                        *what,
                        "Date" | "TypedArray" | "URL" | "URLSearchParams" | "TextCodec" | "Timer"
                    )
                    || (*what == "RegExp"
                        && name.as_deref().is_some_and(|n| {
                            string_methods::regexp_described(n) || collections::regexp_species(n)
                        }))
            }
            _ => false,
        },
        _ => false,
    };
    let name = match name {
        Some(n) if n.starts_with(WK_PREFIX) && !described => None,
        n => n,
    };
    // A described receiver's method under a key a store into ⊤ may have
    // written onto its chain runs what was stored, beside the description,
    // unless the read is final (DEC 250).
    if let (true, Some(n), Key::Named(a)) = (described, name.as_deref(), key) {
        if !locked_chain_read(m, t, a) && !intrinsic_miss(m, a, NativeMiss::Call(t, call)) {
            unknown_method(m, call, &format!("argument of a written method {n}"));
        }
    }
    match m.toks[t as usize].clone() {
        // Inherited Annex B methods run through the prototype-chain
        // summary. An opaque own-method summary beside it would also
        // hand the installed callback to unknown code.
        TokKind::Builtin(ns)
            if name.as_deref().is_some_and(|n| {
                matches!(n, "__defineGetter__" | "__defineSetter__")
                    && matches!(members::ns_member(ns, n), Some(Some(Mem::Fn)))
            }) => {}
        TokKind::BuiltinFn(_)
            if matches!(
                name.as_deref(),
                Some("__defineGetter__" | "__defineSetter__")
            ) => {}
        // `Function.prototype.call/apply/bind` of a described builtin
        // method: its call, the receiver given.
        TokKind::Builtin(BuiltinNs::ObjectProtoOwn(_))
            if matches!(name.as_deref(), Some("call" | "apply" | "bind")) =>
        {
            function_method(m, t, name.as_deref(), call)
        }
        TokKind::Builtin(ns) => ns_method(m, ns, name.as_deref(), call),
        // A class object is a function value: its chain ends at
        // `%Function.prototype%`, whose `bind` makes a bound function of
        // the class (`new` of it constructs the class) and whose
        // `call`/`apply` call it (a class called without `new` throws).
        TokKind::Func(_, _) | TokKind::Bound(_) | TokKind::ClassObj(_) => {
            function_method(m, t, name.as_deref(), call)
        }
        // A builtin function value's methods are `Function.prototype`'s
        // (and `Object.prototype`'s): what one does with its arguments is
        // unknown (`push.call(o, v)` stores). Any other name is absent.
        // `Function.prototype.call/apply/bind` of a named native: the
        // native runs with the receiver given, as a program function does
        // (unless the program wrote the key onto `Function.prototype`).
        TokKind::BuiltinFn(Native::Member { .. })
            if matches!(name.as_deref(), Some("call" | "apply" | "bind")) =>
        {
            let Key::Named(a) = key else { unreachable!() };
            let n = name.as_deref().unwrap_or_default();
            if !locked_chain_read(m, t, a) && !intrinsic_miss(m, a, NativeMiss::Call(t, call)) {
                unknown_method(m, call, &format!("argument of a written method {n}"));
            }
            function_method(m, t, Some(n), call)
        }
        TokKind::BuiltinFn(_) => match name.as_deref() {
            None => {}
            Some(n) => {
                let why = format!("method {n} of an opaque builtin function");
                match members::member(Family::Function, n) {
                    Some(Mem::Fn) => unknown_method_proto(m, call, &why, None, true),
                    Some(_) => {}
                    None => {
                        let a = m.atom(n);
                        if !intrinsic_miss(m, a, NativeMiss::Call(t, call)) {
                            unknown_method_proto(m, call, &why, None, true);
                        }
                    }
                }
            }
        },
        TokKind::NativeProto(_) => {}
        // `Object.prototype`'s own members are its methods: called on it
        // they run with it as the receiver (the chain walk starts past it,
        // at its null prototype, and finds none of them).
        TokKind::ObjectProto => proto_method(m, t, t, key, call),
        TokKind::Obj(tm, _) => match m.tmpls[tm as usize].kind.clone() {
            TmplKind::Arr { .. } => array_method(m, t, name.as_deref(), call),
            TmplKind::BObj { what, .. } => bobj_method(m, t, what, name.as_deref(), call),
            _ => {}
        },
        _ => {}
    }
}

/// Builtin methods found on the prototype chain (`Object.prototype` and
/// native parent prototypes), run with `this` = `origin`.
pub fn proto_method(m: &mut Model, p: Tok, origin: Tok, key: Key, call: CallId) {
    if native_waits(m, NativeRecv::Proto(p, origin), key, call) {
        return;
    }
    if matches!(m.toks[p as usize], TokKind::ObjectProto) {
        if let Key::Named(a) = key {
            if matches!(
                members::member(Family::Object, &m.atoms[a as usize]),
                Some(Mem::Fn)
            ) {
                m.diagnostic_builtin(call, |m| {
                    format!("Object.prototype.{}", m.atoms[a as usize])
                });
            }
        }
    }
    let prev = m.cur_this.replace(origin);
    proto_method_inner(m, p, origin, key, call);
    m.cur_this = prev;
}

/// A call of `BuiltinNs::ObjectProtoMethod`: any method of
/// `Object.prototype`. On a receiver it is each method's call on it
/// (`proto_method`). Without one, `hasOwnProperty` / `propertyIsEnumerable`
/// convert their key before they throw, `isPrototypeOf` of a primitive is
/// `false`, `toString` is `"[object Undefined]"`; the others throw
/// (ToObject of `undefined`).
pub(super) fn object_proto_method_call(m: &mut Model, call: CallId) {
    match (m.cur_this, m.calls[call as usize].this.clone()) {
        (Some(o), _) => {
            let op = m.tok(TokKind::ObjectProto);
            for name in OBJECT_PROTO_METHODS {
                let a = m.atom(name);
                proto_method(m, op, o, Key::Named(a), call);
            }
        }
        (None, ThisArg::Undef) => {
            if let Some(k) = arg(m, call, 0) {
                let reason = m.reason("ToPropertyKey");
                m.cons(
                    k,
                    Cons::Implicit {
                        reason,
                        proto: Protocol::ToPrimitive,
                    },
                );
            }
            ret_prim(m, call, prim::STR | prim::BOOL);
        }
        // A receiver node no token stands for: its values are unknown here.
        (None, _) => unknown_method(m, call, "call of an Object.prototype method"),
    }
}

/// A call of `BuiltinNs::ObjectProtoOwn(own)`, whatever its receiver
/// (ToObject(this) throws on `undefined`/`null`, reads nothing else):
/// `hasOwnProperty` / `propertyIsEnumerable` convert their key and answer
/// a boolean; `isPrototypeOf` answers a boolean (its argument is only
/// walked: no conversion).
pub(super) fn object_proto_own_call(m: &mut Model, own: crate::model::ProtoOwn, call: CallId) {
    if own != crate::model::ProtoOwn::IsPrototypeOf {
        // The key only: further arguments are ignored.
        let c = m.calls[call as usize].clone();
        let r = m.reason("ToPropertyKey");
        for &k in c.args.iter().take(1).chain(c.spread.iter()) {
            m.cons(
                k,
                Cons::Implicit {
                    reason: r,
                    proto: Protocol::ToPrimitive,
                },
            );
        }
    }
    ret_prim(m, call, prim::BOOL);
}

/// `Object.prototype`'s methods, one per distinct summary in
/// `proto_method_inner`: `propertyIsEnumerable` / `isPrototypeOf` run as
/// `hasOwnProperty` does, `toLocaleString` as `toString`, the setter forms
/// as the getter forms.
pub(super) const OBJECT_PROTO_METHODS: [&str; 5] = [
    "hasOwnProperty",
    "toString",
    "valueOf",
    "__defineGetter__",
    "__lookupGetter__",
];

pub(super) fn proto_method_inner(m: &mut Model, p: Tok, origin: Tok, key: Key, call: CallId) {
    let name = match key {
        Key::Named(a) => m.atoms[a as usize].clone(),
        Key::Any(_) => return,
    };
    if name.starts_with(WK_PREFIX) {
        return;
    }
    match m.toks[p as usize].clone() {
        TokKind::ObjectProto => match name.as_str() {
            "hasOwnProperty" | "propertyIsEnumerable" | "isPrototypeOf" => {
                implicit_args(m, call, "ToPropertyKey", Protocol::ToPrimitive);
                ret_prim(m, call, prim::BOOL);
            }
            // 20.1.3.6 step 15: `Get(O, @@toStringTag)` runs a getter there;
            // what it returns is only shown.
            "toString" => {
                let key = Key::Named(m.atom(&format!("{WK_PREFIX}toStringTag")));
                let recv = m.node_l("tostringtag-recv");
                let dst = m.node_l("tostringtag-read");
                m.nodes[dst as usize].sink = true;
                m.cons(recv, Cons::Load { key, dst });
                m.add_tok(recv, origin);
                ret_prim(m, call, prim::STR);
            }
            // 20.1.3.5: `Invoke(O, "toString")`. ToPrimitive's calls include
            // it (they also look up `valueOf` and `@@toPrimitive`: more can
            // run, never less).
            "toLocaleString" => {
                let r = m.reason("ToPrimitive of a toLocaleString receiver");
                m.implicit(origin, r, Protocol::ToPrimitive);
                ret_prim(m, call, prim::STR);
            }
            "valueOf" => ret_tok(m, call, origin),
            // No function to install: TypeError before anything else.
            "__defineGetter__" | "__defineSetter__"
                if m.calls[call as usize].args.len() < 2
                    && m.calls[call as usize].spread.is_empty() => {}
            "__defineGetter__" | "__defineSetter__" => {
                annexb_define(m, origin, call, name == "__defineSetter__");
                ret_prim(m, call, prim::UNDEF);
            }
            "__lookupGetter__" | "__lookupSetter__" => {
                annexb_lookup(m, Some(origin), call, name == "__lookupSetter__");
            }
            _ => {}
        },
        TokKind::NativeProto(name_of) => native_method(m, p, &name_of, origin, &name, call),
        _ => {}
    }
}

/// Annex B defines a descriptor; only an unmodeled receiver keeps the
/// reflective escape fallback. Known holders use the descriptor accessor flow.
pub(crate) fn annexb_define(m: &mut Model, origin: Tok, call: CallId, setter: bool) {
    let c = m.calls[call as usize].clone();
    if c.args.len() < 2 && c.spread.is_empty() {
        return;
    }
    if let Some(&k) = c.args.first() {
        let reason = m.reason("ToPropertyKey");
        m.cons(
            k,
            Cons::Implicit {
                reason,
                proto: Protocol::ToPrimitive,
            },
        );
    }
    let key = c
        .spread
        .is_empty()
        .then(|| m.call_lit.get(&call).copied())
        .flatten()
        .unwrap_or_else(|| m.atom(crate::STAR_KEY));
    // Object.prototype is a modeled holder: the define records the accessor
    // there as defineProperty does (an accessor on it is E23), under a
    // computed key too (the AccessorInstall any-key branch).
    if accessor_receiver(m, origin, key) || matches!(m.toks[origin as usize], TokKind::ObjectProto)
    {
        let holder = m.node_l("annexb-holder");
        m.add_tok(holder, origin);
        // Spread positions are not retained in CallSite: any direct or
        // spread value may be argument 1, and its key is not known here.
        let direct = if c.spread.is_empty() {
            &c.args[1..2]
        } else {
            &c.args[..]
        };
        for &f in direct {
            m.cons(
                f,
                Cons::Accessor {
                    holder,
                    key,
                    setter,
                },
            );
        }
        for &spread in &c.spread {
            let f = m.node_l("annexb-spread-function");
            let key = m.atom(crate::STAR_KEY);
            let elem = m.elem_key();
            m.cons(spread, Cons::Load { key: elem, dst: f });
            m.cons(
                f,
                Cons::Accessor {
                    holder,
                    key,
                    setter,
                },
            );
        }
    } else {
        let r = m.reason("E5 __defineGetter__/__defineSetter__");
        m.mark_hard(origin, Hard::E5DefineProperty, r);
        let star = m.atom(crate::STAR_KEY);
        let ks = m.accessor_keys.entry(origin).or_default();
        if !ks.contains(&star) {
            ks.push(star);
        }
        m.escape(origin, r);
        let r = m.reason("E5 __defineGetter__ argument");
        for &f in c.args.iter().skip(1).take(1).chain(&c.spread) {
            m.sink_node(f, r);
        }
    }
}

/// Receiver shapes for which descriptor accessors have modeled holder groups.
pub(crate) fn accessor_receiver(m: &Model, t: Tok, key: Atom) -> bool {
    !m.is_marker(t)
        && (matches!(
            m.toks[t as usize],
            TokKind::Obj(..) | TokKind::Func(..) | TokKind::Proto(_) | TokKind::ClassObj(_)
        ) || super::undeclared(m, t, key))
}

/// Callable values whose bodies the accessor groups cannot bind precisely.
pub(crate) fn unknown_accessor_callable(m: &Model, t: Tok) -> bool {
    match m.toks[t as usize] {
        TokKind::Top | TokKind::BuiltinFn(_) | TokKind::Bound(_) | TokKind::ClassObj(_) => true,
        TokKind::Builtin(ns) => members::ns_chain(ns) == Family::Function,
        _ => false,
    }
}
