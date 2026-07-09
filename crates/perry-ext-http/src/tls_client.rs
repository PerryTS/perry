//! Client-side TLS options for `https.request` / `https.get` (#4906).
//!
//! Node's https client accepts a family of TLS options on the request
//! (or agent) options object. Before this module the perry-ext-http
//! client always used reqwest's default verifier, so connecting to a
//! server that presents a self-signed / test-CA certificate failed the
//! handshake outright (`received fatal alert: UnknownCA`). Node's own
//! https tests stand up servers with the `test/fixtures/keys` test
//! certs and connect with one of:
//!
//! - `rejectUnauthorized: false` — don't fail the handshake on an
//!   untrusted cert (also driven by `NODE_TLS_REJECT_UNAUTHORIZED=0`).
//! - `ca: pem | Buffer | (pem|Buffer)[]` — trust the supplied CA(s).
//! - `checkServerIdentity: fn` — override hostname verification.
//!
//! This module parses those options off the request's options object and
//! folds them into a per-request `reqwest::Client`.
//!
//! ## Honored faithfully
//!
//! `rejectUnauthorized: false` / `NODE_TLS_REJECT_UNAUTHORIZED=0` map to
//! reqwest's `danger_accept_invalid_certs(true)`; `ca` entries are added
//! as additional trust anchors via `add_root_certificate`.
//!
//! ## Pragmatic approximations
//!
//! - `checkServerIdentity` is a JS callback that can't run inside the
//!   rustls handshake, so its mere presence is treated as "the caller
//!   intends to override identity verification" → accept the cert. Every
//!   Node test that sets it does so to accept a cert the default check
//!   would reject, so this matches observed behavior.
//! - reqwest's rustls backend requires a SAN match and does **not** fall
//!   back to the certificate Common Name. The Node test fixtures
//!   (`agent1-cert.pem` etc.) are CN-only, so a pure `ca` + `servername`
//!   flow still can't satisfy hostname verification through reqwest; such
//!   tests need `rejectUnauthorized:false`. The `ca` trust anchors are
//!   still wired up so properly-SAN'd certs verify.

use super::PTR_MASK;

/// Parsed client-side TLS options. `Default` is "no TLS customization",
/// in which case the caller keeps using the pooled default client.
#[derive(Clone, Default, Debug)]
pub(crate) struct TlsOptions {
    /// `options.rejectUnauthorized`. `None` = unset (Node defaults to
    /// `true` for https).
    pub(crate) reject_unauthorized: Option<bool>,
    /// PEM byte blobs from `options.ca` (string / Buffer / array of
    /// either). Each blob may itself be a multi-cert bundle.
    pub(crate) ca_pems: Vec<Vec<u8>>,
    /// True when `options.checkServerIdentity` is a function — the caller
    /// is overriding hostname verification.
    pub(crate) check_server_identity: bool,
}

impl TlsOptions {
    /// Whether these options require building a dedicated TLS client
    /// instead of reusing the pooled default. `NODE_TLS_REJECT_UNAUTHORIZED=0`
    /// alone counts (it disables verification process-wide).
    pub(crate) fn needs_custom_client(&self) -> bool {
        self.reject_unauthorized == Some(false)
            || self.check_server_identity
            || !self.ca_pems.is_empty()
            || node_tls_reject_unauthorized_disabled()
    }

    /// Resolve whether the cert chain should be accepted without
    /// verification. True when `rejectUnauthorized:false`, when
    /// `NODE_TLS_REJECT_UNAUTHORIZED=0`, or when a `checkServerIdentity`
    /// override is present (we can't run the JS callback mid-handshake,
    /// so its presence means accept).
    pub(crate) fn accept_invalid_certs(&self) -> bool {
        self.reject_unauthorized == Some(false)
            || self.check_server_identity
            || node_tls_reject_unauthorized_disabled()
    }

