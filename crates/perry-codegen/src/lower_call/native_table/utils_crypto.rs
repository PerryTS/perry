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
];
