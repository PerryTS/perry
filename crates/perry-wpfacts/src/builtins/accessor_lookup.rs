//! Annex B descriptor queries return accessor function values, without [[Get]].
use super::*;

/// The getters and setters of the realm's own accessor properties, each the
/// accessor its descriptor query read: an ordinary built-in function object
/// that is no constructor and has no realm declaration, so it is no other
/// builtin. Its fields are its own; a call of it runs `realm_accessor_call`.
/// The getters are one value, the setters two: the `__proto__` setter of
/// `Object.prototype` (20.1.3.8.2), the only one that relinks (perry-runtime
/// property_ops/common.rs `legacy_dunder_proto_set`), and every other one.
/// A query names the `__proto__` setter only under the key `__proto__`; a key
/// of unknown string never names it (DECISIONS 217, `is_proto_key`).
const REALM_GETTER: &str = "%RealmAccessor.get%";
const REALM_SETTER: &str = "%RealmAccessor.set%";
const REALM_PROTO_SETTER: &str = "%RealmAccessor.__proto__.set%";

/// `identity` names one of the realm's own accessor functions.
pub(crate) fn is_realm_accessor(identity: &str) -> bool {
    matches!(identity, REALM_GETTER | REALM_SETTER | REALM_PROTO_SETTER)
}

/// A call of a realm accessor with receiver `r` (`None`: `this` is
/// `undefined`). Each one is a built-in function whose steps the realm
/// fixes: a getter reads internal slots of `this` (`Map`/`Set` size,
/// 24.1.3.10 / 24.2.3.9; `ArrayBuffer` byteLength 25.1.6.2; the TypedArray
/// and DataView views 23.2.3 / 25.3.4; the RegExp flag getters 22.2.6;
/// `Symbol.prototype.description` 20.4.3.2), does `this.[[GetPrototypeOf]]()`
/// (`__proto__` 20.1.3.8.1), [[Get]]s the flag keys of `this`
/// (`RegExp.prototype.flags` 22.2.6.4) or returns `this` (the @@species
/// getters, 24.1.2.2); a setter takes its one argument: `__proto__` does
/// `this.[[SetPrototypeOf]](proto)` (20.1.3.8.2), the crypto streams'
/// `_readableState` / `_writableState` define it under their key, the legacy RegExp statics do
/// ToString of it (Annex B). A receiver of the wrong brand throws a
/// TypeError (an Error the realm makes). No step hands `this` or an argument
/// to code the realm does not name, so neither escapes: `this` is read by
/// [[Get]] under any key (whose program getters run on it), its prototype is
/// read and, with an argument, replaced (the `Object.setPrototypeOf`
/// predicate: E6 and the prototype edge), and the argument may become its
/// `_readableState` / `_writableState` data property. No getter returns a value it reads from `this`: the
/// result is an internal-slot primitive, `this` (@@species), its prototype
/// (`__proto__`), a string (`flags`), or a host value no receiver decides
/// (the lazy global getters return a host module object): ⊤.
pub(crate) fn realm_accessor_call(m: &mut Model, identity: &str, r: Option<Tok>, call: CallId) {
    let c = m.calls[call as usize].clone();
    m.add_prim(c.ret, prim::ALL);
    m.add_top(c.ret, |_| "result of a realm accessor: a host value".into());
    // A getter takes no argument (extra ones are never read); a setter its
    // first.
    let proto_setter = identity == REALM_PROTO_SETTER;
    let has_arg = identity != REALM_GETTER && (!c.args.is_empty() || !c.spread.is_empty());
    let v = m.node_l("realm-accessor-arg");
    if let Some(&a) = c.args.first() {
        m.edge(a, v);
    }
    for &s in &c.spread {
        let key = m.elem_key();
        m.cons(s, Cons::Load { key, dst: v });
    }
    if has_arg {
        let reason = m.reason("ToString of a realm setter argument");
        m.cons(
            v,
            Cons::Implicit {
                reason,
                proto: Protocol::ToPrimitive,
            },
        );
    }
    let Some(r) = r else {
        return;
    };
    let n = m.node_l("realm-accessor-this");
    m.add_tok(n, r);
    m.add_tok(c.ret, r);
    // `RegExp.prototype.flags` [[Get]]s the flag keys of `this` (program
    // getters run on it) and only applies ToBoolean to them (22.2.6.4): the
    // values reach no result.
    let got = m.node_l("realm-accessor-get");
    m.cons(
        n,
        Cons::Load {
            key: Key::any(),
            dst: got,
        },
    );
    m.cons(n, Cons::ProtoOf { dst: c.ret });
    m.add_prim(c.ret, prim::NULL);
    if has_arg && proto_setter {
        let reason = m.reason("E6 __proto__ setter");
        m.cons(
            n,
            Cons::Hard {
                kind: Hard::E6SetPrototypeOf,
                reason,
            },
        );
        m.cons(n, Cons::ProtoFrom { src: v });
    }
    if has_arg && !proto_setter {
        // One realm setter makes its argument a property of `this`: the
        // `_readableState` / `_writableState` setters of the crypto stream
        // prototypes define it as a data property under their own key
        // (perry-runtime node_stream/native_hooks.rs `lazy_state_set`).
        // No other does: `__proto__` (above) only relinks (property_ops/common.rs
        // `legacy_dunder_proto_set`); `localStorage`/`sessionStorage`
        // (web_storage.rs `storage_global_setter`), `Module.prototype.parent`
        // (module_cjs.rs) and the `fs` stream exports
        // (callable_exports.rs `fs_namespace_descriptor_setter_thunk`)
        // ignore it; the stream state setters (node_stream/proto_methods.rs
        // `stream_property_set`) write the native state record; the http
        // setters (node_http_prototypes.rs `http_setter_thunk`) reach a
        // native handle only; the `EventEmitter` statics, URL, Event and
        // the `on*` handler setters (event_target/prototype.rs
        // `event_handler_set`) keep it in captured state or internal slots.
        // So the argument is stored under exactly those two keys, never
        // under any key.
        for name in ["_readableState", "_writableState"] {
            let key = Key::Named(m.atom(name));
            let site = m.anon_post_site("realm accessor setter (lazy stream state)");
            m.cons(n, Cons::Store { key, src: v, site });
        }
    }
}

