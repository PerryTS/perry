use super::*;

pub(super) const UTILS_CRYPTO_ROWS: &[NativeModSig] = &[
    // ========== uuid ==========
    // All generators return `*mut StringHeader`, so they must box as
    // NR_STR (STRING_TAG) — NR_PTR boxed them as a generic native handle
    // and `v4()` read back as `[object Object]` (#5197).
    NativeModSig {
        module: "uuid",
        has_receiver: false,
        method: "v4",
        class_filter: None,
        runtime: "js_uuid_v4",
        args: &[],
        ret: NR_STR,
    },
    NativeModSig {
        module: "uuid",
        has_receiver: false,
        method: "v1",
        class_filter: None,
        runtime: "js_uuid_v1",
        args: &[],
        ret: NR_STR,
    },
    NativeModSig {
        module: "uuid",
        has_receiver: false,
        method: "v7",
        class_filter: None,
        runtime: "js_uuid_v7",
        args: &[],
        ret: NR_STR,
    },
    // v5 (SHA-1) / v3 (MD5) name-based: `vN(name, namespace)`. The shim
    // supports the string-UUID namespace form; the array-namespace form
    // is only reachable via `perry.compilePackages`.
    NativeModSig {
        module: "uuid",
        has_receiver: false,
        method: "v5",
        class_filter: None,
        runtime: "js_uuid_v5",
        args: &[NA_STR, NA_STR],
        ret: NR_STR,
    },
    NativeModSig {
        module: "uuid",
        has_receiver: false,
        method: "v3",
        class_filter: None,
        runtime: "js_uuid_v3",
        args: &[NA_STR, NA_STR],
        ret: NR_STR,
    },
    NativeModSig {
        module: "uuid",
        has_receiver: false,
        method: "validate",
        class_filter: None,
        runtime: "js_uuid_validate",
        // Runtime sig is `*const StringHeader` → coerce the arg to a
        // string pointer (NA_F64 passed raw NaN-box bits, so validate
        // always read 0 — #5197). NR_BOOL boxes the 1.0/0.0 result as a
        // real JS boolean so it prints `true`/`false`, not `1`/`0`.
        args: &[NA_STR],
        ret: NR_BOOL,
    },
    NativeModSig {
        module: "uuid",
        has_receiver: false,
        method: "version",
        class_filter: None,
        runtime: "js_uuid_version",
        args: &[NA_STR],
        ret: NR_F64,
    },
    // ========== jsonwebtoken ==========
    // `sign` and `verify` are intentionally handled in
    // lower_call/native.rs — both need option-dependent runtime
    // selection (HS256 / ES256 / RS256) that the generic table can't
    // express. `decode` stays here because it has no algorithm options.
    NativeModSig {
        module: "jsonwebtoken",
        has_receiver: false,
        method: "decode",
        class_filter: None,
        runtime: "js_jwt_decode",
        // js_jwt_decode(token_ptr) -> *mut StringHeader (JSON of payload).
        // NR_OBJ_FROM_JSON_STR pipes the returned JSON through
        // js_json_parse_or_null so user code sees an object (mirrors
        // `verify`'s post-#927 contract). Issue #927.
        args: &[NA_STR],
        ret: NR_OBJ_FROM_JSON_STR,
    },
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
        ret: NR_PTR,
    },
    NativeModSig {
        module: "nodemailer",
        has_receiver: true,
        method: "verify",
        class_filter: None,
        runtime: "js_nodemailer_verify",
        args: &[],
        ret: NR_PTR,
    },
    // ========== dotenv ==========
    NativeModSig {
        module: "dotenv",
        has_receiver: false,
        method: "config",
        class_filter: None,
        runtime: "js_dotenv_config",
        args: &[],
        ret: NR_F64,
    },
    // `dotenv.parse(src)` → the JSON string `js_dotenv_parse` builds, piped
    // through `js_json_parse` by NR_OBJ_FROM_JSON_STR so TypeScript sees a
    // real object (`{ FOO: "bar" }`), not the encoded string. Without this
    // row the symbol fell through the #463 gate to a deferred runtime throw
    // even though the native implementation was already linked in.
    NativeModSig {
        module: "dotenv",
        has_receiver: false,
        method: "parse",
        class_filter: None,
        runtime: "js_dotenv_parse",
        args: &[NA_STR],
        ret: NR_OBJ_FROM_JSON_STR,
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
    // ========== slugify ==========
    // Second arg is npm slugify's replacement-or-options overload: a
    // plain string ('_') OR an options object ({ replacement, lower,
    // strict, trim }). It must cross as raw NaN-box bits (NA_JSV) so
    // the runtime can distinguish the two — the old NA_STR coercion
    // JSON-stringified the object and its first char '{' became the
    // separator ("hello{world"). Missing arg pads to TAG_UNDEFINED →
    // runtime defaults ("-" separator, no lower/strict, trim).
    // "default" for `import slugify from 'slugify'; slugify(s)` (HIR emits method:"default").
    // "slugify" for `import { slugify } from 'slugify'; slugify(s)` (named import).
    NativeModSig {
        module: "slugify",
        has_receiver: false,
        method: "default",
        class_filter: None,
        runtime: "js_slugify_with_options",
        args: &[NA_STR, NA_JSV],
        ret: NR_STR,
    },
    NativeModSig {
        module: "slugify",
        has_receiver: false,
        method: "slugify",
        class_filter: None,
        runtime: "js_slugify_with_options",
        args: &[NA_STR, NA_JSV],
        ret: NR_STR,
    },
    // ========== validator ==========
    NativeModSig {
        module: "validator",
        has_receiver: false,
        method: "isEmail",
        class_filter: None,
        runtime: "js_validator_is_email",
        args: &[NA_STR],
        ret: NR_F64,
    },
    NativeModSig {
        module: "validator",
        has_receiver: false,
        method: "isURL",
        class_filter: None,
        runtime: "js_validator_is_url",
        args: &[NA_STR],
        ret: NR_F64,
    },
    NativeModSig {
        module: "validator",
        has_receiver: false,
        method: "isUUID",
        class_filter: None,
        runtime: "js_validator_is_uuid",
        args: &[NA_STR],
        ret: NR_F64,
    },
    NativeModSig {
        module: "validator",
        has_receiver: false,
        method: "isJSON",
        class_filter: None,
        runtime: "js_validator_is_json",
        args: &[NA_STR],
        ret: NR_F64,
    },
    NativeModSig {
        module: "validator",
        has_receiver: false,
        method: "isEmpty",
        class_filter: None,
        runtime: "js_validator_is_empty",
        args: &[NA_STR],
        ret: NR_F64,
    },
    // ========== exponential-backoff ==========
    NativeModSig {
        module: "exponential-backoff",
        has_receiver: false,
        method: "backOff",
        class_filter: None,
        runtime: "backOff",
        args: &[NA_PTR, NA_F64],
        ret: NR_PTR,
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
        ret: NR_PTR,
    },
    NativeModSig {
        module: "argon2",
        has_receiver: false,
        method: "verify",
        class_filter: None,
        runtime: "js_argon2_verify",
        args: &[NA_STR, NA_STR],
        ret: NR_PTR,
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
        ret: NR_PTR,
    },
    NativeModSig {
        module: "bcrypt",
        has_receiver: false,
        method: "compare",
        class_filter: None,
        runtime: "js_bcrypt_compare",
        args: &[NA_STR, NA_STR],
        ret: NR_PTR,
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
        ret: NR_PTR,
    },
    NativeModSig {
        module: "node-forge",
        has_receiver: false,
        method: "createCertificate",
        class_filter: None,
        runtime: "js_node_forge_create_certificate",
        args: &[],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "node-forge",
        has_receiver: false,
        method: "certificateFromPem",
        class_filter: None,
        runtime: "js_node_forge_certificate_from_pem",
        args: &[NA_STR],
        ret: NR_PTR,
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
        ret: NR_PTR,
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
        ret: NR_PTR,
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

#[cfg(test)]
mod tests {
    use super::*;

    /// `dotenv.parse` must dispatch to the native implementation and return a
    /// real object.
    ///
    /// `js_dotenv_parse` was declared to codegen and linked into every binary,
    /// but had no dispatch row, so the #463 gate compiled each call site to a
    /// deferred throw-on-reach error. `readConfigFile()`-shaped callers wrap
    /// the call in `try { … } catch {}`, so the throw was swallowed and the
    /// `.env` config silently never loaded.
    ///
    /// The return kind matters as much as the row: `js_dotenv_parse` hands back
    /// a JSON *string*, so only `NR_OBJ_FROM_JSON_STR` (which pipes it through
    /// `js_json_parse`) makes `dotenv.parse(src).FOO` read a property instead
    /// of indexing a string.
    #[test]
    fn dotenv_parse_dispatches_to_native_impl_as_an_object() {
        let row = UTILS_CRYPTO_ROWS
            .iter()
            .find(|r| r.module == "dotenv" && r.method == "parse")
            .expect("dotenv.parse needs a dispatch row");
        assert_eq!(row.runtime, "js_dotenv_parse");
        assert!(!row.has_receiver);
        assert_eq!(row.class_filter, None);
        assert!(matches!(row.args, [NativeArgKind::StrPtr]));
        assert!(
            matches!(row.ret, NativeRetKind::ObjFromJsonStr),
            "dotenv.parse must be JSON-decoded into an object, got {:?}",
            row.ret
        );
    }
}
