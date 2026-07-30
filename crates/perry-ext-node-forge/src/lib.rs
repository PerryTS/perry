//! Native bindings for the npm `node-forge` package — the PKI subset.
//!
//! Scope is exactly what Socket Firewall's TLS-MITM CA uses:
//!   - `forge.pki.rsa.generateKeyPair({ bits })`
//!   - `forge.pki.createCertificate()` + the builder methods
//!     (`setSubject` / `setIssuer` / `setExtensions` / `sign`) and the
//!     settable fields (`publicKey`, `serialNumber`, `validity.*`)
//!   - `forge.pki.certificateFromPem` / `certificateToPem`
//!   - `forge.pki.privateKeyFromPem` / `privateKeyToPem` / `publicKeyToPem`
//!   - `forge.md.sha256.create()`
//!
//! Everything else in forge's surface has no dispatch row here, so a
//! call to it surfaces as an unresolved `node-forge` method naming the
//! API rather than silently succeeding.
//!
//! ## Object model
//!
//! The certificate builder is a real perry JS object (allocated with a
//! fixed 7-field shape) so the plain field assignments sfw performs
//! (`cert.publicKey = …`, `cert.serialNumber = '01'`,
//! `cert.validity.notBefore = new Date()`) are ordinary JS property
//! sets — no native involvement. Only the *methods* dispatch here:
//! `setSubject`/`setIssuer`/`setExtensions` write their argument into a
//! fixed slot, and `sign` serializes the whole object with
//! `JSON.stringify` (via `perry_ffi::json_stringify`), builds + signs
//! the X.509 cert with the RustCrypto core in [`crypto`], and stashes
//! the resulting PEM back into the object's `signaturePem` slot for
//! `certificateToPem` to read.

pub mod crypto;

use perry_ffi::{
    alloc_string, build_object_shape, js_object_alloc_with_shape, js_object_set_field,
    json_stringify, read_string, JsString, JsValue, ObjectHeader, StringHeader,
};
use serde::Deserialize;

use crypto::{
    Attr, BasicConstraintsSpec, CertSpec, DnValueTag, ExtKeyUsageSpec, ExtSet, KeyUsageSpec,
};

// Fixed field layout of the certificate builder object. `create_certificate`
// allocates this shape; the setter FFIs write by index; `sign` /
// `certificateToPem` read `signaturePem` back via JSON.
const CERT_KEYS: &[&str] = &[
    "publicKey",    // 0
    "serialNumber", // 1
    "validity",     // 2 (sub-object { notBefore, notAfter })
    "subject",      // 3 (array set by setSubject)
    "issuer",       // 4 (array set by setIssuer)
    "extensions",   // 5 (array set by setExtensions)
    "signaturePem", // 6 (filled by sign)
];
const FIELD_SUBJECT: u32 = 3;
const FIELD_ISSUER: u32 = 4;
const FIELD_EXTENSIONS: u32 = 5;
const FIELD_SIGNATURE_PEM: u32 = 6;

// ── small FFI helpers ───────────────────────────────────────────────

unsafe fn read_str(ptr: *const StringHeader) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    let handle = JsString::from_raw(ptr as *mut StringHeader);
    read_string(handle).map(String::from)
}

fn str_out(s: &str) -> *mut StringHeader {
    alloc_string(s).as_raw()
}

/// Build a `{ pem: <s> }` JS object — the shape used for key handles.
fn key_object(pem: &str) -> JsValue {
    let (packed, shape_id) = build_object_shape(&["pem"]);
    unsafe {
        let obj = js_object_alloc_with_shape(shape_id, 1, packed.as_ptr(), packed.len() as u32);
        let pem_str = alloc_string(pem);
        js_object_set_field(obj, 0, JsValue::from_string_ptr(pem_str.as_raw()));
        JsValue::from_object_ptr(obj)
    }
}

/// JSON.stringify a NaN-boxed value passed across the FFI as `f64` bits.
fn stringify_arg(bits: f64) -> Option<String> {
    json_stringify(JsValue::from_bits(bits.to_bits()))
}

