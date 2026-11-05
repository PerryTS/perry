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

/// Native function-value tokens, including the Object.prototype method
/// identities represented as namespaces.
pub fn builtin_function(m: &Model, t: Tok) -> bool {
    match m.toks[t as usize] {
        TokKind::BuiltinFn(_) => true,
        TokKind::Builtin(BuiltinNs::ObjectProtoOwn(_) | BuiltinNs::ObjectProtoMethod) => true,
        _ => false,
    }
}

/// A non-callable namespace's undeclared named property is program-owned.
pub fn undeclared(m: &Model, t: Tok, a: Atom) -> bool {
    if t == TOP || !field_key(m, a) || builtin_function(m, t) {
        return false;
    }
    match m.toks[t as usize] {
        TokKind::Builtin(BuiltinNs::GlobalThis) => false,
        TokKind::Builtin(ns) => members::ns_member(ns, &m.atoms[a as usize]) == Some(None),
        _ => false,
    }
}

/// Overlap is a relation between the denotations of two native tokens,
/// not an equivalence relation: the anonymous native overlaps both X and
/// Y without making X overlap Y. The realm resolves named native aliases.
pub fn native_overlap(m: &Model, a: Tok, b: Tok) -> bool {
    if !builtin_function(m, a) || !builtin_function(m, b) {
        return false;
    }
    if a == b
        || matches!(m.toks[a as usize], TokKind::BuiltinFn(Native::Any))
        || matches!(m.toks[b as usize], TokKind::BuiltinFn(Native::Any))
    {
        return true;
    }
    match (native_identity(m, a), native_identity(m, b)) {
        (Some(a), Some(b)) => a == b,
        // An undescribed identity cannot establish disjointness.
        _ => true,
    }
}

fn native_identity(m: &Model, t: Tok) -> Option<String> {
    let path = match m.toks[t as usize] {
        TokKind::BuiltinFn(Native::Member { fam, what, key }) => {
            let owner = match what {
                Some(w) => format!("{w}.prototype"),
                None => crate::native_effects::realm().iter().find_map(|e| {
                    let ns = ns_by_name(e.name)?;
                    let (f, w, _) = super::loads::proto_family(ns)?;
                    (f == fam && w.is_none()).then(|| format!("{}.prototype", e.name))
                })?,
            };
            let name = m.atoms[key as usize].replace(WK_PREFIX, "@@");
            let entry = crate::native_effects::realm()
                .iter()
                .find(|e| e.name == owner)?;
            let facts: serde_json::Value = serde_json::from_str(entry.facts).ok()?;
            let prop = facts
                .get("properties")?
                .as_array()?
                .iter()
                .find(|p| p.get("key").and_then(|k| k.as_str()) == Some(name.as_str()))?;
            prop.get("value")?.get("value")?.as_str()?.to_string()
        }
        TokKind::Builtin(ns) => crate::native_effects::realm().iter().find_map(|e| {
            let matches = match ns {
                BuiltinNs::ObjectProtoOwn(p) => e
                    .name
                    .strip_prefix("Object.prototype.")
                    .is_some_and(|k| ProtoOwn::of_name(k) == Some(p)),
                _ => ns_by_name(e.name) == Some(ns),
            };
            matches.then(|| e.name.to_string())
        })?,
        _ => return None,
    };
    Some(path)
}
