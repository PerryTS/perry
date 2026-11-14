//! VALUE-CALL-MODELS: the steps of `Object.create` (ECMA-262 20.1.2.2),
//! `defineProperties` (20.1.2.3), `defineProperty` (20.1.2.4),
//! `fromEntries` (20.1.2.7), `getOwnPropertyDescriptor(s)` (20.1.2.9,
//! 20.1.2.10), `groupBy` (20.1.2.13) and `setPrototypeOf` (20.1.2.23,
//! 28.1.13) on argument NODES. The direct call (`gen_rest`) and a call of the
//! builtin as a VALUE (`ns_method_inner`) both use them: a value call runs
//! the same steps, it is not unknown code. A value call's key and descriptor
//! are nodes, so it takes the direct call's form for a computed key and a
//! descriptor that is not a literal written in place. A missing argument is
//! `undefined`; a step that throws on it (ToObject, ToPropertyDescriptor,
//! GetIterator of undefined) adds no value.

use super::collections::{cb_call, iterate};
use super::*;

/// E10: `p` is the [[Prototype]] of the object `t` (OrdinaryObjectCreate).
pub(crate) fn create_proto(m: &mut Model, p: NodeId, t: Tok, reason: u32) {
    m.cons(p, Cons::CreateProto { reason });
    let pn = m.proto_node(t);
    m.edge(p, pn);
}

/// E6: `pn` becomes the [[Prototype]] of what `on` holds.
pub(crate) fn set_prototype_of(m: &mut Model, on: NodeId, pn: NodeId, reason: u32) {
    m.cons(
        on,
        Cons::Hard {
            kind: Hard::E6SetPrototypeOf,
            reason,
        },
    );
    m.cons(on, Cons::ProtoFrom { src: pn });
}

/// The [[Get]]s ToPropertyDescriptor makes on `desc` (`DESCRIPTOR_KEYS`);
/// returns the nodes of `value`, `get` and `set`. `node` makes a load's
/// destination.
pub(crate) fn descriptor_reads(
    m: &mut Model,
    desc: NodeId,
    mut node: impl FnMut(&mut Model) -> NodeId,
) -> [NodeId; 3] {
    let mut used = [desc; 3];
    for name in DESCRIPTOR_KEYS {
        let a = m.atom(name);
        let v = node(m);
        m.cons(desc, Cons::Load { key: Key::Named(a), dst: v });
        match name {
            "value" => used[0] = v,
            "get" => used[1] = v,
            "set" => used[2] = v,
            _ => {}
        }
    }
    used
}

/// `defineProperties(on, pn)`: each own value of the map is a descriptor
/// (`descs`, read as `reads`), defined under a computed key.
#[allow(clippy::too_many_arguments)]
pub(crate) fn define_all(
    m: &mut Model,
    on: NodeId,
    pn: NodeId,
    descs: NodeId,
    reads: [NodeId; 3],
    site: SiteId,
    reason: u32,
) {
    let [value, dget, dset] = reads;
    let star = m.atom(crate::STAR_KEY);
    for (f, setter) in [(dget, false), (dset, true)] {
        m.cons(
            f,
            Cons::Accessor {
                holder: on,
                key: star,
                setter,
            },
        );
    }
    m.cons(
        pn,
        Cons::DefineAll {
            o: on,
            descs,
            value,
            site,
            reason,
        },
    );
}

/// The descriptor object `t` of an own property of `o`: its value, getter
/// or setter (`vals`, every own value), the accessors handed out.
pub(crate) fn own_descriptor(m: &mut Model, o: NodeId, vals: NodeId, t: Tok) {
    let r = m.reason("getOwnPropertyDescriptor (accessors handed out)");
    m.cons(
        o,
        Cons::Implicit {
            reason: r,
            proto: Protocol::Any,
        },
    );
    m.cons(o, Cons::Load { key: Key::any(), dst: vals });
    let fa = m.field_any(t);
    m.edge(vals, fa);
    // get/set functions of the object: handed to user code without a
    // receiver — ⊤-called.
    m.add_tok(fa, TOP);
    m.add_prim(fa, prim::ALL);
}

