//! Loads of builtin objects: what reading a key of a family, namespace, primitive or token yields.

use super::*;

/// The family's `constructor`.
pub(super) fn family_ctor(m: &mut Model, fam: Family, dst: NodeId) {
    let nss: &[BuiltinNs] = match fam {
        Family::Object => &[BuiltinNs::Object],
        Family::Function => &[BuiltinNs::Function],
        Family::Array => &[BuiltinNs::Array],
        Family::Str => &[BuiltinNs::String],
        Family::Num => &[BuiltinNs::Number],
        Family::BigInt => &[BuiltinNs::BigInt],
        Family::Bool => &[BuiltinNs::Boolean],
        Family::Sym => &[BuiltinNs::Symbol],
        Family::Collection => &[
            BuiltinNs::Map,
            BuiltinNs::Set,
            BuiltinNs::WeakMap,
            BuiltinNs::WeakSet,
        ],
        Family::Promise => &[BuiltinNs::Promise],
        // An error object's template does not keep its kind.
        Family::Error => {
            for ns in [BuiltinNs::Error, BuiltinNs::AggregateError]
                .into_iter()
                .chain(NATIVE_ERRORS.map(BuiltinNs::NativeError))
            {
                let b = m.tok(TokKind::Builtin(ns));
                m.add_tok(dst, b);
            }
            return;
        }
        Family::Box => &[BuiltinNs::Object, BuiltinNs::Array],
        Family::Wrapper => &[
            BuiltinNs::Object,
            BuiltinNs::String,
            BuiltinNs::Number,
            BuiltinNs::BigInt,
            BuiltinNs::Boolean,
            BuiltinNs::Symbol,
        ],
        // %Iterator% and the constructors of undescribed families: opaque.
        Family::Iterator | Family::Opaque => {
            let bf = builtin_fn(m, "constructor of an undescribed family");
            m.add_tok(dst, bf);
            return;
        }
    };
    for &ns in nss {
        let b = m.tok(TokKind::Builtin(ns));
        m.add_tok(dst, b);
    }
}

/// `dst` ⊇ what builtin object `t` of family `fam` reads under named key `a`
/// from its intrinsic chain: the described member, or the opaque member
/// once the key may have been written (own data fields come from the field
/// storage, `load_from`; an absent key reads nothing more).
pub(super) fn family_load(
    m: &mut Model,
    t: Tok,
    fam: Family,
    what: Option<&'static str>,
    a: Atom,
    dst: NodeId,
) {
    let name = m.atoms[a as usize].clone();
    // An internal slot (`effects::internal_atom`) is no property: no chain
    // has it. Its storage is an ordinary own field of the object, read by
    // `load_from` (an unknown-key store reaches it through `field_any`, and
    // an escape of the object sinks ⊤ into it); only the chain read stops.
    if super::effects::is_internal_key(&name) {
        return;
    }
    if fam == Family::Opaque && member_native(m, fam, what, &name).is_none() {
        if buffer_member_absent(m, what, &name) {
            m.add_prim(dst, prim::UNDEF);
        } else if let Some(e) = declared_accessor(m, fam, what, &name) {
            // An accessor whose getter the runtime declares (`RegExp
            // .prototype.flags`): the getter's declared result on `t`, as
            // the declared getter of a declaration-made prototype reads.
            super::effects::getter_load(m, e, t, a, dst);
        } else if realm_accessor(m, fam, what, &name) {
            undeclared_getter_result(m, dst);
        } else {
            undescribed_member(m, dst);
        }
        if !intrinsic_miss(m, a, NativeMiss::Load(dst)) {
            opaque_member(m, dst);
        }
        return;
    }
    family_member(m, fam, what, a, dst);
    // Described or not, a key a store into ⊤ may have written onto the
    // chain reads what was stored, unless the read is final (DEC 250).
    if !intrinsic_unwritten(m, fam, what, a, NativeMiss::Load(dst)) {
        opaque_member(m, dst);
    }
}

/// `dst` ⊇ the described member `a` of family `fam`'s intrinsic chain (of a
/// value of kind `what`), as the realm defines it: the native, the data
/// kinds, the constructor, the prototype. What a store wrote there is the
/// caller's (`family_load` per key, `family_any` once for any key).
pub(super) fn family_member(
    m: &mut Model,
    fam: Family,
    what: Option<&'static str>,
    a: Atom,
    dst: NodeId,
) {
    let name = m.atoms[a as usize].clone();
    match members::member(fam, &name) {
        // An accessor of the realm (a getter the model lists as a method):
        // the getter's result, never the getter or a method.
        Some(Mem::Fn) if realm_accessor(m, fam, what, &name) => undeclared_getter_result(m, dst),
        Some(Mem::Fn) => {
            let bf = match member_native(m, fam, what, &name) {
                Some(n) => {
                    if m.explain {
                        *m.bf_mints.entry("described member (named)").or_insert(0) += 1;
                    }
                    m.tok(TokKind::BuiltinFn(n))
                }
                None => builtin_fn(m, "described member"),
            };
            m.add_tok(dst, bf);
        }
        Some(Mem::Prim(p)) => m.add_prim(dst, p),
        // An error object's template does not keep its kind (`error_tok`,
        // the runtime's errors): its `constructor` is any Error-family one.
        Some(Mem::Ctor) => {
            if let Some(n) = (!m.unit_mode && fam != Family::Error)
                .then(|| member_native(m, fam, what, &name))
                .flatten()
            {
                let f = m.tok(TokKind::BuiltinFn(n));
                m.add_tok(dst, f);
            } else {
                family_ctor(m, fam, dst);
            }
        }
        Some(Mem::Proto) => {
            let op = m.tok(TokKind::ObjectProto);
            m.add_tok(dst, op);
        }
        None => {}
    }
}

