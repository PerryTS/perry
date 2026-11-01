//! Native prototype/global read, write and constructor effect summaries.
use super::*;

// ------------------------------------------------------ native parents

/// Prototype token of a native parent class (`extends EventEmitter`).
pub fn native_proto(m: &mut Model, name: &str) -> Tok {
    m.tok(TokKind::NativeProto(name.to_string()))
}

/// Native parents with a summary (owner decision 135.5).
pub fn native_parent_summarized(name: &str) -> bool {
    matches!(
        name,
        "Error" | "TypeError" | "RangeError" | "SyntaxError" | "ReferenceError" | "EventEmitter"
    )
}

/// Keys the native parent constructor defines on the instance.
pub fn native_parent_keys(name: &str) -> &'static [&'static str] {
    match name {
        "Error" | "TypeError" | "RangeError" | "SyntaxError" | "ReferenceError" => {
            &["stack", "message"]
        }
        "EventEmitter" => &["_events", "_eventsCount", "_maxListeners"],
        _ => &[],
    }
}

/// `super(...)` into a native parent.
pub fn native_parent_ctor(m: &mut Model, tm: TmplId, call: CallId, inst: Tok) {
    let name = m.tmpls[tm as usize]
        .native_parent
        .as_ref()
        .map(|(_, c)| c.clone())
        .unwrap_or_default();
    if !native_parent_summarized(&name) {
        let r = m.reason(&format!("E21 native parent {name} without a summary"));
        m.mark_hard(inst, Hard::E21NativeParent, r);
        m.escape(inst, r);
        unknown_method(m, call, "E21 native parent constructor argument");
        return;
    }
    implicit_args(m, call, "native parent constructor argument", Protocol::Any);
    let site = m.anon_birth_site(&format!("{name} constructor"));
    for k in native_parent_keys(&name) {
        let a = m.atom(k);
        let v = m.node_l("native-key");
        if k.starts_with('_') {
            // EventEmitter bookkeeping: runtime objects/numbers.
            m.add_prim(v, prim::NUM | prim::UNDEF);
            m.add_top(v, |_| "EventEmitter bookkeeping field".into());
        } else {
            m.add_prim(v, prim::STR | prim::UNDEF);
        }
        let n = m.node_l("inst");
        m.add_tok(n, inst);
        m.cons(
            n,
            Cons::Store {
                key: Key::Named(a),
                src: v,
                site,
            },
        );
    }
}

/// The own keys of a summarized native parent's prototypes below
/// `Object.prototype` (node's `Reflect.ownKeys`; the runtime builds the same
/// sealed static layouts, e.g. `EVENT_EMITTER_PROTOTYPE`). The model's chain
/// goes from the native prototype straight to `Object.prototype`, so an
/// `Error` subclass's list holds `Error.prototype`'s keys too.
pub fn native_proto_keys(name: &str) -> &'static [&'static str] {
    match name {
        "Error" | "TypeError" | "RangeError" | "SyntaxError" | "ReferenceError" => {
            &["constructor", "name", "message", "toString"]
        }
        "EventEmitter" => &[
            "constructor",
            "_events",
            "_eventsCount",
            "_maxListeners",
            "setMaxListeners",
            "getMaxListeners",
            "emit",
            "addListener",
            "on",
            "prependListener",
            "once",
            "prependOnceListener",
            "removeListener",
            "off",
            "removeAllListeners",
            "listeners",
            "rawListeners",
            "listenerCount",
            "eventNames",
        ],
        _ => &[],
    }
}

