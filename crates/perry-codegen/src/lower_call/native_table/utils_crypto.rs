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
];
