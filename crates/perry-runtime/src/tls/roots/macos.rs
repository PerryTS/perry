//! macOS system CA inventory, following Node 26's default-keychain query and
//! user/admin trust-settings selection. This is not the System trust-domain
//! anchor list. Frameworks are loaded only on demand, not linked into every
//! executable. Unlike Node's fallback trust evaluation, ours deliberately
//! disables network fetching: reading local CA inventory must remain local.
//! Reference: https://github.com/nodejs/node/blob/v26.12.1/src/crypto/crypto_context.cc#L285-L572

use super::macos_policy::{
    append_unique, dictionary_trust, expired, selected_by_trust, DomainSettings, Trust,
};
use perry_base64::Engine;
use std::collections::HashSet;
use std::ffi::{c_char, c_void};
use std::ptr;
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};
use x509_cert::der::Decode;

type Cf = *const c_void;
type Release = unsafe extern "C" fn(Cf);

fn pem(der: &[u8]) -> String {
    let encoded = perry_base64::engine::general_purpose::STANDARD.encode(der);
    let mut result = String::from("-----BEGIN CERTIFICATE-----\n");
    for line in encoded.as_bytes().chunks(64) {
        // Base64 output is ASCII by construction.
        result.push_str(std::str::from_utf8(line).expect("base64 is ASCII"));
        result.push('\n');
    }
    result.push_str("-----END CERTIFICATE-----\n");
    result
}

/// The only interface exposed to roots.rs. An unavailable framework/query
/// yields no system certificates, never bundled or synthetic substitutes.
pub(super) fn load() -> Vec<String> {
    read_inventory()
        .unwrap_or_default()
        .iter()
        .map(|der| pem(der))
        .collect()
}

fn read_inventory() -> Option<Vec<Vec<u8>>> {
    let api = api()?;
    // SAFETY: the API table contains SDK-typed function pointers from the two
    // absolute system framework paths, retained for the life of the process.
    unsafe { api.inventory() }
}

/// A Copy/Create result owns exactly one +1 CF reference. Borrowed Get
/// results are never wrapped. The function table outlives all owned values.
struct Owned {
    value: Cf,
    release: Release,
}

impl Owned {
    unsafe fn new(value: Cf, release: Release) -> Option<Self> {
        // Do not use then_some: its eagerly constructed rejected value would
        // run Drop and call CFRelease(NULL).
        if value.is_null() {
            None
        } else {
            Some(Self { value, release })
        }
    }
}

impl Drop for Owned {
    fn drop(&mut self) {
        unsafe { (self.release)(self.value) };
    }
}

struct Image(*mut c_void);
impl Image {
    unsafe fn open(path: &[u8]) -> Option<Self> {
        let raw = libc::dlopen(path.as_ptr().cast(), libc::RTLD_LAZY | libc::RTLD_LOCAL);
        if raw.is_null() {
            None
        } else {
            Some(Self(raw))
        }
    }

    unsafe fn symbol<T: Copy>(&self, name: &[u8]) -> Option<T> {
        let address = libc::dlsym(self.0, name.as_ptr().cast());
        if address.is_null() || std::mem::size_of::<T>() != std::mem::size_of::<*mut c_void>() {
            return None;
        }
        Some(std::mem::transmute_copy(&address))
    }

    unsafe fn constant(&self, name: &[u8]) -> Option<Cf> {
        let slot: *const Cf = self.symbol(name)?;
        let value = *slot;
        (!value.is_null()).then_some(value)
    }
}
impl Drop for Image {
    fn drop(&mut self) {
        unsafe { libc::dlclose(self.0) };
    }
}