// ── JSON shapes coming from json_stringify(cert) ────────────────────

#[derive(Deserialize, Default)]
struct KeyObj {
    pem: Option<String>,
}

#[derive(Deserialize)]
struct AttrJson {
    name: Option<String>,
    #[serde(rename = "shortName")]
    short_name: Option<String>,
    #[serde(rename = "type")]
    type_oid: Option<String>,
    value: Option<serde_json::Value>,
    #[serde(rename = "valueTag")]
    value_tag: Option<String>,
}

#[derive(Deserialize)]
struct AltNameJson {
    #[serde(rename = "type")]
    typ: Option<i64>,
    value: Option<String>,
}

#[derive(Deserialize)]
struct ExtJson {
    name: Option<String>,
    #[serde(rename = "cA")]
    c_a: Option<bool>,
    critical: Option<bool>,
    #[serde(rename = "keyCertSign")]
    key_cert_sign: Option<bool>,
    #[serde(rename = "cRLSign", alias = "crlSign")]
    crl_sign: Option<bool>,
    #[serde(rename = "digitalSignature")]
    digital_signature: Option<bool>,
    #[serde(rename = "keyEncipherment")]
    key_encipherment: Option<bool>,
    #[serde(rename = "serverAuth")]
    server_auth: Option<bool>,
    #[serde(rename = "clientAuth")]
    client_auth: Option<bool>,
    #[serde(rename = "altNames")]
    alt_names: Option<Vec<AltNameJson>>,
}

#[derive(Deserialize, Default)]
struct ValidityJson {
    #[serde(rename = "notBefore")]
    not_before: Option<serde_json::Value>,
    #[serde(rename = "notAfter")]
    not_after: Option<serde_json::Value>,
}

/// A distinguished name as it appears on a builder cert. node-forge models
/// `cert.subject` / `cert.issuer` as `{ attributes: [{name,value}, …] }`, so
/// that is the canonical shape (produced by `setSubject`/`setIssuer` and
/// `certificateFromPem` alike). A bare `[{name,value}]` array is also accepted
/// so a hand-built cert object still round-trips.
#[derive(Deserialize)]
#[serde(untagged)]
enum DnJson {
    Wrapped { attributes: Vec<AttrJson> },
    Bare(Vec<AttrJson>),
}

impl DnJson {
    fn attributes(&self) -> &[AttrJson] {
        match self {
            DnJson::Wrapped { attributes } => attributes,
            DnJson::Bare(v) => v,
        }
    }
}

#[derive(Deserialize)]
struct CertJson {
    #[serde(rename = "publicKey")]
    public_key: Option<KeyObj>,
    #[serde(rename = "serialNumber")]
    serial_number: Option<serde_json::Value>,
    validity: Option<ValidityJson>,
    subject: Option<DnJson>,
    issuer: Option<DnJson>,
    extensions: Option<Vec<ExtJson>>,
}

fn attr_key(a: &AttrJson) -> Option<String> {
    a.name
        .clone()
        .or_else(|| a.short_name.clone())
        .or_else(|| a.type_oid.clone())
}

fn value_to_string(v: &Option<serde_json::Value>) -> String {
    match v {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(serde_json::Value::Number(n)) => n.to_string(),
        _ => String::new(),
    }
}

fn attrs_from(json: &[AttrJson]) -> Vec<Attr> {
    json.iter()
        .filter_map(|a| {
            let key = attr_key(a)?;
            Some(Attr {
                key,
                value: value_to_string(&a.value),
                value_tag: a.value_tag.as_deref().and_then(DnValueTag::from_str),
            })
        })
        .collect()
}

/// Parse a validity endpoint. `JSON.stringify(Date)` yields an ISO-8601
/// string; we also accept an epoch-milliseconds number as a fallback.
fn parse_time(v: &Option<serde_json::Value>, field: &str) -> Result<i64, String> {
    match v {
        Some(serde_json::Value::String(s)) => {
            time::OffsetDateTime::parse(s, &time::format_description::well_known::Rfc3339)
                .map(|dt| dt.unix_timestamp())
                .map_err(|e| format!("node-forge: invalid cert.validity.{field}: {e}"))
        }
        Some(serde_json::Value::Number(n)) => n
            .as_f64()
            .filter(|value| value.is_finite())
            .map(|value| (value / 1000.0) as i64)
            .ok_or_else(|| format!("node-forge: invalid cert.validity.{field}")),
        _ => Err(format!("node-forge: cert.validity.{field} is not set")),
    }
}

