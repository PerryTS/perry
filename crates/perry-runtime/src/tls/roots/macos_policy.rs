//! Node-compatible private selection policy, independent of CoreFoundation.

use std::collections::HashSet;
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Trust {
    Unspecified,
    Trusted,
    Distrusted,
}

/// Facts from one dictionary. Absence, a malformed value, and a non-SSL
/// policy have different meanings; do not collapse them into booleans.
pub(super) fn dictionary_trust(
    self_issued: bool,
    scoped: bool,
    ssl_policy: Option<bool>,
    result: Option<Result<i32, ()>>,
) -> Trust {
    if scoped || ssl_policy == Some(false) {
        return Trust::Unspecified;
    }
    match result.unwrap_or(Ok(1)) {
        Ok(3) => Trust::Distrusted,
        Ok(2) => Trust::Trusted,
        Ok(1) if self_issued => Trust::Trusted,
        _ => Trust::Unspecified,
    }
}

pub(super) fn settings_trust(self_issued: bool, entries: &[Trust]) -> Trust {
    if entries.is_empty() && self_issued {
        return Trust::Trusted;
    }
    entries
        .iter()
        .copied()
        .find(|entry| *entry != Trust::Unspecified)
        .unwrap_or(Trust::Unspecified)
}

pub(super) enum DomainSettings {
    Error,
    Null,
    Entries(Vec<Trust>),
}

/// Query lazily: a decisive user result must not consult admin settings, and
/// successful fallback evaluation on a NULL array returns before admin too.
pub(super) fn selected_by_trust(
    self_issued: bool,
    mut domain: impl FnMut(u32) -> DomainSettings,
    mut evaluate: impl FnMut() -> bool,
) -> bool {
    let mut evaluated = false;
    for index in [0, 1] {
        match domain(index) {
            DomainSettings::Error => continue,
            DomainSettings::Null if !evaluated => {
                evaluated = true;
                if evaluate() {
                    return true;
                }
            }
            DomainSettings::Null => {}
            DomainSettings::Entries(entries) => match settings_trust(self_issued, &entries) {
                Trust::Trusted => return true,
                Trust::Distrusted => return false,
                Trust::Unspecified => {}
            },
        }
    }
    false
}

pub(super) fn expired(validity: &x509_cert::time::Validity, now: Duration) -> bool {
    // Node filters past notAfter, not notBefore. Trust evaluation can impose
    // additional checks itself, but explicit trust settings do not add one.
    validity.not_after.to_unix_duration() < now
}

pub(super) fn append_unique(output: &mut Vec<Vec<u8>>, seen: &mut HashSet<Vec<u8>>, der: Vec<u8>) {
    if seen.insert(der.clone()) {
        output.push(der);
    }
}

#[cfg(test)]
#[path = "macos_policy_tests.rs"]
mod tests;
