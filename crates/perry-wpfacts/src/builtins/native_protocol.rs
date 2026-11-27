//! Native prototype/global read, write and constructor effect summaries.
use super::*;

// ------------------------------------------------------ native parents

/// Prototype token of a native parent class (`extends EventEmitter`).
pub fn native_proto(m: &mut Model, name: &str) -> Tok {
    m.tok(TokKind::native_proto(name))
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

/// The value of the declared constructor that class `tm`'s built-in parent
/// names, read where the heritage binding reads it: the global object's
/// (`extends Error`: the realm's `Error`) or the node module's export
/// (`import { EventEmitter } from "events"`: the module's `EventEmitter`,
/// the value the class object's [[Prototype]] holds). `None` when the class
/// has no built-in parent or its holder declares none under that name.
pub fn native_parent_value(m: &mut Model, tm: TmplId) -> Option<Tok> {
    let (module, name) = m.tmpls[tm as usize].native_parent.clone()?;
    declared_parent_ctor(m, &module, &name)
}

/// The value of the constructor holder `module` (`builtin`: the global
/// object) declares under `name`.
fn declared_parent_ctor(m: &mut Model, module: &str, name: &str) -> Option<Tok> {
    let ns = if module == "builtin" {
        BuiltinNs::GlobalThis
    } else {
        let ns = node_module_ns(&module)?;
        m.tok(TokKind::Builtin(ns));
        ns
    };
    let identity = m.namespace_property(ns, name)?.native?;
    Some(m.tok(TokKind::BuiltinFn(Native::Member {
        identity,
        holder: None,
    })))
}

/// The summarized native parent (`native_parent_summarized`) whose declared
/// constructor `t` is, by token identity: `t` is the value
/// `declared_parent_ctor` gives for the holder and name the token names (a
/// realm namespace constructor: `Error` of the global object; a module
/// export: `node:events.EventEmitter`). However the program reached it
/// (`Error`, `globalThis.Error`, `events.EventEmitter`), it is one value.
pub fn native_parent_of(m: &mut Model, t: Tok) -> Option<&'static str> {
    let (module, name): (&'static str, &'static str) = match m.toks[t as usize] {
        TokKind::Builtin(ns) => ("builtin", super::loads::proto_family(ns)?.2),
        TokKind::BuiltinFn(Native::Member {
            identity,
            holder: None,
        }) => identity.rsplit_once('.').unwrap_or(("builtin", identity)),
        _ => return None,
    };
    (native_parent_summarized(name) && declared_parent_ctor(m, module, name) == Some(t))
        .then_some(name)
}

/// `super(...)` into a native parent.
pub fn native_parent_ctor(m: &mut Model, tm: TmplId, call: CallId, inst: Tok) {
    let name = m.tmpls[tm as usize]
        .native_parent
        .as_ref()
        .map(|(_, c)| c.clone())
        .unwrap_or_default();
    native_parent_ctor_named(m, &name, call, inst)
}

/// `super(...)` into the realm constructor of native parent `name`.
pub fn native_parent_ctor_named(m: &mut Model, name: &str, call: CallId, inst: Tok) {
    if !native_parent_summarized(name) {
        let r = m.reason(&format!("E21 native parent {name} without a summary"));
        m.mark_hard(inst, Hard::E21NativeParent, r);
        m.escape(inst, r);
        unknown_method(m, call, "E21 native parent constructor argument");
        return;
    }
    let error_family = error_family_parent(name);
    if !error_family {
        implicit_args(m, call, "native parent constructor argument", Protocol::Any);
    }
    let site = m.anon_birth_site(&format!("{name} constructor"));
    if error_family {
        error_parent_args(m, call, inst, site);
    }
    for k in native_parent_keys(name) {
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
                region: m.thrown,
            },
        );
    }
}

/// Is the summarized native parent named by the argument the realm's Error family: the global object's
/// `Error` or one of its NativeErrors, as the realm namespaces declare them
/// (`ns_by_name`: `BuiltinNs::Error` / `BuiltinNs::NativeError`, the
/// identities the runtime's `init_error_family_receiver` initializes)?
fn error_family_parent(name: &str) -> bool {
    matches!(
        ns_by_name(name),
        Some(BuiltinNs::Error | BuiltinNs::NativeError(_))
    )
}

/// Rule NPARG: `super(message, options, ...)` into an Error-family parent
/// acts on its arguments only per the constructor's contract (ECMA-262
/// 20.5.1.1 Error / 20.5.6.1.1 NativeError, 20.5.8.1 InstallErrorCause; the
/// runtime's `js_error_subclass_default_init_with_options`, `message` =
/// `js_jsvalue_to_string(arg0)`, and `js_error_apply_cause_to_object`, a
/// [[Get]] of `cause` on a pointer `arg1` whose result, when not undefined,
/// is the instance's own `cause`):
/// - `message`: ToString (Protocol::ToPrimitive: a program `toString`,
///   `valueOf` or `@@toPrimitive` runs with `this` = the message, nothing
///   else of it is read or called);
/// - `options`: one [[Get]] of `cause` (an accessor or a Proxy `get` trap
///   runs), stored on the instance under `cause`;
/// - later arguments: never read.
/// A call the spread view did not reach (`reach_spread_args` off) still
/// holds its spread arrays: each element may stand at either position.
fn error_parent_args(m: &mut Model, call: CallId, inst: Tok, site: SiteId) {
    let c = m.calls[call as usize].clone();
    let mut message: Vec<NodeId> = c.arg(0).copied().into_iter().collect();
    let mut options: Vec<NodeId> = c.arg(1).copied().into_iter().collect();
    if !c.spread.is_empty() && !m.reach_spread_args {
        let ek = m.elem_key();
        for &s in &c.spread {
            let e = m.node_l("error-arg-spread");
            m.cons(s, Cons::Load { key: ek, dst: e });
            message.push(e);
            options.push(e);
        }
    }
    let r = m.reason("Error message ToString");
    for &a in &message {
        m.why_implicit(a, r, call);
        m.cons(
            a,
            Cons::Implicit {
                reason: r,
                proto: Protocol::ToPrimitive,
                region: m.thrown,
            },
        );
    }
    if options.is_empty() {
        return;
    }
    let ca = m.atom("cause");
    let cause = m.node_l("error-cause");
    for &o in &options {
        m.cons(
            o,
            Cons::Load {
                key: Key::Named(ca),
                dst: cause,
            },
        );
    }
    let n = m.node_l("inst");
    m.add_tok(n, inst);
    m.cons(
        n,
        Cons::Store {
            key: Key::Named(ca),
            src: cause,
            site,
            region: m.thrown,
        },
    );
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
    let (top, own) = native_written_by(m, np, a);
    top || own
}

/// [`native_written`], by its writer: `.0` a store into ⊤ (which may be any
/// builtin prototype), `.1` a write onto `np` itself (for the global object
/// or a global's native prototype, onto `Object.prototype` too).
pub fn native_written_by(m: &Model, np: Tok, a: Atom) -> (bool, bool) {
    written_by(m, np, a, false, &|_| true)
}

/// [`native_written`] past the writes that only delete (ECMA-262 13.5.1.2,
/// OrdinaryDelete 10.1.10): a delete stores no value and runs no code (a
/// Proxy's `deleteProperty` trap is the handler's call, modeled at the
/// delete), so a key only deletes reached holds the realm's value or is
/// absent. Under such writes `new Name(...)` of a declared global
/// constructor constructs the realm's constructor or throws the realm's
/// ReferenceError (6.2.5.5 GetValue of an unresolvable binding) before an
/// argument is handed anywhere. True: a writer that may have stored a value
/// (a store, a define, an accessor, a prototype, unmodeled code).
pub fn native_value_written(m: &Model, np: Tok, a: Atom) -> bool {
    let (top, own) = written_by(m, np, a, false, &|w| {
        w.opaque
            || !w.stored.is_empty()
            || w.accessor
            || !w.getters.is_empty()
            || !w.setters.is_empty()
    });
    top || own
}

/// Diagnostic: the reasons of the writes [`native_written`] counts for
/// `np`'s key `a`, with the written key (`None`: any key).
fn cliffc_diag() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("PERRY_CLIFFC_DIAG").is_some_and(|v| v == "1"))
}

pub fn native_writers_why(m: &Model, np: Tok, a: Atom) -> Vec<String> {
    let mut out = Vec::new();
    for (&(t, k), w) in m.native_writes.iter() {
        let (top, own) = written_by(m, np, a, false, &|x| std::ptr::eq(x, w));
        if top || own {
            out.push(format!(
                "{} key={:?} top={top} own={own}: {}",
                m.tok_name(t),
                k.map(|k| m.atoms[k as usize].to_string()),
                m.reasons[w.reason as usize]
            ));
        }
    }
    out
}

/// [`native_written`] for an accessor descriptor query (`__lookupGetter__`,
/// a getter walk): only writes that may install an accessor
/// ([`crate::model::NativeWrite`]) count; a plain store makes a data
/// property (ECMA-262 10.1.9.2), which such a query never returns.
pub fn native_accessor_written(m: &Model, np: Tok, a: Atom) -> bool {
    let (top, own) = written_by(m, np, a, false, &|w| w.accessor);
    top || own
}

/// R9READ5: may a write other than a plain store reach `np`'s key `a`
/// (an accessor, a definition, unknown code, a delete)? A plain store
/// makes or updates a writable data property (10.1.9.2), so only such a
/// write can make an OrdinarySet through `np` run code or fail.
pub fn native_nonplain_written(m: &Model, np: Tok, a: Atom) -> bool {
    let (top, own) = written_by(m, np, a, false, &|w| w.accessor || w.opaque || w.absent);
    top || own
}

/// The known getters (setters if `setter`) that writes reaching `np`'s key
/// `a` installed (definitions through ⊤ with a known descriptor), into
/// `dst`: what an accessor descriptor query that passes `np` returns
/// besides `undefined` (ECMA-262 10.1.6.3: the descriptor's own functions).
pub fn native_accessor_known(m: &mut Model, np: Tok, a: Atom, dst: NodeId, setter: bool) {
    let mut found: Vec<Tok> = Vec::new();
    for e in written_entries(m, np, a, false) {
        let w = &m.native_writes[&e];
        found.extend(if setter { &w.setters } else { &w.getters }.iter().copied());
    }
    for f in found {
        m.add_tok(dst, f);
    }
}