/// The same transfer serves intrinsic dispatch and the direct runtime lowering.
/// `None` is a primitive wrapper: its native descriptors are not represented by
/// program accessor records, so its result remains conservative.
pub(crate) fn annexb_lookup(m: &mut Model, origin: Option<Tok>, call: CallId, setter: bool) {
    let c = m.calls[call as usize].clone();
    let reason = m.reason("ToPropertyKey");
    for &key in c.args.iter().take(1) {
        m.cons(
            key,
            Cons::Implicit {
                reason,
                proto: Protocol::ToPrimitive,
            },
        );
    }
    for &spread in &c.spread {
        let keys = m.node_l("accessor-lookup-spread-key");
        let key = m.elem_key();
        m.cons(spread, Cons::Load { key, dst: keys });
        m.cons(
            keys,
            Cons::Implicit {
                reason,
                proto: Protocol::ToPrimitive,
            },
        );
    }
    m.add_prim(c.ret, prim::UNDEF);
    let Some(origin) = origin else {
        m.add_top(c.ret, |_| {
            "accessor descriptor on a primitive wrapper".into()
        });
        return;
    };
    if origin == TOP || (m.is_marker(origin) && !m.virt_protos.contains_key(&origin)) {
        m.implicit(origin, reason, Protocol::Any);
        m.add_top(c.ret, |_| {
            "accessor descriptor on an unknown receiver".into()
        });
        return;
    }
    let key = if c.spread.is_empty() {
        match m.call_lit.get(&call).copied() {
            Some(a) => Key::Named(a),
            None if c.args.is_empty() => Key::Named(m.atom("undefined")),
            None => Key::any(),
        }
    } else {
        Key::any()
    };
    // B.2.2.4/5: an own accessor of a holder on the chain answers its
    // [[Get]]/[[Set]], a data property or the end of the chain `undefined`.
    // The realm's own accessors answer only from the intrinsic holders the
    // walk passes (`lookup_native_descriptor`): those on `origin`'s chain.
    let act = ChainAct::Lookup { dst: c.ret, setter };
    m.accessor_walk(origin, origin, key, act);
    m.builtin_field_use(origin, key, act);
}

