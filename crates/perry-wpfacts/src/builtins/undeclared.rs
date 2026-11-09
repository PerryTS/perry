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
                TokKind::BuiltinFn(Native::Member { identity }),
            )
            | (
                TokKind::BuiltinFn(Native::Member { identity }),
                TokKind::Builtin(BuiltinNs::ObjectProtoMethod),
            ) => identity.starts_with("Object.prototype."),
            _ => false,
        }
}
