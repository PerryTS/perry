//! `facts_read`: the facts half of a field read's proof, shared by `prove`
//! and the parameter fate. It admits a definite own data birth field of
//! every receiver class as a bare load, and refuses or guards each case the
//! fate relies on: a key that is not a birth data field of a class (an
//! accessor or an inherited key: the facts list only birth data fields), an
//! unknown or too large receiver set, a private key, and every guarded
//! verdict (checked binding, interpreted code, declared or reserved key).

use super::facts_read;
use crate::codegen::{ElisionFacts, FactsClass, FactsField, FactsRecv, FactsRep};

fn field(key: &str, slot: u32) -> FactsField {
    FactsField {
        key: key.into(),
        slot,
        readonly: false,
        kind_mask: 1,
        ta_stable: false,
        ta_kind: None,
        declared: false,
        reserved: false,
        fns: vec![],
        classes: vec![],
        nullable: false,
        rep: FactsRep::Boxed,
        rest: false,
    }
}

/// Class 1 has own data fields `x`, `y`; class 2 only `x` (its `y` is an
/// accessor or inherited, so the facts do not list it); classes 3..=7
/// each have `x`.
fn facts() -> ElisionFacts {
    let mut f = ElisionFacts::default();
    let class = |fields| FactsClass {
        display: "C".into(),
        fixed_layout: true,
        fields,
    };
    f.classes
        .insert(1, class(vec![field("x", 0), field("y", 1)]));
    f.classes.insert(2, class(vec![field("x", 0)]));
    for cid in 3..=7 {
        f.classes.insert(cid, class(vec![field("x", 0)]));
    }
    f
}

fn recv(class_ids: &[u32]) -> FactsRecv {
    FactsRecv {
        class_ids: class_ids.to_vec(),
        nullable: false,
        checked: false,
    }
}

#[test]
fn an_own_data_field_of_every_class_is_a_bare_load() {
    let f = facts();
    assert_eq!(facts_read(&f, &recv(&[1]), "y"), Ok(true));
    assert_eq!(facts_read(&f, &recv(&[1, 2, 3, 4]), "x"), Ok(true));
}

#[test]
fn a_key_missing_from_one_class_is_refused() {
    // Accessor or inherited key: not a birth data field of class 2.
    let f = facts();
    assert!(facts_read(&f, &recv(&[2]), "y").is_err());
    assert!(facts_read(&f, &recv(&[1, 2]), "y").is_err());
    assert!(facts_read(&f, &recv(&[1]), "z").is_err());
    // A class the facts do not know at all.
    assert!(facts_read(&f, &recv(&[9]), "x").is_err());
}

#[test]
fn an_unknown_or_large_receiver_set_is_refused() {
    let f = facts();
    assert!(facts_read(&f, &recv(&[]), "x").is_err());
    assert!(facts_read(&f, &recv(&[3, 4, 5, 6, 7]), "x").is_err());
}

#[test]
fn a_private_key_is_refused() {
    let mut f = facts();
    f.classes.get_mut(&1).unwrap().fields.push(field("#p", 2));
    assert!(facts_read(&f, &recv(&[1]), "#p").is_err());
}

#[test]
fn every_guarded_verdict_is_not_bare() {
    let f = facts();
    let mut checked = recv(&[1]);
    checked.checked = true;
    assert_eq!(facts_read(&f, &checked, "x"), Ok(false));

    let mut interp = facts();
    interp.interp_guard = true;
    assert_eq!(facts_read(&interp, &recv(&[1]), "x"), Ok(false));

    for (declared, reserved) in [(true, false), (false, true)] {
        let mut g = facts();
        let y = &mut g.classes.get_mut(&1).unwrap().fields[1];
        y.declared = declared;
        y.reserved = reserved;
        assert_eq!(
            facts_read(&g, &recv(&[1]), "y"),
            Ok(false),
            "declared={declared} reserved={reserved}"
        );
        // One guarded class guards the whole read.
        assert_eq!(facts_read(&g, &recv(&[1]), "x"), Ok(true));
    }
}
