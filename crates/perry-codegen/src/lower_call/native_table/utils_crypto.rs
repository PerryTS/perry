use super::*;

pub(super) const UTILS_CRYPTO_ROWS: &[NativeModSig] = &[
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
    // The sync forms return the value itself: a raw `StringHeader` the row
    // boxes (`NR_STR`, null on failure reads as `null`) or a NaN-boxed boolean
    // (`NR_F64`). `genSalt`'s rounds is optional; a missing one arrives as
    // `undefined` and the binding applies node's default of 10.
    NativeModSig {
        module: "bcrypt",
        has_receiver: false,
        method: "hashSync",
        class_filter: None,
        runtime: "js_bcrypt_hash_sync",
        args: &[NA_STR, NA_F64],
        ret: NR_STR,
    },
    NativeModSig {
        module: "bcrypt",
        has_receiver: false,
        method: "compareSync",
        class_filter: None,
        runtime: "js_bcrypt_compare_sync",
        args: &[NA_STR, NA_STR],
        ret: NR_F64,
    },
    NativeModSig {
        module: "bcrypt",
        has_receiver: false,
        method: "genSalt",
        class_filter: None,
        runtime: "js_bcrypt_gen_salt",
        args: &[NA_F64],
        ret: NR_GCPTR,
    },
    NativeModSig {
        module: "bcrypt",
        has_receiver: false,
        method: "genSaltSync",
        class_filter: None,
        runtime: "js_bcrypt_gen_salt_sync",
        args: &[NA_F64],
        ret: NR_STR,
    },
];