struct Api {
    release: Release,
    type_id: unsafe extern "C" fn(Cf) -> usize,
    equal: unsafe extern "C" fn(Cf, Cf) -> u8,
    array_type: unsafe extern "C" fn() -> usize,
    dictionary_type: unsafe extern "C" fn() -> usize,
    number_type: unsafe extern "C" fn() -> usize,
    data_type: unsafe extern "C" fn() -> usize,
    string_type: unsafe extern "C" fn() -> usize,
    array_count: unsafe extern "C" fn(Cf) -> isize,
    array_value: unsafe extern "C" fn(Cf, isize) -> Cf,
    dictionary_get: unsafe extern "C" fn(Cf, Cf) -> Cf,
    dictionary_contains: unsafe extern "C" fn(Cf, Cf) -> u8,
    dictionary_create: unsafe extern "C" fn(Cf, *const Cf, *const Cf, isize, Cf, Cf) -> Cf,
    number_get: unsafe extern "C" fn(Cf, isize, *mut c_void) -> u8,
    data_length: unsafe extern "C" fn(Cf) -> isize,
    data_bytes: unsafe extern "C" fn(Cf) -> *const u8,
    certificate_type: unsafe extern "C" fn() -> usize,
    certificate_data: unsafe extern "C" fn(Cf) -> Cf,
    certificate_subject: unsafe extern "C" fn(Cf) -> Cf,
    certificate_issuer: unsafe extern "C" fn(Cf) -> Cf,
    item_copy: unsafe extern "C" fn(Cf, *mut Cf) -> i32,
    settings_copy: unsafe extern "C" fn(Cf, u32, *mut Cf) -> i32,
    policy_type: unsafe extern "C" fn() -> usize,
    policy_properties: unsafe extern "C" fn(Cf) -> Cf,
    policy_ssl: unsafe extern "C" fn(u8, Cf) -> Cf,
    trust_create: unsafe extern "C" fn(Cf, Cf, *mut Cf) -> i32,
    trust_network: unsafe extern "C" fn(Cf, u8) -> i32,
    trust_evaluate: unsafe extern "C" fn(Cf, *mut Cf) -> bool,
    class_key: Cf,
    certificate_class: Cf,
    limit_key: Cf,
    limit_all: Cf,
    return_ref: Cf,
    boolean_true: Cf,
    dictionary_key_callbacks: Cf,
    dictionary_value_callbacks: Cf,
    policy_oid: Cf,
    apple_ssl: Cf,
    application_key: Owned,
    policy_string_key: Owned,
    policy_key: Owned,
    result_key: Owned,
}

// SAFETY: pointers refer to immutable framework constants or owned immutable
// CFStrings. Both framework images stay mapped forever after successful load;
// no keychain/trust object is shared or mutated through this table.
unsafe impl Send for Api {}
unsafe impl Sync for Api {}

fn api() -> Option<&'static Api> {
    static API: OnceLock<Option<Api>> = OnceLock::new();
    API.get_or_init(|| unsafe { Api::load() }).as_ref()
}

impl Api {
    unsafe fn load() -> Option<Self> {
        let cf =
            Image::open(b"/System/Library/Frameworks/CoreFoundation.framework/CoreFoundation\0")?;
        let security = Image::open(b"/System/Library/Frameworks/Security.framework/Security\0")?;
        let release: Release = cf.symbol(b"CFRelease\0")?;
        let string_create: unsafe extern "C" fn(Cf, *const c_char, u32) -> Cf =
            cf.symbol(b"CFStringCreateWithCString\0")?;
        let key = |name: &[u8]| {
            Owned::new(
                string_create(ptr::null(), name.as_ptr().cast(), 0x08000100),
                release,
            )
        };
        let result = Self {
            release,
            type_id: cf.symbol(b"CFGetTypeID\0")?,
            equal: cf.symbol(b"CFEqual\0")?,
            array_type: cf.symbol(b"CFArrayGetTypeID\0")?,
            dictionary_type: cf.symbol(b"CFDictionaryGetTypeID\0")?,
            number_type: cf.symbol(b"CFNumberGetTypeID\0")?,
            data_type: cf.symbol(b"CFDataGetTypeID\0")?,
            string_type: cf.symbol(b"CFStringGetTypeID\0")?,
            array_count: cf.symbol(b"CFArrayGetCount\0")?,
            array_value: cf.symbol(b"CFArrayGetValueAtIndex\0")?,
            dictionary_get: cf.symbol(b"CFDictionaryGetValue\0")?,
            dictionary_contains: cf.symbol(b"CFDictionaryContainsKey\0")?,
            dictionary_create: cf.symbol(b"CFDictionaryCreate\0")?,
            number_get: cf.symbol(b"CFNumberGetValue\0")?,
            data_length: cf.symbol(b"CFDataGetLength\0")?,
            data_bytes: cf.symbol(b"CFDataGetBytePtr\0")?,
            certificate_type: security.symbol(b"SecCertificateGetTypeID\0")?,
            certificate_data: security.symbol(b"SecCertificateCopyData\0")?,
            certificate_subject: security
                .symbol(b"SecCertificateCopyNormalizedSubjectSequence\0")?,
            certificate_issuer: security.symbol(b"SecCertificateCopyNormalizedIssuerSequence\0")?,
            item_copy: security.symbol(b"SecItemCopyMatching\0")?,
            settings_copy: security.symbol(b"SecTrustSettingsCopyTrustSettings\0")?,
            policy_type: security.symbol(b"SecPolicyGetTypeID\0")?,
            policy_properties: security.symbol(b"SecPolicyCopyProperties\0")?,
            policy_ssl: security.symbol(b"SecPolicyCreateSSL\0")?,
            trust_create: security.symbol(b"SecTrustCreateWithCertificates\0")?,
            trust_network: security.symbol(b"SecTrustSetNetworkFetchAllowed\0")?,
            trust_evaluate: security.symbol(b"SecTrustEvaluateWithError\0")?,
            class_key: security.constant(b"kSecClass\0")?,
            certificate_class: security.constant(b"kSecClassCertificate\0")?,
            limit_key: security.constant(b"kSecMatchLimit\0")?,
            limit_all: security.constant(b"kSecMatchLimitAll\0")?,
            return_ref: security.constant(b"kSecReturnRef\0")?,
            boolean_true: cf.constant(b"kCFBooleanTrue\0")?,
            dictionary_key_callbacks: cf.symbol(b"kCFTypeDictionaryKeyCallBacks\0")?,
            dictionary_value_callbacks: cf.symbol(b"kCFTypeDictionaryValueCallBacks\0")?,
            policy_oid: security.constant(b"kSecPolicyOid\0")?,
            apple_ssl: security.constant(b"kSecPolicyAppleSSL\0")?,
            application_key: key(b"kSecTrustSettingsApplication\0")?,
            policy_string_key: key(b"kSecTrustSettingsPolicyString\0")?,
            policy_key: key(b"kSecTrustSettingsPolicy\0")?,
            result_key: key(b"kSecTrustSettingsResult\0")?,
        };
        // No dlclose on success: cached function/data pointers must remain valid.
        std::mem::forget(cf);
        std::mem::forget(security);
        Some(result)
    }