/// Is `name` absent from the intrinsic chain of a `DataView`, `ArrayBuffer`
/// or `SharedArrayBuffer` (its own prototype, then `Object.prototype`)? The
/// realm declares those prototypes' string keys exactly and none of them an
/// `@@iterator`; another symbol key is left undescribed.
pub(super) fn buffer_member_absent(m: &Model, what: Option<&str>, name: &str) -> bool {
    let Some(w @ ("DataView" | "ArrayBuffer" | "SharedArrayBuffer")) = what else {
        return false;
    };
    if name.starts_with("@@") && name != "@@__perry_wk_iterator" {
        return false;
    }
    let declared = |m: &Model, holder: Option<usize>| {
        holder.is_some_and(|h| m.declarations.property(h, name, m).is_some())
    };
    !declared(m, m.declarations.named(&format!("{w}.prototype"), m))
        && !declared(m, m.declarations.family(Family::Object, m))
}

/// The family (and site kind) of the intrinsic prototype builtin namespace
/// `ns` holds under `prototype` (the inverse of `family_ctor`), and the
/// global binding that names `ns`; `None`: no described prototype, or not
/// a constructor.
pub(super) fn proto_family(ns: BuiltinNs) -> Option<(Family, Option<&'static str>, &'static str)> {
    Some(match ns {
        BuiltinNs::Object => (Family::Object, None, "Object"),
        BuiltinNs::Function => (Family::Function, None, "Function"),
        BuiltinNs::Array => (Family::Array, None, "Array"),
        BuiltinNs::String => (Family::Str, None, "String"),
        BuiltinNs::Number => (Family::Num, None, "Number"),
        BuiltinNs::BigInt => (Family::BigInt, None, "BigInt"),
        BuiltinNs::Boolean => (Family::Bool, None, "Boolean"),
        BuiltinNs::Symbol => (Family::Sym, None, "Symbol"),
        BuiltinNs::Promise => (Family::Promise, Some("Promise"), "Promise"),
        BuiltinNs::Map => (Family::Collection, Some("Map"), "Map"),
        BuiltinNs::Set => (Family::Collection, Some("Set"), "Set"),
        BuiltinNs::WeakMap => (Family::Collection, Some("WeakMap"), "WeakMap"),
        BuiltinNs::WeakSet => (Family::Collection, Some("WeakSet"), "WeakSet"),
        BuiltinNs::Error => (Family::Error, None, "Error"),
        BuiltinNs::NativeError(n) => (Family::Error, Some(n), n),
        BuiltinNs::AggregateError => (Family::Error, Some("AggregateError"), "AggregateError"),
        BuiltinNs::Date => (Family::Opaque, Some("Date"), "Date"),
        BuiltinNs::RegExp => (Family::Opaque, Some("RegExp"), "RegExp"),
        // A concrete typed array's own prototype holds `constructor`,
        // `BYTES_PER_ELEMENT` (and `Uint8Array`'s base64/hex methods); the
        // rest of its members are `%TypedArray%.prototype`'s.
        BuiltinNs::TypedArray => (Family::Opaque, Some("TypedArray"), "%TypedArray%"),
        BuiltinNs::TypedArrayKind(n) => (Family::Opaque, Some("TypedArray"), n),
        BuiltinNs::DataView => (Family::Opaque, Some("DataView"), "DataView"),
        BuiltinNs::ArrayBuffer => (Family::Opaque, Some("ArrayBuffer"), "ArrayBuffer"),
        BuiltinNs::SharedArrayBuffer => (
            Family::Opaque,
            Some("SharedArrayBuffer"),
            "SharedArrayBuffer",
        ),
        _ => return None,
    })
}

/// A named member read directly from a constructor's intrinsic prototype.
/// The generated realm locates its function identity. Unlocked member and
/// global-binding writes retain the ordinary written-key replay.
pub(crate) fn proto_own_native(
    m: &mut Model,
    ns: BuiltinNs,
    a: Atom,
    dst: NodeId,
) -> Option<Native> {
    let (fam, what, global) = proto_family(ns)?;
    let name = m.atoms[a as usize].clone();
    if !matches!(members::member(fam, &name), Some(Mem::Fn))
        && (m.unit_mode || !matches!(members::member(fam, &name), Some(Mem::Ctor)))
    {
        return None;
    }
    let native = member_native(m, fam, what, &name)?;
    let g = m.tok(TokKind::Builtin(BuiltinNs::GlobalThis));
    let ga = m.atom(global);
    if native_written(m, g, ga) {
        return None;
    }
    m.native_missing.push((g, ga, NativeMiss::Load(dst)));
    if !intrinsic_unwritten(m, fam, what, a, NativeMiss::Load(dst)) {
        opaque_member(m, dst);
    }
    Some(native)
}

/// `dst` ⊇ what builtin namespace `ns` (token `t`) reads under named key
/// `a`: its described static, else its chain's member (`Function.prototype`
/// or `Object.prototype`), else nothing (absent: `undefined`). A key the
/// program may have written onto the namespace itself, or onto a builtin
/// prototype of its chain, reads the opaque member.
/// The node module object the realm declares under node module token `t`'s
/// data property `key` (`node:inspector.default` for `default`).
fn module_object(m: &Model, t: Tok, key: &str) -> Option<BuiltinNs> {
    m.native_property(t, key)
        .and_then(|p| p.object)
        .and_then(super::node_module)
}

/// The declared getter (`declared_getter`) of the accessor host object token
/// `t` holds or inherits under `key` (the realm's `accessor` rows: ordinary
/// lookup on the object's chain).
fn module_getter(
    m: &Model,
    t: Tok,
    key: &str,
) -> Option<&'static perry_hir::realm_manifest::RealmEntry> {
    let getter = m.native_property(t, key)?.get?;
    let (proto, k) = getter.strip_suffix(".<get>")?.rsplit_once('.')?;
    super::effects::declared_getter(proto, k)
}