/// May the global object's key `a` (`np` the global object or the HIR
/// global sentinel) hold a value its field ([`global_field`]) does not
/// carry? A program store under the named key puts its value in that
/// field, so a read that takes the field sees it; every other writer
/// counts as [`native_written`] does: an accessor installed on the global
/// object, a prototype given to it, a write under any key (an unseen
/// module, interpreted code, the object handed to unknown code), a store
/// into ⊤ once the global object reached unmodeled code, a write onto
/// `Object.prototype`.
pub fn global_value_written(m: &Model, np: Tok, a: Atom) -> bool {
    let (top, own) = written_by(m, np, a, true, &|_| true);
    top || own
}

/// [`global_value_written`] past the writes that only delete (ECMA-262
/// 13.5.1.2, OrdinaryDelete 10.1.10): a delete stores no value and runs no
/// code (a Proxy's `deleteProperty` trap is the handler's call, modeled at
/// the delete), so under any key, through ⊤ that may be the global object
/// included, it can only make the binding absent. True: a writer that may
/// have stored a value (a store, a define, an accessor, a prototype) may
/// have written the key: the read takes ⊤ as before.
pub fn global_value_stored(m: &Model, np: Tok, a: Atom) -> bool {
    let (top, own) = written_by(m, np, a, true, &|w| {
        w.opaque
            || !w.stored.is_empty()
            || w.accessor
            || !w.getters.is_empty()
            || !w.setters.is_empty()
    });
    top || own
}

/// A global binding read (`NativeMiss::Global` into `dst`) that only
/// deletes may have reached ([`global_value_stored`] false): it reads
/// {its prior values} ∪ {`undefined`}; the caller keeps the read parked,
/// so a later writer of a value replays it. A non-configurable global
/// (`undefined`, `NaN`, `Infinity`; ECMA-262 19.1) is never removed.
pub fn global_maybe_deleted(m: &mut Model, np: Tok, a: Atom, dst: NodeId) {
    if !global_value_written(m, np, a) {
        return;
    }
    if matches!(
        m.atoms[a as usize].as_str(),
        "undefined" | "NaN" | "Infinity"
    ) {
        return;
    }
    m.add_prim(dst, prim::UNDEF);
}

fn written_by(
    m: &Model,
    np: Tok,
    a: Atom,
    field_carries_stores: bool,
    counts: &dyn Fn(&crate::model::NativeWrite) -> bool,
) -> (bool, bool) {
    let has = |t: Tok, k: Option<Atom>| m.native_writes.get(&(t, k)).is_some_and(counts);
    let any_on = |t: Tok, keyed: &dyn Fn(Option<Atom>) -> bool| {
        m.native_writes
            .range((t, None)..=(t, Some(Atom::MAX)))
            .any(|(&(_, k), x)| counts(x) && keyed(k))
    };
    // An element store's write (`NUM_KEY`) is one of every numeric key and
    // of `length` (`push` on an array-like moves it).
    let num = m
        .atom_index
        .get(crate::model::NUM_KEY)
        .copied()
        .filter(|_| m.numeric_atom(a) || m.atoms[a as usize] == "length");
    // A computed read (`STAR_KEY`) may name the key of any write.
    let any = m.atoms[a as usize] == crate::STAR_KEY;
    // A read under a number (`NUM_KEY`, DECISIONS 217 c) names a numeric key
    // only: an element store's, a computed one's, or a numeric name's.
    let num_read = m.atoms[a as usize] == crate::model::NUM_KEY;
    // A named write onto the global object that only a program store made:
    // its value is in the global's field.
    let stored_key = |t: Tok, k: Atom| {
        field_carries_stores
            && super::global_object(m, t)
            && !m.accessors.contains_key(&(t, k))
            && !m.native_given.iter().any(|&(w, _)| w == t)
    };
    let stored = |t: Tok| stored_key(t, a);
    // DEL-KEY: a named delete of the global object (`delete g.name`,
    // `Model::delete_global_key`) is no program store: its key's field holds
    // no `undefined`, so the entry counts as a write that only deletes,
    // for the writers `counts` takes such a write for.
    let gone_counts = counts(&crate::model::NativeWrite {
        reason: 0,
        accessor: false,
        getters: Vec::new(),
        setters: Vec::new(),
        opaque: false,
        stored: Vec::new(),
        absent: true,
    });
    let gone = |t: Tok, k: Option<Atom>| {
        gone_counts && m.native_writes.get(&(t, k)).is_some_and(|w| w.absent)
    };
    // A computed read of the global object takes the field of each named
    // key it may spell (`global_shaped_read`, `global_shaped_replay`): a
    // named program store it may name is one of those fields, not a write
    // of a value the read misses (`g["globalThis"] = globalThis`).
    let field_named = |t: Tok, k: Option<Atom>| {
        k.is_some_and(|k| {
            let n = &m.atoms[k as usize];
            n != crate::STAR_KEY && n != crate::model::NUM_KEY && stored_key(t, k)
        })
    };
    let hit = |t: Tok| {
        has(t, None)
            || (has(t, Some(a)) && !stored(t))
            || (stored(t) && gone(t, Some(a)))
            || (any
                && gone_counts
                && m.native_writes
                    .range((t, None)..=(t, Some(Atom::MAX)))
                    .any(|(&(_, k), x)| x.absent && field_named(t, k)))
            || num.is_some_and(|n| has(t, Some(n)))
            || (any && any_on(t, &|k| !field_named(t, k)))
            || (num_read && any_on(t, &|k| k.is_some_and(|k| m.numeric_atom(k))))
    };
    // The HIR global sentinel (`Other`, what a bare global name is read
    // off) is the global object: a write through either token is a write
    // of the one object.
    let is_global = super::global_object(m, np);
    // Taken last: a name match, after the cheap tests of the write.
    let global = || match &m.toks[np as usize] {
        TokKind::NativeProto(_, ns) => ns.is_some(),
        _ => is_global,
    };
    let other_global = |t: Tok| {
        is_global
            && [BuiltinNs::GlobalThis, BuiltinNs::Other]
                .into_iter()
                .any(|ns| {
                    m.tok_index
                        .get(&TokKind::Builtin(ns))
                        .is_some_and(|&g| g != t && hit(g))
                })
    };
    // ⊤ stands for the values unknown code may hold. A built-in object is
    // one of them once it was handed to unknown code (`SharedWrite::Handed`;
    // one reached along an escaped value's prototype chain is not handed,
    // `Model::escape_body`), or once the global object was
    // (`global_in_top`): unknown code then reaches every built-in by name.
    // Only then may a store into ⊤ write it. Per unit, unknown code
    // includes the other units' code, which holds every built-in object.
    (
        hit(TOP) && (m.global_in_top || (!is_global && (m.unit_mode || handed_to_unknown(m, np)))),
        realm_replaced(m, np, a)
            || hit(np)
            || other_global(np)
            // A prototype write is a chain contribution, separate from
            // the namespace's own hit above and its TOP reachability.
            || (m.tok_index
                .get(&TokKind::ObjectProto)
                .is_some_and(|&op| hit(op))
                && global()
                && !locked_chain_read(m, np, a)),
    )
}

/// The write entries [`written_by`] counts for `np`'s key `a`, each once, in
/// key order: those `written_by(m, np, a, f, &|x| ptr::eq(x, w))` holds
/// for, found in one walk of the entries it looks at instead of one
/// `written_by` per entry.
fn written_entries(
    m: &Model,
    np: Tok,
    a: Atom,
    field_carries_stores: bool,
) -> Vec<(Tok, Option<Atom>)> {
    if realm_replaced(m, np, a) {
        return m.native_writes.keys().copied().collect();
    }
    let name = &m.atoms[a as usize];
    let num = m
        .atom_index
        .get(crate::model::NUM_KEY)
        .copied()
        .filter(|_| m.numeric_atom(a) || name == "length");
    let any = name == crate::STAR_KEY;
    let num_read = name == crate::model::NUM_KEY;
    let stored_key = |t: Tok, k: Atom| {
        field_carries_stores
            && super::global_object(m, t)
            && !m.accessors.contains_key(&(t, k))
            && !m.native_given.iter().any(|&(w, _)| w == t)
    };
    let field_named = |t: Tok, k: Option<Atom>| {
        k.is_some_and(|k| {
            let n = &m.atoms[k as usize];
            n != crate::STAR_KEY && n != crate::model::NUM_KEY && stored_key(t, k)
        })
    };
    let mut out: Vec<(Tok, Option<Atom>)> = Vec::new();
    // `written_by`'s `hit(t)`, as the entries it matches.
    let mut on = |t: Tok| {
        let mut key = |k: Option<Atom>| {
            if m.native_writes.contains_key(&(t, k)) {
                out.push((t, k));
            }
        };
        key(None);
        if !stored_key(t, a) {
            key(Some(a));
        }
        if let Some(n) = num {
            key(Some(n));
        }
        if any || num_read {
            for (&(_, k), _) in m.native_writes.range((t, None)..=(t, Some(Atom::MAX))) {
                if (any && !field_named(t, k)) || (num_read && k.is_some_and(|k| m.numeric_atom(k)))
                {
                    out.push((t, k));
                }
            }
        }
    };
    let is_global = super::global_object(m, np);
    if m.global_in_top || (!is_global && (m.unit_mode || handed_to_unknown(m, np))) {
        on(TOP);
    }
    on(np);
    if is_global {
        for ns in [BuiltinNs::GlobalThis, BuiltinNs::Other] {
            if let Some(&g) = m.tok_index.get(&TokKind::Builtin(ns)) {
                if g != np {
                    on(g);
                }
            }
        }
    }
    if let Some(&op) = m.tok_index.get(&TokKind::ObjectProto) {
        let global = match &m.toks[np as usize] {
            TokKind::NativeProto(_, ns) => ns.is_some(),
            _ => is_global,
        };
        if global && !locked_chain_read(m, np, a) {
            on(op);
        }
    }
    out.sort_unstable();
    out.dedup();
    out
}