/// May native prototype `np`'s key `a` be written ([`Model::native_writes`])?
pub fn native_written(m: &Model, np: Tok, a: Atom) -> bool {
    // An element store's write (`NUM_KEY`) is one of every numeric key and
    // of `length` (`push` on an array-like moves it).
    let num = m
        .atom_index
        .get(crate::model::NUM_KEY)
        .copied()
        .filter(|_| {
            let n = &m.atoms[a as usize];
            crate::solve::numeric_key(n) || n == "length"
        });
    // A computed read (`STAR_KEY`) may name the key of any write.
    let any = m.atoms[a as usize] == crate::STAR_KEY;
    // A read under a number (`NUM_KEY`, DECISIONS 217 c) names a numeric key
    // only: an element store's, a computed one's, or a numeric name's.
    let num_read = m.atoms[a as usize] == crate::model::NUM_KEY;
    let hit = |t: Tok| {
        m.native_writes.contains_key(&(t, None))
            || m.native_writes.contains_key(&(t, Some(a)))
            || num.is_some_and(|n| m.native_writes.contains_key(&(t, Some(n))))
            || (any && m.native_writes.keys().any(|&(w, _)| w == t))
            || (num_read
                && m.native_writes.keys().any(|&(w, k)| {
                    w == t && k.is_some_and(|k| crate::solve::numeric_key(&m.atoms[k as usize]))
                }))
    };
    let global = match &m.toks[np as usize] {
        TokKind::NativeProto(n) => ns_by_name(n).is_some(),
        TokKind::Builtin(BuiltinNs::GlobalThis) => true,
        _ => false,
    };
    let is_global = matches!(m.toks[np as usize], TokKind::Builtin(BuiltinNs::GlobalThis));
    // A store into ⊤ writes the global object only once it reached
    // unmodeled code. It may always write a builtin prototype: unknown code
    // reaches every one of them without being handed one (the chain of any
    // object or function it holds, the globals interpreted code reads), and
    // gives them to compiled code as ⊤.
    (hit(TOP) && (!is_global || m.global_in_top))
        || hit(np)
        || (global
            && m.tok_index
                .get(&TokKind::ObjectProto)
                .is_some_and(|&op| hit(op)))
}

/// Does a lookup of `key` pass native prototype `np` (a key it does not
/// define, no write may have added it)? Then it is recorded, to run as an
/// opaque member if the key turns out written.
pub(super) fn native_skips(m: &mut Model, np: Tok, key: Key, miss: NativeMiss) -> bool {
    let Key::Named(a) = key else {
        return false;
    };
    let TokKind::NativeProto(native) = &m.toks[np as usize] else {
        return false;
    };
    let name = m.atoms[a as usize].as_str();
    if !native_parent_summarized(native)
        || name.starts_with(WK_PREFIX)
        || name == crate::STAR_KEY
        || native_proto_keys(native).contains(&name)
        || native_written(m, np, a)
    {
        return false;
    }
    m.native_missing.push((np, a, miss));
    true
}

/// A write that may reach a native prototype: `t` is ⊤, `ObjectProto` or the
/// prototype itself; `key` `None` (or computed): every key. Lookups that
/// passed it under a now-written key run as opaque members.
pub fn note_native_write(m: &mut Model, t: Tok, key: Option<Key>, what: &str) {
    if t != TOP
        && !matches!(
            m.toks[t as usize],
            TokKind::ObjectProto | TokKind::NativeProto(_) | TokKind::Builtin(_)
        )
    {
        return;
    }
    let star = m.atom(crate::STAR_KEY);
    let k = match key {
        Some(Key::Named(a)) if a != star => Some(a),
        _ => None,
    };
    if k.is_none() && !m.shared_replay && t != TOP {
        m.global_written_own |=
            matches!(m.toks[t as usize], TokKind::Builtin(BuiltinNs::GlobalThis));
    }
    if m.explain && !m.shared_replay && t == TOP && k.is_none() {
        eprintln!("perry: wpfacts explain: computed write on ⊤: {what}");
    }
    if m.native_writes.contains_key(&(t, None)) || m.native_writes.contains_key(&(t, k)) {
        return;
    }
    let on = if t == TOP {
        "⊤".to_string()
    } else {
        m.tok_name(t)
    };
    if m.explain && !m.shared_replay {
        eprintln!(
            "perry: wpfacts explain: may be written: {what} on {on} key {:?}",
            k.map(|a| m.atoms[a as usize].clone())
        );
    }
    let r = m.reason(&format!("native prototype may be written: {what} on {on}"));
    m.native_writes.insert((t, k), r);
    replay_native_missing(m);
}