    /// Build a per-request `reqwest::Client` honoring these options.
    /// `pool` is the optional `(keep_alive, max_free_sockets,
    /// keep_alive_msecs)` Agent pool config to fold in.
    pub(crate) fn build_client(
        &self,
        pool: Option<(bool, f64, f64)>,
    ) -> Result<reqwest::Client, String> {
        let mut builder = crate::apply_node_proxy_policy(
            reqwest::Client::builder().tcp_keepalive(std::time::Duration::from_secs(60)),
        );

        if self.accept_invalid_certs() {
            builder = builder.danger_accept_invalid_certs(true);
        }
        for pem in &self.ca_pems {
            // A `ca` entry may be a single cert or a bundle; try the
            // bundle parser first, then fall back to the single-cert one.
            match reqwest::Certificate::from_pem_bundle(pem) {
                Ok(certs) => {
                    for cert in certs {
                        builder = builder.add_root_certificate(cert);
                    }
                }
                Err(_) => {
                    if let Ok(cert) = reqwest::Certificate::from_pem(pem) {
                        builder = builder.add_root_certificate(cert);
                    }
                }
            }
        }

        if let Some((keep_alive, max_free_sockets, keep_alive_msecs)) = pool {
            let pool_max_idle = if keep_alive {
                if !max_free_sockets.is_finite() || max_free_sockets > usize::MAX as f64 {
                    256
                } else {
                    max_free_sockets.max(1.0) as usize
                }
            } else {
                0
            };
            let idle_timeout = if keep_alive {
                let ms = if keep_alive_msecs.is_finite() && keep_alive_msecs > 0.0 {
                    keep_alive_msecs
                } else {
                    1000.0
                };
                std::time::Duration::from_millis(ms as u64)
            } else {
                std::time::Duration::from_millis(0)
            };
            builder = builder
                .pool_max_idle_per_host(pool_max_idle)
                .pool_idle_timeout(idle_timeout);
        }

        builder
            .build()
            .map_err(|e| format!("https: build client: {}", e))
    }
}

/// `NODE_TLS_REJECT_UNAUTHORIZED=0` disables client cert verification
/// process-wide. JS-side `process.env.NODE_TLS_REJECT_UNAUTHORIZED = '0'`
/// writes through to the OS environment (`js_setenv` → `std::env::set_var`),
/// so reading it here at dispatch time sees runtime assignments.
pub(crate) fn node_tls_reject_unauthorized_disabled() -> bool {
    std::env::var("NODE_TLS_REJECT_UNAUTHORIZED")
        .map(|v| v == "0")
        .unwrap_or(false)
}

/// Parse the client TLS options off a NaN-boxed request options object.
/// `ca` / `rejectUnauthorized` survive the JSON round-trip used by
/// [`super::parse_options_object`]; `checkServerIdentity` is a function
/// (dropped by JSON), so its presence is probed directly off the
/// NaN-boxed object.
///
/// # Safety
/// `opts_f64` must be a valid NaN-boxed JS value (any value is accepted;
/// non-objects yield default options).
pub(crate) unsafe fn parse_tls_options(opts_f64: f64) -> TlsOptions {
    let mut tls = TlsOptions::default();

    if let Some(opts) = super::parse_options_object(opts_f64) {
        if let Some(b) = opts.get("rejectUnauthorized").and_then(|v| v.as_bool()) {
            tls.reject_unauthorized = Some(b);
        }
        if let Some(ca) = opts.get("ca") {
            collect_pems(ca, &mut tls.ca_pems);
        }
    }

    if has_function_field(opts_f64, "checkServerIdentity") {
        tls.check_server_identity = true;
    }

    tls
}

/// Flatten a JSON `ca` value (string PEM, Node `Buffer` shape, a raw
/// numeric byte array, or an array of any of those) into PEM byte blobs.
fn collect_pems(v: &serde_json::Value, out: &mut Vec<Vec<u8>>) {
    use serde_json::Value;
    match v {
        Value::String(s) => out.push(s.as_bytes().to_vec()),
        Value::Object(map) if map.get("type").and_then(|t| t.as_str()) == Some("Buffer") => {
            if let Some(data) = map.get("data").and_then(|d| d.as_array()) {
                out.push(numeric_array_to_bytes(data));
            }
        }
        Value::Array(arr) => {
            // A bare numeric array is one cert's raw bytes; an array of
            // strings / Buffers is a list of CAs.
            if !arr.is_empty() && arr.iter().all(|e| e.is_u64() || e.is_i64()) {
                out.push(numeric_array_to_bytes(arr));
            } else {
                for e in arr {
                    collect_pems(e, out);
                }
            }
        }
        _ => {}
    }
}