/// The (key, native prototype) pairs of `idx` whose [`written_by`] write
/// entry `(t, k)` may count, a superset: an entry counts only on ⊤, the
/// lookup's own token, the global object's tokens or `Object.prototype`
/// (`wide`: any prototype), and only under the lookup's key, any key
/// (`k` `None`), a computed key (`STAR_KEY`: any write), a number
/// (`NUM_KEY` reads a numeric write) or, for an element store (`NUM_KEY`
/// written), a numeric key or `length`. `realm_replaced` counts every entry
/// but never holds for a lookup still parked or answered from stored values
/// once `note_realm_held` replayed them; before that every replay is full.
fn reached<V>(
    m: &Model,
    idx: &std::collections::BTreeMap<(Atom, Tok), V>,
    (t, k): (Tok, Option<Atom>),
) -> Vec<(Atom, Tok)> {
    let wide = t == TOP
        || super::global_object(m, t)
        || matches!(m.toks[t as usize], TokKind::ObjectProto);
    let tok_ok = |np: Tok| wide || np == t;
    let Some(b) = k else {
        return idx.keys().filter(|&&(_, np)| tok_ok(np)).copied().collect();
    };
    let mut out: Vec<(Atom, Tok)> = Vec::new();
    let key = |a: Atom, out: &mut Vec<(Atom, Tok)>| {
        if wide {
            out.extend(idx.range((a, 0)..=(a, Tok::MAX)).map(|(&g, _)| g));
        } else if idx.contains_key(&(a, t)) {
            out.push((a, t));
        }
    };
    key(b, &mut out);
    if let Some(&s) = m.atom_index.get(crate::STAR_KEY) {
        if s != b {
            key(s, &mut out);
        }
    }
    if let Some(&n) = m.atom_index.get(crate::model::NUM_KEY) {
        if n == b {
            out.extend(idx.keys().copied().filter(|&(a, np)| {
                a != b && tok_ok(np) && (m.numeric_atom(a) || m.atoms[a as usize] == "length")
            }));
        } else if m.numeric_atom(b) {
            key(n, &mut out);
        }
    }
    out.sort_unstable();
    out.dedup();
    out
}

/// Before `note_realm_held` replayed (`realm_replaced` may count every write
/// for a parked or answered lookup): every replay runs all of them.
fn realm_window(m: &Model) -> bool {
    m.realm_held && !m.native_realm_replayed
}

/// An accessor or a prototype was given to `t`: for the global object the
/// next replay runs every parked lookup ([`Model::native_replay_all`]).
pub fn note_global_holder(m: &mut Model, t: Tok) {
    if super::global_object(m, t) {
        m.native_replay_all = true;
        m.native_replay_seq += 1;
    }
}

/// Unknown code holds the realm (ECMA-262 9.3, `Model::realm_held`): it may
/// have replaced (or made an accessor of) a member of a built-in family's own
/// prototype that the runtime's lock does not make final, with any function
/// or value it made. A call of such a member is the realm's function or
/// unknown code (`opaque_written` -> `NativeMiss::Call` -> `unknown_method`),
/// a read is the realm's value or the opaque member. Monotone:
/// `note_realm_held` replays the lookups parked before it held. A namespace
/// member, the module's own `require` and the global object are not members
/// it replaces (owner decisions 2026-11-18, closed world 11-14).
fn realm_replaced(m: &Model, np: Tok, a: Atom) -> bool {
    if !m.realm_held {
        return false;
    }
    // A built-in family's own prototype (`Buffer.prototype`,
    // `%TypedArray%.prototype`, `URL.prototype`, `Error.prototype`, ...; the
    // holder a lookup on a value of that family passes before
    // `Object.prototype`, `builtins::intrinsic_miss_on`): unknown code that
    // holds the realm holds it, and its [[Set]] of `a` succeeds unless the
    // runtime's lock refuses it (ECMA-262 10.1.9; DEC 250 `final_on`). A
    // read the lock makes final there stays exact; `Object.prototype`
    // (`ObjectProto`) is asked under its own lock and never here.
    if let TokKind::NativeProto(what, _) = &m.toks[np as usize] {
        return !super::proto_read_final(m, what, a);
    }
    // A namespace member (`Math.max`, `console.error`, any key of one) and
    // the global object (`globalThis` included): owner decisions 2026-11-18
    // (closed world 11-14). The unknown code that holds the realm is the
    // runtime's natives, which replace no member of an intrinsic namespace
    // and write no key of the global object. Their writes are the program's
    // own stores (`Math.foo = v`) and its stores into ⊤ once the object is in
    // ⊤'s reach (`Model::global_in_top`, the caller); interpreted code and
    // unseen modules keep their any-key writes there.
    false
}

/// Unknown code now holds the realm: the lookups parked past a member it may
/// have replaced (`realm_replaced`) take the opaque member.
pub fn note_realm_held(m: &mut Model) {
    replay_native_missing(m);
    m.native_realm_replayed = true;
    // Unknown code may register a process listener: it is handed the
    // process events (`Model::process_event`).
    let (pe, e) = (m.process_event, m.esc_raise);
    m.edge(pe, e);
}

/// Built-in object `np` was handed to unknown code (not reached along a
/// prototype chain): `Model::escape_body` notes `SharedWrite::Handed`.
fn handed_to_unknown(m: &Model, np: Tok) -> bool {
    m.shared_of(np)
        .is_some_and(|s| m.shared_out.contains(&crate::model::SharedWrite::Handed(s)))
}

/// Does a lookup of `key` pass native prototype `np` (a key it does not
/// define, no write may have added it)? Then it is recorded, to run as an
/// opaque member if the key turns out written.
pub(super) fn native_skips(m: &mut Model, np: Tok, key: Key, miss: NativeMiss) -> bool {
    let Key::Named(a) = key else {
        return false;
    };
    m.census_native_read(np, a, miss.clone());
    let TokKind::NativeProto(native, ..) = &m.toks[np as usize] else {
        return false;
    };
    let name = m.atoms[a as usize].as_str();
    if !native_parent_summarized(native)
        || name.starts_with(WK_PREFIX)
        || name == crate::STAR_KEY
        || native_proto_keys(native).contains(&name)
    {
        return false;
    }
    if native_written(m, np, a) {
        return answer_stored(m, np, a, miss);
    }
    m.native_missing.push((np, a, miss));
    true
}

/// A write that may reach a native prototype: `t` is ⊤, `ObjectProto` or the
/// prototype itself; `key` `None` (or computed): every key. Lookups that
/// passed it under a now-written key run as opaque members.
pub fn note_native_write(m: &mut Model, t: Tok, key: Option<Key>, what: &str) {
    note_write(m, t, key, what, true, None, WriteValue::Opaque)
}

/// What a write onto a native prototype stored
/// ([`crate::model::NativeWrite`]): a program node's values (a plain store,
/// ECMA-262 10.1.9.2), nothing (a delete: the key may be missing), or a
/// value the solver does not name (every other writer).
#[derive(Clone, Copy, Debug)]
pub enum WriteValue {
    Opaque,
    Value(NodeId),
    Absent,
}

/// [`note_native_write`] for a known getter (setter if `setter`) `f`
/// installed by a definition through ⊤ (ECMA-262 10.1.6.3): an accessor
/// query that may reach it returns `f`, not an unknown value.
pub fn note_native_accessor(m: &mut Model, t: Tok, key: Option<Key>, f: Tok, setter: bool) {
    note_write(
        m,
        t,
        key,
        "accessor",
        false,
        Some((f, setter)),
        WriteValue::Opaque,
    )
}

/// [`note_native_write`] for a plain store ([[Set]], ECMA-262 10.1.9.2) or a
/// delete: it makes, changes or removes a data property, never an accessor;
/// `value` is what it stored.
pub fn note_native_data_write(
    m: &mut Model,
    t: Tok,
    key: Option<Key>,
    what: &str,
    value: WriteValue,
) {
    note_write(m, t, key, what, false, None, value)
}

/// Does a recorded write already cover a write of `t`'s key `k` (an
/// accessor-capable one only if `accessor`) storing `value`?
///
/// Whole program a write onto ⊤ writes no intrinsic (P-N1, [`note_write`]):
/// there is nothing to record, so it is covered. The settle steps that
/// write (`settle_native_given`) ask this before they report a change;
/// without it each pass of the fixpoint found the same write new again.
fn write_covered(m: &Model, t: Tok, k: Option<Atom>, accessor: bool, value: WriteValue) -> bool {
    if t == TOP && !m.foreign_code() {
        return true;
    }
    let covers = |x: &crate::model::NativeWrite| {
        (x.accessor || !accessor)
            && (x.opaque
                || match value {
                    WriteValue::Opaque => false,
                    WriteValue::Value(n) => x.stored.contains(&n),
                    WriteValue::Absent => x.absent,
                })
    };
    [(t, None), (t, k)]
        .iter()
        .any(|w| m.native_writes.get(w).is_some_and(covers))
}