pub(super) fn ns_load(m: &mut Model, t: Tok, ns: BuiltinNs, a: Atom, dst: NodeId) {
    m.census_native_read(t, a, NativeMiss::Load(dst));
    let name = m.atoms[a as usize].clone();
    if namespace_getter_load(m, t, t, ns, a, dst) {
        return;
    }
    if !m.unit_mode {
        if let Some(n) = namespace_native(m, t, ns, &name) {
            if native_written(m, t, a) {
                return opaque_member(m, dst);
            }
            m.native_missing.push((t, a, NativeMiss::Load(dst)));
            let f = m.tok(TokKind::BuiltinFn(n));
            m.add_tok(dst, f);
            return;
        }
    }
    let Some(mem) = members::ns_member(ns, &name) else {
        undescribed_member(m, dst);
        if native_written(m, t, a) {
            return opaque_member(m, dst);
        }
        m.native_missing.push((t, a, NativeMiss::Load(dst)));
        return;
    };
    if native_written(m, t, a) {
        return opaque_member(m, dst);
    }
    // A write of the key onto the namespace later in the solve.
    m.native_missing.push((t, a, NativeMiss::Load(dst)));
    match mem {
        Some(Mem::Fn) => {
            let bf = builtin_fn(m, "builtin namespace static");
            m.add_tok(dst, bf);
        }
        Some(Mem::Prim(p)) => m.add_prim(dst, p),
        // `@@species` is the constructor itself; `constructor` is the
        // chain's (`Function` for a constructor, `Object` for a namespace).
        Some(Mem::Ctor) if name == "@@__perry_wk_species" => m.add_tok(dst, t),
        Some(Mem::Ctor) => family_ctor(m, members::ns_chain(ns), dst),
        Some(Mem::Proto) => builtin_proto_value(m, (name == "prototype").then_some(ns), dst),
        None => m.add_prim(dst, prim::UNDEF),
    }
    // Absent on the namespace and on its chain: a key the program writes
    // onto a builtin prototype is a member of the chain then.
    if mem.is_none() && !locked_chain_read(m, t, a) && !intrinsic_miss(m, a, NativeMiss::Load(dst))
    {
        opaque_member(m, dst);
    }
}

// -------------------------------------------------------------- loads

pub fn prim_load(m: &mut Model, prim: u16, key: Key, dst: NodeId) {
    if prim & (prim::NULL | prim::UNDEF) == prim {
        return; // TypeError
    }
    // A primitive is no builtin `IterResult`: its protocol value is `value`.
    let key = if m.is_yield_key(key) {
        Key::Named(m.atom("value"))
    } else {
        key
    };
    let a = match key {
        // The elements of a string: its characters.
        k if m.is_elem_key(k) || m.is_iter_key(k) => {
            return m.add_prim(dst, prim::STR | prim::UNDEF);
        }
        // A computed key: a character of a string, or a member of the
        // chain of each kind under a name the key may spell.
        Key::Any(_) => {
            if prim & prim::STR != 0 {
                m.add_prim(dst, prim::STR | prim::UNDEF);
            }
            for fam in members::prim_families(prim) {
                family_any(m, None, fam, None, key, dst);
            }
            return;
        }
        Key::Named(a) => a,
    };
    let name = m.atoms[a as usize].clone();
    if members::is_index_name(&name) {
        return m.add_prim(dst, prim::STR | prim::UNDEF);
    }
    // A kind whose chain lacks the member reads `undefined`.
    if !members::prim_families(prim).all(|f| members::member(f, &name).is_some()) {
        m.add_prim(dst, prim::UNDEF);
    }
    match members::prim_member(prim, &name) {
        Some(Mem::Fn) => {
            for fam in members::prim_families(prim) {
                let bf = match member_native(m, fam, None, &name) {
                    Some(n) => m.tok(TokKind::BuiltinFn(n)),
                    None => builtin_fn(m, "primitive member"),
                };
                m.add_tok(dst, bf);
            }
        }
        Some(Mem::Prim(p)) => m.add_prim(dst, p),
        Some(Mem::Ctor) => {
            for fam in members::prim_families(prim) {
                family_ctor(m, fam, dst);
            }
        }
        Some(Mem::Proto) => {
            let op = m.tok(TokKind::ObjectProto);
            m.add_tok(dst, op);
        }
        None => {}
    }
    // Described or not: a key written onto the chain reads what was stored,
    // unless the read is final on every kind of the mask (DEC 250), as for
    // a call (`prim_method`).
    let locked = members::prim_families(prim).all(|f| members::locked_read(f, None, &name));
    if !locked && !intrinsic_miss(m, a, NativeMiss::Load(dst)) {
        opaque_member(m, dst);
    }
}

/// Global `n`, whose value the realm declares (`birth`: namespace `Error`,
/// `Math`; the object Node's `get navigator` returns), read off the
/// global object `t` (`globalThis.Error`, or the bare name off
/// the HIR sentinel, the same object): what the program stored under the
/// key (the global's field, as for a program-owned global), and the
/// realm's value. A key some other writer may have written
/// (`global_value_written`: unmodeled code, an accessor, a write under any
/// key) also reads ⊤, and what the program stored there reaches unmodeled
/// code; a later such write replays the read (`NativeMiss::Global`) to the
/// same result.
fn declared_global_load(m: &mut Model, t: Tok, birth: Tok, n: &str, dst: NodeId) {
    if !m.unit_mode {
        let a = m.atom(n);
        let g = m.tok(TokKind::Builtin(BuiltinNs::GlobalThis));
        let f = global_field(m, g, a);
        m.edge(f, dst);
        if global_value_written(m, t, a) {
            m.add_top(dst, |_| format!("globalThis.{n} written by unmodeled code"));
            m.add_prim(dst, prim::ALL);
            let r = m.reason(&format!("globalThis.{n} reaches unmodeled code"));
            m.sink_node(f, r);
        } else {
            m.native_missing.push((t, a, NativeMiss::Global(dst, f)));
        }
    }
    m.add_tok(dst, birth);
}