fn ext_set_from(exts: &[ExtJson]) -> ExtSet {
    let mut set = ExtSet::default();
    for e in exts {
        match e.name.as_deref() {
            Some("basicConstraints") => {
                set.basic_constraints = Some(BasicConstraintsSpec {
                    ca: e.c_a.unwrap_or(false),
                    critical: e.critical.unwrap_or(false),
                });
            }
            Some("keyUsage") => {
                set.key_usage = Some(KeyUsageSpec {
                    digital_signature: e.digital_signature.unwrap_or(false),
                    key_encipherment: e.key_encipherment.unwrap_or(false),
                    key_cert_sign: e.key_cert_sign.unwrap_or(false),
                    crl_sign: e.crl_sign.unwrap_or(false),
                    critical: e.critical.unwrap_or(false),
                });
            }
            Some("extKeyUsage") => {
                set.ext_key_usage = Some(ExtKeyUsageSpec {
                    server_auth: e.server_auth.unwrap_or(false),
                    client_auth: e.client_auth.unwrap_or(false),
                });
            }
            Some("subjectAltName") => {
                if let Some(alts) = &e.alt_names {
                    for a in alts {
                        // type 2 == dNSName (the only form sfw emits).
                        if a.typ.unwrap_or(2) == 2 {
                            if let Some(v) = &a.value {
                                set.subject_alt_names.push(v.clone());
                            }
                        }
                    }
                }
            }
            Some("subjectKeyIdentifier") => set.subject_key_identifier = true,
            _ => {}
        }
    }
    set
}

/// Assemble a `CertSpec` from the JSON serialization of a builder cert
/// object. Pure (no FFI) so it can be unit-tested directly.
fn cert_spec_from_json(cert_json: &str) -> Result<CertSpec, String> {
    let c: CertJson = serde_json::from_str(cert_json).map_err(|e| e.to_string())?;
    let public_key_pem = c
        .public_key
        .and_then(|k| k.pem)
        .ok_or("node-forge: cert.publicKey is not set")?;
    let validity = c.validity.unwrap_or_default();
    let not_before_unix = parse_time(&validity.not_before, "notBefore")?;
    let not_after_unix = parse_time(&validity.not_after, "notAfter")?;
    if not_after_unix <= not_before_unix {
        return Err(
            "node-forge: cert.validity.notAfter must be later than cert.validity.notBefore"
                .to_string(),
        );
    }
    Ok(CertSpec {
        public_key_pem,
        serial_hex: value_to_string(&c.serial_number),
        not_before_unix,
        not_after_unix,
        subject: c
            .subject
            .as_ref()
            .map(|d| attrs_from(d.attributes()))
            .unwrap_or_default(),
        issuer: c
            .issuer
            .as_ref()
            .map(|d| attrs_from(d.attributes()))
            .unwrap_or_default(),
        extensions: c
            .extensions
            .as_deref()
            .map(ext_set_from)
            .unwrap_or_default(),
    })
}

// ────────────────────────────────────────────────────────────────────
// FFI entry points
// ────────────────────────────────────────────────────────────────────

/// `forge.pki.rsa.generateKeyPair({ bits })` →
/// `{ publicKey: { pem }, privateKey: { pem } }`.
///
/// `bits` is the numeric key size (sfw passes `2048`). The `workers`
/// option is ignored — keygen is synchronous here.
#[no_mangle]
pub extern "C" fn js_node_forge_generate_key_pair(bits: f64) -> JsValue {
    let bits = if bits.is_finite() && bits > 0.0 {
        bits as usize
    } else {
        2048
    };
    match crypto::generate_key_pair(bits) {
        Ok((priv_pem, pub_pem)) => {
            let (packed, shape_id) = build_object_shape(&["publicKey", "privateKey"]);
            unsafe {
                let obj =
                    js_object_alloc_with_shape(shape_id, 2, packed.as_ptr(), packed.len() as u32);
                js_object_set_field(obj, 0, key_object(&pub_pem));
                js_object_set_field(obj, 1, key_object(&priv_pem));
                JsValue::from_object_ptr(obj)
            }
        }
        Err(_) => JsValue::NULL,
    }
}