fn note_write(
    m: &mut Model,
    t: Tok,
    key: Option<Key>,
    what: &str,
    accessor: bool,
    known: Option<(Tok, bool)>,
    value: WriteValue,
) {
    // P-N1 (owner 2026-11-23): whole program, a value the analysis does not
    // model is no intrinsic, so a write onto ⊤ writes none of them.
    if t == TOP && !m.foreign_code() {
        return;
    }
    if t != TOP
        && !matches!(
            m.toks[t as usize],
            TokKind::ObjectProto | TokKind::NativeProto(..) | TokKind::Builtin(_)
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
        m.global_written_own |= super::global_object(m, t);
    }
    if t != TOP
        && matches!(m.toks[t as usize], TokKind::ObjectProto)
        && !m.objproto_alias
        && !m.objproto_direct.contains_key(&k)
    {
        let r = m.reason(&format!(
            "native prototype may be written: {what} on Object.prototype"
        ));
        m.objproto_direct.insert(k, r);
    }
    if m.explain && !m.shared_replay && t == TOP && k.is_none() {
        eprintln!("perry: wpfacts explain: computed write on ⊤: {what}");
    }
    m.census_native_write(t, k);
    let new_fn = known.is_some_and(|(f, setter)| {
        !m.native_writes
            .get(&(t, k))
            .is_some_and(|w| if setter { &w.setters } else { &w.getters }.contains(&f))
    });
    if !new_fn && write_covered(m, t, k, accessor, value) {
        return;
    }
    let fresh = !m.native_writes.contains_key(&(t, k));
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
    if fresh {
        m.census_opener(t, k, what, None);
    }
    let r = m.reason(&format!("native prototype may be written: {what} on {on}"));
    let w = m
        .native_writes
        .entry((t, k))
        .or_insert(crate::model::NativeWrite {
            reason: r,
            accessor,
            getters: Vec::new(),
            setters: Vec::new(),
            opaque: false,
            stored: Vec::new(),
            absent: false,
        });
    w.accessor |= accessor;
    // A global read only deletes reached stays parked
    // (`global_maybe_deleted`): this entry's first stored value replays it.
    let first_value = matches!(value, WriteValue::Value(_)) && w.stored.is_empty() && w.absent;
    // A parked global read (`NativeMiss::Global`) the entry's stored values
    // reached took no `undefined`: its first delete replays it (a delete
    // made after the read, ECMA-262 10.1.10, is one of its writers too).
    let first_absent = matches!(value, WriteValue::Absent) && !w.absent;
    match value {
        WriteValue::Opaque => w.opaque = true,
        WriteValue::Value(n) => {
            if !w.stored.contains(&n) {
                w.stored.push(n);
            }
        }
        WriteValue::Absent => w.absent = true,
    }
    if let Some((f, setter)) = known {
        let fns = if setter {
            &mut w.setters
        } else {
            &mut w.getters
        };
        if !fns.contains(&f) {
            fns.push(f);
        }
    }
    // ENUM-DEL: a write of a global object a for-in delete reaches
    // (`Model::delete_global_enum`) makes its key one the for-in may yield.
    if !matches!(value, WriteValue::Absent) {
        m.enum_delete_written(t, k);
    }
    if accessor || known.is_some() || matches!(value, WriteValue::Opaque) {
        replay_native_by(m, Some((t, k)));
        return;
    }
    // A plain store or a delete adds one stored value (or `undefined`) to
    // this one entry: only the answered lookups the entry reaches gain it.
    // Which lookups an entry reaches depends on its token and key and on
    // the environment events that replay everything (`note_global_in_top`,
    // `note_handed_to_unknown`), never on the other entries. Lookups parked
    // unwritten change only when the entry is new.
    feed_answered(m, (t, k), value);
    if fresh || first_value || first_absent {
        replay_pending(m, Some((t, k)));
    }
}

/// Once the worklist is empty: a runtime object given prototype `src`
/// (`Object.setPrototypeOf`, `__proto__`) may now find, past itself, any key
/// of `src`'s chain. Each such key is written on it (an unknown key set:
/// every key). True when a write is new.
pub fn settle_native_given(m: &mut Model) -> bool {
    if m.native_given.is_empty() {
        return false;
    }
    let mut more = false;
    for (t, src) in m.native_given.clone() {
        let mut keys = Vec::new();
        let mut seen = Vec::new();
        let mut accessors = Vec::new();
        let known = m.nodes[src as usize]
            .pts
            .to_sorted()
            .into_iter()
            .all(|u| chain_keys(m, u, &mut seen, &mut keys, &mut accessors));
        // GP1: on a built-in each key of `src`'s chain holds what a [[Get]]
        // of it on `src` finds (ECMA-262 10.1.8.1), when the keys are known
        // and none is an accessor (whose getter would run with `t` as
        // `this`). Otherwise every member escapes once and the keys hold
        // opaque values, as for ⊤.
        let exact = known && accessors.is_empty();
        if m.gives_exactly(t) && !exact && m.given_inexact.insert((t, src)) {
            more = true;
            let members = m.node_l("given-prototype-members");
            let key = Key::Any(0);
            m.cons(src, crate::model::Cons::Load { key, dst: members });
            let r = m.reason("E6 member of a prototype given to a runtime object");
            m.sink_node(members, r);
        }
        let exact = exact && m.gives_exactly(t);
        if exact {
            // The runtime's own lookups on `t` (its protocols) may find, and
            // call, what `src`'s chain holds under their keys.
            let protocol = crate::model::Protocol::runtime_keys();
            let r = m.reason("E6 member of a prototype given to a runtime object");
            for &a in &keys {
                if protocol.contains(&m.atoms[a as usize].as_str()) {
                    let n = given_read(m, src, a);
                    m.sink_node(n, r);
                }
            }
        }
        let ks: Vec<Option<Atom>> = match known {
            true => keys.into_iter().map(Some).collect(),
            false => vec![None],
        };
        for k in ks {
            let value = match k {
                Some(a) if exact => WriteValue::Value(given_read(m, src, a)),
                _ => WriteValue::Opaque,
            };
            if write_covered(m, t, k, !exact, value) {
                continue;
            }
            more = true;
            // The census attributes the write to the operation that gave the
            // prototype (its receiver node, or its call), and keeps the given
            // value as `proto`: a ⊤ on either side makes the write derived.
            let (by_node, by_call) = match m.census_given_by(t, src) {
                Some((n, c)) => (
                    if n == NO_NODE { src } else { n },
                    (c != u32::MAX).then_some(c),
                ),
                None => (src, None),
            };
            let saved = m
                .diagnostic
                .as_mut()
                .map(|d| (d.cur_call.take(), d.cause_node.replace(by_node)));
            if let Some(d) = m.diagnostic.as_mut() {
                d.cur_call = by_call;
            }
            m.census_settling(Some(src));
            match value {
                WriteValue::Opaque => {
                    note_native_write(m, t, k.map(Key::Named), "E6 prototype given")
                }
                _ => note_native_data_write(m, t, k.map(Key::Named), "E6 prototype given", value),
            }
            m.census_settling(None);
            if let (Some(d), Some(saved)) = (m.diagnostic.as_mut(), saved) {
                (d.cur_call, d.cause_node) = saved;
            }
        }
    }
    more
}

/// GP1: the node holding what a [[Get]] of `a` on `src`'s chain finds.
fn given_read(m: &mut Model, src: NodeId, a: Atom) -> NodeId {
    if let Some(&n) = m.given_reads.get(&(src, a)) {
        return n;
    }
    let n = m.node_l("given-prototype-read");
    m.cons(
        src,
        crate::model::Cons::Load {
            key: Key::Named(a),
            dst: n,
        },
    );
    m.given_reads.insert((src, a), n);
    n
}

/// Collect into `out` every key an object of token `u` or of its chain may
/// hold. False when that set is unknown. Each key a define (E5) or an
/// accessor gives it is one of `define_on` / `accessor_keys` (`STAR_KEY`
/// under a computed key: unknown), so E5 alone keeps the set known. `ObjectProto` adds none: it stands
/// for every builtin prototype, already on every runtime object's chain. A
/// write through ⊤ is already a write of its key on every runtime object.
/// The keys an accessor or a define may give go to `acc` too.
fn chain_keys(
    m: &mut Model,
    u: Tok,
    seen: &mut Vec<Tok>,
    out: &mut Vec<Atom>,
    acc: &mut Vec<Atom>,
) -> bool {
    if seen.contains(&u) {
        return true;
    }
    seen.push(u);
    if u == TOP {
        return false;
    }
    let tm = match m.toks[u as usize] {
        TokKind::ObjectProto => return true,
        TokKind::Obj(tm, _) | TokKind::Proto(tm) => tm,
        _ => return false,
    };
    if m.is_marker(u)
        || m.foreign.contains_key(&tm)
        || m.any_writers.contains_key(&tm)
        || m.hard.get(&u).is_some_and(|(k, _)| {
            !matches!(
                k,
                Hard::E5DefineProperty | Hard::E6SetPrototypeOf | Hard::E7Delete
            )
        })
    {
        return false;
    }
    let tpl = &m.tmpls[tm as usize];
    // `Object.create` is born with no own key (its layout is open: the flag
    // says so); every key it gains is a store or a define, collected below.
    if tpl.has_computed_members && !matches!(tpl.kind, TmplKind::ObjCreate { .. }) {
        return false;
    }
    let add = |a: Atom, out: &mut Vec<Atom>| {
        if !out.contains(&a) {
            out.push(a);
        }
    };
    let own: Vec<Atom> = match m.toks[u as usize] {
        TokKind::Proto(_) => tpl.proto_members.clone(),
        _ => tpl
            .birth_keys
            .iter()
            .chain(&tpl.accessor_keys)
            .chain(&tpl.proto_members)
            .copied()
            .collect(),
    };
    for a in own {
        add(a, out);
    }
    for &a in &tpl.accessor_keys {
        add(a, acc);
    }
    let late = match m.toks[u as usize] {
        TokKind::Proto(_) => &m.proto_writers,
        _ => &m.writers,
    };
    let late: Vec<Atom> = late
        .keys()
        .filter(|(w, _)| *w == tm)
        .map(|&(_, a)| a)
        .collect();
    for a in late {
        add(a, out);
    }
    let defined: Vec<Atom> = m
        .define_on
        .iter()
        .filter(|(o, _, _)| m.nodes[*o as usize].pts.contains_tok(u))
        .map(|&(_, a, _)| a)
        .collect();
    let accessors = m.accessor_keys.get(&u).cloned().unwrap_or_default();
    for a in defined.into_iter().chain(accessors) {
        add(a, out);
        add(a, acc);
    }
    // A key defined, or an accessor installed, under a computed key.
    let star = m.atom(crate::STAR_KEY);
    if out.contains(&star) {
        return false;
    }
    let pn = m.proto_node(u);
    let protos: Vec<Tok> = m.nodes[pn as usize].pts.to_sorted();
    protos.into_iter().all(|p| chain_keys(m, p, seen, out, acc))
}

/// A built-in object was handed to unknown code: the stores into ⊤ made
/// before now may have written it (`written_by`). Lookups parked past it
/// run again, as for the global object (`note_global_in_top`).
pub fn note_handed_to_unknown(m: &mut Model) {
    if m.native_writes.keys().any(|&(w, _)| w == TOP) {
        replay_native_missing(m);
    }
}

