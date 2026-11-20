use super::*;
use std::cell::{Cell, RefCell};

#[test]
fn dictionary_scopes_policies_defaults_and_malformed_results() {
    let cases = [
        (true, false, None, None, Trust::Trusted),
        (false, false, None, None, Trust::Unspecified),
        (true, true, None, None, Trust::Unspecified),
        (true, true, Some(true), Some(Ok(3)), Trust::Unspecified),
        (true, false, Some(false), Some(Ok(1)), Trust::Unspecified),
        (true, false, Some(true), Some(Ok(1)), Trust::Trusted),
        (false, false, Some(true), Some(Ok(1)), Trust::Unspecified),
        (false, false, Some(true), Some(Ok(2)), Trust::Trusted),
        (true, false, None, Some(Ok(2)), Trust::Trusted),
        (true, false, None, Some(Ok(3)), Trust::Distrusted),
        (false, false, None, Some(Ok(3)), Trust::Distrusted),
        (true, false, None, Some(Ok(0)), Trust::Unspecified),
        (true, false, None, Some(Ok(4)), Trust::Unspecified),
        (true, false, None, Some(Ok(99)), Trust::Unspecified),
        (true, false, None, Some(Err(())), Trust::Unspecified),
    ];
    for (self_issued, scoped, policy, result, expected) in cases {
        assert_eq!(
            dictionary_trust(self_issued, scoped, policy, result),
            expected,
            "self-issued={self_issued} scoped={scoped} ssl={policy:?} result={result:?}"
        );
    }
}

#[test]
fn empty_settings_and_first_decisive_dictionary() {
    assert_eq!(settings_trust(true, &[]), Trust::Trusted);
    assert_eq!(settings_trust(false, &[]), Trust::Unspecified);
    for self_issued in [false, true] {
        assert_eq!(
            settings_trust(self_issued, &[Trust::Unspecified]),
            Trust::Unspecified
        );
        assert_eq!(
            settings_trust(
                self_issued,
                &[Trust::Unspecified, Trust::Trusted, Trust::Distrusted]
            ),
            Trust::Trusted
        );
        assert_eq!(
            settings_trust(
                self_issued,
                &[Trust::Unspecified, Trust::Distrusted, Trust::Trusted]
            ),
            Trust::Distrusted
        );
    }
}

#[test]
fn user_decision_precedes_admin_and_fallback() {
    for (decision, expected) in [(Trust::Trusted, true), (Trust::Distrusted, false)] {
        let calls = RefCell::new(Vec::new());
        assert_eq!(
            selected_by_trust(
                true,
                |domain| {
                    calls.borrow_mut().push(domain);
                    DomainSettings::Entries(vec![decision])
                },
                || panic!("decisive settings must not evaluate trust")
            ),
            expected
        );
        assert_eq!(*calls.borrow(), vec![0]);
    }
    assert!(selected_by_trust(
        true,
        |domain| DomainSettings::Entries(vec![if domain == 0 {
            Trust::Unspecified
        } else {
            Trust::Trusted
        }]),
        || panic!("non-NULL unspecified settings do not evaluate")
    ));
}

#[test]
fn null_settings_evaluate_once_and_success_returns_before_admin() {
    let domains = RefCell::new(Vec::new());
    let evaluations = Cell::new(0);
    assert!(selected_by_trust(
        false,
        |domain| {
            domains.borrow_mut().push(domain);
            DomainSettings::Null
        },
        || {
            evaluations.set(evaluations.get() + 1);
            true
        }
    ));
    assert_eq!(*domains.borrow(), vec![0]);
    assert_eq!(evaluations.get(), 1);
    assert!(!selected_by_trust(
        false,
        |_| DomainSettings::Null,
        || {
            evaluations.set(evaluations.get() + 1);
            false
        }
    ));
    assert_eq!(
        evaluations.get(),
        2,
        "one evaluation per certificate, not per domain"
    );
}

#[test]
fn null_empty_unspecified_and_errors_remain_distinct() {
    // A non-NULL empty array trusts only self-issued certificates. For a
    // non-self-issued one, admin still gets a chance without fallback.
    assert!(selected_by_trust(
        false,
        |domain| {
            if domain == 0 {
                DomainSettings::Entries(vec![])
            } else {
                DomainSettings::Entries(vec![Trust::Trusted])
            }
        },
        || panic!("empty is not NULL")
    ));
    assert!(!selected_by_trust(
        false,
        |_| DomainSettings::Entries(vec![]),
        || panic!("empty is not NULL")
    ));
    assert!(selected_by_trust(
        true,
        |_| DomainSettings::Entries(vec![]),
        || panic!("empty self-issued trusts")
    ));
    assert!(!selected_by_trust(
        true,
        |_| DomainSettings::Error,
        || panic!("unexpected errors skip domain")
    ));
    assert!(selected_by_trust(
        false,
        |domain| {
            if domain == 0 {
                DomainSettings::Error
            } else {
                DomainSettings::Null
            }
        },
        || true
    ));
    // Failed NULL fallback does not prevent a later explicit admin decision.
    assert!(selected_by_trust(
        false,
        |domain| {
            if domain == 0 {
                DomainSettings::Null
            } else {
                DomainSettings::Entries(vec![Trust::Trusted])
            }
        },
        || false
    ));
}

#[test]
fn inventory_preserves_zero_single_multiple_and_first_seen_order() {
    let mut output = Vec::new();
    let mut seen = HashSet::new();
    assert!(output.is_empty());
    append_unique(&mut output, &mut seen, vec![2]);
    assert_eq!(output, vec![vec![2]]);
    append_unique(&mut output, &mut seen, vec![2]);
    assert_eq!(
        output,
        vec![vec![2]],
        "no forced multi-certificate fallback"
    );
    append_unique(&mut output, &mut seen, vec![1]);
    append_unique(&mut output, &mut seen, vec![2]);
    assert_eq!(
        output,
        vec![vec![2], vec![1]],
        "deduplicate without sorting"
    );
}

#[test]
fn expiry_filters_only_past_not_after_not_future_not_before() {
    use std::str::FromStr;
    use x509_cert::time::{Time, Validity};
    let before = Time::from_str("2050-01-01T00:00:00Z").unwrap();
    let after = Time::from_str("2051-01-01T00:00:00Z").unwrap();
    let validity = Validity::new(before, after);
    assert!(
        !expired(
            &validity,
            before.to_unix_duration() - Duration::from_secs(1)
        ),
        "future notBefore alone is not an inventory filter"
    );
    assert!(!expired(&validity, after.to_unix_duration()));
    assert!(expired(
        &validity,
        after.to_unix_duration() + Duration::from_secs(1)
    ));
}