/// `forge.pki.privateKeyToPem(key)` — PKCS#1 `RSA PRIVATE KEY` PEM.
#[no_mangle]
pub extern "C" fn js_node_forge_private_key_to_pem(key_bits: f64) -> *mut StringHeader {
    let Some(json) = stringify_arg(key_bits) else {
        return std::ptr::null_mut();
    };
    let Ok(k) = serde_json::from_str::<KeyObj>(&json) else {
        return std::ptr::null_mut();
    };
    let Some(pem) = k.pem else {
        return std::ptr::null_mut();
    };
    match crypto::normalize_private_key_pem(&pem) {
        Ok(out) => str_out(&out),
        // Already PKCS#1 or unparseable — echo the stored PEM.
        Err(_) => str_out(&pem),
    }
}

/// `forge.pki.publicKeyToPem(key)` — SPKI `PUBLIC KEY` PEM.
#[no_mangle]
pub extern "C" fn js_node_forge_public_key_to_pem(key_bits: f64) -> *mut StringHeader {
    let Some(json) = stringify_arg(key_bits) else {
        return std::ptr::null_mut();
    };
    match serde_json::from_str::<KeyObj>(&json)
        .ok()
        .and_then(|k| k.pem)
    {
        Some(pem) => str_out(&pem),
        None => std::ptr::null_mut(),
    }
}

/// `forge.pki.privateKeyFromPem(pem)` → `{ pem }` key handle.
///
/// # Safety
/// `pem_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_node_forge_private_key_from_pem(
    pem_ptr: *const StringHeader,
) -> JsValue {
    let Some(pem) = read_str(pem_ptr) else {
        return JsValue::NULL;
    };
    // Normalize to PKCS#1 so downstream signing / toPem is uniform;
    // fall back to the raw PEM if it doesn't parse as RSA.
    let normalized = crypto::normalize_private_key_pem(&pem).unwrap_or(pem);
    key_object(&normalized)
}

/// `forge.pki.certificateToPem(cert)` — the signed PEM stashed by `sign`.
#[no_mangle]
pub extern "C" fn js_node_forge_certificate_to_pem(cert_bits: f64) -> *mut StringHeader {
    let Some(json) = stringify_arg(cert_bits) else {
        return std::ptr::null_mut();
    };
    #[derive(Deserialize)]
    struct Sig {
        #[serde(rename = "signaturePem")]
        signature_pem: Option<String>,
    }
    match serde_json::from_str::<Sig>(&json)
        .ok()
        .and_then(|s| s.signature_pem)
    {
        Some(pem) => str_out(&pem),
        None => std::ptr::null_mut(),
    }
}