/// The global object may now be ⊤: stores into ⊤ may have written globals.
pub fn note_global_in_top(m: &mut Model) {
    m.global_written_own |= !m.shared_replay;
    if !m.global_in_top {
        m.global_in_top = true;
        let fields: Vec<NodeId> = m.global_fields.iter().map(|&(_, n)| n).collect();
        for n in fields {
            global_field_read_by_unknown(m, n);
        }
        replay_native_missing(m);
    }
}

/// Foreign code (a declared extern, `Model::foreign_calls`) was handed a
/// value: it is unknown code beside the runtime that holds ⊤, so once the
/// global object is in ⊤ (`global_in_top`) it may hold it and read every
/// global (`global_field_read_by_unknown`). Owner 2026-11-16: only an
/// all-primitive extern reaches no program object; owner 2026-11-24
/// decision 2 (declared names only) holds where the runtime is the holder.
pub fn note_foreign_holds(m: &mut Model) {
    if m.foreign_holds {
        return;
    }
    m.foreign_holds = true;
    if m.global_in_top {
        let fields: Vec<NodeId> = m.global_fields.iter().map(|&(_, n)| n).collect();
        for n in fields {
            global_field_read_by_unknown(m, n);
        }
    }
}

/// Unknown code that holds the global object (`global_in_top`) reads its
/// properties: what the program stored in global `n` leaves. Whole program
/// that code is the runtime, which reads only the global names the realm
/// declares for it to read (`undeclared::global_store_escapes`: the
/// declared constructors, `members::global_runtime_reads`, `process`, the
/// host objects); the program's own stores into ⊤ are recorded with their
/// keys where they happen (`written_by`). Owner decision 2026-11-24, the
/// read twin of R-GLOBAL (2026-11-18, which dropped that code's any-key
/// write). Per unit, beside an unseen module or interpreted code
/// (`realm_nameable`), or once foreign code holds ⊤ (`note_foreign_holds`:
/// a declared extern was handed a value), other code reads every global.
fn global_field_read_by_unknown(m: &mut Model, n: NodeId) {
    if !m.realm_nameable() && !m.foreign_holds {
        let a = m
            .global_fields
            .iter()
            .find(|&&(_, f)| f == n)
            .map(|&(a, _)| a);
        let g = m
            .tok_index
            .get(&TokKind::Builtin(BuiltinNs::GlobalThis))
            .copied();
        if let (Some(a), Some(g)) = (a, g) {
            if !super::undeclared::global_store_escapes(m, g, Key::Named(a)) {
                return;
            }
        }
    }
    if crate::unksrc::on() {
        let a = m
            .global_fields
            .iter()
            .find(|&&(_, f)| f == n)
            .map(|&(a, _)| a);
        eprintln!(
            "perry: wpfacts unksrc GLOBSINK node={n} name={:?} nameable={} foreign={} global_tok={}",
            a.map(|a| m.atoms[a as usize].clone()),
            m.realm_nameable(),
            m.foreign_holds,
            m.tok_index
                .get(&TokKind::Builtin(BuiltinNs::GlobalThis))
                .is_some(),
        );
    }
    let r = m.reason("global property read by unknown code that holds the global object");
    m.sink_node(n, r);
}

/// Lookups that passed a native prototype (or read a program-owned global)
/// under a key that may now be written: they take the opaque member.
pub(super) fn replay_native_missing(m: &mut Model) {
    replay_native_by(m, None)
}

/// [`replay_native_missing`] after write entry `by` changed (`None`: every
/// lookup): only the lookups whose answer it may change ([`reached`]).
fn replay_native_by(m: &mut Model, by: Option<(Tok, Option<Atom>)>) {
    if m.native_missing.is_empty() && m.native_parked.is_empty() && m.native_answered.is_empty() {
        return;
    }
    replay_pending(m, by);
    let by = by.filter(|_| !realm_window(m));
    // Lookups answered by what the writes stored: refilled, or run as
    // opaque members once an opaque write reaches their key.
    m.native_replays += 1;
    m.with_b(|m| {
        let keys: Vec<(Atom, Tok)> = match by {
            None => m.native_answer_keys.keys().copied().collect(),
            Some(e) => reached(m, &m.native_answer_keys, e),
        };
        m.charge_work(keys.len() as u64);
        let mut opaque: std::collections::HashSet<(Tok, Atom)> = Default::default();
        for (a, np) in keys {
            let Some(&e) = m.native_answer_nodes.get(&(np, a)) else {
                continue;
            };
            match stored_values(m, np, a) {
                Some((stored, absent)) => fill_stored(m, e, &stored, absent),
                None => {
                    m.native_answer_nodes.remove(&(np, a));
                    m.native_answer_keys.remove(&(a, np));
                    opaque.insert((np, a));
                }
            }
        }
        if opaque.is_empty() {
            return;
        }
        let gone: Vec<_> = m
            .native_answered
            .keys()
            .filter(|k| opaque.contains(&(k.0, k.1)))
            .cloned()
            .collect();
        for (np, a, miss) in gone {
            m.native_answered.remove(&(np, a, miss.clone()));
            answer_opaque(m, np, a, miss);
        }
    });
}

/// Lookups parked past a native prototype whose key a write may now reach:
/// answered from the stored values, or run as opaque members.
///
/// `by`: the write entry that changed (`None`: every lookup). The lookups
/// no replay has seen (`native_missing`) always run; of those parked
/// (`Model::native_parked`) only the groups the entry may reach
/// ([`reached`]): a parked lookup's answer changes only by such a write or
/// by an event that replays every lookup (`note_realm_held`,
/// `note_global_in_top`, `note_handed_to_unknown`,
/// `Model::native_replay_all`).
fn replay_pending(m: &mut Model, by: Option<(Tok, Option<Atom>)>) {
    let all = std::mem::take(&mut m.native_replay_all) || realm_window(m);
    let by = by.filter(|_| !all);
    if m.native_missing.is_empty() && m.native_parked.is_empty() {
        return;
    }
    m.native_replay_seq += 1;
    let seq = m.native_replay_seq;
    let mut pending = std::mem::take(&mut m.native_missing);
    let groups: Vec<((Atom, Tok), Vec<NativeMiss>)> = match by {
        None => std::mem::take(&mut m.native_parked).into_iter().collect(),
        Some(e) => reached(m, &m.native_parked, e)
            .into_iter()
            .filter_map(|g| m.native_parked.remove(&g).map(|v| (g, v)))
            .collect(),
    };
    for ((a, np), v) in groups {
        pending.extend(v.into_iter().map(|miss| (np, a, miss)));
    }
    m.charge_work(pending.len() as u64);
    let mut keep = Vec::with_capacity(pending.len());
    m.with_b(|m| {
        for (np, a, miss) in pending {
            m.census_native_read(np, a, miss.clone());
            let written = match miss {
                NativeMiss::Global(dst, _) => {
                    let stored = global_value_stored(m, np, a);
                    if !stored {
                        global_maybe_deleted(m, np, a, dst);
                    }
                    stored
                }
                NativeMiss::New(_) => native_value_written(m, np, a),
                NativeMiss::Lookup(dst, setter) => {
                    let unknown = native_accessor_written(m, np, a);
                    if !unknown {
                        native_accessor_known(m, np, a, dst, setter);
                    }
                    unknown
                }
                _ => native_written(m, np, a),
            };
            if !written {
                keep.push((np, a, miss));
                continue;
            }
            if answer_stored(m, np, a, miss) {
                continue;
            }
            answer_opaque(m, np, a, miss);
        }
    });
    if m.native_replay_seq == seq {
        for (np, a, miss) in keep {
            m.native_parked.entry((a, np)).or_default().push(miss);
        }
    } else {
        // Another replay ran meanwhile (a write it made, an event) or the
        // global object gained a holder: what this one kept is seen again
        // by the next replay, as every lookup is.
        keep.append(&mut m.native_missing);
        m.native_missing = keep;
    }
}

/// A plain store or a delete recorded in write entry `entry` (`value` not
/// opaque): the answered lookups that entry reaches ([`written_by`]) take
/// that one value, or `undefined` for a delete (ECMA-262 10.1.9.2, 10.1.10).
/// No opaque write reaches an answered lookup (one turns it into an opaque
/// member, `replay_native_missing`), so this is all the lookup gains.
fn feed_answered(m: &mut Model, entry: (Tok, Option<Atom>), value: WriteValue) {
    if m.native_answer_nodes.is_empty() {
        return;
    }
    let Some(x) = m.native_writes.get(&entry) else {
        return;
    };
    let x: *const crate::model::NativeWrite = x;
    let t = entry.0;
    // `written_by` counts an entry only on ⊤, the lookup's own token, the
    // global object's tokens or `Object.prototype`.
    let wide = t == TOP
        || super::global_object(m, t)
        || matches!(m.toks[t as usize], TokKind::ObjectProto);
    let keys: Vec<(Atom, Tok)> = if realm_window(m) {
        m.native_answer_keys.keys().copied().collect()
    } else {
        reached(m, &m.native_answer_keys, entry)
    };
    m.charge_work(keys.len() as u64);
    let reached: Vec<NodeId> = keys
        .into_iter()
        .filter(|&(a, np)| {
            (wide || np == t) && {
                let (top, own) = written_by(m, np, a, false, &|y| std::ptr::eq(y, x));
                top || own
            }
        })
        .map(|(a, np)| m.native_answer_keys[&(a, np)])
        .collect();
    m.native_fed += reached.len() as u64;
    m.with_b(|m| {
        for w in reached {
            match value {
                WriteValue::Value(n) => m.edge(n, w),
                WriteValue::Absent => m.add_prim(w, prim::UNDEF),
                WriteValue::Opaque => unreachable!(),
            }
        }
    });
}

/// The values the writes that may have written native prototype `np`'s key
/// `a` stored, and whether one of them removed it (a delete): `None` when
/// one of those writes stored a value no record names (opaque).
fn stored_values(m: &Model, np: Tok, a: Atom) -> Option<(Vec<NodeId>, bool)> {
    let (top, own) = written_by(m, np, a, false, &|w| w.opaque);
    if top || own {
        return None;
    }
    let mut out: Vec<NodeId> = Vec::new();
    let mut absent = false;
    for e in written_entries(m, np, a, false) {
        let w = &m.native_writes[&e];
        for &n in &w.stored {
            if !out.contains(&n) {
                out.push(n);
            }
        }
        absent |= w.absent;
    }
    Some((out, absent))
}

