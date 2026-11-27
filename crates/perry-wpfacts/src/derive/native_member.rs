//! The solved builtin-member lookup verdict, shared by named and folded sites.
use super::{builtin_protos_unwritten, exact_array, native_own_free, nullish_arms, why_indexed};
use crate::model::{Model, TmplKind, Tok, TokKind};

/// BIV (CHB §2): the receivers of a NativeMember site, or why there are
/// none. `Err(Some(..))` names the write onto a builtin prototype that alone
/// keeps an otherwise proven site (obligations 1-3 hold); `Err(None)`: an
/// obligation 1-3 fails, the site keeps its other reason.
///
/// 1. One family: every primitive a String and no object, or every object
///    an Array exotic object ([`exact_array`]), or every object a
///    collection of one kind, or every object a RegExp, or every object a
///    native Promise, or every primitive a Number and no object, or every
///    object a Buffer (a method `Buffer.prototype` owns); closed markers are skipped (their callers'
///    values are beside them), ⊤ and other markers are none.
/// 2. The family's intrinsic chain defines the key: a function for a call,
///    a function or a data member for a read (`builtins::native_described`).
/// 3. No receiver holds the key as an own property ([`native_own_free`]).
/// 4. No builtin prototype is written under the key or under any key: no
///    accessor on `Object.prototype` (E23), no redefinition of a builtin
///    prototype's key (defineProperty on one or through ⊤) and no hard
///    operation on them, no program write onto them, no store into ⊤
///    (which may be any of them). Per unit, other units' writes are
///    replayed into the same facts (`SharedWrite`).
pub(crate) fn native_member(
    m: &Model,
    s: &crate::gen::ReadSite,
    toks: &[Tok],
    prim: u16,
) -> Result<Vec<String>, Option<String>> {
    use crate::builtins::NativeFamily;
    let mut fam: Option<NativeFamily> = None;
    // A nullish value beside one primitive family: its TypeError arm.
    let (prim, mut receivers) = nullish_arms(m, toks, prim).unwrap_or((prim, Vec::new()));
    let join = |f: NativeFamily, fam: &mut Option<NativeFamily>| match fam {
        None => {
            *fam = Some(f);
            true
        }
        Some(g) => *g == f,
    };
    if prim != 0 {
        let (f, r) = match prim {
            crate::model::prim::STR => (NativeFamily::Str, "primitive:string"),
            crate::model::prim::NUM => (NativeFamily::Num, "primitive:number"),
            _ => return Err(None),
        };
        join(f, &mut fam);
        receivers.push(r.to_string());
    }
    for &t in toks {
        if m.closed_marker(t) {
            continue;
        }
        let f = if exact_array(m, t) {
            NativeFamily::Array
        } else {
            match m.toks[t as usize] {
                TokKind::Obj(tm, _) if !m.is_marker(t) => match m.tmpls[tm as usize].kind {
                    TmplKind::BObj {
                        what: w @ ("Map" | "Set" | "WeakMap" | "WeakSet"),
                        ..
                    } => NativeFamily::Collection(w),
                    TmplKind::BObj { what: "RegExp", .. } => NativeFamily::RegExp,
                    TmplKind::BObj {
                        what: "Promise", ..
                    } => NativeFamily::Promise,
                    // The per-unit solve misses a write onto `Buffer.prototype`
                    // through `Object.assign`: it proves no Buffer member.
                    TmplKind::BObj { what: "Buffer", .. } if !m.unit_mode => NativeFamily::Buffer,
                    TmplKind::BObj {
                        what: "TypedArray", ..
                    } => NativeFamily::TypedArray,
                    TmplKind::BObj { what, .. } if crate::builtins::declared_family(m, what) => {
                        NativeFamily::Declared(what)
                    }
                    _ => return Err(None),
                },
                _ => return Err(None),
            }
        };
        if !join(f, &mut fam) {
            return Err(None);
        }
        let Some(tm) = m.tmpl_of(t) else {
            return Err(None);
        };
        receivers.push(m.tmpls[tm as usize].display.clone());
    }
    let Some(fam) = fam else {
        return Err(None);
    };
    let a = s.key;
    if !crate::builtins::native_described(m, fam, &m.atoms[a as usize], s.is_call) {
        return Err(None);
    }
    for &t in toks {
        if !m.closed_marker(t) && !native_own_free(m, fam, t, a) {
            return Err(None);
        }
    }
    builtin_protos_unwritten(m, Some(fam), a).map_err(Some)?;
    receivers.sort();
    receivers.dedup();
    Ok(receivers)
}