/// The global object may now be ⊤: stores into ⊤ may have written globals.
pub fn note_global_in_top(m: &mut Model) {
    m.global_written_own |= !m.shared_replay;
    if !m.global_in_top {
        m.global_in_top = true;
        replay_native_missing(m);
    }
}

/// Lookups that passed a native prototype (or read a program-owned global)
/// under a key that may now be written: they take the opaque member.
pub(super) fn replay_native_missing(m: &mut Model) {
    if m.native_missing.is_empty() {
        return;
    }
    let pending = std::mem::take(&mut m.native_missing);
    let mut keep = Vec::with_capacity(pending.len());
    m.with_b(|m| {
        for (np, a, miss) in pending {
            if !native_written(m, np, a) {
                keep.push((np, a, miss));
                continue;
            }
            match miss {
                NativeMiss::Load(dst) => {
                    let bf = builtin_fn(m, "written key replay");
                    m.add_tok(dst, bf);
                    m.add_top(dst, |_| {
                        "member of a builtin prototype written through ⊤".into()
                    });
                    m.add_prim(dst, prim::ALL);
                }
                NativeMiss::Global(dst, f) => {
                    let name = m.atoms[a as usize].clone();
                    if m.explain && !m.shared_replay {
                        let ws: Vec<String> = m
                            .native_writes
                            .iter()
                            .map(|((w, k), r)| {
                                format!(
                                    "{}/{:?}: {}",
                                    m.tok_name(*w),
                                    k.map(|x| m.atoms[x as usize].clone()),
                                    m.reasons[*r as usize]
                                )
                            })
                            .collect();
                        eprintln!(
                            "perry: wpfacts explain: replay global {name} in_top {} writes {ws:?}",
                            m.global_in_top
                        );
                    }
                    m.add_top(dst, |_| {
                        format!("globalThis.{name} written by unmodeled code")
                    });
                    m.add_prim(dst, prim::ALL);
                    // Unmodeled code may also read what the program stored.
                    let r = m.reason(&format!("globalThis.{name} reaches unmodeled code"));
                    m.sink_node(f, r);
                }
                NativeMiss::Call(origin, call) => {
                    let native = m.tok_name(np);
                    let name = m.atoms[a as usize].clone();
                    let prev = m.cur_this.replace(origin);
                    unknown_method(m, call, &format!("argument of native {native}.{name}"));
                    m.cur_this = prev;
                }
                NativeMiss::PrimCall(_, call) => {
                    let name = m.atoms[a as usize].clone();
                    unknown_method(
                        m,
                        call,
                        &format!("argument of a written primitive method {name}"),
                    );
                }
            }
        }
    });
    keep.append(&mut m.native_missing);
    m.native_missing = keep;
}

/// The `cache` and `extensions` objects of [`BuiltinNs::Require`].
pub fn require_parts(m: &mut Model) -> (Tok, Tok) {
    if let Some(p) = m.require_parts {
        return p;
    }
    let c = anon_tmpl(m, "require.cache");
    let e = anon_tmpl(m, "require.extensions");
    m.require_parts = Some((c, e));
    let (cache, ext) = (m.cjs_cache, m.cjs_ext);
    m.add_tok(cache, c);
    m.add_tok(ext, e);
    (c, e)
}

pub(super) fn anon_tmpl(m: &mut Model, what: &'static str) -> Tok {
    let site = m.anon_birth_site(what);
    let tm = m.tmpls.len() as TmplId;
    m.tmpls.push(Template {
        kind: TmplKind::BObj {
            module: u32::MAX,
            site,
            what,
        },
        display: what.to_string(),
        birth_keys: vec![],
        definite: vec![],
        declared: Vec::new(),
        reserved: Vec::new(),
        parent: None,
        ctor: None,
        native_parent: None,
        accessor_keys: vec![],
        has_computed_members: false,
        field_init: None,
        proto_members: vec![],
        parent_value: None,
        parent_links: Vec::new(),
        null_proto: false,
        species: None,
    });
    m.tok(TokKind::Obj(tm, NO_CTX))
}