/// The value the realm declares for global `n` other than a namespace
/// (`ns_by_name`) or a function (`global_function`): `navigator`, a
/// getter-only accessor (Node 26: `get navigator`, no setter,
/// configurable), whose getter returns the realm's one navigator object.
/// Whole-program only: a unit does not see the other units' writes.
fn realm_global_object(m: &Model, n: &str) -> bool {
    n == "navigator" && !m.unit_mode
}

pub fn tok_load(m: &mut Model, t: Tok, key: Key, dst: NodeId) {
    let name = match key {
        Key::Named(a) => Some(m.atoms[a as usize].clone()),
        Key::Any(_) => None,
    };
    match m.toks[t as usize].clone() {
        TokKind::Builtin(BuiltinNs::Other) => match name.as_deref() {
            Some(n) => {
                if let Some(ns) = ns_by_name(n) {
                    let birth = m.tok(TokKind::Builtin(ns));
                    declared_global_load(m, t, birth, n, dst);
                } else if realm_global_object(m, n) {
                    let birth = super::native_protocol::navigator_object(m);
                    declared_global_load(m, t, birth, n, dst);
                } else if !m.unit_mode && global_function(m, n) {
                    let g = m.tok(TokKind::Builtin(BuiltinNs::GlobalThis));
                    let key = Key::Named(m.atom(n));
                    m.load_from(g, g, key, dst);
                } else if matches!(n, "undefined" | "NaN" | "Infinity") {
                    m.add_prim(dst, prim::NUM | prim::UNDEF);
                } else {
                    m.add_top(dst, |_| format!("global {n}"));
                    m.add_prim(dst, prim::ALL);
                }
            }
            None => {
                m.add_top(dst, |_| "global[computed]".into());
                m.add_prim(dst, prim::ALL);
            }
        },
        TokKind::Builtin(BuiltinNs::GlobalThis) => {
            match name.as_deref() {
                Some(n) if global_user_key(m, n) => {
                    let Key::Named(a) = key else { unreachable!() };
                    let f = global_field(m, t, a);
                    if global_value_written(m, t, a) {
                        if m.explain {
                            let op = m.tok_index.get(&TokKind::ObjectProto).copied();
                            let hits: Vec<String> = m
                                .native_writes
                                .iter()
                                .filter(|((w, k), _)| {
                                    (*w == TOP || *w == t || Some(*w) == op)
                                        && (k.is_none() || *k == Some(a))
                                })
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
                                "perry: wpfacts explain: globalThis.{n} written (in_top {}): {hits:?}",
                                m.global_in_top
                            );
                        }
                        m.add_top(dst, |_| format!("globalThis.{n} written by unmodeled code"));
                        m.add_prim(dst, prim::ALL);
                    } else {
                        m.native_missing.push((t, a, NativeMiss::Global(dst, f)));
                    }
                    m.edge(f, dst);
                    m.add_prim(dst, prim::UNDEF);
                    // The global object's [[Prototype]] is `Object.prototype`:
                    // an own miss reads what was stored there under the key.
                    let op = m.tok(TokKind::ObjectProto);
                    tok_load(m, op, key, dst);
                }
                Some(n) if ns_by_name(n).is_some() => {
                    let birth = m.tok(TokKind::Builtin(ns_by_name(n).unwrap()));
                    declared_global_load(m, t, birth, n, dst);
                }
                Some(n) if realm_global_object(m, n) => {
                    let birth = super::native_protocol::navigator_object(m);
                    declared_global_load(m, t, birth, n, dst);
                }
                Some(n) if !m.unit_mode && global_function(m, n) => {
                    let Key::Named(a) = key else { unreachable!() };
                    ns_load(m, t, BuiltinNs::GlobalThis, a, dst);
                }
                _ => {
                    // A host global, or under a computed key any global: the
                    // Function constructor only under a key that may be
                    // "Function" (E14, DECISIONS 235). The program stores
                    // into the global object only through `__` keys or as
                    // an escape.
                    if name.is_none() {
                        let f = m.tok(TokKind::Builtin(BuiltinNs::Function));
                        m.add_tok(dst, f);
                    }
                    m.add_top(dst, |_| {
                        format!("globalThis.{}", name.as_deref().unwrap_or("[computed]"))
                    });
                    m.add_prim(dst, prim::ALL);
                }
            }
        }
        TokKind::Builtin(BuiltinNs::Require) => match name.as_deref() {
            Some("cache") | Some("extensions") => {
                require_parts(m);
                let p = if name.as_deref() == Some("cache") {
                    m.cjs_cache
                } else {
                    m.cjs_ext
                };
                m.edge(p, dst);
            }
            Some("main") => {
                let u = m.cjs_union;
                m.edge(u, dst);
                m.add_prim(dst, prim::UNDEF | prim::NULL);
            }
            _ => {
                let bf = builtin_fn(m, "require.<x>");
                m.add_tok(dst, bf);
                m.add_top(dst, |_| {
                    format!("require.{}", name.as_deref().unwrap_or("[computed]"))
                });
                m.add_prim(dst, prim::ALL);
            }
        },
        TokKind::Builtin(BuiltinNs::Process) => {
            // `process.mainModule` is the CommonJS main record the support
            // module installed (or `undefined`); a computed key may name it.
            if m.key_may_name_str(key, "mainModule") {
                let main = m.cjs_main;
                m.edge(main, dst);
            }
            // A key the realm declares as a host array of strings (`argv`,
            // `execArgv`) holds the one array the runtime made (or what the
            // program stored there); a computed key may name it.
            let mut array_key = false;
            for i in 0..m.process_arrays.len() {
                let (k, a, _) = m.process_arrays[i];
                let k = m.atoms[k as usize].clone();
                if m.key_may_name_str(key, &k) {
                    m.edge(a, dst);
                }
                array_key |= name.as_deref() == Some(k.as_str());
            }
            match name.as_deref() {
                Some("mainModule") => {}
                _ if array_key => {}
                // A key the realm declares as a native function reads it.
                Some(n)
                    if !m.unit_mode && namespace_native(m, t, BuiltinNs::Process, n).is_some() =>
                {
                    let Key::Named(a) = key else { unreachable!() };
                    ns_load(m, t, BuiltinNs::Process, a, dst);
                }
                _ => {
                    m.add_top(dst, |_| {
                        format!("process.{}", name.as_deref().unwrap_or("[computed]"))
                    });
                    m.add_prim(dst, prim::ALL);
                }
            }
        }
        // A node module object: a key its realm declaration names as a
        // native function reads it, a key it declares holding another node
        // module object (`require("inspector")`'s `default`,
        // `node:inspector.default`) reads that object (or, either way, what
        // the program stored there); any other key is not described: ⊤,
        // named.
        TokKind::Builtin(BuiltinNs::Module(module)) => match name.as_deref() {
            Some(n)
                if !m.unit_mode
                    && namespace_native(m, t, BuiltinNs::Module(module), n).is_some() =>
            {
                let Key::Named(a) = key else { unreachable!() };
                ns_load(m, t, BuiltinNs::Module(module), a, dst);
            }
            Some(n) if !m.unit_mode && module_object(m, t, n).is_some() => {
                let Key::Named(a) = key else { unreachable!() };
                let ns = module_object(m, t, n).expect("guarded");
                if native_written(m, t, a) {
                    return opaque_member(m, dst);
                }
                m.native_missing.push((t, a, NativeMiss::Load(dst)));
                let o = m.tok(TokKind::Builtin(ns));
                m.add_tok(dst, o);
            }
            // An accessor the object holds or inherits (`performance
            // .timeOrigin`, on `Performance.prototype`) with a declared
            // getter: its declared result on this object.
            Some(n) if !m.unit_mode && module_getter(m, t, n).is_some() => {
                let Key::Named(a) = key else { unreachable!() };
                let e = module_getter(m, t, n).expect("guarded");
                if native_written(m, t, a) {
                    return opaque_member(m, dst);
                }
                m.native_missing.push((t, a, NativeMiss::Load(dst)));
                super::effects::getter_load(m, e, t, a, dst);
            }
            _ => {
                m.add_top(dst, |_| {
                    format!(
                        "native module ref {module}.{}",
                        name.as_deref().unwrap_or("[computed]")
                    )
                });
                m.add_prim(dst, prim::ALL);
            }
        },
        // A method has no `prototype`.
        TokKind::Builtin(BuiltinNs::ObjectProtoMethod | BuiltinNs::ObjectProtoOwn(_))
            if name.as_deref() == Some("prototype") =>
        {
            m.add_prim(dst, prim::UNDEF)
        }
        TokKind::Builtin(ns) => match name.as_deref() {
            Some("@@__perry_wk_species")
                if matches!(
                    ns,
                    BuiltinNs::TypedArray
                        | BuiltinNs::TypedArrayKind(_)
                        | BuiltinNs::ArrayBuffer
                        | BuiltinNs::SharedArrayBuffer
                ) =>
            {
                m.add_tok(dst, t);
                let Key::Named(a) = key else { unreachable!() };
                if !intrinsic_miss(m, a, NativeMiss::Load(dst)) {
                    opaque_member(m, dst);
                }
            }
            Some("prototype") => builtin_proto_value(m, Some(ns), dst),
            // Its `constructor` is its chain's (`Function` for a
            // constructor, E14; `Object` for a namespace object).
            Some(n)
                if members::ns_member(ns, n).is_some()
                    || members::is_host(ns)
                    || (!m.unit_mode
                        && (namespace_native(m, t, ns, n).is_some()
                            || namespace_receiver_getter(m, t, n))) =>
            {
                let Key::Named(a) = key else { unreachable!() };
                ns_load(m, t, ns, a, dst);
            }
            // A computed key on a described namespace: each own static (and
            // its parent constructors') under a name the key may spell, as
            // its named read finds it, then its chain's members. Not
            // `prototype` (`Model::proto_link`, DECISIONS 217).
            None if members::ns_static_names(ns).is_some() => {
                for n in members::ns_static_names(ns).unwrap() {
                    if !Model::proto_link(&n) && m.key_may_name_str(key, &n) {
                        let a = m.atom(&n);
                        ns_load(m, t, ns, a, dst);
                    }
                }
                let star = m.atom(crate::STAR_KEY);
                if native_written(m, t, star) {
                    opaque_member(m, dst);
                } else {
                    m.native_missing.push((t, star, NativeMiss::Load(dst)));
                }
                family_any(m, Some(t), members::ns_chain(ns), None, key, dst);
            }
            _ => {
                let bf = builtin_fn(m, "builtin namespace static");
                m.add_tok(dst, bf);
                m.add_prim(dst, prim::ALL);
                // A computed key may name the chain's `constructor`.
                if name.is_none() && m.key_may_name_str(key, "constructor") {
                    family_ctor(m, members::ns_chain(ns), dst);
                }
            }
        },
        TokKind::Func(f, _) => match name.as_deref() {
            Some("constructor") => {
                let fc = m.tok(TokKind::Builtin(BuiltinNs::Function));
                m.add_tok(dst, fc);
            }
            // DECISIONS 210: another unit's constructor function: its
            // prototype is its owner's layout's once the owner published it
            // (a store there is this unit's census report); before that an
            // unknown value, never a prototype of this unit's own.
            Some("prototype") if m.foreign_ctor.contains_key(&f) => match m.foreign_ctor[&f] {
                Some((_, tm)) => {
                    let p = m.tok(TokKind::Proto(tm));
                    let a = m.atom("prototype");
                    let fl = m.field(t, a);
                    m.add_tok(fl, p);
                }
                None => {
                    // Beyond the contract: the owner's chain would take
                    // what is stored through it unseen. Reported, so the
                    // owner escapes the function (its chain is ⊤ there).
                    m.fn_touched.insert(t);
                    m.add_top(dst, |_| {
                        "prototype of another unit's constructor function (layout not published)"
                            .into()
                    });
                    m.add_prim(dst, prim::ALL);
                }
            },
            Some("prototype") => {
                let tm = m.fn_inst_template(f);
                let p = m.tok(TokKind::Proto(tm));
                let a = m.atom("prototype");
                let fl = m.field(t, a);
                m.add_tok(fl, p);
            }
            Some("name") => m.add_prim(dst, prim::STR),
            Some("length") => m.add_prim(dst, prim::NUM),
            Some("call" | "apply" | "bind" | "toString") => {
                let Key::Named(a) = key else { unreachable!() };
                let bf = match (!m.unit_mode)
                    .then(|| member_native(m, Family::Function, None, name.as_deref().unwrap()))
                    .flatten()
                {
                    Some(n) => m.tok(TokKind::BuiltinFn(n)),
                    None => builtin_fn(m, "function call/apply/bind/toString"),
                };
                m.add_tok(dst, bf);
                // Final on a `Function.prototype`-only chain (DEC 250).
                if !locked_chain_read(m, t, a) && !intrinsic_miss(m, a, NativeMiss::Load(dst)) {
                    opaque_member(m, dst);
                }
            }
            // An inherited key a store into ⊤ may have written onto
            // `Function.prototype` (or `Object.prototype`), unless the
            // chain is `Function.prototype` only (DEC 250: add-locked).
            Some(_) => {
                m.add_prim(dst, prim::UNDEF);
                let Key::Named(a) = key else { unreachable!() };
                if !locked_chain_read(m, t, a) && !intrinsic_miss(m, a, NativeMiss::Load(dst)) {
                    opaque_member(m, dst);
                }
            }
            None => m.add_prim(dst, prim::UNDEF),
        },
        TokKind::ClassObj(_) => match name.as_deref() {
            Some("constructor") => {
                let fc = m.tok(TokKind::Builtin(BuiltinNs::Function));
                m.add_tok(dst, fc);
            }
            Some("name") => m.add_prim(dst, prim::STR),
            Some("length") => m.add_prim(dst, prim::NUM),
            _ => m.add_prim(dst, prim::UNDEF),
        },
        TokKind::Obj(tm, _) => match m.tmpls[tm as usize].kind.clone() {
            TmplKind::Arr { .. } => match (key, name.as_deref()) {
                (_, Some("length")) => m.add_prim(dst, prim::NUM),
                // A computed key: an element (`load_from`'s edge), `length`,
                // or a member of the chain under a name the key may spell.
                (_, None) => {
                    m.add_prim(dst, prim::UNDEF | prim::NUM);
                    family_any(m, Some(t), Family::Array, None, key, dst);
                }
                // An index names an element.
                (_, Some(n)) if members::is_index_name(n) => {
                    let all = m.all_fields(t);
                    m.edge(all, dst);
                    m.add_prim(dst, prim::UNDEF);
                }
                (Key::Named(a), Some(_)) => family_load(m, t, Family::Array, None, a, dst),
                (Key::Any(_), Some(_)) => unreachable!(),
            },
            TmplKind::BObj { what, .. } => {
                let fam = members::bobj_family(what);
                match (key, name.as_deref()) {
                    // A typed array's template does not keep its kind: its
                    // `constructor` is any concrete typed-array constructor.
                    // A buffer's or a DataView's is its own.
                    (_, Some("constructor"))
                        if matches!(
                            what,
                            "TypedArray" | "DataView" | "ArrayBuffer" | "SharedArrayBuffer"
                        ) =>
                    {
                        if what == "TypedArray" {
                            for n in TYPED_ARRAYS {
                                let ctor = m.tok(TokKind::Builtin(BuiltinNs::TypedArrayKind(n)));
                                m.add_tok(dst, ctor);
                            }
                        } else if let Some(ns) = ns_by_name(what) {
                            let ctor = m.tok(TokKind::Builtin(ns));
                            m.add_tok(dst, ctor);
                        }
                        let Key::Named(a) = key else { unreachable!() };
                        if !intrinsic_miss(m, a, NativeMiss::Load(dst)) {
                            opaque_member(m, dst);
                        }
                    }
                    (_, Some("buffer")) if matches!(what, "TypedArray" | "Buffer" | "DataView") => {
                        // B1: the backing buffer is observed (it can be
                        // transferred, detached or wrapped by another view).
                        let r = m.reason("B1 .buffer read");
                        m.mark_hard(t, Hard::B1Exposed, r);
                        m.add_tok(dst, t);
                    }
                    (_, Some("byteOffset"))
                        if matches!(what, "TypedArray" | "Buffer" | "DataView") =>
                    {
                        let r = m.reason("B1 .byteOffset read");
                        m.mark_hard(t, Hard::B1Exposed, r);
                        m.add_prim(dst, prim::NUM)
                    }
                    (_, Some(n @ ("size" | "length" | "byteLength" | "byteOffset")))
                        if fam == Family::Opaque && !buffer_member_absent(m, Some(what), n) =>
                    {
                        m.add_prim(dst, prim::NUM);
                        let Key::Named(a) = key else { unreachable!() };
                        if !intrinsic_miss(m, a, NativeMiss::Load(dst)) {
                            opaque_member(m, dst);
                        }
                    }
                    // `require.cache` / `require.extensions` are plain
                    // objects: their fields hold what was stored (by compiled
                    // code, or ⊤ from the loader, `call_builtin_ns`); an
                    // absent key reads `undefined`.
                    _ if what == "require.cache" || what == "require.extensions" => {
                        m.add_prim(dst, prim::UNDEF)
                    }
                    // A computed key on an ordinary object a builtin makes
                    // (`box_chain` Object: a `JSON.parse` object, a
                    // descriptor, an `Object.fromEntries` result): its own
                    // values, or what `Object.prototype` holds: `constructor`
                    // (`Object`), its methods and what a store into ⊤ wrote
                    // (`family_any`). A computed key of unknown string never
                    // names the `__proto__` accessor (DECISIONS 217,
                    // `Model::proto_link`): the prototype is a value only by
                    // the named read (`Model::load_from`, `proto_value`). A
                    // prototype the program gave it is read on the chain
                    // (`Model::load_from`, ordinary box).
                    (_, None) if ordinary_box(m, t) => {
                        m.add_prim(dst, prim::ALL);
                        family_any(m, Some(t), Family::Object, None, key, dst);
                    }
                    // A computed key: the container's own values (primitives,
                    // or an iterator result's `value`/`done`: all it has), and
                    // a member of its chain under a name the key may spell: an
                    // array a builtin makes reads the array chain; a family
                    // with no member table an undescribed builtin function.
                    (_, None) => {
                        if members::bobj_family(what) == Family::Iterator || what == "IterResult" {
                            m.add_prim(dst, prim::UNDEF);
                        } else {
                            m.add_prim(dst, prim::ALL);
                        }
                        if array_chain(m, t) {
                            family_any(m, Some(t), Family::Array, None, key, dst);
                        } else if members::chain_names(fam).is_some() {
                            family_any(m, Some(t), fam, Some(what), key, dst);
                        } else {
                            let bf = builtin_fn(m, "builtin object computed key");
                            m.add_tok(dst, bf);
                        }
                    }
                    // A getter the realm declares on the prototype of an
                    // object a declaration made (`AbortController.prototype
                    // .signal`): its declared result, never the getter
                    // function; a write of the key onto the chain (now, or
                    // later: the replay) reads what was stored.
                    (Key::Named(a), Some(n))
                        if what.ends_with(".prototype")
                            && super::effects::declared_getter(what, n).is_some() =>
                    {
                        let e = super::effects::declared_getter(what, n).expect("declared");
                        super::effects::getter_load(m, e, t, a, dst);
                        if !intrinsic_miss(m, a, NativeMiss::Load(dst)) {
                            opaque_member(m, dst);
                        }
                    }
                    (Key::Named(a), Some(_)) => {
                        // A container of primitives (split/match results,

                        // keys …) holds them under every key.
                        if what == "primbox" || what == "arraybox" {
                            m.add_prim(dst, prim::ALL);
                        }
                        family_load(m, t, fam, Some(what), a, dst);
                    }
                    (Key::Any(_), Some(_)) => unreachable!(),
                }
            }
            _ => {}
        },
        TokKind::ObjectProto if name.as_deref() == Some("constructor") || name.is_none() => {
            // Object.prototype stands for every builtin prototype here:
            // its `constructor` may be Object or Function (E14). On a chain
            // its object's kind says which one it is (`chain_load`). A
            // computed key whose shape excludes `constructor` reads neither.
            if m.key_may_name_str(key, "constructor") {
                for ns in [BuiltinNs::Object, BuiltinNs::Function] {
                    let b = m.tok(TokKind::Builtin(ns));
                    m.add_tok(dst, b);
                }
            }
            m.add_prim(dst, prim::UNDEF);
            if name.is_none() {
                // A computed key reads each of `Object.prototype`'s methods
                // its shape can spell (`t["__define" + s]`), as `family_any`
                // reads them under a key that may name any member.
                if members::object_proto_methods().any(|n| m.key_may_name_str(key, n)) {
                    let f = m.tok(TokKind::Builtin(BuiltinNs::ObjectProtoMethod));
                    m.add_tok(dst, f);
                }
                any_written(m, dst);
            }
        }
        // A program object's lookup that reaches `Object.prototype`: a key
        // a store into ⊤ may have written there reads what was stored.
        TokKind::ObjectProto => {
            m.add_prim(dst, prim::UNDEF);
            let Key::Named(a) = key else { unreachable!() };
            object_proto_method_value(m, a, dst);
            if !intrinsic_miss(m, a, NativeMiss::Load(dst)) {
                opaque_member(m, dst);
            }
        }
        TokKind::ModNs(_) | TokKind::Proto(_) => m.add_prim(dst, prim::UNDEF),
        // A key the native prototype does not define: the chain goes on.
        TokKind::NativeProto(_) if native_skips(m, t, key, NativeMiss::Load(dst)) => {}
        // A builtin's prototype object taken as a value (`builtin_proto_tok`):
        // its described members, as a receiver of its family reads them.
        TokKind::NativeProto(ref n) if ns_by_name(n).and_then(proto_family).is_some() => {
            let (fam, what, _) = ns_by_name(n).and_then(proto_family).unwrap();
            match key {
                Key::Named(a) => family_load(m, t, fam, what, a, dst),
                Key::Any(_) => family_any(m, Some(t), fam, what, key, dst),
            }
        }
        // A builtin function value (or a bound function): the members of
        // `Function.prototype`; a computed key may name any of them.
        TokKind::BuiltinFn(_) | TokKind::Bound(_) if matches!(key, Key::Named(_)) => {
            if native_own_load(m, t, t, key, dst) {
                return;
            }
            let Key::Named(a) = key else { unreachable!() };
            family_load(m, t, Family::Function, None, a, dst);
        }
        // A builtin function value under a computed key: its own data
        // (`name`, `length`, what the program stored: `load_from`), or a
        // member of `Function.prototype`'s chain under a name the key may
        // spell.
        TokKind::BuiltinFn(_) | TokKind::Bound(_) => {
            m.add_prim(dst, prim::ALL);
            family_any(m, Some(t), Family::Function, None, key, dst);
        }
        TokKind::NativeProto(_) => {
            let bf = builtin_fn(m, "native proto / builtin fn computed key");
            m.add_tok(dst, bf);
            m.add_prim(dst, prim::ALL);
            // A function's `constructor` is the Function constructor (E14);
            // a native prototype's is its native class (the builtin
            // function above).
            if m.key_may_name_str(key, "constructor")
                && !matches!(m.toks[t as usize], TokKind::NativeProto(_))
            {
                let fc = m.tok(TokKind::Builtin(BuiltinNs::Function));
                m.add_tok(dst, fc);
            }
        }
        TokKind::Top => {}
    }
}