fn fill_stored(m: &mut Model, w: NodeId, stored: &[NodeId], absent: bool) {
    for &n in stored {
        m.edge(n, w);
    }
    if absent {
        m.add_prim(w, prim::UNDEF);
    }
}

/// A read or a method call of native prototype `np`'s key `a` that writes
/// may have written, each with a known stored value: the key holds what a
/// store stored (ECMA-262 10.1.9.2), `undefined` after a delete, or what
/// the chain holds where no write happened (the caller continues the
/// lookup: true). A read yields the stored values; a call calls them, with
/// the receiver as `this` (a primitive receiver: that primitive), and one
/// of a value that is not callable throws (ECMA-262 7.3.14 Call), handing
/// its arguments to no one. False: a write that stored a value no record
/// names (opaque) may have written the key, or the lookup is of another
/// kind; the caller runs today's opaque member.
pub(super) fn answer_stored(m: &mut Model, np: Tok, a: Atom, miss: NativeMiss) -> bool {
    if !matches!(
        miss,
        NativeMiss::Load(_) | NativeMiss::Call(..) | NativeMiss::PrimCall(..)
    ) {
        return false;
    }
    let Some((stored, absent)) = stored_values(m, np, a) else {
        return false;
    };
    if m.native_answered.contains_key(&(np, a, miss)) {
        return true;
    }
    let w = match m.native_answer_nodes.get(&(np, a)) {
        Some(&w) => w,
        None => {
            let w = m.node_l("written-value");
            m.native_answer_nodes.insert((np, a), w);
            m.native_answer_keys.insert((a, np), w);
            w
        }
    };
    match miss {
        NativeMiss::Load(dst) => m.edge(w, dst),
        NativeMiss::Call(origin, call) => m.cons(w, Cons::CallThis { call, this: origin }),
        NativeMiss::PrimCall(p, call) => {
            let c = m.calls[call as usize].clone();
            let this = m.node_l("written-prim-this");
            m.add_prim(this, p);
            // One call per (root of `call`, key, primitive kinds): a stored
            // method this call runs may reach the same written key on the
            // same primitives again (`ObjectProtoMethod` runs every
            // Object.prototype method on its primitive `this`); that is the
            // call already made, joined, not a new one.
            if let Some(call2) = m.solver_call_keyed(
                call,
                (crate::summary::MK_WRITTEN_PRIM, a, p as u32),
                Some(w),
                crate::model::CallSite {
                    first_arg_fixed: false,
                    this: crate::model::ThisArg::Node(this),
                    ..c
                },
            ) {
                m.cons(w, Cons::Call { call: call2 });
            }
        }
        _ => unreachable!(),
    }
    fill_stored(m, w, &stored, absent);
    m.native_answered.insert((np, a, miss), w);
    true
}

/// A lookup of native prototype `np`'s key `a` that the caller answers
/// from the intrinsic's description: parked until a write reaches the key
/// (`replay_native_missing`), or, written already, answered by what the
/// writes stored as well ([`answer_stored`]). True: a write of a value no
/// record names may have written the key: the caller runs today's opaque
/// answer instead.
pub(super) fn opaque_written(m: &mut Model, np: Tok, a: Atom, miss: NativeMiss) -> bool {
    if !native_written(m, np, a) {
        m.native_missing.push((np, a, miss));
        return false;
    }
    !answer_stored(m, np, a, miss)
}

