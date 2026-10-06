//! Database / data-store / crypto / OS stdlib FFI declarations
//! (extracted from stdlib_ffi.rs): sqlite, OS, crypto, nanoid.

use crate::module::LlModule;
use crate::types::{DOUBLE, I32, I64, VOID};

pub(crate) fn declare_data_stores(module: &mut LlModule) {
    // ========== SQLite ==========
    module.declare_function("js_sqlite_close", VOID, &[I64]);
    module.declare_function("js_sqlite_exec", VOID, &[I64, I64]);
    module.declare_function("js_sqlite_open", I64, &[I64]);
    module.declare_function("js_sqlite_pragma", I64, &[I64, I64, I64]);
    module.declare_function("js_sqlite_prepare", I64, &[I64, I64]);
    module.declare_function("js_sqlite_stmt_all", I64, &[I64, I64]);
    module.declare_function("js_sqlite_stmt_columns", I64, &[I64]);
    module.declare_function("js_sqlite_stmt_get", DOUBLE, &[I64, I64]);
    module.declare_function("js_sqlite_stmt_run", I64, &[I64, I64]);
    module.declare_function("js_sqlite_transaction", I64, &[I64, I64]);
    module.declare_function("js_sqlite_transaction_commit", VOID, &[I64]);
    module.declare_function("js_sqlite_transaction_rollback", VOID, &[I64]);
    module.declare_function("js_bun_sqlite_database_call", I64, &[DOUBLE, DOUBLE]);
    module.declare_function("js_bun_sqlite_database_new", I64, &[DOUBLE, DOUBLE]);
    module.declare_function("js_bun_sqlite_database_query", I64, &[I64, DOUBLE]);
    module.declare_function("js_bun_sqlite_database_run", I64, &[I64, DOUBLE, I64]);
    module.declare_function("js_bun_sqlite_database_filename", I64, &[I64]);
    module.declare_function("js_bun_sqlite_database_transaction", I64, &[I64, DOUBLE]);
    module.declare_function("js_bun_sqlite_statement_values", I64, &[I64, I64]);
    module.declare_function(
        "js_bun_sqlite_statement_safe_integers",
        DOUBLE,
        &[I64, DOUBLE],
    );
    module.declare_function("js_bun_sqlite_statement_finalize", VOID, &[I64]);
    module.declare_function("js_bun_sqlite_database_close", I32, &[I64]);
    module.declare_function("js_bun_sqlite_database_serialize", I64, &[I64, DOUBLE]);
    module.declare_function("js_bun_sqlite_database_load_extension", I32, &[I64, DOUBLE]);
    module.declare_function("js_bun_sqlite_statement_run", I64, &[I64, I64]);
    module.declare_function("js_bun_sqlite_statement_get", DOUBLE, &[I64, I64]);
    module.declare_function("js_bun_sqlite_statement_all", I64, &[I64, I64]);
    module.declare_function("js_node_sqlite_backup", I64, &[DOUBLE, DOUBLE, DOUBLE]);
    module.declare_function(
        "js_node_sqlite_database_sync_call",
        DOUBLE,
        &[DOUBLE, DOUBLE],
    );
    module.declare_function(
        "js_node_sqlite_database_sync_new",
        DOUBLE,
        &[DOUBLE, DOUBLE],
    );
    module.declare_function(
        "js_node_sqlite_statement_sync_call",
        DOUBLE,
        &[DOUBLE, DOUBLE],
    );
    module.declare_function(
        "js_node_sqlite_statement_sync_new",
        DOUBLE,
        &[DOUBLE, DOUBLE],
    );
    module.declare_function("js_node_sqlite_session_call", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_node_sqlite_session_new", DOUBLE, &[DOUBLE, DOUBLE]);

    // ========== OS ==========
    module.declare_function("js_os_cpus", I64, &[]);
    module.declare_function("js_os_freemem", DOUBLE, &[]);
    module.declare_function("js_os_homedir", I64, &[]);
    module.declare_function("js_os_network_interfaces", I64, &[]);
    module.declare_function("js_os_tmpdir", I64, &[]);
    module.declare_function("js_os_totalmem", DOUBLE, &[]);
    module.declare_function("js_os_uptime", DOUBLE, &[]);
    module.declare_function("js_os_user_info", I64, &[]);
    module.declare_function("js_os_user_info_buffer", I64, &[]);
    // #3004 — dynamic-options form: inspects `options.encoding` at runtime.
    module.declare_function("js_os_user_info_options", I64, &[I64]);

    // ========== Crypto ==========
    module.declare_function("js_crypto_aes256_decrypt", I64, &[I64, I64, I64]);
    module.declare_function("js_crypto_aes256_encrypt", I64, &[I64, I64, I64]);
    module.declare_function("js_crypto_aes256_gcm_decrypt", I64, &[I64, I64, I64]);
    module.declare_function("js_crypto_aes256_gcm_encrypt", I64, &[I64, I64, I64]);
    // Handle-based createCipheriv / createDecipheriv (#1075) — return a
    // pre-NaN-boxed f64 carrying POINTER_TAG + handle id. Dispatched
    // through HANDLE_METHOD_DISPATCH → `dispatch_cipher` for .update() /
    // .final() / .getAuthTag() / .setAuthTag().
    module.declare_function(
        "js_crypto_create_cipheriv",
        DOUBLE,
        &[I64, I64, I64, DOUBLE],
    );
    module.declare_function(
        "js_crypto_create_decipheriv",
        DOUBLE,
        &[I64, I64, I64, DOUBLE],
    );
    // crypto.createSign(alg) / createVerify(alg) -> SignHandle (NaN-boxed).
    module.declare_function("js_crypto_create_sign", DOUBLE, &[I64]);
    module.declare_function("js_crypto_create_verify", DOUBLE, &[I64]);
    module.declare_function("js_crypto_hkdf_sha256", I64, &[I64, I64, I64, DOUBLE]);
    // crypto.hkdfSync(digest, ikm, salt, info, keylen) -> ArrayBuffer.
    module.declare_function("js_crypto_hkdf_sync", I64, &[I64, I64, I64, I64, DOUBLE]);
    module.declare_function("js_crypto_pbkdf2", I64, &[I64, I64, DOUBLE, DOUBLE]);
    module.declare_function("js_crypto_argon2_sync", I64, &[I64, DOUBLE]);
    module.declare_function("js_crypto_argon2_async", DOUBLE, &[I64, DOUBLE, DOUBLE]);
    module.declare_function("js_crypto_random_bytes_hex", I64, &[DOUBLE]);
    module.declare_function("js_crypto_random_nonce", I64, &[]);
    module.declare_function("js_crypto_scrypt", I64, &[I64, I64, DOUBLE]);
    // crypto.scryptSync(password, salt, keylen, options?) -> Buffer. The 4th
    // arg is the full NaN-boxed options value so validation can distinguish
    // objects, primitives, and undefined.
    module.declare_function("js_crypto_scrypt_bytes", I64, &[I64, I64, DOUBLE, DOUBLE]);
    // crypto.generateKeyPairSync(type, options) -> { publicKey, privateKey }.
    module.declare_function("js_crypto_generate_key_pair_sync", DOUBLE, &[I64, I64]);
    module.declare_function(
        "js_crypto_scrypt_custom",
        I64,
        &[I64, I64, DOUBLE, DOUBLE, DOUBLE, DOUBLE],
    );
    module.declare_function("js_crypto_x25519_keypair", I64, &[]);
    module.declare_function("js_crypto_x25519_shared_secret", I64, &[I64, I64]);
    module.declare_function("js_keccak256_native", I64, &[I64]);
    module.declare_function("js_keccak256_native_bytes", I64, &[I64]);
}