/// Node's `module.isBuiltin` (prefix-only builtins need `node:`).
pub fn is_builtin_module(s: &str) -> bool {
    match s.strip_prefix("node:") {
        Some(rest) => perry_hir::is_node_builtin_module(rest),
        None => !perry_hir::is_bare_prefix_only_builtin(s) && perry_hir::is_node_builtin_module(s),
    }
}

/// Object.prototype's legacy members: the only `__` names the global object
/// inherits.
pub const OBJECT_PROTO_DUNDER: [&str; 5] = [
    "__proto__",
    "__defineGetter__",
    "__defineSetter__",
    "__lookupGetter__",
    "__lookupSetter__",
];

/// A global-object key the program owns: the host defines no global whose
/// name starts with `__` beyond `Object.prototype`'s legacy members (the
/// test `host_defines_no_dunder_global` reads the runtime's global-object
/// sources). Such a global holds what the program stored, plus ⊤ once
/// something unmodeled may write it (`native_written`, as for native
/// prototypes).
pub fn global_user_key(name: &str) -> bool {
    name.starts_with("__") && !OBJECT_PROTO_DUNDER.contains(&name) && name != crate::STAR_KEY
}

/// The global object's field under program-owned key `a`.
pub fn global_field(m: &mut Model, g: Tok, a: Atom) -> NodeId {
    if let Some(&(_, n)) = m.global_fields.iter().find(|(k, _)| *k == a) {
        return n;
    }
    let n = m.field(g, a);
    m.global_fields.push((a, n));
    if m.global_cells {
        m.global_late = true;
        m.global_written_own |= !m.shared_replay;
        m.add_top(n, |_| "global named after the unit boundary".into());
        m.add_prim(n, prim::ALL);
        let r = m.reason("stored into a global named after the unit boundary");
        m.sink_node(n, r);
    }
    n
}

pub(super) fn native_method(
    m: &mut Model,
    np: Tok,
    native: &str,
    origin: Tok,
    name: &str,
    call: CallId,
) {
    let a = m.atom(name);
    if native_skips(m, np, Key::Named(a), NativeMiss::Call(origin, call)) {
        return;
    }
    let c = m.calls[call as usize].clone();
    match (native, name) {
        (
            "EventEmitter",
            "on" | "addListener" | "once" | "prependListener" | "prependOnceListener",
        ) => {
            if let Some(&f) = c.args.get(1) {
                let a = m.atom("#listeners");
                let fl = m.field(origin, a);
                m.edge(f, fl);
            }
            ret_tok(m, call, origin);
        }
        ("EventEmitter", "emit") => {
            let a = m.atom("#listeners");
            let fl = m.field(origin, a);
            let ret = m.node_l("emit-ret");
            let call2 = m.calls.len() as CallId;
            m.solver_call(
                call,
                CallSite {
                    this: ThisArg::Tok(origin),
                    args: c.args.iter().skip(1).copied().collect(),
                    spread: c.spread.clone(),
                    ret,
                    site: call2,
                },
            );
            m.cons(fl, Cons::Call { call: call2 });
            ret_prim(m, call, prim::BOOL);
        }
        ("EventEmitter", "off" | "removeListener" | "removeAllListeners" | "setMaxListeners") => {
            ret_tok(m, call, origin)
        }
        ("EventEmitter", "listenerCount" | "getMaxListeners") => ret_prim(m, call, prim::NUM),
        ("EventEmitter", "listeners" | "rawListeners" | "eventNames") => {
            let a = m.atom("#listeners");
            let fl = m.field(origin, a);
            let arr = site_tmpl(m, call, "Array");
            let fa = m.field_any(arr);
            m.edge(fl, fa);
            m.add_prim(fa, prim::STR);
            ret_tok(m, call, arr);
        }
        (_, "toString") => ret_prim(m, call, prim::STR),
        _ => unknown_method(m, call, &format!("argument of native {native}.{name}")),
    }
}
