//! Annex B descriptor queries return accessor function values, without [[Get]].
use super::*;

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
    if key.is_any() {
        m.add_top(c.ret, |_| "accessor descriptor under an unknown key".into());
    }
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
) {
    let Key::Named(a) = key else { return };
    let name = m.atoms[a as usize].clone();
    let family = match m.toks[holder as usize] {
        TokKind::ObjectProto => Some(seed_family(m, origin).unwrap_or(Family::Object)),
        TokKind::BuiltinFn(Native::Any) => {
            m.add_top(dst, |_| "unrepresented native accessor descriptor".into());
            None
        }
        TokKind::BuiltinFn(_) => Some(Family::Function),
        TokKind::Builtin(ns) => {
            if members::ns_member(ns, &name) != Some(None) || native_written(m, holder, a) {
                m.add_top(dst, |_| "unrepresented native accessor descriptor".into());
            } else {
                m.native_missing.push((holder, a, NativeMiss::Lookup(dst)));
            }
            None
        }
        TokKind::NativeProto(ref n) => ns_by_name(n)
            .and_then(loads::proto_family)
            .map(|(f, _, _)| f)
            .or(Some(Family::Opaque)),
        _ => return,
    };
    if let Some(family) = family {
        if members::member(family, &name).is_some()
            || !intrinsic_unwritten(m, family, None, a, NativeMiss::Lookup(dst))
        {
            m.add_top(dst, |_| "unrepresented native accessor descriptor".into());
        }
    }
}
