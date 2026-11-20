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
    per_test_global! {
        static RELEASES: AtomicUsize = AtomicUsize::new(0);
    }
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

// Public FNMT CA certificate already present in roots::FALLBACK_ROOT_CERTS.
// No private key or machine-specific keychain data is used by these fixtures.
const SYSTEM_CA_DER: &[u8] = include_bytes!("test_data/system-ca.der");

struct InventoryFixtureState {
    items: Cf,
    settings: Cf,
    certificate: Cf,
    retain: unsafe extern "C" fn(Cf) -> Cf,
    queries: usize,
    domains: Vec<u32>,
    certificate_seen: bool,
}

thread_local! {
    static INVENTORY_FIXTURE: std::cell::RefCell<Option<InventoryFixtureState>> = const {
        std::cell::RefCell::new(None)
    };
}

// These callbacks implement Copy's +1 reference contract. They never panic
// across the C ABI: a missing/reentrantly borrowed TLS fixture returns an
// OSStatus error instead. The guard below keeps every borrowed CF value alive.
unsafe extern "C" fn fixture_item_copy(_query: Cf, result: *mut Cf) -> i32 {
    if result.is_null() {
        return -50;
    }
    INVENTORY_FIXTURE
        .try_with(|slot| {
            let Ok(mut slot) = slot.try_borrow_mut() else {
                return -50;
            };
            let Some(state) = slot.as_mut() else {
                return -50;
            };
            state.queries += 1;
            *result = (state.retain)(state.items);
            0
        })
        .unwrap_or(-50)
}

unsafe extern "C" fn fixture_settings_copy(certificate: Cf, domain: u32, result: *mut Cf) -> i32 {
    if result.is_null() {
        return -50;
    }
    INVENTORY_FIXTURE
        .try_with(|slot| {
            let Ok(mut slot) = slot.try_borrow_mut() else {
                return -50;
            };
            let Some(state) = slot.as_mut() else {
                return -50;
            };
            state.domains.push(domain);
            state.certificate_seen |= certificate == state.certificate;
            *result = (state.retain)(state.settings);
            0
        })
        .unwrap_or(-50)
}

struct InventoryFixtureGuard {
    _items: Owned,
    _settings: Owned,
    _certificate: Option<Owned>,
}

impl Drop for InventoryFixtureGuard {
    fn drop(&mut self) {
        // Disable callbacks before releasing the CF objects they borrow.
        INVENTORY_FIXTURE.with(|slot| *slot.borrow_mut() = None);
    }
}

fn deterministic_inventory(eligible: bool) -> (Option<Vec<Vec<u8>>>, usize, Vec<u32>, bool) {
    unsafe {
        // Only this test-local table is modified; the process-wide API cache
        // and the real keychain/trust settings remain untouched.
        let mut api = Api::load().expect("framework symbols");
        let cf =
            Image::open(b"/System/Library/Frameworks/CoreFoundation.framework/CoreFoundation\0")
                .unwrap();
        let security =
            Image::open(b"/System/Library/Frameworks/Security.framework/Security\0").unwrap();
        let data_create: unsafe extern "C" fn(Cf, *const u8, isize) -> Cf =
            cf.symbol(b"CFDataCreate\0").unwrap();
        let certificate_create: unsafe extern "C" fn(Cf, Cf) -> Cf =
            security.symbol(b"SecCertificateCreateWithData\0").unwrap();
        let array_create: unsafe extern "C" fn(Cf, *const Cf, isize, Cf) -> Cf =
            cf.symbol(b"CFArrayCreate\0").unwrap();
        let callbacks: Cf = cf.symbol(b"kCFTypeArrayCallBacks\0").unwrap();
        let retain: unsafe extern "C" fn(Cf) -> Cf = cf.symbol(b"CFRetain\0").unwrap();
        let certificate = if eligible {
            let data = api
                .owned(data_create(
                    ptr::null(),
                    SYSTEM_CA_DER.as_ptr(),
                    SYSTEM_CA_DER.len() as isize,
                ))
                .unwrap();
            Some(
                api.owned(certificate_create(ptr::null(), data.value))
                    .unwrap(),
            )
        } else {
            None
        };
        let values: Vec<Cf> = certificate.iter().map(|value| value.value).collect();
        let items = api
            .owned(array_create(
                ptr::null(),
                values.as_ptr(),
                values.len() as isize,
                callbacks,
            ))
            .unwrap();
        // An actual empty CF settings array trusts a self-issued certificate;
        // inventory still has to read the real normalized issuer/subject.
        let settings = api
            .owned(array_create(ptr::null(), ptr::null(), 0, callbacks))
            .unwrap();
        INVENTORY_FIXTURE.with(|slot| {
            assert!(slot.borrow().is_none(), "fixture guards must not overlap");
            *slot.borrow_mut() = Some(InventoryFixtureState {
                items: items.value,
                settings: settings.value,
                certificate: certificate
                    .as_ref()
                    .map_or(ptr::null(), |value| value.value),
                retain,
                queries: 0,
                domains: Vec::new(),
                certificate_seen: false,
            });
        });
        let _guard = InventoryFixtureGuard {
            _items: items,
            _settings: settings,
            _certificate: certificate,
        };
        api.item_copy = fixture_item_copy;
        api.settings_copy = fixture_settings_copy;
        // Fixed 2027 instant within this public certificate's validity, so
        // the fixture remains deterministic after its real expiry in 2043.
        let output =
            api.inventory_with_clock(|| Some(std::time::Duration::from_secs(1_800_000_000)));
        INVENTORY_FIXTURE.with(|slot| {
            let slot = slot.borrow();
            let state = slot.as_ref().unwrap();
            (
                output,
                state.queries,
                state.domains.clone(),
                state.certificate_seen,
            )
        })
    }
}

#[test]
fn deterministic_empty_inventory_queries_source_without_trust_lookup() {
    let (output, queries, domains, certificate_seen) = deterministic_inventory(false);
    assert_eq!(queries, 1, "the injected query must actually run");
    assert!(
        domains.is_empty(),
        "empty inventory has no certificate to evaluate"
    );
    assert!(!certificate_seen);
    assert_eq!(output, Some(Vec::new()));
}

#[test]
fn deterministic_eligible_inventory_returns_the_exact_selected_der() {
    let (output, queries, domains, certificate_seen) = deterministic_inventory(true);
    assert_eq!(queries, 1, "the injected query must actually run");
    assert!(
        certificate_seen,
        "trust settings must receive the queried certificate"
    );
    assert_eq!(
        domains,
        vec![0],
        "explicit user trust must select before consulting admin"
    );
    let output = output.expect("a successful native query returns an inventory");
    assert_eq!(
        output.len(),
        1,
        "an eligible certificate must not be silently filtered out"
    );
    assert_eq!(
        output[0], SYSTEM_CA_DER,
        "inventory must publish the queried certificate's original DER"
    );
}