/// A site whose receivers are all host objects (`builtins::host_of`):
/// `Ok` when each one's member under the key is its described native
/// member and no store reached it; `None` when a receiver is no host
/// object or a primitive flows.
pub(super) fn host_member(
    m: &Model,
    s: &crate::gen::ReadSite,
    toks: &[Tok],
    prim: u16,
) -> Option<Result<Vec<String>, String>> {
    if prim != 0 || toks.is_empty() {
        return None;
    }
    let name = &m.atoms[s.key as usize];
    let mut receivers = Vec::new();
    let mut native = true;
    for &t in toks {
        let TokKind::Builtin(ns) = m.toks[t as usize] else {
            return None;
        };
        match crate::builtins::host_of(ns, name, s.is_call) {
            Some(h) => {
                native &= crate::builtins::host_native(m, h, s.key, s.is_call);
                receivers.push(format!("host:{h:?}"));
            }
            None if crate::builtins::ns_native(m, t, ns, s.key, s.is_call) => {
                receivers.push(m.tok_name(t));
            }
            None => {
                // Diagnostic (off: one flag read): who writes the key.
                if why_indexed() {
                    eprintln!(
                        "WHY_NS @{} {} .{name}: {:?}",
                        s.byte_offset,
                        m.tok_name(t),
                        crate::builtins::native_writers_why(m, t, s.key)
                    );
                }
                return None;
            }
        }
    }
    if !native {
        return Some(Err("host member undescribed or written".into()));
    }
    // Interpreted code (DEC 150) or an unseen module names every global and
    // may write the key; no namespace object is a lock holder, so nothing
    // refuses the write. Unknown code that merely holds the realm
    // (`realm_held`) does not: under the closed world (owner 2026-11-14) it
    // is the runtime's natives, which replace no member of a namespace (owner
    // decision 2026-11-18, R-GLOBAL step 3; `realm_replaced`).
    if m.interp_runs || m.unseen_module_runs {
        return Some(Err(
            "namespace member: interpreted code may write the key".into()
        ));
    }
    receivers.sort();
    receivers.dedup();
    Some(Ok(receivers))
}

