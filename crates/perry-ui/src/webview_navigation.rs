use url::{Host, Url};

/// Match the destination host or blob creator origin, never URL credentials.
pub fn navigation_allowed(url: &str, allowed_domains: &[String]) -> bool {
    if allowed_domains.is_empty() {
        return true;
    }
    let Ok(url) = Url::parse(url) else {
        return false;
    };
    let url = if url.scheme() == "blob" {
        let Ok(origin) = Url::parse(url.path()) else {
            return false;
        };
        if !matches!(origin.scheme(), "http" | "https") {
            return false;
        }
        origin
    } else {
        url
    };
    let Some(host) = url.host() else {
        return false;
    };

    allowed_domains
        .iter()
        .any(|domain| match (&host, Host::parse(domain)) {
            (Host::Domain(host), Ok(Host::Domain(domain))) => {
                *host == domain
                    || host
                        .strip_suffix(domain.as_str())
                        .is_some_and(|prefix| prefix.ends_with('.'))
            }
            (Host::Ipv4(host), Ok(Host::Ipv4(domain))) => *host == domain,
            (Host::Ipv6(host), Ok(Host::Ipv6(domain))) => *host == domain,
            _ => false,
        })
}

#[cfg(test)]
mod tests {
    use super::navigation_allowed;

    fn allows(url: &str, domains: &[&str]) -> bool {
        navigation_allowed(
            url,
            &domains.iter().map(|d| d.to_string()).collect::<Vec<_>>(),
        )
    }

    #[test]
    fn credentials_cannot_impersonate_the_destination_host() {
        assert!(!allows(
            "https://allowed.example:pw@evil.example/",
            &["allowed.example"]
        ));
        assert!(!allows(
            "https://allowed.example:pw@sub.evil.example/",
            &["allowed.example"]
        ));
        assert!(allows(
            "https://user:pw@allowed.example/",
            &["allowed.example"]
        ));
    }

    #[test]
    fn blob_urls_use_their_embedded_http_origin() {
        for url in [
            "blob:https://allowed.example/id",
            "blob:http://sub.allowed.example:8080/id",
        ] {
            assert!(allows(url, &["allowed.example"]), "{url}");
        }
        for url in [
            "blob:https://attacker.test/id",
            "blob:https://allowed.example:secret@attacker.test/id",
            "blob:null/id",
            "blob:blob:https://allowed.example/id",
            "blob:ftp://allowed.example/id",
            "blob:file://allowed.example/id",
            "blob://allowed.example/id",
        ] {
            assert!(!allows(url, &["allowed.example"]), "{url}");
        }
    }

    #[test]
    fn matches_later_allowlist_entries() {
        assert!(allows(
            "https://second.example/",
            &["first.example", "second.example"]
        ));
    }

    #[test]
    fn exact_hosts_and_subdomains_match_but_suffix_lookalikes_do_not() {
        for url in [
            "https://allowed.example/",
            "https://sub.allowed.example/",
            "http://deep.sub.allowed.example:8080/path",
            "ftp://allowed.example/path",
            "custom://allowed.example/path",
        ] {
            assert!(allows(url, &["allowed.example"]), "{url}");
        }
        for url in [
            "https://notallowed.example/",
            "https://allowed.example.evil.test/",
            "https://evil.example/?allowed.example",
        ] {
            assert!(!allows(url, &["allowed.example"]), "{url}");
        }
    }

    #[test]
    fn canonicalizes_domain_case_and_idna_in_both_inputs() {
        assert!(allows("https://ALLOWED.EXAMPLE/", &["allowed.example"]));
        assert!(allows("https://allowed.example/", &["ALLOWED.EXAMPLE"]));
        assert!(allows(
            "https://bücher.example/",
            &["xn--bcher-kva.example"]
        ));
        assert!(allows(
            "https://xn--bcher-kva.example/",
            &["bücher.example"]
        ));
    }

    #[test]
    fn ip_addresses_match_as_addresses_not_domain_suffixes() {
        assert!(allows("http://127.0.0.1:8080/", &["127.0.0.1"]));
        assert!(allows("http://[::1]:8080/", &["[0:0:0:0:0:0:0:1]"]));
        assert!(!allows("http://[::2]/", &["[::1]"]));
        assert!(!allows("https://sub.127.0.0.1/", &["127.0.0.1"]));
    }

    #[test]
    fn rejects_invalid_and_hostless_urls_when_restricted() {
        for url in [
            "not a URL",
            "https://[invalid]/",
            "https://allowed.example:invalid/",
            "about:blank",
            "data:text/html,allowed.example",
        ] {
            assert!(!allows(url, &["allowed.example"]), "{url}");
        }
        assert!(!allows(
            "https://allowed.example/",
            &["https://allowed.example", ""]
        ));
    }

    #[test]
    fn uses_url_semantics_for_backslashes_and_encoded_delimiters() {
        assert!(!allows(
            r"https://evil.example\@allowed.example/",
            &["allowed.example"]
        ));
        assert!(allows(
            r"https://allowed.example\@evil.example/",
            &["allowed.example"]
        ));
        assert!(allows("https://%61llowed.example/", &["allowed.example"]));
        assert!(!allows(
            "https://allowed.example%40evil.example/",
            &["allowed.example"]
        ));
    }

    #[test]
    fn empty_allowlist_preserves_unrestricted_behavior() {
        for url in ["https://evil.example/", "about:blank", "not a URL"] {
            assert!(allows(url, &[]));
        }
    }
}
