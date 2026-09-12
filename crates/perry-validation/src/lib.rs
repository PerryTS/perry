//! Fixed-grammar validators shared by Perry's two validator bindings.
//!
//! These functions borrow their inputs and call no Perry allocator or callback.
//! UUID and the ASCII email fast path allocate nothing. IDNA conversion and URL
//! parsing retain their existing library behavior and temporary native storage.
//! No regular-expression compiler, program cache or matcher is involved.
//!
//! Email behavior follows the previously used `validator` 0.21.0 implementation
//! (https://github.com/Keats/validator), including its IP-literal suffix rule.
//! Its license is retained in `UPSTREAM_VALIDATOR_LICENSE`.

/// Check the existing 8-4-4-4-12 ASCII hexadecimal UUID grammar.
/// Version and variant bits are deliberately unrestricted, as before.
pub fn is_uuid(input: &str) -> bool {
    let bytes = input.as_bytes();
    bytes.len() == 36
        && bytes.iter().enumerate().all(|(i, b)| {
            if matches!(i, 8 | 13 | 18 | 23) {
                *b == b'-'
            } else {
                b.is_ascii_hexdigit()
            }
        })
}

/// Check the email grammar and length limits previously supplied by validator.
pub fn is_email(input: &str) -> bool {
    // At most 64 ASCII local bytes, '@', and 255 four-byte domain characters.
    // Reject longer input before scanning it or invoking IDNA.
    if input.len() > 64 + 1 + 255 * 4 {
        return false;
    }
    let Some((local, domain)) = input.rsplit_once('@') else {
        return false;
    };
    if local.is_empty()
        || local.len() > 64
        || !local
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b".!#$%&'*+/=?^_`{|}~-".contains(&b))
        || domain.chars().count() > 255
    {
        return false;
    }
    if domain_part(domain) {
        return true;
    }
    idna::domain_to_ascii(domain).is_ok_and(|ascii| domain_part(&ascii))
}

fn domain_part(domain: &str) -> bool {
    if domain.split('.').all(|label| {
        let b = label.as_bytes();
        !b.is_empty()
            && b.len() <= 63
            && b[0].is_ascii_alphanumeric()
            && b[b.len() - 1].is_ascii_alphanumeric()
            && b.iter().all(|b| b.is_ascii_alphanumeric() || *b == b'-')
    }) {
        return true;
    }
    // The prior literal regex was anchored only at the end. Preserve that
    // observable suffix behavior, including prefixes before '[', in this
    // engine-removal change. IpAddr enforces the same IPv4/IPv6 grammar.
    domain
        .strip_suffix(']')
        .and_then(|s| s.rsplit_once('['))
        .is_some_and(|(_, ip)| ip.parse::<std::net::IpAddr>().is_ok())
}

/// Preserve the URL parser used by the previous validator trait.
pub fn is_url(input: &str) -> bool {
    url::Url::parse(input).is_ok()
}

#[cfg(test)]
mod tests;