/// `forge.pki.certificateFromPem(pem)` → a cert object exposing
/// `subject.attributes` (the only field sfw reads back off a parsed
/// cert, for `setIssuer(caCert.subject.attributes)`).
///
/// # Safety
/// `pem_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_node_forge_certificate_from_pem(
    pem_ptr: *const StringHeader,
) -> JsValue {
    let Some(pem) = read_str(pem_ptr) else {
        return JsValue::NULL;
    };
    let attrs = match crypto::cert_subject_attrs(&pem) {
        Ok(a) => a,
        Err(_) => return JsValue::NULL,
    };
    // Build `subject.attributes = [{ name, value, valueTag }, …]`. The
    // valueTag metadata preserves the certificate's ASN.1 DN string encoding
    // when callers reuse these attributes as an issuer.
    let attrs_arr = {
        let arr = perry_ffi::js_array_alloc(attrs.len() as u32);
        let mut arr = arr;
        for a in &attrs {
            let (packed, shape_id) = build_object_shape(&["name", "value", "valueTag"]);
            let o = js_object_alloc_with_shape(shape_id, 3, packed.as_ptr(), packed.len() as u32);
            js_object_set_field(
                o,
                0,
                JsValue::from_string_ptr(alloc_string(&a.key).as_raw()),
            );
            js_object_set_field(
                o,
                1,
                JsValue::from_string_ptr(alloc_string(&a.value).as_raw()),
            );
            js_object_set_field(
                o,
                2,
                a.value_tag
                    .map(|tag| JsValue::from_string_ptr(alloc_string(tag.as_str()).as_raw()))
                    .unwrap_or(JsValue::NULL),
            );
            arr = perry_ffi::js_array_push(arr, JsValue::from_object_ptr(o));
        }
        JsValue::from_object_ptr(arr)
    };
    let (spacked, sshape) = build_object_shape(&["attributes"]);
    let subject = js_object_alloc_with_shape(sshape, 1, spacked.as_ptr(), spacked.len() as u32);
    js_object_set_field(subject, 0, attrs_arr);

    let (packed, shape_id) = build_object_shape(&["subject", "signaturePem"]);
    let obj = js_object_alloc_with_shape(shape_id, 2, packed.as_ptr(), packed.len() as u32);
    js_object_set_field(obj, 0, JsValue::from_object_ptr(subject));
    // Store the original PEM so a re-`certificateToPem(caCert)` round-trips.
    js_object_set_field(
        obj,
        1,
        JsValue::from_string_ptr(alloc_string(&pem).as_raw()),
    );
    JsValue::from_object_ptr(obj)
}

/// `forge.pki.createCertificate()` — a builder object with the fixed
/// 7-field shape plus a pre-created `validity` sub-object so
/// `cert.validity.notBefore = …` is an ordinary JS set.
#[no_mangle]
pub extern "C" fn js_node_forge_create_certificate() -> JsValue {
    unsafe {
        let (vpacked, vshape) = build_object_shape(&["notBefore", "notAfter"]);
        let validity =
            js_object_alloc_with_shape(vshape, 2, vpacked.as_ptr(), vpacked.len() as u32);
        js_object_set_field(validity, 0, JsValue::NULL);
        js_object_set_field(validity, 1, JsValue::NULL);

        let (packed, shape_id) = build_object_shape(CERT_KEYS);
        let obj = js_object_alloc_with_shape(
            shape_id,
            CERT_KEYS.len() as u32,
            packed.as_ptr(),
            packed.len() as u32,
        );
        js_object_set_field(obj, 0, JsValue::NULL); // publicKey
        js_object_set_field(obj, 1, JsValue::NULL); // serialNumber
        js_object_set_field(obj, 2, JsValue::from_object_ptr(validity));
        js_object_set_field(obj, 3, JsValue::NULL); // subject
        js_object_set_field(obj, 4, JsValue::NULL); // issuer
        js_object_set_field(obj, 5, JsValue::NULL); // extensions
        js_object_set_field(obj, 6, JsValue::NULL); // signaturePem
        JsValue::from_object_ptr(obj)
    }
}

/// Wrap a distinguished-name attribute array as node-forge's `{ attributes }`
/// object, so a later `cert.subject.attributes` read returns the array (the
/// shape `certificateFromPem` also produces). `setIssuer(caCert.subject.
/// attributes)` — the sfw idiom — passes the already-unwrapped array back in,
/// and it is re-wrapped here, keeping both DN fields uniform for
/// `certificateToPem`.
unsafe fn wrap_dn_attributes(attrs_bits: f64) -> JsValue {
    let attrs = JsValue::from_bits(attrs_bits.to_bits());
    if json_stringify(attrs)
        .and_then(|json| serde_json::from_str::<serde_json::Value>(&json).ok())
        .is_some_and(|value| value.get("attributes").is_some())
    {
        return attrs;
    }
    let (packed, shape_id) = build_object_shape(&["attributes"]);
    let dn = js_object_alloc_with_shape(shape_id, 1, packed.as_ptr(), packed.len() as u32);
    js_object_set_field(dn, 0, attrs);
    JsValue::from_object_ptr(dn)
}