/// Native member descriptions do not distinguish data from accessor descriptors.
/// A described member can therefore return an unknown function; its value and
/// unrelated member functions are never read or invoked by a descriptor query.
pub(crate) fn lookup_native_descriptor(
    m: &mut Model,
    origin: Tok,
    holder: Tok,
    key: Key,
    dst: NodeId,
    setter: bool,
) {
    let Key::Named(a) = key else {
        native_accessor_any(m, origin, holder, dst, setter);
        return;
    };
    let name = m.atoms[a as usize].clone();
    let family = match m.toks[holder as usize] {
        TokKind::ObjectProto => Some(seed_family(m, origin).unwrap_or(Family::Object)),
        TokKind::BuiltinFn(Native::Any) => {
            m.add_top(dst, |_| "unrepresented native accessor descriptor".into());
            None
        }
        TokKind::BuiltinFn(_) => Some(Family::Function),
        TokKind::Builtin(ns) => {
            if members::ns_member(ns, &name) != Some(None) || native_accessor_written(m, holder, a)
            {
                m.add_top(dst, |_| "unrepresented native accessor descriptor".into());
            } else {
                native_accessor_known(m, holder, a, dst, setter);
                m.native_missing
                    .push((holder, a, NativeMiss::Lookup(dst, setter)));
            }
            None
        }
        TokKind::NativeProto(_, ns) => ns
            .and_then(loads::proto_family)
            .map(|(f, _, _)| f)
            .or(Some(Family::Opaque)),
        _ => return,
    };
    if let Some(family) = family {
        // A described family declares its members' kinds: a function member
        // and `constructor` are data properties (B.2.2.4: the query answers
        // `undefined`); `__proto__` (20.1.3.8) and a primitive member may be
        // one of the realm's own accessors. A family without a member table
        // (Box, Wrapper, Opaque) may hold any accessor.
        let unknown = match members::member(family, &name) {
            None => false,
            Some(_) if !members::described(family) => true,
            Some(members::Mem::Proto | members::Mem::Prim(_)) => {
                realm_accessor_into(m, dst, setter && name == "__proto__", setter);
                false
            }
            Some(members::Mem::Fn | members::Mem::Ctor) => false,
        };
        if unknown || !intrinsic_unwritten(m, family, None, a, NativeMiss::Lookup(dst, setter)) {
            m.add_top(dst, |_| "unrepresented native accessor descriptor".into());
        }
    }
}

/// The realm accessor a descriptor query reads, into `dst`: a setter
/// query under `__proto__` (`proto`) the `__proto__` setter.
fn realm_accessor_into(m: &mut Model, dst: NodeId, proto: bool, setter: bool) {
    let f = m.tok(TokKind::BuiltinFn(Native::Member {
        identity: match (proto, setter) {
            (true, _) => REALM_PROTO_SETTER,
            (false, true) => REALM_SETTER,
            (false, false) => REALM_GETTER,
        },
        holder: None,
    }));
    m.add_tok(dst, f);
}

/// A descriptor query under a key that may name any property, on an
/// intrinsic holder of the chain: program accessors installed on a modeled
/// holder come from the walk itself (`accessor_hit`); the realm's own
/// accessors are the realm's (`annexb_lookup`), not the holder's. Only a
/// write the model does not place on the holder (through ⊤, or onto it under
/// any key) may have installed a program function: that write answers ⊤,
/// now or when it arrives (`NativeMiss::Lookup`).
fn native_accessor_any(m: &mut Model, origin: Tok, holder: Tok, dst: NodeId, setter: bool) {
    // The realm's own accessors on this holder: those its declaration may
    // hold under a key of unknown string. Such a key never names `__proto__`
    // (DECISIONS 217, `is_proto_key`), so `Object.prototype` holds none; a
    // primitive member may be an accessor (`Map.prototype.size`); a family
    // without a member table or a namespace may hold any.
    let family = match m.toks[holder as usize] {
        TokKind::BuiltinFn(Native::Any) => {
            m.add_top(dst, |_| "unrepresented native accessor descriptor".into());
            return;
        }
        TokKind::ObjectProto => Some(seed_family(m, origin).unwrap_or(Family::Object)),
        TokKind::BuiltinFn(_) => Some(Family::Function),
        TokKind::NativeProto(ref n, ..) => Some(
            ns_by_name(n)
                .and_then(loads::proto_family)
                .map(|(f, _, _)| f)
                .unwrap_or(Family::Opaque),
        ),
        TokKind::Builtin(_) => None,
        _ => return,
    };
    if family.is_none_or(|f| members::any_member(f).1 != 0) {
        realm_accessor_into(m, dst, false, setter);
    }
    let star = m.atom(crate::STAR_KEY);
    if native_accessor_written(m, holder, star) {
        m.add_top(dst, |_| {
            "accessor descriptor of a written intrinsic key".into()
        });
        return;
    }
    native_accessor_known(m, holder, star, dst, setter);
    m.native_missing
        .push((holder, star, NativeMiss::Lookup(dst, setter)));
}