    unsafe fn is_type(&self, value: Cf, expected: usize) -> bool {
        !value.is_null() && (self.type_id)(value) == expected
    }

    unsafe fn owned(&self, value: Cf) -> Option<Owned> {
        Owned::new(value, self.release)
    }

    unsafe fn data(&self, value: Cf) -> Option<Vec<u8>> {
        if !self.is_type(value, (self.data_type)()) {
            return None;
        }
        let length = (self.data_length)(value);
        if length < 0 {
            return None;
        }
        if length == 0 {
            return Some(Vec::new());
        }
        let bytes = (self.data_bytes)(value);
        if bytes.is_null() {
            return None;
        }
        Some(std::slice::from_raw_parts(bytes, length as usize).to_vec())
    }

    unsafe fn dictionary_trust(&self, dictionary: Cf, self_issued: bool) -> Trust {
        if !self.is_type(dictionary, (self.dictionary_type)()) {
            return Trust::Unspecified;
        }
        let has = |key: Cf| (self.dictionary_contains)(dictionary, key) != 0;
        let scoped = has(self.application_key.value) || has(self.policy_string_key.value);
        let ssl = if has(self.policy_key.value) {
            let policy = (self.dictionary_get)(dictionary, self.policy_key.value);
            if !self.is_type(policy, (self.policy_type)()) {
                return Trust::Unspecified;
            }
            let Some(properties) = self.owned((self.policy_properties)(policy)) else {
                return Trust::Unspecified;
            };
            if !self.is_type(properties.value, (self.dictionary_type)()) {
                return Trust::Unspecified;
            }
            let oid = (self.dictionary_get)(properties.value, self.policy_oid);
            Some(self.is_type(oid, (self.string_type)()) && (self.equal)(oid, self.apple_ssl) != 0)
        } else {
            None
        };
        let result = if has(self.result_key.value) {
            let number = (self.dictionary_get)(dictionary, self.result_key.value);
            let mut value: i32 = 0;
            Some(
                if self.is_type(number, (self.number_type)())
                    && (self.number_get)(number, 9, (&mut value as *mut i32).cast()) != 0
                {
                    Ok(value)
                } else {
                    Err(())
                },
            )
        } else {
            None
        };
        dictionary_trust(self_issued, scoped, ssl, result)
    }

