use super::*;

#[test]
fn frameworks_symbols_and_read_only_query_are_available() {
    assert!(api().is_some(), "local framework symbol binding failed");
    let certificates = read_inventory().expect("default keychain certificate query failed");
    let mut seen = HashSet::new();
    for der in certificates {
        assert!(x509_cert::Certificate::from_der(&der).is_ok());
        assert!(seen.insert(der), "native query must not publish duplicates");
    }
}

#[test]
fn pem_encoding_preserves_der_without_inventory_substitutes() {
    let body = [0, 1, 2, 3, 255];
    let encoded = pem(&body);
    assert!(encoded.starts_with("-----BEGIN CERTIFICATE-----\n"));
    assert!(encoded.ends_with("-----END CERTIFICATE-----\n"));
    let base64: String = encoded
        .lines()
        .filter(|line| !line.starts_with("-----"))
        .collect();
    assert_eq!(
        perry_base64::engine::general_purpose::STANDARD
            .decode(base64)
            .unwrap(),
        body
    );
}

#[test]
fn native_inventory_pem_preserves_selected_certificates() {
    let certificates = read_inventory().expect("default keychain certificate query failed");
    for der in certificates {
        let encoded = pem(&der);
        let body: String = encoded
            .lines()
            .filter(|line| !line.starts_with("-----"))
            .collect();
        let decoded = perry_base64::engine::general_purpose::STANDARD
            .decode(body)
            .unwrap();
        assert_eq!(decoded, der);
        assert!(x509_cert::Certificate::from_der(&decoded).is_ok());
    }
}

#[test]
fn native_dictionary_type_and_policy_checks_match_selection_rules() {
    let api = api().expect("framework symbols");
    unsafe {
        let dictionary = |keys: &[Cf], values: &[Cf]| {
            api.owned((api.dictionary_create)(
                ptr::null(),
                keys.as_ptr(),
                values.as_ptr(),
                keys.len() as isize,
                api.dictionary_key_callbacks,
                api.dictionary_value_callbacks,
            ))
            .unwrap()
        };
        let empty = dictionary(&[], &[]);
        assert_eq!(api.dictionary_trust(empty.value, true), Trust::Trusted);
        assert_eq!(api.dictionary_trust(empty.value, false), Trust::Unspecified);
        for key in [api.application_key.value, api.policy_string_key.value] {
            let scoped = dictionary(&[key], &[api.boolean_true]);
            assert_eq!(api.dictionary_trust(scoped.value, true), Trust::Unspecified);
        }
        // A CFBoolean is not a numeric trust result or a SecPolicyRef.
        for key in [api.result_key.value, api.policy_key.value] {
            let malformed = dictionary(&[key], &[api.boolean_true]);
            assert_eq!(
                api.dictionary_trust(malformed.value, true),
                Trust::Unspecified
            );
        }
        let ssl = api.owned((api.policy_ssl)(0, ptr::null())).unwrap();
        let ssl_settings = dictionary(&[api.policy_key.value], &[ssl.value]);
        assert_eq!(
            api.dictionary_trust(ssl_settings.value, true),
            Trust::Trusted
        );
        let security =
            Image::open(b"/System/Library/Frameworks/Security.framework/Security\0").unwrap();
        let basic_create: unsafe extern "C" fn() -> Cf =
            security.symbol(b"SecPolicyCreateBasicX509\0").unwrap();
        let basic = api.owned(basic_create()).unwrap();
        let basic_settings = dictionary(&[api.policy_key.value], &[basic.value]);
        assert_eq!(
            api.dictionary_trust(basic_settings.value, true),
            Trust::Unspecified
        );
        assert_eq!(
            api.dictionary_trust(api.boolean_true, true),
            Trust::Unspecified
        );
    }
}

#[test]
fn null_owned_reference_never_releases_and_nonnull_releases_once() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static RELEASES: AtomicUsize = AtomicUsize::new(0);
    unsafe extern "C" fn count_release(_value: Cf) {
        RELEASES.fetch_add(1, Ordering::Relaxed);
    }
    RELEASES.store(0, Ordering::Relaxed);
    // then_some eagerly constructs its argument: using it here would drop
    // a rejected Owned and call CFRelease(NULL) before returning None.
    assert!(unsafe { Owned::new(ptr::null(), count_release) }.is_none());
    assert_eq!(RELEASES.load(Ordering::Relaxed), 0);
    let owned = unsafe { Owned::new(std::ptr::dangling::<c_void>(), count_release) }.unwrap();
    assert_eq!(RELEASES.load(Ordering::Relaxed), 0);
    drop(owned);
    assert_eq!(RELEASES.load(Ordering::Relaxed), 1);
}

#[test]
fn missing_framework_image_returns_none_without_a_null_handle() {
    let missing = unsafe {
        Image::open(
            b"/System/Library/Frameworks/PerryMissingInventory.framework/PerryMissingInventory\0",
        )
    };
    assert!(missing.is_none());
}