/// HOSTSURF H2 (§2.3): the verdict of a site whose receivers are all
/// objects a provider declares (a `fresh` of a host prototype, a birth) or
/// the global object under a key a provider adds to it, beside `undefined`
/// / `null` (the operation's own TypeError arm: `nullable`). `None`: a
/// receiver of another kind (the site keeps its other verdicts).
///
/// OrdinaryGet (ECMA-262 10.1.8.1) over each receiver's chain finds the
/// key as one manifest member of one holder, when: no receiver holds the
/// key as an own property (`native_own_free`); no prototype of any chain
/// is written under the key by the program or through ⊤, nothing redefines
/// a builtin prototype, no interpreted or unknown code holds the realm
/// (`builtin_protos_unwritten`, per chain); and every receiver gives the
/// same holder and member. A host prototype is no lock holder: the
/// program's own store onto it is that chain's member (F1).
pub(super) fn host_site(
    m: &Model,
    s: &crate::gen::ReadSite,
    toks: &[Tok],
    prim: u16,
) -> Option<(Vec<String>, crate::Verdict)> {
    use crate::builtins::NativeFamily;
    use crate::model::prim::{NULL, UNDEF};
    use crate::Verdict;
    use perry_hir::realm_manifest::RealmOwnValue as V;
    crate::host::current()?;
    let nn = prim & (NULL | UNDEF);
    if prim & !nn != 0 || toks.is_empty() {
        return None;
    }
    let a = s.key;
    let name = m.atoms[a as usize].as_str();
    let store = s.access.is_store();
    let mut receivers: Vec<String> = Vec::new();
    let mut found: Option<(&'static str, V)> = None;
    let mut why: Option<String> = None;
    let mut any = false;
    for &t in toks {
        if m.closed_marker(t) {
            continue;
        }
        let (holder, value) = match m.toks[t as usize] {
            TokKind::Builtin(crate::model::BuiltinNs::GlobalThis)
                if crate::host::is_host_global(name) =>
            {
                let (top, own) = crate::builtins::native_written_by(m, t, a);
                if top || own {
                    why.get_or_insert(format!("host: the program or ⊤ writes globalThis.{name}"));
                }
                if m.interp_runs || m.unseen_module_runs || m.realm_held {
                    why.get_or_insert("host: unknown code holds the realm".into());
                }
                receivers.push("host:%Global%".into());
                let g = crate::builtins::realm_entry_of("%Global%")?;
                let o = g.own.iter().find(|o| !o.symbol && o.key == name)?;
                ("%Global%", o.value)
            }
            TokKind::Obj(tm, _) if !m.is_marker(t) => {
                let TmplKind::BObj { what, .. } = m.tmpls[tm as usize].kind else {
                    return None;
                };
                if !crate::host::is_host_proto(what) {
                    return None;
                }
                let fam = NativeFamily::Declared(what);
                let (holder, value) = crate::builtins::host_chain_find(what, name)?;
                if !native_own_free(m, fam, t, a) {
                    why.get_or_insert(format!("host: a receiver owns .{name}"));
                }
                if let Err(w) = builtin_protos_unwritten(m, Some(fam), a) {
                    why.get_or_insert(format!("host: {w}"));
                }
                receivers.push(m.tmpls[tm as usize].display.clone());
                (holder, value)
            }
            _ => return None,
        };
        any = true;
        match found {
            None => found = Some((holder, value)),
            Some(f) if f == (holder, value) => {}
            Some(_) => {
                why.get_or_insert("host: receivers resolve the key to different members".into());
            }
        }
    }
    let (_, value) = found.filter(|_| any)?;
    if nn & UNDEF != 0 {
        receivers.push("primitive:undefined (TypeError)".into());
    }
    if nn & NULL != 0 {
        receivers.push("primitive:null (TypeError)".into());
    }
    receivers.sort();
    receivers.dedup();
    let nullable = nn != 0;
    let verdict = match (why, value, store, s.is_call) {
        (Some(w), ..) => Verdict::No(w),
        (None, V::Accessor { set: Some(f), .. }, true, _) => Verdict::HostSet {
            member: f.to_string(),
            nullable,
        },
        (None, V::Accessor { get: Some(f), .. }, false, false) => Verdict::HostGet {
            member: f.to_string(),
            nullable,
        },
        (None, V::Accessor { .. }, ..) => {
            Verdict::No("host: an accessor without the part the site runs".into())
        }
        // A store onto a data member or method key: an own property of the
        // receiver (OrdinarySet), not the provider's.
        (None, _, true, _) => Verdict::No("host: a store onto a data member".into()),
        (None, V::Reference(r), false, _) => Verdict::HostMember {
            member: r.to_string(),
            nullable,
        },
        (None, _, false, true) => Verdict::No("host: a call of a primitive data member".into()),
        (None, _, false, false) => Verdict::HostMember {
            member: format!("{name}:data"),
            nullable,
        },
    };
    Some((receivers, verdict))
}
