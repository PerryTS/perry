//! Rule R1 (STR1 design §3): the keys a built-in namespace or native
//! function object does not declare. The runtime reads only the keys its
//! realm declares on its own objects (the description in `members`), so a
//! program value stored under any other key of such an object is read back
//! by the program alone: a field of that object, not a value gone to the
//! runtime.

use super::*;

/// The keys no built-in object takes as a program field: a symbol key (the
/// runtime reads well-known symbols on any object), `prototype` (read on
/// any callable by `instanceof` and `new`), `__proto__`, a number, and the
/// solver's computed and element keys.
fn field_key(m: &Model, a: Atom) -> bool {
    let name = m.atoms[a as usize].as_str();
    !(m.unit_mode
        || m.is_private_atom(a)
        || name.starts_with(WK_PREFIX)
        || name == crate::STAR_KEY
        || name == NUM_KEY
        || name == "prototype"
        || name == "__proto__"
        || crate::solve::numeric_key(name))
}

/// The global object: `globalThis`, and the HIR global sentinel (`Other`,
/// the receiver a bare global name is read off), which is the same object.
pub fn global_object(m: &Model, t: Tok) -> bool {
    matches!(
        m.toks[t as usize],
        TokKind::Builtin(BuiltinNs::GlobalThis | BuiltinNs::Other)
    )
}

/// Native function-value tokens, including the Object.prototype method
/// identities represented as namespaces.
pub fn builtin_function(m: &Model, t: Tok) -> bool {
    match m.toks[t as usize] {
        TokKind::BuiltinFn(_) => true,
        TokKind::Builtin(BuiltinNs::ObjectProtoOwn(_) | BuiltinNs::ObjectProtoMethod) => true,
        TokKind::Builtin(ns) => !m.unit_mode && members::ns_chain(ns) == Family::Function,
        _ => false,
    }
}

/// May a store under `key` on built-in constructor or namespace `t` write a
/// static the realm itself reads (`members::realm_read_statics`)? A
/// namespace whose statics are undescribed may have any. Any other key is
/// an own property of `t` (OrdinarySet) that only reads of `t` see: program
/// code and the protocols the model runs on `t`, all through `t`'s fields.
pub fn realm_reads_store(m: &Model, t: Tok, key: Key) -> bool {
    let TokKind::Builtin(ns) = m.toks[t as usize] else {
        return true;
    };
    members::ns_statics(ns).members.is_none()
        || members::realm_read_statics(ns)
            .iter()
            .any(|n| m.key_may_name_str(key, n))
}

/// A named-key store on a non-callable built-in namespace object writes its
/// own property, not one the realm reads (`realm_reads_store` for the
/// callable ones): `performance`, `Math`, `JSON`, `Reflect`, a builtin
/// module namespace. The runtime implements these natively and
/// never reads a property of the object back by name, beside the statics
/// `members::realm_read_statics` declares. Not the global object
/// (R-GLOBAL-NAME, `global_store_escapes`), not `process` (the runtime reads
/// `process.exitCode` and its streams by name), not an undescribed object
/// (`Other`), not `Object.prototype`, not a host object (`console`, the
/// stdio streams: node's console calls a replaced `process.stdout.write`).
/// A computed key may name any member.
pub fn plain_namespace_own_store(m: &Model, t: Tok, key: Key) -> bool {
    let Key::Named(a) = key else {
        return false;
    };
    let TokKind::Builtin(ns) = m.toks[t as usize] else {
        return false;
    };
    if m.unit_mode
        || global_object(m, t)
        || members::ns_chain(ns) == Family::Function
        || members::is_host(ns)
        || matches!(
            ns,
            BuiltinNs::GlobalThis
                | BuiltinNs::Other
                | BuiltinNs::Process
                | BuiltinNs::ObjectProtoOwn(_)
                | BuiltinNs::ObjectProtoMethod
        )
    {
        return false;
    }
    let name = &m.atoms[a as usize];
    !members::realm_read_statics(ns).iter().any(|n| n == name)
}

