//! Rule R1 ([`undeclared`]) is sound only if the solver's realm description
//! (`members`) declares every key the runtime's realm declares on the
//! objects it covers: a key the runtime declares (and may read) is no
//! program field. `realm_declared_keys.tsv` is the runtime realm's list
//! (generated from `realm-v1.json` by `scripts/wpfacts_realm_declared_keys.py`,
//! whose `--check` keeps it current); every row must be a member here.

#[cfg(test)]
use super::*;

const DECLARED: &str = include_str!("realm_declared_keys.tsv");

/// The canonical realm reference under a native prototype member key.
pub(super) fn native_identity(owner: &str, key: &str) -> Option<&'static str> {
    DECLARED.lines().find_map(|line| {
        let mut row = line.split('\t');
        (row.next()? == "native" && row.next()? == owner && row.next()? == key)
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
