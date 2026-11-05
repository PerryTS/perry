//! Rule R1 (STR1 design §3): the keys a built-in namespace or native
//! function object does not declare. The runtime reads only the keys its
//! realm declares on its own objects (the description in `members`), so a
//! program value stored under any other key of such an object is read back
//! by the program alone: a field of that object, not a value gone to the
//! runtime.

use super::*;

/// Where a store under a key built-in object `t` does not declare goes
/// (`Model::builtin_field`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Undeclared {
    /// A described namespace or constructor (`Object`, `Math`, ...): its own
    /// field under the key.
    Namespace,
    /// A native function value whose keys are `Function.prototype`'s: the
    /// one field every native function value shares under the key. Natives
    /// alias (`Set.prototype.keys === Set.prototype.values`, an
    /// `Object.prototype` method token is also a `Native::Member`), and the
    /// anonymous native stands for any of them.
    NativeFn,
}

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

/// A native function value or `Object.prototype` method token: its keys
/// are `Function.prototype`'s (and `Object.prototype`'s).
fn native_fn(m: &Model, t: Tok) -> bool {
    matches!(
        m.toks[t as usize],
        TokKind::BuiltinFn(_)
            | TokKind::Builtin(BuiltinNs::ObjectProtoMethod | BuiltinNs::ObjectProtoOwn(_))
    )
}

/// Rule R1: `Some` when a store under named key `a` into built-in object
/// `t` is a field store, because the description does not declare `a` for
/// `t` (own or on its chain). `None`: a declared or undescribed key, or a
/// holder the rule does not cover; they keep the opaque path. Only whole
/// program mode. `Native::Any` is no store holder: it may be a native the
/// description does not cover (an undescribed family's constructor, an
/// undescribed namespace's static), whose declared keys are unknown.
pub fn undeclared(m: &Model, t: Tok, a: Atom) -> Option<Undeclared> {
    if t == TOP || !field_key(m, a) {
        return None;
    }
    let name = m.atoms[a as usize].as_str();
    match &m.toks[t as usize] {
        TokKind::BuiltinFn(Native::Any) => None,
        _ if native_fn(m, t) => members::member(Family::Function, name)
            .is_none()
            .then_some(Undeclared::NativeFn),
        TokKind::Builtin(BuiltinNs::GlobalThis) => None,
        TokKind::Builtin(ns) => {
            (members::ns_member(*ns, name) == Some(None)).then_some(Undeclared::Namespace)
        }
        _ => None,
    }
}

/// Does a read of named key `a` through native function value `t` (any
/// native, `Native::Any` included, or an `Object.prototype` method token)
/// find the field R1 stores make under `a` (`Undeclared::NativeFn`)?
pub fn native_fn_field(m: &Model, t: Tok, a: Atom) -> bool {
    t != TOP
        && native_fn(m, t)
        && field_key(m, a)
        && members::member(Family::Function, &m.atoms[a as usize]).is_none()
}

/// Is `t` a native function value whose computed-key reads see every R1
/// field of native function values.
pub fn native_fn_holder(m: &Model, t: Tok) -> bool {
    t != TOP && !m.unit_mode && native_fn(m, t)
}
