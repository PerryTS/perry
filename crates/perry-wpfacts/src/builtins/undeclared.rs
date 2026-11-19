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
/// global the runtime itself reads by name? The runtime reads a built-in
/// constructor's global by name (`js_get_global_this_builtin_value`,
/// `builtin_value_core`, `builtin_prototype_value`: instanceof, class
/// metadata, the realm's own inheritance links, messaging), so a key the
/// realm declares with a function value (or with no native value the model
/// knows) keeps E20; so do the names `members::global_runtime_reads`
/// declares, `process` and the host objects. A declared object the runtime
/// cannot call (`performance`, `Math`) is the program's field
/// (`declared_callable`). Any other named key (`globalThis`, an undeclared global such
/// as `web3d`) is an own property of the global object that only program
/// code reads, through its field. A computed key may name any global.
/// A named key the realm declares as the global object's reference to
/// itself (`globalThis`, `global`). The runtime reaches the global object
/// through `js_get_global_this`, never by reading these names, but a bare
/// `globalThis` / `global` read is the global object in the model
/// (`ns_by_name`), so only the global object itself may be stored there
/// without escaping (`tok_store`).
pub fn global_self_key(m: &Model, t: Tok, key: Key) -> bool {
    let Key::Named(a) = key else {
        return false;
    };
    let name = &m.atoms[a as usize];
    matches!(ns_by_name(name), Some(BuiltinNs::GlobalThis)) && m.native_property(t, name).is_some()
}

pub fn global_store_escapes(m: &mut Model, t: Tok, key: Key) -> bool {
    let Key::Named(a) = key else {
        return true;
    };
    let name = m.atoms[a as usize].clone();
    if members::global_runtime_reads(&name) {
        return true;
    }
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
    // The declared value: its native identity, else the realm declaration
    // the global is named after (`performance`, `Math`: a namespace row).
    let identity = p.native.unwrap_or(name.as_str());
    // `process` and the host objects (`console`) keep E20, as their own
    // stores do (`plain_namespace_own_store`).
    if ns_by_name(&name).is_some_and(|ns| ns == BuiltinNs::Process || members::is_host(ns)) {
        return true;
    }
    declared_callable(identity)
}

/// Is the realm's declared value `identity` callable (a constructor, a
/// function)? The runtime reads a callable global by name; a declared
/// object it cannot call (`performance`, `Math`, `JSON`, `Reflect`, `Intl`,
/// `Atomics`: a `Namespace` in the realm table) it reaches through its own
/// cells, never by reading the global (the names it does read are
/// `members::global_runtime_reads`). An identity the table does not list
/// (no native value the model knows) keeps E20.
fn declared_callable(identity: &str) -> bool {
    use perry_hir::realm_manifest::RealmKind;
    crate::native_effects::realm()
        .iter()
        .find(|e| e.name == identity)
        .is_none_or(|e| {
            !matches!(
                e.kind,
                RealmKind::Namespace
                    | RealmKind::Exotic
                    | RealmKind::Prototype
                    | RealmKind::Constant
            )
        })
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