/// A method `Object.prototype` holds under `a` (the three no builtin
/// prototype overrides are `ObjectProtoOwn`, `chain_load`), read as a value
/// through a chain: that method's native. A call of the value runs the
/// method on the call's receiver (`native_call`), as a named call does:
/// `box.m = plain.__defineGetter__; box.m(k, f)` installs on `box`.
fn object_proto_method_value(m: &mut Model, a: Atom, dst: NodeId) {
    let name = m.atoms[a as usize].clone();
    if ProtoOwn::of_name(&name).is_some()
        || !matches!(members::member(Family::Object, &name), Some(Mem::Fn))
    {
        return;
    }
    let f = match member_native(m, Family::Object, None, &name) {
        Some(n) => m.tok(TokKind::BuiltinFn(n)),
        None => builtin_fn(m, "described member"),
    };
    m.add_tok(dst, f);
}

/// `tok_load` of prototype `q` on the chain of `of` (`Cons::ChainUnion`).
/// `ObjectProto` collapses the builtin prototypes; where `of`'s kind put it
/// there it is one of them (`seed_family`), and the members it reads under
/// `constructor` or a computed key are that family's chain's (`members`,
/// as on a builtin object itself): only a function's chain reaches
/// `Function.prototype`, whose `constructor` is the Function constructor
/// (DECISIONS 213). A site builtin object's chain (a typed array's ...)
/// under a computed key stays `tok_load`'s.
pub fn chain_load(m: &mut Model, of: Tok, q: Tok, key: Key, dst: NodeId) {
    if let (TokKind::Builtin(ns), Key::Named(a)) = (m.toks[q as usize].clone(), key) {
        if namespace_getter_load(m, of, q, ns, a, dst) {
            return;
        }
    }
    // A member no intrinsic overrides has Object.prototype's identity on
    // every instance path too, including final reads of a plain object.
    if matches!(m.toks[q as usize], TokKind::ObjectProto) {
        if let Key::Named(a) = key {
            if ProtoOwn::of_name(&m.atoms[a as usize]).is_some() {
                if locked_chain_read(m, of, a) {
                    let own = ProtoOwn::of_name(&m.atoms[a as usize]).unwrap();
                    let f = m.tok(TokKind::Builtin(BuiltinNs::ObjectProtoOwn(own)));
                    m.add_tok(dst, f);
                } else {
                    intrinsic_proto_own(m, key, dst);
                }
                return;
            }
        }
    }
    // DEC 250: where `of`'s family makes the read final, no store (through
    // ⊤ or onto the collapsed token) wrote the key there: the unwritten
    // answer, with no replay.
    if let Key::Named(a) = key {
        if matches!(m.toks[q as usize], TokKind::ObjectProto)
            && !m.is_constructor_key(key)
            && locked_chain_read(m, of, a)
        {
            m.add_prim(dst, prim::UNDEF);
            object_proto_method_value(m, a, dst);
            return;
        }
    }
    if !matches!(m.toks[q as usize], TokKind::ObjectProto) || !m.is_constructor_key(key) {
        return tok_load(m, q, key, dst);
    }
    let Some(fam) = seed_family(m, of) else {
        // A parentless class's instance under a computed key: its chain's
        // `Object.prototype`, final but for `constructor` (DEC 250).
        let star = m.atom(crate::STAR_KEY);
        if key.is_any() && locked_chain_read(m, of, star) {
            return family_any(m, Some(of), Family::Object, None, key, dst);
        }
        return tok_load(m, q, key, dst);
    };
    let what = if !m.unit_mode {
        match m.toks[of as usize] {
            TokKind::Obj(tm, _) => match m.tmpls[tm as usize].kind {
                TmplKind::BObj { what, .. } => Some(what),
                _ => None,
            },
            _ => None,
        }
    } else {
        None
    };
    match key {
        Key::Named(a) => family_load(m, of, fam, what, a, dst),
        Key::Any(_) if is_site_builtin(m, of) => tok_load(m, q, key, dst),
        Key::Any(_) => family_any(m, Some(of), fam, what, key, dst),
    }
}

/// A store of `src` under named key `key` on an object a declaration made
/// (`AbortSignal.prototype`'s token) whose prototype declares a setter for
/// the key that keeps its argument in a receiver slot (`onabort`: the
/// event handler): the slot holds `src`, as the setter's run does. The
/// ordinary store (`store_into`) stays beside it: a superset where the
/// chain's setter may have been replaced.
pub fn declared_setter_store(m: &mut Model, t: Tok, key: Key, src: NodeId) {
    let (TokKind::Obj(tm, _), Key::Named(a)) = (&m.toks[t as usize], key) else {
        return;
    };
    let TmplKind::BObj { what, .. } = m.tmpls[*tm as usize].kind else {
        return;
    };
    if !what.ends_with(".prototype") {
        return;
    }
    let name = m.atoms[a as usize].clone();
    if let Some(e) = super::effects::declared_setter(what, &name) {
        super::effects::setter_store(m, e, t, src);
    }
}