/// R-GLOBAL-NAME: may a store under `key` on the global object `t` write a
/// global the runtime itself reads by name? Owner decision 2026-11-24
/// (RTREAD): only a name perry-runtime reads at run time
/// (`members::global_runtime_reads`, derived from the runtime's code),
/// `process` and the host objects keep E20. A realm constructor's global
/// (`EventTarget`, `Map`) is read by name only at program sites
/// (`x instanceof Name`, `extends Name`, a bare identifier), which read the
/// global binding and see the store; any other named key (`globalThis`,
/// `performance`, an undeclared global such as `web3d`) is an own property
/// of the global object that only program code reads, through its field. A
/// computed key may name any global.
/// The global object's self-references (`globalThis`, `global`, R-GLOBAL-SELF)
/// are such keys: the runtime reaches the global object through
/// `js_get_global_this`, never by reading these names, and a program read
/// of the name (bare, `globalThis.X`, `typeof`) takes the field
/// (`declared_global_value`), so what the program stores there is its own.
pub fn global_store_escapes(m: &mut Model, t: Tok, key: Key) -> bool {
    let Key::Named(a) = key else {
        return true;
    };
    let name = m.atoms[a as usize].clone();
    // A name the runtime reads at run time (`onmessage`,
    // `RUNTIME_GLOBAL_READS`) is the program's field too: the runtime's
    // whole use of it is the handler call `Gen::runtime_global_handlers`
    // hangs on that field (RTREAD14), no escape.
    let Some(p) = m.native_property(t, &name) else {
        return false;
    };
    // A key the realm declares as a getter-only accessor (`navigator`,
    // perry-runtime global_this/populate.rs: `set: 0`): the runtime reaches
    // its object through the getter it installed, never by reading the key.
    // A [[Set]] runs no setter and writes nothing (ECMA-262 10.1.9.2), and a
    // data define (20.1.2.4) replaces the accessor with the value it gives,
    // which only program code reads, through the global's field
    // (`declared_global_value`).
    if p.native.is_none() && p.get.is_some() && p.set.is_none() {
        return false;
    }
    // `process` and the host objects (`console`) keep E20, as their own
    // stores do (`plain_namespace_own_store`).
    ns_by_name(&name).is_some_and(|ns| ns == BuiltinNs::Process || members::is_host(ns))
}

/// Owner decision 2026-11-19: a key the realm declares on the global
/// object as a writable data property holding an object (`performance`,
/// `Math`, `JSON`; owner 2026-11-24 RTREAD: a constructor such as `Array`
/// or `EventTarget` too, whose global the runtime never reads by name: a
/// `Reference` own of `%Global%`) is the program's field once the program
/// stores it, the same field `global_store_escapes` keeps the stored value
/// in and `declared_global_value` reads back. [[Set]] (ECMA-262 10.1.9.2
/// step 2.c-2.d) replaces the value of a writable own data property; the
/// realm's non-writable global data properties are its primitive constants
/// (`NaN`, `Infinity`, `undefined`, 19.1), which hold no `Reference`.
/// `global_runtime_reads` (`WebAssembly`), `process`, the host objects
/// (`console`) and accessors (`navigator`, `crypto`) stay out: the runtime
/// reads those by name or through a getter.
pub fn declared_data_global(m: &Model, t: Tok, name: &str) -> bool {
    use perry_hir::realm_manifest::RealmOwnValue;
    if members::global_runtime_reads(name)
        || ns_by_name(name).is_some_and(|ns| ns == BuiltinNs::Process || members::is_host(ns))
    {
        return false;
    }
    let Some(p) = m.native_property(t, name) else {
        return false;
    };
    if p.get.is_some() || p.set.is_some() {
        return false;
    }
    let reference = members::realm_entry("%Global%").and_then(|e| {
        e.own
            .iter()
            .find(|o| members::realm_key_is(o, name))
            .and_then(|o| match o.value {
                RealmOwnValue::Reference(r) => Some(r),
                _ => None,
            })
    });
    reference.is_some()
}

/// A described built-in object and its chain declare no such program key.
pub fn undeclared(m: &Model, t: Tok, a: Atom) -> bool {
    if t == TOP || !field_key(m, a) {
        return false;
    }
    match m.toks[t as usize] {
        TokKind::BuiltinFn(Native::Member { .. }) => {
            !m.native_property(t, &m.atoms[a as usize])
                .is_some_and(|p| p.own_namespace)
                && members::member(Family::Function, &m.atoms[a as usize]).is_none()
        }
        _ if global_object(m, t) => false,
        TokKind::Builtin(ns) => members::ns_member(ns, &m.atoms[a as usize]) == Some(None),
        _ => false,
    }
}

/// Denotation, not equivalence: Any may be X or Y without identifying X
/// with Y. All value observations use this predicate, including chain and
/// accessor reads and the reach exposed to unknown code.
pub fn native_may_be(m: &Model, a: Tok, b: Tok) -> bool {
    if !builtin_function(m, a) || !builtin_function(m, b) {
        return false;
    }
    a == b
        || matches!(m.toks[a as usize], TokKind::BuiltinFn(Native::Any))
        || matches!(m.toks[b as usize], TokKind::BuiltinFn(Native::Any))
        || match (&m.toks[a as usize], &m.toks[b as usize]) {
            // A computed Object.prototype read has a narrower call model.
            (
                TokKind::Builtin(BuiltinNs::ObjectProtoMethod),
                TokKind::Builtin(BuiltinNs::ObjectProtoOwn(_)),
            )
            | (
                TokKind::Builtin(BuiltinNs::ObjectProtoOwn(_)),
                TokKind::Builtin(BuiltinNs::ObjectProtoMethod),
            ) => true,
            (
                TokKind::Builtin(BuiltinNs::ObjectProtoMethod),
                TokKind::BuiltinFn(Native::Member { identity, .. }),
            )
            | (
                TokKind::BuiltinFn(Native::Member { identity, .. }),
                TokKind::Builtin(BuiltinNs::ObjectProtoMethod),
            ) => identity.starts_with("Object.prototype."),
            _ => false,
        }
}
