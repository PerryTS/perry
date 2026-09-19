use super::*;

pub(super) const UTILS_CRYPTO_ROWS: &[NativeModSig] = &[
    // ========== nodemailer ==========
    NativeModSig {
        module: "nodemailer",
        has_receiver: false,
        method: "createTransport",
        class_filter: None,
        runtime: "js_nodemailer_create_transport",
        args: &[NA_PTR],
        ret: NR_F64,
    },
    NativeModSig {
        module: "nodemailer",
        has_receiver: true,
        method: "sendMail",
        class_filter: None,
        runtime: "js_nodemailer_send_mail",
        args: &[NA_PTR],
        ret: NR_GCPTR,
    },
    NativeModSig {
        module: "nodemailer",
        has_receiver: true,
        method: "verify",
        class_filter: None,
        runtime: "js_nodemailer_verify",
        args: &[],
        ret: NR_GCPTR,
    },
    // ========== nanoid ==========
    // js_nanoid_sized(NaN) → size=0 → falls back to js_nanoid() (21-char default),
    // so nanoid() and nanoid(N) both route through the same entry safely.
    NativeModSig {
        module: "nanoid",
        has_receiver: false,
        method: "nanoid",
        class_filter: None,
        runtime: "js_nanoid_sized",
        args: &[NA_F64],
        ret: NR_STR,
    },
    // ========== exponential-backoff ==========
    NativeModSig {
        module: "exponential-backoff",
        has_receiver: false,
        method: "backOff",
        class_filter: None,
        runtime: "backOff",
        args: &[NA_PTR, NA_F64],
        ret: NR_GCPTR,
    },
    // ========== argon2 ==========
    // Runtime FFI signatures take `*const StringHeader`, NOT NaN-boxed f64.
    // NA_STR routes through `js_get_string_pointer_unified` to extract the
    // raw pointer; NA_F64 would pass the f64 in d0 while the callee reads
    // x0 → null/garbage StringHeader → "Invalid password" (#591).
    NativeModSig {
        module: "argon2",
        has_receiver: false,
        method: "hash",
        class_filter: None,
        runtime: "js_argon2_hash",
        args: &[NA_STR],
        ret: NR_GCPTR,
    },
    NativeModSig {
        module: "argon2",
        has_receiver: false,
        method: "verify",
        class_filter: None,
        runtime: "js_argon2_verify",
        args: &[NA_STR, NA_STR],
        ret: NR_GCPTR,
    },
    // ========== bcrypt ==========
    // Same ABI rule as argon2 above: password / hash args are
    // `*const StringHeader`. The salt-rounds arg of bcrypt.hash is a
    // genuine f64 number and stays NA_F64.
    NativeModSig {
        module: "bcrypt",
        has_receiver: false,
        method: "hash",
        class_filter: None,
        runtime: "js_bcrypt_hash",
        args: &[NA_STR, NA_F64],
        ret: NR_GCPTR,
    },
    NativeModSig {
        module: "bcrypt",
        has_receiver: false,
        method: "compare",
        class_filter: None,
        runtime: "js_bcrypt_compare",
        args: &[NA_STR, NA_STR],
        ret: NR_GCPTR,
    },
    // ========== node-forge (PKI subset — perry-ext-node-forge) ==========
    // Namespaced statics (`forge.pki.rsa.generateKeyPair`,
    // `forge.pki.createCertificate`, `forge.md.sha256.create`, ...). These
    // dispatch once perry-hir flattens the `forge.pki.*` / `forge.md.*`
    // sub-namespace member chains to `NativeMethodCall { module:
    // "node-forge", method }`. Object-returning fns box as NR_PTR (they
    // return `JsValue::from_object_ptr`, which the double-tag-idempotent
    // NR_PTR path leaves intact); PEM emitters return `*mut StringHeader`
    // → NR_STR. Key/cert handles cross as NaN-boxed objects (NA_F64); PEM
    // inputs as raw string pointers (NA_STR).
    NativeModSig {
        module: "node-forge",
        has_receiver: false,
        method: "generateKeyPair",
        class_filter: None,
        runtime: "js_node_forge_generate_key_pair",
        args: &[NA_F64],
        ret: NR_JS_VALUE,
    },
    NativeModSig {
        module: "node-forge",
        has_receiver: false,
        method: "createCertificate",
        class_filter: None,
        runtime: "js_node_forge_create_certificate",
        args: &[],
        ret: NR_JS_VALUE,
    },
    NativeModSig {
        module: "node-forge",
        has_receiver: false,
        method: "certificateFromPem",
        class_filter: None,
        runtime: "js_node_forge_certificate_from_pem",
        args: &[NA_STR],
        ret: NR_JS_VALUE,
    },
    NativeModSig {
        module: "node-forge",
        has_receiver: false,
        method: "certificateToPem",
        class_filter: None,
        runtime: "js_node_forge_certificate_to_pem",
        args: &[NA_F64],
        ret: NR_STR,
    },
    NativeModSig {
        module: "node-forge",
        has_receiver: false,
        method: "privateKeyFromPem",
        class_filter: None,
        runtime: "js_node_forge_private_key_from_pem",
        args: &[NA_STR],
        ret: NR_JS_VALUE,
    },
    NativeModSig {
        module: "node-forge",
        has_receiver: false,
        method: "privateKeyToPem",
        class_filter: None,
        runtime: "js_node_forge_private_key_to_pem",
        args: &[NA_F64],
        ret: NR_STR,
    },
    NativeModSig {
        module: "node-forge",
        has_receiver: false,
        method: "publicKeyToPem",
        class_filter: None,
        runtime: "js_node_forge_public_key_to_pem",
        args: &[NA_F64],
        ret: NR_STR,
    },
    // `forge.md.sha256.create()` → a marker digest object.
    NativeModSig {
        module: "node-forge",
        has_receiver: false,
        method: "create",
        class_filter: None,
        runtime: "js_node_forge_md_sha256_create",
        args: &[],
        ret: NR_JS_VALUE,
    },
    // Certificate builder instance methods. The receiver (the JS cert
    // object) is NaN-unboxed to an `i64` `*mut ObjectHeader` and passed
    // as the first arg; the FFI writes into fixed object slots.
    NativeModSig {
        module: "node-forge",
        has_receiver: true,
        method: "setSubject",
        class_filter: Some("Certificate"),
        runtime: "js_node_forge_cert_set_subject",
        args: &[NA_F64],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "node-forge",
        has_receiver: true,
        method: "setIssuer",
        class_filter: Some("Certificate"),
        runtime: "js_node_forge_cert_set_issuer",
        args: &[NA_F64],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "node-forge",
        has_receiver: true,
        method: "setExtensions",
        class_filter: Some("Certificate"),
        runtime: "js_node_forge_cert_set_extensions",
        args: &[NA_F64],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "node-forge",
        has_receiver: true,
        method: "sign",
        class_filter: Some("Certificate"),
        runtime: "js_node_forge_cert_sign",
        args: &[NA_F64, NA_F64],
        ret: NR_VOID,
    },
];