    unsafe fn domain(&self, certificate: Cf, domain: u32, self_issued: bool) -> DomainSettings {
        let mut raw = ptr::null();
        let status = (self.settings_copy)(certificate, domain, &mut raw);
        let settings = self.owned(raw);
        if status != 0 && status != -25300 {
            return DomainSettings::Error;
        }
        let Some(settings) = settings else {
            return DomainSettings::Null;
        };
        // A non-NULL array only contributes settings on success. It must not
        // accidentally trigger the NULL-array trust-evaluation fallback.
        if status != 0 || !self.is_type(settings.value, (self.array_type)()) {
            return DomainSettings::Entries(vec![Trust::Unspecified]);
        }
        let count = (self.array_count)(settings.value);
        if count < 0 {
            return DomainSettings::Error;
        }
        let mut entries = Vec::new();
        for index in 0..count {
            let decision =
                self.dictionary_trust((self.array_value)(settings.value, index), self_issued);
            entries.push(decision);
            if decision != Trust::Unspecified {
                break;
            }
        }
        DomainSettings::Entries(entries)
    }

    unsafe fn evaluate(&self, certificate: Cf) -> bool {
        let Some(policy) = self.owned((self.policy_ssl)(0, ptr::null())) else {
            return false;
        };
        let mut raw = ptr::null();
        let status = (self.trust_create)(certificate, policy.value, &mut raw);
        let Some(trust) = self.owned(raw) else {
            return false;
        };
        if status != 0 {
            return false;
        }
        // This alters only our newly-created evaluation object, never the
        // keychain or persisted trust settings. Do not evaluate on failure.
        if (self.trust_network)(trust.value, 0) != 0 {
            return false;
        }
        (self.trust_evaluate)(trust.value, ptr::null_mut())
    }

    unsafe fn inventory(&self) -> Option<Vec<Vec<u8>>> {
        self.inventory_with_clock(|| SystemTime::now().duration_since(UNIX_EPOCH).ok())
    }

    unsafe fn inventory_with_clock(
        &self,
        now: impl FnOnce() -> Option<std::time::Duration>,
    ) -> Option<Vec<Vec<u8>>> {
        let keys = [self.class_key, self.limit_key, self.return_ref];
        let values = [self.certificate_class, self.limit_all, self.boolean_true];
        let query = self.owned((self.dictionary_create)(
            ptr::null(),
            keys.as_ptr(),
            values.as_ptr(),
            3,
            self.dictionary_key_callbacks,
            self.dictionary_value_callbacks,
        ))?;
        let mut raw = ptr::null();
        let status = (self.item_copy)(query.value, &mut raw);
        let certificates = self.owned(raw);
        if status == -25300 {
            return Some(Vec::new());
        }
        if status != 0 {
            return None;
        }
        let certificates = certificates?;
        if !self.is_type(certificates.value, (self.array_type)()) {
            return None;
        }
        let count = (self.array_count)(certificates.value);
        if count < 0 {
            return None;
        }
        // Sample only after the query/count checks, as in the production path.
        let now = now()?;
        let mut output = Vec::new();
        let mut seen = HashSet::new();
        for index in 0..count {
            let certificate = (self.array_value)(certificates.value, index);
            if !self.is_type(certificate, (self.certificate_type)()) {
                continue;
            }
            let Some(data) = self.owned((self.certificate_data)(certificate)) else {
                continue;
            };
            let Some(der) = self.data(data.value) else {
                continue;
            };
            let Ok(parsed) = x509_cert::Certificate::from_der(&der) else {
                continue;
            };
            let Some(subject) = self.owned((self.certificate_subject)(certificate)) else {
                continue;
            };
            let Some(issuer) = self.owned((self.certificate_issuer)(certificate)) else {
                continue;
            };
            if !self.is_type(subject.value, (self.data_type)())
                || !self.is_type(issuer.value, (self.data_type)())
            {
                continue;
            }
            // Self-issued is name equality, not a claim about the signature.
            let self_issued = (self.equal)(subject.value, issuer.value) != 0;
            if selected_by_trust(
                self_issued,
                |domain| self.domain(certificate, domain, self_issued),
                || self.evaluate(certificate),
            ) && !expired(parsed.tbs_certificate().validity(), now)
            {
                append_unique(&mut output, &mut seen, der);
            }
        }
        Some(output)
    }
}

#[cfg(test)]
#[path = "macos_tests.rs"]
mod tests;