/// `fromEntries`: each entry `it`'s `"0"` and `"1"` (`inner`, a [[Get]]
/// under a number key) are the values of the object `t`.
pub(crate) fn from_entries(m: &mut Model, it: NodeId, inner: NodeId, t: Tok) {
    let num = Key::Named(m.atom(crate::model::NUM_KEY));
    m.cons(it, Cons::Load { key: num, dst: inner });
    let fa = m.field_any(t);
    m.edge(inner, fa);
}

/// `groupBy`: the key `k` the callback returned (ToPropertyKey) and the
/// group arrays `group` are the values of the result `t`.
pub(crate) fn group_by(m: &mut Model, k: NodeId, group: NodeId, t: Tok) {
    let why = m.reason("groupBy key");
    m.cons(
        k,
        Cons::Implicit {
            reason: why,
            proto: Protocol::ToPrimitive,
        },
    );
    let fa = m.field_any(t);
    m.edge(group, fa);
    m.edge(k, fa);
}

fn node(m: &mut Model, call: CallId, what: &'static str) -> NodeId {
    let n = m.node_l(what);
    m.nodes[n as usize].frame = m.call_run(call);
    n
}

fn arg_or_undef(m: &mut Model, call: CallId, i: usize) -> NodeId {
    if let Some(a) = arg(m, call, i) {
        return a;
    }
    let n = node(m, call, "undefined-arg");
    m.add_prim(n, prim::UNDEF);
    n
}

/// The `Object.create` object born at `call`'s source site.
fn site_obj_create(m: &mut Model, call: CallId) -> Tok {
    let site = m.calls[call as usize].site;
    let key = (site, "Object.create", false);
    let tm = match m.site_tmpls.get(&key) {
        Some(&t) => m.tmpl_of(t).expect("site template"),
        None => {
            let tm = m.tmpls.len() as TmplId;
            m.tmpls.push(Template {
                kind: TmplKind::ObjCreate {
                    module: u32::MAX,
                    site: site as SiteId,
                },
                display: format!("Object.create@call{site}"),
                birth_keys: vec![],
                definite: vec![],
                declared: Vec::new(),
                reserved: Vec::new(),
                parent: None,
                ctor: None,
                native_parent: None,
                accessor_keys: vec![],
                has_computed_members: true,
                field_init: None,
                proto_members: vec![],
                parent_value: None,
                parent_links: Vec::new(),
                null_proto: false,
                chain_miss: false,
                chain_miss_open: false,
                species: None,
                per_evaluation: false,
                static_methods: Vec::new(),
            });
            let t = m.tok(TokKind::Obj(tm, NO_CTX));
            m.site_tmpls.insert(key, t);
            tm
        }
    };
    let ctx = m.birth_ctx(call);
    m.site_tok(tm, ctx)
}

/// `defineProperty(o, key, desc)` with a computed key and a descriptor that
/// is not a literal written in place (`Gen::define_property` with no key,
/// not an accessor literal): E5 on the target, the descriptor's value a
/// post-construction write under any key, its get / set accessors of the
/// target under any key.
fn define_computed(m: &mut Model, call: CallId, o: NodeId, desc: NodeId, r: u32) {
    let star = m.atom(crate::STAR_KEY);
    m.define_on.insert((o, star, true));
    m.cons(
        o,
        Cons::Hard {
            kind: Hard::E5DefineProperty,
            reason: r,
        },
    );
    let [v, dget, dset] = descriptor_reads(m, desc, |m| node(m, call, "b-desc"));
    let site = m.anon_post_site("defineProperty (value call)");
    m.cons(o, Cons::Store { key: Key::any(), src: v, site });
    for (f, setter) in [(dget, false), (dset, true)] {
        m.cons(
            f,
            Cons::Accessor {
                holder: o,
                key: star,
                setter,
            },
        );
    }
}

