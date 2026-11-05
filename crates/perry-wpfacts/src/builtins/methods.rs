//! Method calls on primitives, own objects and prototypes: the summaries a call through a key runs.

use super::*;

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
        ChainAct::Store(_) => {}
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
        unknown_method(m, call, "argument of a written primitive method");
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
            m.sink_node(src, r);
            // `process.argv = v`: later reads of the property see `v`.
            if matches!(m.toks[t as usize], TokKind::Builtin(BuiltinNs::Process))
                && match _key {
                    Key::Named(a) => m.atoms[a as usize] == "argv",
                    Key::Any(_) => true,
                }
            {
                let a = m.process_argv;
                m.edge(src, a);
            }
        }
        _ => {}
    }
}

// ------------------------------------------------------------- methods

pub fn prim_method(m: &mut Model, prim: u16, key: Key, call: CallId) {
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
        unknown_method(
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
            ret_prim(m, call, prim::UNDEF)
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
    // iteration protocol of the builtin iterables is); on anything else it
    // is a computed-key method.
    let described = match &m.toks[t as usize] {
        TokKind::Func(..) | TokKind::Bound(_) => true,
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
        // `Function.prototype.call/apply/bind` of a described builtin
        // method: its call, the receiver given.
        TokKind::Builtin(BuiltinNs::ObjectProtoOwn(_))
            if matches!(name.as_deref(), Some("call" | "apply" | "bind")) =>
        {
            function_method(m, t, name.as_deref(), call)
        }
        TokKind::Builtin(ns) => ns_method(m, ns, name.as_deref(), call),
        TokKind::Func(_, _) | TokKind::Bound(_) => function_method(m, t, name.as_deref(), call),
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
            "toString" | "toLocaleString" => {
                ret_prim(m, call, prim::STR);
            }
            "valueOf" => ret_tok(m, call, origin),
            // No function to install: TypeError before anything else.
            "__defineGetter__" | "__defineSetter__"
                if m.calls[call as usize].args.len() < 2
                    && m.calls[call as usize].spread.is_empty() => {}
            "__defineGetter__" | "__defineSetter__" => {
                let r = m.reason("E5 __defineGetter__/__defineSetter__");
                m.mark_hard(origin, Hard::E5DefineProperty, r);
                // An accessor under a key not known here.
                let star = m.atom(crate::STAR_KEY);
                let ks = m.accessor_keys.entry(origin).or_default();
                if !ks.contains(&star) {
                    ks.push(star);
                }
                m.escape(origin, r);
                // ToPropertyKey of the key; the function becomes an accessor
                // of the escaped receiver, which unknown code runs.
                let c = m.calls[call as usize].clone();
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
                let r = m.reason("E5 __defineGetter__ argument");
                for &a in c.args.iter().skip(1).take(1).chain(&c.spread) {
                    m.sink_node(a, r);
                }
                ret_prim(m, call, prim::UNDEF);
            }
            "__lookupGetter__" | "__lookupSetter__" => {
                implicit_args(m, call, "ToPropertyKey", Protocol::ToPrimitive);
                m.implicit(origin, 0, Protocol::Any);
                if origin != TOP && m.diagnostic.as_ref().is_some_and(|d| d.census.is_some()) {
                    // The unmodeled lookup creates a new unknown result;
                    // it does not copy TOP from a mixed receiver node.
                    let ret = m.calls[call as usize].ret;
                    m.add_top(ret, |_| {
                        "result of an unmodeled native accessor lookup".into()
                    });
                } else {
                    ret_tok(m, call, TOP);
                }
            }
            _ => {}
        },
        TokKind::NativeProto(name_of) => native_method(m, p, &name_of, origin, &name, call),
        _ => {}
    }
}