/// Today's opaque member for a lookup whose key a write of an unnamed value
/// may have written.
fn answer_opaque(m: &mut Model, np: Tok, a: Atom, miss: NativeMiss) {
    let saved_gate = m
        .diagnostic
        .as_mut()
        .and_then(|d| d.census.as_mut())
        .map(|c| {
            let old = c.active_gate;
            c.active_gate = Some((np, a));
            old
        });
    match miss {
        NativeMiss::Lookup(dst, _) => {
            m.add_top(dst, |_| {
                "accessor descriptor of a written intrinsic key".into()
            });
        }
        NativeMiss::Load(dst) => {
            if cliffc_diag() && !m.shared_replay {
                static SEEN: std::sync::Mutex<Option<std::collections::HashSet<(u32, u32)>>> =
                    std::sync::Mutex::new(None);
                let first = SEEN
                    .lock()
                    .unwrap()
                    .get_or_insert_with(Default::default)
                    .insert((np as u32, a as u32));
                if first {
                    eprintln!(
                        "WHY CLIFFC opaque n{} {}.{} writers={:?}",
                        dst,
                        m.tok_name(np),
                        m.atoms[a as usize],
                        native_writers_why(m, np, a)
                    );
                }
            }
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
                            m.reasons[r.reason as usize]
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
        NativeMiss::Store(src, site) => {
            let prev = m.store_at.take();
            m.store_into(np, Key::Named(a), src, site);
            m.store_at = prev;
        }
        NativeMiss::Call(origin, call) => {
            let native = m.tok_name(np);
            let name = m.atoms[a as usize].clone();
            if m.explain && !m.shared_replay && name.starts_with(super::WK_PREFIX) {
                let ws: Vec<String> = m
                    .native_writes
                    .iter()
                    .filter(|((_, k), _)| k.is_none() || *k == Some(a))
                    .map(|((w, k), r)| {
                        format!(
                            "{}/{:?}: {}",
                            m.tok_name(*w),
                            k.map(|x| m.atoms[x as usize].clone()),
                            m.reasons[r.reason as usize]
                        )
                    })
                    .collect();
                eprintln!(
                    "perry: wpfacts explain: replay call {native}.{name} by {:?} in_top {} handed {} writes {ws:?}",
                    native_written_by(m, np, a),
                    m.global_in_top,
                    handed_to_unknown(m, np)
                );
            }
            let prev = m.cur_this.replace(origin);
            unknown_method(m, call, &format!("argument of native {native}.{name}"));
            m.cur_this = prev;
        }
        NativeMiss::PrimCall(_, call) => {
            let name = m.atoms[a as usize].clone();
            unknown_prim_method(
                m,
                call,
                &format!("argument of a written primitive method {name}"),
            );
        }
        NativeMiss::New(call) => {
            let name = m.atoms[a as usize].clone();
            let prev = m.cur_this.take();
            unknown_method(m, call, &format!("argument of written constructor {name}"));
            m.cur_this = prev;
        }
    }
    if let Some(old) = saved_gate {
        let c = m.diagnostic.as_mut().unwrap().census.as_mut().unwrap();
        c.active_gate = old;
        c.pending_gate = None;
    }
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

/// The realm's `navigator` (runtime navigator.rs: `realm { navigator, kind =
/// namespace, extends = Navigator.prototype }`), the one object the global
/// getter returns: an object of `Navigator.prototype`, as a declaration's
/// `fresh(Navigator.prototype)` makes, whose loads read the getters'
/// declared results (`effects::declared_getter`).
pub(super) fn navigator_object(m: &mut Model) -> Tok {
    if let Some(t) = m.navigator {
        return t;
    }
    let t = anon_tmpl(m, "Navigator.prototype");
    m.navigator = Some(t);
    global_any_navigator(m);
    t
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
        inert_definite: Vec::new(),
        inert_reads: Vec::new(),
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
        chain_miss: false,
        chain_miss_open: false,
        species: None,
        per_evaluation: false,
        static_methods: Vec::new(),
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

/// A global-object key the program owns: the runtime realm declares it
/// neither on the global object nor on its chain (`Object.prototype`), so
/// only stores the solver sees put a value there (the runtime's unresolved
/// read is `[[Get]](globalThis, name)`, absent = ReferenceError). Such a
/// global holds what the program stored, plus ⊤ once something unmodeled
/// may write it (`native_written`, as for native prototypes).
///
/// Per-unit mode (retiring) keeps the old cut: another unit may store any
/// global, so only `__` names the realm does not declare are owned there.
pub fn global_user_key(m: &Model, name: &str) -> bool {
    name != crate::STAR_KEY
        && (!m.unit_mode || name.starts_with("__"))
        && !["%Global%", "Object.prototype"].into_iter().any(|holder| {
            m.declarations
                .named(holder, m)
                .is_some_and(|s| m.declarations.property(s, name, m).is_some())
        })
}

/// GLOBS13: a method call on the global object (`globalThis.k()`, a
/// receiver that may be the global object) under a program-owned key
/// (`global_user_key`) calls what [[Get]] reads there (ECMA-262 13.3.6.1
/// EvaluateCall, 7.3.21 Invoke): the program's stores, ⊤ once unmodeled
/// code may have written the key, else `undefined`, whose Call throws a
/// TypeError (7.3.14 step 2) and hands no argument on.
pub fn global_user_method(m: &Model, t: Tok, a: Atom) -> bool {
    matches!(m.toks[t as usize], TokKind::Builtin(BuiltinNs::GlobalThis))
        && global_user_key(m, &m.atoms[a as usize])
}

/// S8: a name on the global sentinel (HIR `GlobalGet(0)`: an identifier
/// with no binding; `console` lowers to it too) that is a program-owned key
/// of the global object (`global_user_key`) and that the sentinel does not
/// route by name. Codegen lowers the Console methods (lower_call
/// console_promise.rs; `host_of`), `resolve` / `reject` (the Promise
/// statics) and `fromAsync` (`Array.fromAsync`) on the sentinel itself,
/// whatever the global object holds. Any other such name is
/// [[Get]](globalThis, name).
pub fn sentinel_user_global(m: &Model, name: &str) -> bool {
    global_user_key(m, name)
        && !matches!(name, "resolve" | "reject" | "fromAsync")
        && super::host_of(BuiltinNs::Other, name, true).is_none()
}

/// A global function the realm declares (`fetch`, `setTimeout`), stored
/// over by the program: the global object is an ordinary object (ECMA-262
/// 9.3, 19), so the store makes the value property `name` of it, read by
/// the program's global reads of `name` (`tok_load`: the global's field
/// beside the realm function). No native reads a global function by name:
/// the runtime's by-name reads (`js_get_global_this_builtin_value`) name
/// constructors (`Error`, `URL`, `Uint8Array`, the error and stream
/// classes) and answer intrinsics otherwise (`builtin_constructor_value`),
/// so the value reaches no unknown code. Constructors and namespaces keep
/// the sink.
pub fn program_global_fn(m: &Model, name: &str) -> bool {
    !m.unit_mode
        && name != crate::STAR_KEY
        && super::global_function(m, name)
        && super::ns_by_name(name).is_none()
        && !NATIVE_READ_GLOBALS.contains(&name)
}

/// The global functions a native reads by name after the realm is set up
/// and hands on (`js_get_global_this_builtin_value` / `builtin_value_core`
/// = the live `globalThis[name]`): the module exports equal to the global
/// (perry-runtime native_module/static_exports.rs:209, native_module.rs:1068,
/// constants.rs:597/678, node_submodules/mod.rs:634), the `.constructor` of
/// a fresh text-encoding stream (class_meta.rs:402) and the realm's
/// `Navigator` (navigator.rs:119, at the first `navigator` read). A value
/// the program stores under one of them reaches that native's caller, so
/// the store keeps the sink. (The error classes, `Object`, `SyntaxError`
/// are namespaces: `ns_by_name`.) The other by-name reads are the
/// program's own (bare-name value reads, `instanceof`, the `super()` of a
/// builtin parent: class_return.rs:15) or run before program code
/// (populate.rs).
pub(crate) const NATIVE_READ_GLOBALS: &[&str] = &[
    "URL",
    "URLSearchParams",
    "URLPattern",
    "Blob",
    "File",
    "TextEncoder",
    "TextDecoder",
    "TextEncoderStream",
    "TextDecoderStream",
    "ReadableStream",
    "WritableStream",
    "TransformStream",
    "CompressionStream",
    "DecompressionStream",
    "Navigator",
];

/// GLOBK G2: does a store under an unknown String key on the global object
/// (`global_any_field`) replace global binding `n`? Every string-named
/// own property of the global object, except the non-writable,
/// non-configurable `undefined`, `NaN`, `Infinity` (ECMA-262 19.1: the
/// [[Set]] returns false, a strict store throws) and the getter-only
/// `navigator` (no setter: the same).
pub fn global_any_names(m: &Model, n: &str) -> bool {
    !m.unit_mode
        && !matches!(n, "undefined" | "NaN" | "Infinity" | "navigator")
        && n != crate::STAR_KEY
}

/// G2: what a global binding read names: one global name, the names a
/// computed key's shape spells, or (a key of no shape) every name.
#[derive(Clone, Debug, PartialEq)]
pub enum GlobalAnyRead {
    Name(String),
    Shape(KeyShapeId),
    All,
}

/// GLOBS4: may a store under a String key of shape `s` (0: any String)
/// write what read `r` names? A store writes exactly the key
/// ToPropertyKey gives (ECMA-262 10.1.9.1 OrdinarySet, 7.1.19); a read of
/// name N sees the value only if that key can be N (10.1.8.1 OrdinaryGet).
fn global_any_hits(m: &Model, s: KeyShapeId, r: &GlobalAnyRead) -> bool {
    match r {
        GlobalAnyRead::All => true,
        GlobalAnyRead::Name(n) => m.key_may_name_str(Key::Any(s), n),
        GlobalAnyRead::Shape(k) => m.shapes_meet(s, *k),
    }
}

fn global_any_link(m: &mut Model, r: GlobalAnyRead, dst: NodeId) {
    if m.unit_mode {
        return;
    }
    let hits: Vec<NodeId> = m
        .global_any_fields
        .iter()
        .filter(|(s, _)| global_any_hits(m, *s, &r))
        .map(|(_, f)| *f)
        .collect();
    for f in hits {
        m.edge(f, dst);
    }
    m.global_any_reads.push((r, dst));
}

/// G2: a read of global binding `n` (bare name, `globalThis.n`, a module
/// export equal to a global a native reads by name) also reads what the
/// program stored under unknown String keys that can spell `n` (GLOBS4).
pub fn global_any_read(m: &mut Model, n: &str, dst: NodeId) {
    global_any_link(m, GlobalAnyRead::Name(n.to_string()), dst);
}

/// G2: a read of the global object under `key`: a named key is that
/// binding's read, a shaped key reads the stores whose shape meets it, a
/// key of no shape reads them all.
pub fn global_any_read_key(m: &mut Model, key: Key, dst: NodeId) {
    let r = match key {
        Key::Named(a) => GlobalAnyRead::Name(m.atoms[a as usize].clone()),
        Key::Any(0) => GlobalAnyRead::All,
        Key::Any(s) => GlobalAnyRead::Shape(s),
    };
    global_any_link(m, r, dst);
}

/// G2: `new K(...)` of a literal global name constructs what the program
/// stored under unknown String keys that can spell `K` (13.3.5.1
/// evaluates `K`).
pub fn global_any_new(m: &mut Model, name: &str, call: CallId) {
    if m.unit_mode {
        return;
    }
    let hits: Vec<NodeId> = m
        .global_any_fields
        .iter()
        .filter(|(s, _)| m.key_may_name_str(Key::Any(*s), name))
        .map(|(_, f)| *f)
        .collect();
    for f in hits {
        m.cons(f, Cons::New { call });
    }
    m.global_any_news.push((name.to_string(), call));
}

/// G2: the global object's field under String keys of shape `s` (0: any
/// String), made by the first store under such a key; the reads and
/// constructs its shape can spell are linked to it, those made before it
/// too.
pub fn global_any_field(m: &mut Model, s: KeyShapeId) -> NodeId {
    if let Some(&(_, f)) = m.global_any_fields.iter().find(|(k, _)| *k == s) {
        return f;
    }
    let ga = m.node_l("global-any-key");
    m.global_any_fields.push((s, ga));
    let reads: Vec<NodeId> = m
        .global_any_reads
        .iter()
        .filter(|(r, _)| global_any_hits(m, s, r))
        .map(|(_, d)| *d)
        .collect();
    for d in reads {
        m.edge(ga, d);
    }
    let news: Vec<CallId> = m
        .global_any_news
        .iter()
        .filter(|(n, _)| m.key_may_name_str(Key::Any(s), n))
        .map(|(_, c)| *c)
        .collect();
    for call in news {
        m.cons(ga, Cons::New { call });
    }
    let calls: Vec<CallId> = m
        .runtime_global_calls
        .iter()
        .filter(|(n, _)| m.key_may_name_str(Key::Any(s), n))
        .map(|(_, c)| *c)
        .collect();
    for call in calls {
        m.cons(ga, Cons::Call { call });
    }
    global_any_navigator(m);
    ga
}

/// RTREAD14: the runtime's call `call` of what the global object holds
/// under `name`, a name it reads at run time (`Gen::runtime_global_handlers`):
/// on the name's field and on every any-key field whose key can spell it
/// (made now or later, `global_any_field`).
pub fn runtime_global_call(m: &mut Model, name: &str, call: CallId) {
    let g = m.tok(TokKind::Builtin(BuiltinNs::GlobalThis));
    let a = m.atom(name);
    let f = global_field(m, g, a);
    m.cons(f, Cons::Call { call });
    let hits: Vec<NodeId> = m
        .global_any_fields
        .iter()
        .filter(|(s, _)| m.key_may_name_str(Key::Any(*s), name))
        .map(|(_, f)| *f)
        .collect();
    for f in hits {
        m.cons(f, Cons::Call { call });
    }
    m.runtime_global_calls.push((name.to_string(), call));
}

/// GLOBK3: the global object's field of what a `defineProperty` under a
/// computed key defines there (`Model::define_any_key`): every global
/// binding the any-key field reaches, and `navigator` (a configurable
/// accessor a define replaces, though a store cannot).
pub fn global_def_field(m: &mut Model) -> NodeId {
    if let Some(d) = m.global_def {
        return d;
    }
    let d = m.node_l("global-define-key");
    m.global_def = Some(d);
    let ga = global_any_field(m, 0);
    m.edge(d, ga);
    for n in std::mem::take(&mut m.global_def_navs) {
        m.edge(d, n);
    }
    d
}

/// GLOBK3: a read of `navigator` also reads what a computed-key define
/// put on the global object.
pub fn global_def_nav_read(m: &mut Model, dst: NodeId) {
    if m.unit_mode {
        return;
    }
    match m.global_def {
        Some(d) => m.edge(d, dst),
        None => m.global_def_navs.push(dst),
    }
}

/// G2: the realm's navigator object takes the live `Navigator`'s
/// `prototype` as its [[Prototype]] when the getter first runs
/// (perry-runtime navigator.rs:119 `builtin_value_core("Navigator")`,
/// `navigator_object_with_constructor`): with a value stored under an
/// unknown String key that may be what `Navigator` holds then (a store
/// whose key can spell `Navigator`, GLOBS4).
pub fn global_any_navigator(m: &mut Model) {
    let Some(nav) = m.navigator else {
        return;
    };
    if m.global_any_nav
        || !m
            .global_any_fields
            .iter()
            .any(|(s, _)| m.key_may_name_str(Key::Any(*s), "Navigator"))
    {
        return;
    }
    m.global_any_nav = true;
    let r = m.node_l("global-any-Navigator");
    let p = m.node_l("global-any-Navigator.prototype");
    let key = Key::Named(m.atom("prototype"));
    m.cons(r, Cons::Load { key, dst: p });
    m.given_prototype(nav, p);
    global_any_read(m, "Navigator", r);
}

/// G2 off: a native the model does not follow reads a global by name and
/// hands the value on (`TextEncoderStream` / `TextDecoderStream`
/// `.constructor`, perry-runtime class_meta): the stores under unknown
/// String keys take the realm's sink as before G2.
pub fn global_any_disable(m: &mut Model) {
    if m.global_any_off || m.unit_mode {
        return;
    }
    m.global_any_off = true;
    for (t, key, src, site) in std::mem::take(&mut m.global_any_stores) {
        let prev = m.store_at.take();
        m.store_into(t, key, src, site);
        m.store_at = prev;
    }
    // GLOBK3: the computed-key defines on the global object as well.
    for (d, g) in std::mem::take(&mut m.global_defs) {
        m.define_any_old(d, g);
    }
}

/// The global object's field under program-owned key `a`.
pub fn global_field(m: &mut Model, g: Tok, a: Atom) -> NodeId {
    if let Some(&(_, n)) = m.global_fields.iter().find(|(k, _)| *k == a) {
        return n;
    }
    let n = m.field(g, a);
    m.global_fields.push((a, n));
    if m.global_in_top {
        global_field_read_by_unknown(m, n);
    }
    super::loads::global_shaped_replay(m, a);
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
            if let Some(&f) = c.arg(1) {
                let a = m.atom("#listeners");
                let fl = m.field(origin, a);
                m.edge(f, fl);
            }
            ret_tok(m, call, origin);
        }
        ("EventEmitter", "emit") => {
            // `emit("error", er)` with no `error` listener throws `er` itself
            // when it is an Error (`event_emitter_error.rs`); otherwise a
            // runtime error (always in `thrown`).
            let thrown = m.calls[call as usize].region;
            if let Some(&er) = c.arg(1) {
                m.edge(er, thrown);
            }
            for &s in &c.spread {
                let key = m.elem_key();
                m.cons(s, Cons::Load { key, dst: thrown });
            }
            let a = m.atom("#listeners");
            let fl = m.field(origin, a);
            let ret = m.node_l("emit-ret");
            let call2 = m.calls.len() as CallId;
            let _call2 = m.chain_call(
                call,
                0,
                fl,
                CallSite {
                    first_arg_fixed: false,
                    this: ThisArg::Tok(origin),
                    args: c.args_from(1).iter().copied().collect(),
                    spread: c.spread.clone(),
                    ret,
                    site: call2,
                    region: m.calls[call as usize].region,
                },
                move |m, tg, call2| m.cons(tg, Cons::Call { call: call2 }),
            );
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
        // `Array.prototype` taken as a value (`Object.create(Array.prototype)`,
        // a prototype given to an array): the runtime refuses to replace its
        // image methods (DEC 250), so the method found is the intrinsic one,
        // run with the receiver as `this` (`native_receiver`), as the call of
        // that method's value is.
        ("Array", n)
            if array_image_method(n) && native_receiver(m, Family::Array, None, origin) =>
        {
            array_method_on(m, origin, n, call)
        }
        // Past `Array.prototype` taken as a value, under a key it does not own
        // in the image and no write may have added: the lookup continues on
        // `Object.prototype`, add-locked and image-locked (DEC 250), so what
        // it finds there is final. Its method runs with the receiver as
        // `this` (the models `proto_method` gives `Object.prototype`'s own
        // methods); a key absent on both is no callee (the call throws).
        // Parked: a later write of the key onto `Array.prototype` (or one that
        // may reach it) replays the call as the opaque written member.
        ("Array", n) if members::own(Family::Array, n).is_none() && !native_written(m, np, a) => {
            m.native_missing
                .push((np, a, NativeMiss::Call(origin, call)));
            match members::member(Family::Object, n) {
                Some(Mem::Fn) => {
                    let op = m.tok(TokKind::ObjectProto);
                    proto_method(m, op, origin, Key::Named(a), call);
                }
                None => {}
                Some(_) => unknown_method(m, call, &format!("argument of native {native}.{name}")),
            }
        }
        // A typed array's prototypes, under a key neither `%TypedArray%.prototype`
        // nor the kind's own prototype declares (ECMA-262 23.2.3, 23.2.7: the
        // realm's declarations list their members) and no write may have
        // added: [[Get]] continues on `Object.prototype` (as for `Array`
        // above), whose method runs with the receiver as `this`
        // (`Object.prototype.valueOf`, 20.1.3.7: ToObject(this value), no
        // program code). A well-known symbol key stays as before.
        (n0, n)
            if typed_array_proto_name(n0)
                && !n.starts_with("@@")
                && !typed_array_proto_declares(m, n0, n)
                && !native_written(m, np, a) =>
        {
            m.native_missing
                .push((np, a, NativeMiss::Call(origin, call)));
            match members::member(Family::Object, n) {
                Some(Mem::Fn) => {
                    let op = m.tok(TokKind::ObjectProto);
                    proto_method(m, op, origin, Key::Named(a), call);
                }
                None => {}
                Some(_) => unknown_method(m, call, &format!("argument of native {native}.{name}")),
            }
        }
        (_, "toString") => ret_prim(m, call, prim::STR),
        _ => unknown_method(m, call, &format!("argument of native {native}.{name}")),
    }
}

/// Is `name` a method `Array.prototype` owns in the image, whose read there
/// is final (`members::locked_read`)? `constructor`, a data member and an
/// absent key (`Array.prototype` takes named adds) are not.
fn array_image_method(name: &str) -> bool {
    matches!(members::member(Family::Array, name), Some(Mem::Fn))
        && members::locked_read(Family::Array, None, name)
}

pub(crate) fn census_native_written(m: &Model, t: Tok, a: Atom) -> bool {
    native_written(m, t, a)
}
pub(crate) fn census_native_op_writes(m: &Model, np: Tok) -> bool {
    match &m.toks[np as usize] {
        TokKind::NativeProto(_, ns) => ns.is_some(),
        _ => super::global_object(m, np),
    }
}

/// `%TypedArray%.prototype` or a concrete kind's own prototype.
fn typed_array_proto_name(n: &str) -> bool {
    matches!(
        n,
        "%TypedArray%"
            | "Int8Array"
            | "Uint8Array"
            | "Uint8ClampedArray"
            | "Int16Array"
            | "Uint16Array"
            | "Int32Array"
            | "Uint32Array"
            | "Float16Array"
            | "Float32Array"
            | "Float64Array"
            | "BigInt64Array"
            | "BigUint64Array"
    )
}

/// The realm declares `name` on `%TypedArray%.prototype` or on the kind's own
/// prototype (`constructor`, `BYTES_PER_ELEMENT`), or on `Uint8Array.prototype`
/// (base64/hex: those keep today's model on every kind).
fn typed_array_proto_declares(m: &Model, native: &str, name: &str) -> bool {
    let declared = |holder: Option<usize>| {
        holder.is_some_and(|h| m.declarations.property(h, name, m).is_some())
    };
    declared(m.declarations.named("%TypedArray%.prototype", m))
        || declared(m.declarations.named("Uint8Array.prototype", m))
        || (native != "%TypedArray%"
            && declared(m.declarations.named(&format!("{native}.prototype"), m)))
}

/// GUARDS1 class 3: may a write the program can make reach a typed view's
/// structural metadata accessors (`length`, `byteLength`, `byteOffset`,
/// `buffer` on `%TypedArray%.prototype`, a kind's own prototype or
/// `Buffer.prototype`) or change one of those prototypes' [[Prototype]]?
/// The runtime withdraws its default-accessor proof
/// (`PERRY_TYPED_NAMED_PROPS_INVALIDATED`, perry-runtime
/// `typedarray_named::note_named_mutation` / `note_prototype_mutation`)
/// exactly on such a mutation. `None`: compiled code never makes one. Every
/// write onto a builtin prototype is a `native_writes` entry (a key of
/// `None`, `*` or a number: any of these keys), every prototype change of a
/// builtin object a `native_given` entry; `Object.prototype` is counted as
/// standing for every builtin prototype, a write onto ⊤ is recorded only
/// when foreign code exists (P-N1), and unknown code holding the realm may
/// write these keys (they are not final under the lock, `realm_replaced`).
pub(crate) fn typed_metadata_writes(m: &Model) -> Option<String> {
    let holder = |t: Tok| {
        t == TOP
            || match &m.toks[t as usize] {
                TokKind::ObjectProto => true,
                TokKind::NativeProto(n, _) => typed_array_proto_name(n) || n == "Buffer",
                _ => false,
            }
    };
    let keyed = |k: Option<Atom>| {
        k.is_none_or(|a| {
            matches!(
                m.atoms[a as usize].as_str(),
                "length" | "byteLength" | "byteOffset" | "buffer" | "__proto__"
            ) || m.atoms[a as usize] == crate::STAR_KEY
                || m.atoms[a as usize] == crate::model::NUM_KEY
        })
    };
    chain_writes(m, &holder, &keyed)
}

/// GUARDS1 class 2: may a write the program can make modify
/// `%Array.prototype%` or `%Object.prototype%` (any key, or their
/// [[Prototype]])? The runtime records exactly that in
/// `ARRAY_METHODS_MODIFIED` (perry-runtime `builtin_proto::note_write` for the
/// Array and Object families), the bit the inline `push`/`pop` admission and
/// the confined element guard test. `None`: compiled code never makes one.
pub(crate) fn array_methods_writes(m: &Model) -> Option<String> {
    let holder = |t: Tok| {
        t == TOP
            || match &m.toks[t as usize] {
                TokKind::ObjectProto => true,
                TokKind::NativeProto(n, _) => n == "Array",
                _ => false,
            }
    };
    chain_writes(m, &holder, &|_| true)
}

/// The first reason (by id, independent of hashing) of a write onto a
/// `holder` under a `keyed` key, of a prototype change of a `holder`, or
/// unknown code holding the realm.
fn chain_writes(
    m: &Model,
    holder: &dyn Fn(Tok) -> bool,
    keyed: &dyn Fn(Option<Atom>) -> bool,
) -> Option<String> {
    if m.realm_held {
        return Some("unknown code holds the realm".to_string());
    }
    // An `ObjectProto` entry counts only where the write may land on
    // `%Object.prototype%` itself (`Model::objproto_direct`); one made only
    // through another builtin prototype is recorded under that prototype.
    let op = |t: Tok| t != TOP && matches!(m.toks[t as usize], TokKind::ObjectProto);
    let op_held = m
        .tok_index
        .get(&TokKind::ObjectProto)
        .is_some_and(|&t| holder(t));
    let direct = m
        .objproto_direct
        .iter()
        .filter(|&(&k, _)| op_held && keyed(k))
        .map(|(_, &r)| r);
    let first = m
        .native_writes
        .iter()
        .filter(|(&(t, k), _)| !op(t) && holder(t) && keyed(k))
        .map(|(_, w)| w.reason)
        .chain(direct)
        .min();
    if let Some(r) = first {
        return Some(m.reasons[r as usize].clone());
    }
    m.native_given
        .iter()
        .find(|&&(t, _)| holder(t))
        .map(|&(t, _)| format!("E6 prototype given to {}", m.tok_name(t)))
}