/// A call of one of the eight statics as a value: true when modelled. A
/// call with a spread argument keeps the unknown-code model (its argument
/// positions are not nodes of the call).
pub(super) fn value_call(m: &mut Model, ns: BuiltinNs, name: &str, call: CallId) -> bool {
    if !m.calls[call as usize].spread.is_empty() {
        return false;
    }
    let ret = m.calls[call as usize].ret;
    match (ns, name) {
        (BuiltinNs::Object, "create") => {
            let p = arg_or_undef(m, call, 0);
            let r = m.reason("E10 Object.create");
            let t = site_obj_create(m, call);
            create_proto(m, p, t, r);
            // 20.1.2.2 steps 2-3: the properties are defined on the object
            // this call creates (ObjectDefineProperties(obj, Properties)),
            // not on the call's result node, which holds the results of
            // every other callee of the call too.
            let obj = node(m, call, "b-create");
            m.add_tok(obj, t);
            m.edge(obj, ret);
            if let Some(props) = arg(m, call, 1) {
                let descs = node(m, call, "b-load");
                m.cons(props, Cons::Load { key: Key::any(), dst: descs });
                define_computed(m, call, obj, descs, r);
            }
        }
        (BuiltinNs::Object | BuiltinNs::Reflect, "setPrototypeOf") => {
            let Some(o) = arg(m, call, 0) else { return true };
            let p = arg_or_undef(m, call, 1);
            let r = m.reason("E6 Object.setPrototypeOf");
            set_prototype_of(m, o, p, r);
            if ns == BuiltinNs::Reflect {
                m.add_prim(ret, prim::BOOL);
            } else {
                m.edge(o, ret);
            }
        }
        (BuiltinNs::Object, "defineProperty") => {
            let (Some(o), Some(d)) = (arg(m, call, 0), arg(m, call, 2)) else {
                return true;
            };
            let k = arg_or_undef(m, call, 1);
            let r = m.reason("E5 Object.defineProperty");
            m.cons(
                k,
                Cons::Implicit {
                    reason: r,
                    proto: Protocol::ToPrimitive,
                },
            );
            define_computed(m, call, o, d, r);
            m.edge(o, ret);
        }
        (BuiltinNs::Object, "defineProperties") => {
            let (Some(o), Some(pn)) = (arg(m, call, 0), arg(m, call, 1)) else {
                return true;
            };
            let r = m.reason("E5 Object.defineProperties");
            let descs = node(m, call, "own-descriptors");
            let reads = descriptor_reads(m, descs, |m| node(m, call, "b-desc"));
            let site = m.anon_post_site("defineProperty (value call)");
            define_all(m, o, pn, descs, reads, site, r);
            m.edge(o, ret);
        }
        (BuiltinNs::Object, "getOwnPropertyDescriptor") => {
            let Some(o) = arg(m, call, 0) else { return true };
            let k = arg_or_undef(m, call, 1);
            let r = m.reason("ToPropertyKey");
            m.cons(
                k,
                Cons::Implicit {
                    reason: r,
                    proto: Protocol::ToPrimitive,
                },
            );
            let t = site_tmpl(m, call, "descriptor");
            let vals = node(m, call, "b-load");
            own_descriptor(m, o, vals, t);
            ret_tok(m, call, t);
        }
        (BuiltinNs::Object, "getOwnPropertyDescriptors") => {
            let Some(o) = arg(m, call, 0) else { return true };
            let td = site_tmpl(m, call, "descriptor");
            let vals = node(m, call, "b-load");
            own_descriptor(m, o, vals, td);
            let t = site_tmpl(m, call, "descriptors");
            let fa = m.field_any(t);
            m.add_tok(fa, td);
            ret_tok(m, call, t);
        }
        (BuiltinNs::Object, "fromEntries") => {
            let Some(x) = arg(m, call, 0) else { return true };
            let it = iterate(m, call, x);
            let inner = node(m, call, "b-load");
            let t = site_tmpl(m, call, "fromEntries");
            from_entries(m, it, inner, t);
            ret_tok(m, call, t);
        }
        (BuiltinNs::Object, "groupBy") => {
            let Some(x) = arg(m, call, 0) else { return true };
            let f = arg_or_undef(m, call, 1);
            let it = iterate(m, call, x);
            let num = node(m, call, "b-index");
            m.add_prim(num, prim::NUM);
            let k = cb_call(m, call, f, ThisArg::Undef, vec![it, num]);
            let arr = site_tmpl(m, call, "Array");
            let afa = m.field_any(arr);
            m.edge(it, afa);
            let group = node(m, call, "b-group");
            m.add_tok(group, arr);
            let t = site_tmpl(m, call, "groups");
            group_by(m, k, group, t);
            ret_tok(m, call, t);
        }
        _ => return false,
    }
    true
}
