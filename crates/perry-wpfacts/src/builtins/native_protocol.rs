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

/// The value of the declared constructor that class `tm`'s built-in parent
/// names, read where the heritage binding reads it: the global object's
/// (`extends Error`: the realm's `Error`) or the node module's export
/// (`import { EventEmitter } from "events"`: the module's `EventEmitter`,
/// the value the class object's [[Prototype]] holds). `None` when the class
/// has no built-in parent or its holder declares none under that name.
pub fn native_parent_value(m: &mut Model, tm: TmplId) -> Option<Tok> {
    let (module, name) = m.tmpls[tm as usize].native_parent.clone()?;
    let ns = if module == "builtin" {
        BuiltinNs::GlobalThis
    } else {
        let ns = node_module_ns(&module)?;
        m.tok(TokKind::Builtin(ns));
        ns
    };
    let identity = m.namespace_property(ns, &name)?.native?;
    Some(m.tok(TokKind::BuiltinFn(Native::Member {
        identity,
        holder: None,
    })))
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
    let (top, own) = native_written_by(m, np, a);
    top || own
}

/// [`native_written`], by its writer: `.0` a store into ⊤ (which may be any
/// builtin prototype), `.1` a write onto `np` itself (for the global object
/// or a global's native prototype, onto `Object.prototype` too).
pub fn native_written_by(m: &Model, np: Tok, a: Atom) -> (bool, bool) {
    written_by(m, np, a, false, &|_| true)
}

/// [`native_written`] for an accessor descriptor query (`__lookupGetter__`,
/// a getter walk): only writes that may install an accessor
/// ([`crate::model::NativeWrite`]) count; a plain store makes a data
/// property (ECMA-262 10.1.9.2), which such a query never returns.
pub fn native_accessor_written(m: &Model, np: Tok, a: Atom) -> bool {
    let (top, own) = written_by(m, np, a, false, &|w| w.accessor);
    top || own
}

/// The known getters (setters if `setter`) that writes reaching `np`'s key
/// `a` installed (definitions through ⊤ with a known descriptor), into
/// `dst`: what an accessor descriptor query that passes `np` returns
/// besides `undefined` (ECMA-262 10.1.6.3: the descriptor's own functions).
pub fn native_accessor_known(m: &mut Model, np: Tok, a: Atom, dst: NodeId, setter: bool) {
    let mut found: Vec<Tok> = Vec::new();
    for w in m.native_writes.values() {
        let fns = if setter { &w.setters } else { &w.getters };
        if fns.is_empty() {
            continue;
        }
        let (top, own) = written_by(m, np, a, false, &|x| std::ptr::eq(x, w));
        if top || own {
            found.extend(fns.iter().copied());
        }
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
            .iter()
            .any(|(&(w, k), x)| w == t && counts(x) && keyed(k))
    };
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
    // A named write onto the global object that only a program store made:
    // its value is in the global's field.
    let stored = |t: Tok| {
        field_carries_stores
            && super::global_object(m, t)
            && !m.accessors.contains_key(&(t, a))
            && !m.native_given.iter().any(|&(w, _)| w == t)
    };
    let hit = |t: Tok| {
        has(t, None)
            || (has(t, Some(a)) && !stored(t))
            || num.is_some_and(|n| has(t, Some(n)))
            || (any && any_on(t, &|_| true))
            || (num_read
                && any_on(t, &|k| {
                    k.is_some_and(|k| crate::solve::numeric_key(&m.atoms[k as usize]))
                }))
    };
    // The HIR global sentinel (`Other`, what a bare global name is read
    // off) is the global object: a write through either token is a write
    // of the one object.
    let is_global = super::global_object(m, np);
    let global = match &m.toks[np as usize] {
        TokKind::NativeProto(n) => ns_by_name(n).is_some(),
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
        hit(np)
            || other_global(np)
            // A prototype write is a chain contribution, separate from
            // the namespace's own hit above and its TOP reachability.
            || (global
                && !locked_chain_read(m, np, a)
                && m.tok_index
                    .get(&TokKind::ObjectProto)
                    .is_some_and(|&op| hit(op))),
    )
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
    let TokKind::NativeProto(native) = &m.toks[np as usize] else {
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
    note_write(m, t, key, "accessor", false, Some((f, setter)), WriteValue::Opaque)
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
fn write_covered(m: &Model, t: Tok, k: Option<Atom>, accessor: bool, value: WriteValue) -> bool {
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
        m.global_written_own |= super::global_object(m, t);
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
    replay_native_missing(m);
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
        let known = m.nodes[src as usize]
            .pts
            .to_sorted()
            .into_iter()
            .all(|u| chain_keys(m, u, &mut seen, &mut keys));
        let ks: Vec<Option<Atom>> = match known {
            true => keys.into_iter().map(Some).collect(),
            false => vec![None],
        };
        for k in ks {
            if write_covered(m, t, k, true, WriteValue::Opaque) {
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
            note_native_write(m, t, k.map(Key::Named), "E6 prototype given");
            m.census_settling(None);
            if let (Some(d), Some(saved)) = (m.diagnostic.as_mut(), saved) {
                (d.cur_call, d.cause_node) = saved;
            }
        }
    }
    more
}

/// Collect into `out` every key an object of token `u` or of its chain may
/// hold. False when that set is unknown. Each key a define (E5) or an
/// accessor gives it is one of `define_on` / `accessor_keys` (`STAR_KEY`
/// under a computed key: unknown), so E5 alone keeps the set known. `ObjectProto` adds none: it stands
/// for every builtin prototype, already on every runtime object's chain. A
/// write through ⊤ is already a write of its key on every runtime object.
fn chain_keys(m: &mut Model, u: Tok, seen: &mut Vec<Tok>, out: &mut Vec<Atom>) -> bool {
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
    }
    // A key defined, or an accessor installed, under a computed key.
    let star = m.atom(crate::STAR_KEY);
    if out.contains(&star) {
        return false;
    }
    let pn = m.proto_node(u);
    let protos: Vec<Tok> = m.nodes[pn as usize].pts.to_sorted();
    protos.into_iter().all(|p| chain_keys(m, p, seen, out))
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
        replay_native_missing(m);
    }
}