/// `cert.setSubject(attrs)` — store `{ attributes: attrs }` in slot 3.
///
/// # Safety
/// `cert` must be the NaN-unboxed `*mut ObjectHeader` of a builder cert.
#[no_mangle]
pub unsafe extern "C" fn js_node_forge_cert_set_subject(cert: i64, attrs_bits: f64) {
    let obj = cert as *mut ObjectHeader;
    if !obj.is_null() {
        js_object_set_field(obj, FIELD_SUBJECT, wrap_dn_attributes(attrs_bits));
    }
}

/// `cert.setIssuer(attrs)` — store `{ attributes: attrs }` in slot 4.
///
/// # Safety
/// See [`js_node_forge_cert_set_subject`].
#[no_mangle]
pub unsafe extern "C" fn js_node_forge_cert_set_issuer(cert: i64, attrs_bits: f64) {
    let obj = cert as *mut ObjectHeader;
    if !obj.is_null() {
        js_object_set_field(obj, FIELD_ISSUER, wrap_dn_attributes(attrs_bits));
    }
}

/// `cert.setExtensions(exts)` — store the extension array in slot 5.
///
/// # Safety
/// See [`js_node_forge_cert_set_subject`].
#[no_mangle]
pub unsafe extern "C" fn js_node_forge_cert_set_extensions(cert: i64, exts_bits: f64) {
    let obj = cert as *mut ObjectHeader;
    if !obj.is_null() {
        js_object_set_field(
            obj,
            FIELD_EXTENSIONS,
            JsValue::from_bits(exts_bits.to_bits()),
        );
    }
}

/// `cert.sign(privateKey, md)` — serialize the builder object, build +
/// sign the X.509 cert with the issuer's private key, and stash the
/// resulting PEM in slot 6 (`signaturePem`). `md` (a `forge.md.*`
/// digest) is accepted for API compatibility; only SHA-256 is
/// supported, matching sfw.
///
/// # Safety
/// `cert` must be the NaN-unboxed `*mut ObjectHeader` of a builder cert.
#[no_mangle]
pub unsafe extern "C" fn js_node_forge_cert_sign(cert: i64, key_bits: f64, _md_bits: f64) {
    let obj = cert as *mut ObjectHeader;
    if obj.is_null() {
        return;
    }
    let cert_value = JsValue::from_object_ptr(obj);
    let Some(cert_json) = json_stringify(cert_value) else {
        perry_ffi::throw_with_code(
            "node-forge: unable to serialize certificate",
            "ERR_NODE_FORGE_CERT_SIGN",
            perry_ffi::ErrorKind::Error,
        );
    };
    let Some(key_json) = stringify_arg(key_bits) else {
        perry_ffi::throw_with_code(
            "node-forge: unable to serialize signing key",
            "ERR_NODE_FORGE_CERT_SIGN",
            perry_ffi::ErrorKind::Error,
        );
    };
    let key = match serde_json::from_str::<KeyObj>(&key_json) {
        Ok(key) => key,
        Err(err) => perry_ffi::throw_with_code(
            &format!("node-forge: invalid signing key: {err}"),
            "ERR_NODE_FORGE_CERT_SIGN",
            perry_ffi::ErrorKind::Error,
        ),
    };
    let Some(signer_pem) = key.pem else {
        perry_ffi::throw_with_code(
            "node-forge: signing key PEM is not set",
            "ERR_NODE_FORGE_CERT_SIGN",
            perry_ffi::ErrorKind::Error,
        );
    };
    let spec = match cert_spec_from_json(&cert_json) {
        Ok(spec) => spec,
        Err(err) => perry_ffi::throw_with_code(
            &err,
            "ERR_NODE_FORGE_CERT_SIGN",
            perry_ffi::ErrorKind::Error,
        ),
    };
    let pem = match crypto::build_and_sign(&spec, &signer_pem) {
        Ok(pem) => pem,
        Err(err) => perry_ffi::throw_with_code(
            &format!("node-forge: certificate signing failed: {err}"),
            "ERR_NODE_FORGE_CERT_SIGN",
            perry_ffi::ErrorKind::Error,
        ),
    };
    let pem_str = alloc_string(&pem);
    js_object_set_field(
        obj,
        FIELD_SIGNATURE_PEM,
        JsValue::from_string_ptr(pem_str.as_raw()),
    );
}