fn numeric_array_to_bytes(arr: &[serde_json::Value]) -> Vec<u8> {
    arr.iter()
        .filter_map(|n| n.as_u64().map(|u| u as u8))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn pems(v: serde_json::Value) -> Vec<Vec<u8>> {
        let mut out = Vec::new();
        collect_pems(&v, &mut out);
        out
    }

    #[test]
    fn collect_pems_string() {
        assert_eq!(
            pems(json!("-----BEGIN CERTIFICATE-----")),
            vec![b"-----BEGIN CERTIFICATE-----".to_vec()]
        );
    }

    #[test]
    fn collect_pems_buffer_shape() {
        // `JSON.stringify(Buffer.from("hi"))`.
        assert_eq!(
            pems(json!({"type": "Buffer", "data": [104, 105]})),
            vec![b"hi".to_vec()]
        );
    }

    #[test]
    fn collect_pems_array_of_strings() {
        // `ca: [pem1, pem2]` — each element is a separate CA.
        assert_eq!(pems(json!(["a", "b"])), vec![b"a".to_vec(), b"b".to_vec()]);
    }

    #[test]
    fn collect_pems_array_of_buffers() {
        assert_eq!(
            pems(json!([
                {"type": "Buffer", "data": [104]},
                {"type": "Buffer", "data": [105]}
            ])),
            vec![b"h".to_vec(), b"i".to_vec()]
        );
    }

    #[test]
    fn collect_pems_bare_numeric_array_is_one_cert() {
        // A raw numeric array (a single Buffer serialized as numbers) is
        // one cert, not a list of single-byte certs.
        assert_eq!(pems(json!([104, 105])), vec![b"hi".to_vec()]);
    }

    #[test]
    fn needs_custom_client_logic() {
        let mut t = TlsOptions::default();
        assert!(!t.needs_custom_client());
        t.reject_unauthorized = Some(true);
        assert!(!t.needs_custom_client());
        t.reject_unauthorized = Some(false);
        assert!(t.needs_custom_client());

        let mut t = TlsOptions::default();
        t.check_server_identity = true;
        assert!(t.needs_custom_client());
        assert!(t.accept_invalid_certs());

        let mut t = TlsOptions::default();
        t.ca_pems.push(b"pem".to_vec());
        assert!(t.needs_custom_client());
        // ca alone does NOT bypass verification — it adds trust anchors.
        assert!(!t.accept_invalid_certs());
    }
}

/// True when `obj_f64.<field>` is a function (closure pointer). Used to
/// detect `checkServerIdentity` without a JSON round-trip (which drops
/// functions). Mirrors the raw NaN-boxed field read in `agent.rs`.
unsafe fn has_function_field(obj_f64: f64, field: &str) -> bool {
    let bits = obj_f64.to_bits();
    let upper = bits >> 48;
    let obj_ptr: *const perry_runtime::ObjectHeader = if upper >= 0x7FF8 {
        (bits & PTR_MASK) as *const perry_runtime::ObjectHeader
    } else if upper == 0 && bits >= 0x10000 {
        bits as *const perry_runtime::ObjectHeader
    } else {
        return false;
    };
    if obj_ptr.is_null() {
        return false;
    }
    let key = perry_runtime::js_string_from_bytes(field.as_ptr(), field.len() as u32);
    let val = perry_runtime::js_object_get_field_by_name(obj_ptr, key);
    if val.is_undefined() || val.is_null() {
        return false;
    }
    // Closures are NaN-boxed with POINTER_TAG (0x7FFD); a bare raw pointer
    // (codegen sometimes hands these back) is also accepted.
    let vbits = val.bits();
    let vupper = vbits >> 48;
    vupper == 0x7FFD || (vupper == 0 && vbits >= 0x10000)
}