/// Lookups that passed a native prototype (or read a program-owned global)
/// under a key that may now be written: they take the opaque member.
pub(super) fn replay_native_missing(m: &mut Model) {
    if m.native_missing.is_empty() && m.native_answered.is_empty() {
        return;
    }
    let pending = std::mem::take(&mut m.native_missing);
    let mut keep = Vec::with_capacity(pending.len());
    m.with_b(|m| {
        for (np, a, miss) in pending {
            m.census_native_read(np, a, miss.clone());
            let written = match miss {
                NativeMiss::Global(..) => global_value_written(m, np, a),
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
        // Lookups answered by what the writes stored: refilled, or run as
        // opaque members once an opaque write reaches their key.
        let answered: Vec<_> = m.native_answered.iter().map(|(&k, &w)| (k, w)).collect();
        for ((np, a, miss), w) in answered {
            match stored_values(m, np, a) {
                Some((stored, absent)) => fill_stored(m, w, &stored, absent),
                None => {
                    m.native_answered.remove(&(np, a, miss));
                    answer_opaque(m, np, a, miss);
                }
            }
        }
    });
    keep.append(&mut m.native_missing);
    m.native_missing = keep;
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
    for w in m.native_writes.values() {
        if w.stored.is_empty() && !w.absent {
            continue;
        }
        let (top, own) = written_by(m, np, a, false, &|x| std::ptr::eq(x, w));
        if top || own {
            for &n in &w.stored {
                if !out.contains(&n) {
                    out.push(n);
                }
            }
            absent |= w.absent;
        }
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
    let w = m.node_l("written-value");
    match miss {
        NativeMiss::Load(dst) => m.edge(w, dst),
        NativeMiss::Call(origin, call) => m.cons(w, Cons::CallThis { call, this: origin }),
        NativeMiss::PrimCall(p, call) => {
            let c = m.calls[call as usize].clone();
            let this = m.node_l("written-prim-this");
            m.add_prim(this, p);
            let call2 = m.solver_call(
                call,
                crate::model::CallSite {
                    this: crate::model::ThisArg::Node(this),
                    ..c
                },
            );
            m.cons(w, Cons::Call { call: call2 });
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
            // `emit("error", er)` with no `error` listener throws `er` itself
            // when it is an Error (`event_emitter_error.rs`); otherwise a
            // runtime error (always in `thrown`).
            let thrown = m.calls[call as usize].region;
            if let Some(&er) = c.args.get(1) {
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
            m.solver_call(
                call,
                CallSite {
                    this: ThisArg::Tok(origin),
                    args: c.args.iter().skip(1).copied().collect(),
                    spread: c.spread.clone(),
                    ret,
                    site: call2,
                    region: m.thrown,
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
        TokKind::NativeProto(n) => ns_by_name(n).is_some(),
        _ => super::global_object(m, np),
    }
}