/// `forge.md.sha256.create()` — a small marker object. `sign` reads the
/// certificate's own fields and always uses SHA-256, so the digest just
/// needs to exist as a value the caller can pass through.
#[no_mangle]
pub extern "C" fn js_node_forge_md_sha256_create() -> JsValue {
    let (packed, shape_id) = build_object_shape(&["algorithm"]);
    unsafe {
        let obj = js_object_alloc_with_shape(shape_id, 1, packed.as_ptr(), packed.len() as u32);
        js_object_set_field(
            obj,
            0,
            JsValue::from_string_ptr(alloc_string("sha256").as_raw()),
        );
        JsValue::from_object_ptr(obj)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cert_spec_parses_sfw_shaped_json() {
        // Shape produced by JSON.stringify of a builder cert after sfw's
        // host.ts populates it.
        let json = r#"{
            "publicKey": { "pem": "-----BEGIN PUBLIC KEY-----\nAAA\n-----END PUBLIC KEY-----\n" },
            "serialNumber": "02",
            "validity": {
                "notBefore": "2026-07-29T00:00:00.000Z",
                "notAfter": "2027-07-29T00:00:00.000Z"
            },
            "subject": [{ "name": "commonName", "value": "example.com" }],
            "issuer": [
                { "name": "commonName", "value": "Socket Security CA" },
                { "name": "organizationName", "value": "Socket Security" }
            ],
            "extensions": [
                { "name": "basicConstraints", "cA": false },
                { "name": "keyUsage", "digitalSignature": true, "keyEncipherment": true },
                { "name": "extKeyUsage", "serverAuth": true },
                { "name": "subjectAltName", "altNames": [
                    { "type": 2, "value": "example.com" },
                    { "type": 2, "value": "www.example.com" }
                ]}
            ],
            "signaturePem": null
        }"#;
        let spec = cert_spec_from_json(json).expect("parse");
        assert_eq!(spec.serial_hex, "02");
        assert_eq!(spec.subject.len(), 1);
        assert_eq!(spec.subject[0].key, "commonName");
        assert_eq!(spec.issuer.len(), 2);
        assert_eq!(spec.issuer[1].key, "organizationName");
        assert!(spec.not_before_unix > 0 && spec.not_after_unix > spec.not_before_unix);
        let ext = &spec.extensions;
        assert!(ext.basic_constraints.as_ref().unwrap().ca == false);
        assert!(ext.key_usage.as_ref().unwrap().digital_signature);
        assert!(ext.ext_key_usage.as_ref().unwrap().server_auth);
        assert_eq!(
            ext.subject_alt_names,
            vec!["example.com", "www.example.com"]
        );
    }

    #[test]
    fn ca_ext_shape_from_json() {
        let json = r#"{
            "publicKey": { "pem": "x" },
            "serialNumber": "01",
            "validity": { "notBefore": 1700000000000, "notAfter": 1800000000000 },
            "subject": [{ "shortName": "CN", "value": "CA" }],
            "issuer": [{ "shortName": "CN", "value": "CA" }],
            "extensions": [
                { "name": "basicConstraints", "cA": true, "critical": true },
                { "name": "keyUsage", "keyCertSign": true, "critical": true },
                { "name": "subjectKeyIdentifier" }
            ]
        }"#;
        let spec = cert_spec_from_json(json).expect("parse");
        // Numeric epoch-ms fallback for validity.
        assert_eq!(spec.not_before_unix, 1_700_000_000);
        assert_eq!(spec.subject[0].key, "CN");
        assert!(spec.extensions.basic_constraints.as_ref().unwrap().ca);
        assert!(spec.extensions.key_usage.as_ref().unwrap().key_cert_sign);
        assert!(spec.extensions.subject_key_identifier);
    }
}
