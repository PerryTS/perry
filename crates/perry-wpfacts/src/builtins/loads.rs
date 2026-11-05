//! Loads of builtin objects: what reading a key of a family, namespace, primitive or token yields.

use super::*;

/// The family's `constructor`.
pub(super) fn family_ctor(m: &mut Model, fam: Family, dst: NodeId) {
    let nss: &[BuiltinNs] = match fam {
        Family::Object => &[BuiltinNs::Object],
        Family::Function => &[BuiltinNs::Function],
        Family::Array => &[BuiltinNs::Array],
        Family::Str => &[BuiltinNs::String],
        Family::Num | Family::BigInt => &[BuiltinNs::Number],
        Family::Bool => &[BuiltinNs::Boolean],
        Family::Sym => &[BuiltinNs::Symbol],
        Family::Collection => &[
            BuiltinNs::Map,
            BuiltinNs::Set,
            BuiltinNs::WeakMap,
            BuiltinNs::WeakSet,
        ],
        Family::Promise => &[BuiltinNs::Promise],
        Family::Error => &[BuiltinNs::Error],
        Family::Box => &[BuiltinNs::Object, BuiltinNs::Array],
        Family::Wrapper => &[
            BuiltinNs::Object,
            BuiltinNs::String,
            BuiltinNs::Number,
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

/// `dst` ⊇ what a builtin object of family `fam` reads under named key `a`
/// from its intrinsic chain: the described member, or the opaque member
/// once the key may have been written (own data fields come from the field
/// storage, `load_from`; an absent key reads nothing more).
pub(super) fn family_load(
    m: &mut Model,
    fam: Family,
    what: Option<&'static str>,
    a: Atom,
    dst: NodeId,
) {
    let name = m.atoms[a as usize].clone();
    if fam == Family::Opaque {
        undescribed_member(m, dst);
        if !intrinsic_miss(m, a, NativeMiss::Load(dst)) {
            opaque_member(m, dst);
        }
        return;
    }
    match members::member(fam, &name) {
        Some(Mem::Fn) => {
            let bf = match member_native(fam, what, a, &name) {
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
        Some(Mem::Ctor) => family_ctor(m, fam, dst),
        Some(Mem::Proto) => {
            let op = m.tok(TokKind::ObjectProto);
            m.add_tok(dst, op);
        }
        None => {}
    }
    // Described or not, a key a store into ⊤ may have written onto the
    // chain reads what was stored, unless the read is final (DEC 250).
    if !intrinsic_unwritten(m, fam, what, a, NativeMiss::Load(dst)) {
        opaque_member(m, dst);
    }
}

/// The family (and site kind) of the intrinsic prototype builtin namespace
/// `ns` holds under `prototype` (the inverse of `family_ctor`), and the
/// global binding that names `ns`; `None`: no described prototype, or not
/// a constructor.
fn proto_family(ns: BuiltinNs) -> Option<(Family, Option<&'static str>, &'static str)> {
    Some(match ns {
        BuiltinNs::Object => (Family::Object, None, "Object"),
        BuiltinNs::Function => (Family::Function, None, "Function"),
        BuiltinNs::Array => (Family::Array, None, "Array"),
        BuiltinNs::String => (Family::Str, None, "String"),
        BuiltinNs::Number => (Family::Num, None, "Number"),
        BuiltinNs::Boolean => (Family::Bool, None, "Boolean"),
        BuiltinNs::Symbol => (Family::Sym, None, "Symbol"),
        BuiltinNs::Promise => (Family::Promise, Some("Promise"), "Promise"),
        BuiltinNs::Map => (Family::Collection, Some("Map"), "Map"),
        BuiltinNs::Set => (Family::Collection, Some("Set"), "Set"),
        BuiltinNs::WeakMap => (Family::Collection, Some("WeakMap"), "WeakMap"),
        BuiltinNs::WeakSet => (Family::Collection, Some("WeakSet"), "WeakSet"),
        _ => return None,
    })
}

/// `F.prototype.K` through builtin namespace `ns`: the native `K` names
/// when it is an own method of `F`'s intrinsic prototype (the realm's
/// description, `members::own`) whose read is final (DEC 250,
/// `members::locked_read`: the runtime refuses a store there, so no write
/// replaces it). The prototype itself, not a value of the family, is
/// read: `Function.prototype`'s own chain is that prototype only. The
/// namespace token is the runtime's object only while the global binding
/// that names it is unwritten (DEC 255: a store into the global object
/// under that name, `globalThis.Array = x`); a write later in the solve
/// replays into `dst` as a written key (`NativeMiss::Load`).
pub(crate) fn proto_own_native(m: &mut Model, ns: BuiltinNs, a: Atom, dst: NodeId) -> Option<Native> {
    let (fam, what, global) = proto_family(ns)?;
    let name = m.atoms[a as usize].clone();
    let lock = if fam == Family::Function {
        Some(members::FN_PROTO_ONLY)
    } else {
        what
    };
    if !matches!(members::own(fam, &name), Some(Mem::Fn)) || !members::locked_read(fam, lock, &name) {
        return None;
    }
    let native = member_native(fam, what, a, &name)?;
    let g = m.tok(TokKind::Builtin(BuiltinNs::GlobalThis));
    let ga = m.atom(global);
    if native_written(m, g, ga) {
        return None;
    }
    m.native_missing.push((g, ga, NativeMiss::Load(dst)));
    Some(native)
}

/// `dst` ⊇ what builtin namespace `ns` (token `t`) reads under named key
/// `a`: its described static, else its chain's member (`Function.prototype`
/// or `Object.prototype`), else nothing (absent: `undefined`). A key the
/// program may have written onto the namespace itself, or onto a builtin
/// prototype of its chain, reads the opaque member.
pub(super) fn ns_load(m: &mut Model, t: Tok, ns: BuiltinNs, a: Atom, dst: NodeId) {
    m.census_native_read(t, a, NativeMiss::Load(dst));
    let name = m.atoms[a as usize].clone();
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
        Some(Mem::Proto) => {
            let op = m.tok(TokKind::ObjectProto);
            m.add_tok(dst, op);
        }
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
        // A computed key may name any member of the chain, or a character.
        Key::Any(_) => {
            m.add_prim(dst, prim::STR | prim::NUM | prim::UNDEF | prim::BOOL);
            let bf = builtin_fn(m, "primitive computed key");
            m.add_tok(dst, bf);
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
            let bf = builtin_fn(m, "primitive member");
            m.add_tok(dst, bf);
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
    // Described or not: a key written onto the chain reads what was stored.
    if !intrinsic_miss(m, a, NativeMiss::Load(dst)) {
        opaque_member(m, dst);
    }
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
                    let bt = m.tok(TokKind::Builtin(ns));
                    m.add_tok(dst, bt);
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
                Some(n) if global_user_key(n) => {
                    let Key::Named(a) = key else { unreachable!() };
                    let f = global_field(m, t, a);
                    if native_written(m, t, a) {
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
                                        m.reasons[*r as usize]
                                    )
                                })
                                .collect();
                            eprintln!("perry: wpfacts explain: globalThis.{n} written (in_top {}): {hits:?}", m.global_in_top);
                        }
                        m.add_top(dst, |_| format!("globalThis.{n} written by unmodeled code"));
                        m.add_prim(dst, prim::ALL);
                    } else {
                        m.native_missing.push((t, a, NativeMiss::Global(dst, f)));
                    }
                    m.edge(f, dst);
                    m.add_prim(dst, prim::UNDEF);
                }
                Some(n) if ns_by_name(n).is_some() => {
                    let bt = m.tok(TokKind::Builtin(ns_by_name(n).unwrap()));
                    m.add_tok(dst, bt);
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
            // `process.argv` is the one array the runtime made (or what the
            // program stored there); a computed key may name it.
            if m.key_may_name_str(key, "argv") {
                let a = m.process_argv;
                m.edge(a, dst);
            }
            if !matches!(name.as_deref(), Some("mainModule") | Some("argv")) {
                m.add_top(dst, |_| {
                    format!("process.{}", name.as_deref().unwrap_or("[computed]"))
                });
                m.add_prim(dst, prim::ALL);
            }
        }
        // A method has no `prototype`.
        TokKind::Builtin(BuiltinNs::ObjectProtoMethod | BuiltinNs::ObjectProtoOwn(_))
            if name.as_deref() == Some("prototype") =>
        {
            m.add_prim(dst, prim::UNDEF)
        }
        TokKind::Builtin(ns) => match name.as_deref() {
            Some("@@__perry_wk_species")
                if matches!(ns, BuiltinNs::TypedArray | BuiltinNs::ArrayBuffer) =>
            {
                m.add_tok(dst, t);
                let Key::Named(a) = key else { unreachable!() };
                if !intrinsic_miss(m, a, NativeMiss::Load(dst)) {
                    opaque_member(m, dst);
                }
            }
            Some("prototype") => {
                let op = m.tok(TokKind::ObjectProto);
                m.add_tok(dst, op);
            }
            // Its `constructor` is its chain's (`Function` for a
            // constructor, E14; `Object` for a namespace object).
            Some(n) if members::ns_member(ns, n).is_some() || members::is_host(ns) => {
                let Key::Named(a) = key else { unreachable!() };
                ns_load(m, t, ns, a, dst);
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
                let bf = builtin_fn(m, "function call/apply/bind/toString");
                m.add_tok(dst, bf);
                let Key::Named(a) = key else { unreachable!() };
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
                // A computed key may name a builtin method too.
                (_, None) => {
                    let bf = builtin_fn(m, "array computed key");
                    m.add_tok(dst, bf);
                    m.add_prim(dst, prim::UNDEF | prim::NUM);
                }
                // An index names an element.
                (_, Some(n)) if members::is_index_name(n) => {
                    let all = m.all_fields(t);
                    m.edge(all, dst);
                    m.add_prim(dst, prim::UNDEF);
                }
                (Key::Named(a), Some(_)) => family_load(m, Family::Array, None, a, dst),
                (Key::Any(_), Some(_)) => unreachable!(),
            },
            TmplKind::BObj { what, .. } => {
                let fam = members::bobj_family(what);
                match (key, name.as_deref()) {
                    (_, Some("constructor")) if what == "TypedArray" => {
                        let ctor = m.tok(TokKind::Builtin(BuiltinNs::TypedArray));
                        m.add_tok(dst, ctor);
                        let Key::Named(a) = key else { unreachable!() };
                        if !intrinsic_miss(m, a, NativeMiss::Load(dst)) {
                            opaque_member(m, dst);
                        }
                    }
                    (_, Some("buffer")) if what == "TypedArray" => {
                        // B1: the backing buffer is observed (it can be
                        // transferred, detached or wrapped by another view).
                        let r = m.reason("B1 .buffer read");
                        m.mark_hard(t, Hard::B1Exposed, r);
                        m.add_tok(dst, t);
                    }
                    (_, Some("byteOffset")) if what == "TypedArray" => {
                        let r = m.reason("B1 .byteOffset read");
                        m.mark_hard(t, Hard::B1Exposed, r);
                        m.add_prim(dst, prim::NUM)
                    }
                    (_, Some("size" | "length" | "byteLength" | "byteOffset"))
                        if fam == Family::Opaque =>
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
                    // A computed key: any member of the chain, and the
                    // container's own values (primitives, or an iterator
                    // result's `value`/`done`: all it has).
                    (_, None) => {
                        let bf = builtin_fn(m, "builtin object computed key");
                        m.add_tok(dst, bf);
                        if what == "Iterator" || what == "IterResult" {
                            m.add_prim(dst, prim::UNDEF);
                        } else {
                            m.add_prim(dst, prim::ALL);
                        }
                    }
                    (Key::Named(a), Some(_)) => {
                        // A container of primitives (split/match results,
                        // keys …) holds them under every key.
                        if what == "primbox" || what == "arraybox" {
                            m.add_prim(dst, prim::ALL);
                        }
                        family_load(m, fam, Some(what), a, dst);
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
                any_written(m, dst);
            }
        }
        // A program object's lookup that reaches `Object.prototype`: a key
        // a store into ⊤ may have written there reads what was stored.
        TokKind::ObjectProto => {
            m.add_prim(dst, prim::UNDEF);
            let Key::Named(a) = key else { unreachable!() };
            if !intrinsic_miss(m, a, NativeMiss::Load(dst)) {
                opaque_member(m, dst);
            }
        }
        TokKind::ModNs(_) | TokKind::Proto(_) => m.add_prim(dst, prim::UNDEF),
        // A key the native prototype does not define: the chain goes on.
        TokKind::NativeProto(_) if native_skips(m, t, key, NativeMiss::Load(dst)) => {}
        // A builtin function value (or a bound function): the members of
        // `Function.prototype`; a computed key may name any of them.
        TokKind::BuiltinFn(_) | TokKind::Bound(_) if matches!(key, Key::Named(_)) => {
            let Key::Named(a) = key else { unreachable!() };
            family_load(m, Family::Function, None, a, dst);
        }
        TokKind::NativeProto(_) | TokKind::BuiltinFn(_) | TokKind::Bound(_) => {
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

/// `tok_load` of prototype `q` on the chain of `of` (`Cons::ChainUnion`).
/// `ObjectProto` collapses the builtin prototypes; where `of`'s kind put it
/// there it is one of them (`seed_family`), and the members it reads under
/// `constructor` or a computed key are that family's chain's (`members`,
/// as on a builtin object itself): only a function's chain reaches
/// `Function.prototype`, whose `constructor` is the Function constructor
/// (DECISIONS 213). A site builtin object's chain (a typed array's ...)
/// under a computed key stays `tok_load`'s.
pub fn chain_load(m: &mut Model, of: Tok, q: Tok, key: Key, dst: NodeId) {
    // DEC 250: where `of`'s family makes the read final, no store (through
    // ⊤ or onto the collapsed token) wrote the key there: the unwritten
    // answer, with no replay.
    if let Key::Named(a) = key {
        if matches!(m.toks[q as usize], TokKind::ObjectProto)
            && !m.is_constructor_key(key)
            && locked_chain_read(m, of, a)
        {
            m.add_prim(dst, prim::UNDEF);
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
            return family_any(m, of, Family::Object, dst);
        }
        return tok_load(m, q, key, dst);
    };
    match key {
        Key::Named(a) => family_load(m, fam, None, a, dst),
        Key::Any(_) if is_site_builtin(m, of) => tok_load(m, q, key, dst),
        Key::Any(_) => family_any(m, of, fam, dst),
    }
}
