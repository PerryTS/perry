//! Rule R1 ([`undeclared`]) is sound only if the solver's realm description
//! (`members`) declares every key the runtime's realm declares on the
//! objects it covers: a key the runtime declares (and may read) is no
//! program field. `realm_declared_keys.tsv` is the runtime realm's list
//! (generated from `realm-v1.json` by `scripts/wpfacts_realm_declared_keys.py`,
//! whose `--check` keeps it current); every row must be a member here.

#[cfg(test)]
use super::*;

pub(super) const DECLARED: &str = include_str!("realm_declared_keys.tsv");

/// The canonical realm reference under a native prototype member key.
#[cfg(test)]
pub(super) fn native_identity(owner: &str, key: &str) -> Option<&'static str> {
    DECLARED.lines().find_map(|line| {
        let mut row = line.split('\t');
        (row.next()? == "native" && row.next()? == owner && row.next()? == key)
            .then(|| row.next())
            .flatten()
    })
}

/// Coordinates of the canonical reference, including bracketed symbol
/// names. Alias rows do not choose the call model's holder or key.
#[cfg(test)]
pub(super) fn identity_member(identity: &str) -> Option<(&'static str, &'static str)> {
    DECLARED
        .lines()
        .find_map(|line| {
            let mut row = line.split('\t');
            if row.next()? != "native" {
                return None;
            }
            let owner = row.next()?;
            let key = row.next()?;
            if row.next()? != identity {
                return None;
            }
            let canonical = owner == "%Global%" && key == identity
                || identity.strip_prefix(owner).is_some_and(|suffix| {
                    suffix.strip_prefix('.') == Some(key)
                        || suffix
                            .strip_prefix("[\"")
                            .and_then(|s| s.strip_suffix("\"]"))
                            == Some(key)
                });
            canonical.then_some((owner, key))
        })
        .or_else(|| {
            let reference = DECLARED.lines().find_map(|line| {
                let mut row = line.split('\t');
                if row.next()? != "native" {
                    return None;
                }
                row.next()?;
                row.next()?;
                let reference = row.next()?;
                (reference == identity).then_some(reference)
            })?;
            if let Some((owner, key)) = reference.rsplit_once("[\"") {
                return Some((owner, key.strip_suffix("\"]")?));
            }
            reference.rsplit_once('.')
        })
}

/// A namespace token may summarize several constructors. Exempting it as
/// one known stored value requires every global reference it denotes to
/// name the same realm object.
#[cfg(test)]
pub(super) fn namespace_singleton(ns: BuiltinNs, identity: &str) -> bool {
    DECLARED
        .lines()
        .filter_map(|line| {
            let mut row = line.split('\t');
            if row.next()? != "native" || row.next()? != "%Global%" {
                return None;
            }
            row.next()?;
            row.next()
        })
        .filter(|candidate| ns_by_name(candidate) == Some(ns))
        .all(|candidate| candidate == identity)
}

/// The realm gives this own accessor exactly the pure returns(this) effect.
#[cfg(test)]
pub(super) fn getter_returns_receiver(ns: BuiltinNs, key: &str) -> bool {
    let key = key.replace(WK_PREFIX, "@@");
    DECLARED.lines().any(|line| {
        let mut row = line.split('\t');
        row.next() == Some("getter-this")
            && row.next().and_then(ns_by_name) == Some(ns)
            && row.next() == Some(key.as_str())
    })
}

/// Own declared keys of an identified constructor/namespace value. The
/// same rows cover holders without a specialized namespace call model.
#[cfg(test)]
pub(super) fn namespace_own_key(identity: &str, key: &str) -> bool {
    DECLARED.lines().any(|line| {
        let mut row = line.split('\t');
        row.next() == Some("ns") && row.next() == Some(identity) && row.next() == Some(key)
    })
}

/// A callable property of a namespace (Other is the HIR global sentinel).
/// Holder names and references come from the generated realm, not a second
/// list of global function names.
#[cfg(test)]
pub(super) fn namespace_identity(ns: BuiltinNs, key: &str) -> Option<&'static str> {
    let key = key.replace(WK_PREFIX, "@@");
    let ns = if ns == BuiltinNs::Other {
        BuiltinNs::GlobalThis
    } else {
        ns
    };
    DECLARED.lines().find_map(|line| {
        let mut row = line.split('\t');
        (row.next()? == "native" && ns_by_name(row.next()?) == Some(ns) && row.next()? == key)
            .then(|| row.next())
            .flatten()
    })
}

/// The realm-declared keys of `list` the description does not know, as
/// `holder.key`: keys rule R1 would take as program fields.
#[cfg(test)]
fn missing(list: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in list
        .lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
    {
        let row: Vec<&str> = line.split('\t').collect();
        if let ["getter-this" | "strings" | "object", _, _, _] = row[..] {
            continue;
        }
        if let ["native", owner, key, identity] = row[..] {
            assert_eq!(native_identity(owner, key), Some(identity));
            continue;
        }
        let [kind, holder, key] = row[..] else {
            panic!("bad row {line:?}")
        };
        // R1 never takes these keys (`undeclared::field_key`).
        if matches!(key, "prototype" | "__proto__") {
            continue;
        }
        let known = match kind {
            // A global the solver does not name as a namespace is no R1 holder;
            // an undescribed namespace (`ns_member` None) keeps every key opaque.
            "ns" => ns_by_name(holder).is_none_or(|ns| members::ns_member(ns, key) != Some(None)),
            "proto" | "fn" => members::member(Family::Function, key).is_some(),
            // The global object is no R1 holder (`undeclared`); its own keys
            // decide which globals the program owns (`global_user_key`).
            "global" => true,
            _ => panic!("bad row kind {line:?}"),
        };
        if !known {
            out.push(format!("{holder}.{key}"));
        }
    }
    out
}

#[test]
fn the_solver_knows_every_key_the_realm_declares() {
    // The list is the realm's: it holds the namespaces' statics and the
    // chain both kinds of holder share.
    assert!(DECLARED.contains("ns\tError\tprepareStackTrace\n"));
    assert!(DECLARED.contains("proto\tFunction.prototype\tcall\n"));
    assert!(DECLARED.lines().count() > 200);
    assert_eq!(missing(DECLARED), Vec::<String>::new());
    let keys = native_identity("Set.prototype", "keys");
    assert!(keys.is_some());
    assert_eq!(keys, native_identity("Set.prototype", "values"));
    assert_eq!(
        native_identity("Array.prototype", "map"),
        Some("Array.prototype.map")
    );
    assert_eq!(native_identity("Array.prototype", "zzNoSuchMember"), None);
}

#[test]
fn twin_a_key_the_description_lacks_is_reported() {
    let twin = "ns\tObject\tlineMap\nproto\tFunction.prototype\tlineMap\nns\tDate\tlineMap\n";
    assert_eq!(
        missing(twin),
        ["Object.lineMap", "Function.prototype.lineMap"]
    );
}
