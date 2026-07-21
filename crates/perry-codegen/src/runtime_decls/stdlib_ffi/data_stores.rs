//! Database / data-store / crypto / OS stdlib FFI declarations
//! (extracted from stdlib_ffi.rs): pg, redis, mongodb, sqlite, OS, crypto, nanoid.

use crate::module::LlModule;
use crate::types::{DOUBLE, I32, I64, VOID};

pub(crate) fn declare_data_stores(module: &mut LlModule) {
    // ========== PostgreSQL (pg) ==========
    module.declare_function("js_pg_client_connect", I64, &[I64]);
    module.declare_function("js_pg_client_end", I64, &[I64]);
    module.declare_function("js_pg_client_new", I64, &[I64]);
    module.declare_function("js_pg_client_query", I64, &[I64, I64]);
    module.declare_function("js_pg_client_query_params", I64, &[I64, I64, I64]);
    module.declare_function("js_pg_connect", I64, &[I64]);
    module.declare_function("js_pg_create_pool", I64, &[I64]);
    module.declare_function("js_pg_pool_end", I64, &[I64]);
    module.declare_function("js_pg_pool_new", I64, &[I64]);
    module.declare_function("js_pg_pool_query", I64, &[I64, I64]);

    // ========== Redis / ioredis ==========
    module.declare_function("js_ioredis_connect", I64, &[I64]);
    module.declare_function("js_ioredis_decr", I64, &[I64, I64]);
    module.declare_function("js_ioredis_del", I64, &[I64, I64]);
    module.declare_function("js_ioredis_disconnect", VOID, &[I64]);
    module.declare_function("js_ioredis_exists", I64, &[I64, I64]);
    module.declare_function("js_ioredis_expire", I64, &[I64, I64, DOUBLE]);
    module.declare_function("js_ioredis_get", I64, &[I64, I64]);
    module.declare_function("js_ioredis_hdel", I64, &[I64, I64, I64]);
    module.declare_function("js_ioredis_hget", I64, &[I64, I64, I64]);
    module.declare_function("js_ioredis_hgetall", I64, &[I64, I64]);
    module.declare_function("js_ioredis_hlen", I64, &[I64, I64]);
    module.declare_function("js_ioredis_hset", I64, &[I64, I64, I64, I64]);
    module.declare_function("js_ioredis_incr", I64, &[I64, I64]);
    module.declare_function("js_ioredis_new", I64, &[I64]);
    module.declare_function("js_ioredis_ping", I64, &[I64]);
    module.declare_function("js_ioredis_quit", I64, &[I64]);
    module.declare_function("js_ioredis_set", I64, &[I64, I64, I64]);
    module.declare_function("js_ioredis_setex", I64, &[I64, I64, DOUBLE, I64]);

    // ========== MongoDB ==========
    module.declare_function("js_mongodb_client_close", I64, &[I64]);
    module.declare_function("js_mongodb_client_connect", I64, &[I64]);
    module.declare_function("js_mongodb_client_db", I64, &[I64, I64]);
    module.declare_function("js_mongodb_client_list_databases", I64, &[I64]);
    module.declare_function("js_mongodb_client_new", I64, &[I64]);
    // _value wrappers (JSON-stringify f64 JSValue arg, forward to existing fns)
    module.declare_function("js_mongodb_collection_count_value", I64, &[I64, DOUBLE]);
    module.declare_function(
        "js_mongodb_collection_delete_many_value",
        I64,
        &[I64, DOUBLE],
    );
    module.declare_function(
        "js_mongodb_collection_delete_one_value",
        I64,
        &[I64, DOUBLE],
    );
    module.declare_function("js_mongodb_collection_find_one_value", I64, &[I64, DOUBLE]);
    module.declare_function("js_mongodb_collection_find_value", I64, &[I64, DOUBLE]);
    module.declare_function(
        "js_mongodb_collection_insert_many_value",
        I64,
        &[I64, DOUBLE],
    );
    module.declare_function(
        "js_mongodb_collection_insert_one_value",
        I64,
        &[I64, DOUBLE],
    );
    module.declare_function(
        "js_mongodb_collection_update_many_value",
        I64,
        &[I64, DOUBLE, DOUBLE],
    );
    module.declare_function(
        "js_mongodb_collection_update_one_value",
        I64,
        &[I64, DOUBLE, DOUBLE],
    );
    module.declare_function("js_mongodb_collection_count", I64, &[I64, I64]);
    module.declare_function("js_mongodb_collection_delete_many", I64, &[I64, I64]);
    module.declare_function("js_mongodb_collection_delete_one", I64, &[I64, I64]);
    module.declare_function("js_mongodb_collection_find", I64, &[I64, I64]);
    module.declare_function("js_mongodb_collection_find_one", I64, &[I64, I64]);
    module.declare_function("js_mongodb_collection_insert_many", I64, &[I64, I64]);
    module.declare_function("js_mongodb_collection_insert_one", I64, &[I64, I64]);
    module.declare_function("js_mongodb_collection_update_many", I64, &[I64, I64, I64]);
    module.declare_function("js_mongodb_collection_update_one", I64, &[I64, I64, I64]);
    module.declare_function("js_mongodb_connect", I64, &[I64]);
    module.declare_function("js_mongodb_db_collection", I64, &[I64, I64]);
    module.declare_function("js_mongodb_db_list_collections", I64, &[I64]);

    // ========== SQLite ==========
    module.declare_function("js_sqlite_close", VOID, &[I64]);
    module.declare_function("js_sqlite_exec", VOID, &[I64, I64]);
    module.declare_function("js_sqlite_open", I64, &[I64]);
    module.declare_function("js_sqlite_pragma", I64, &[I64, I64, I64]);
    module.declare_function("js_sqlite_prepare", I64, &[I64, I64]);
    module.declare_function("js_sqlite_stmt_all", I64, &[I64, I64]);
    module.declare_function("js_sqlite_stmt_columns", I64, &[I64]);
    module.declare_function("js_sqlite_stmt_get", I64, &[I64, I64]);
    module.declare_function("js_sqlite_stmt_run", I64, &[I64, I64]);
    module.declare_function("js_sqlite_transaction", I64, &[I64, I64]);
    module.declare_function("js_sqlite_transaction_commit", VOID, &[I64]);
    module.declare_function("js_sqlite_transaction_rollback", VOID, &[I64]);
    module.declare_function("js_node_sqlite_backup", I64, &[DOUBLE, DOUBLE, DOUBLE]);
    module.declare_function("js_node_sqlite_database_sync_call", I64, &[DOUBLE, DOUBLE]);
    module.declare_function("js_node_sqlite_database_sync_new", I64, &[DOUBLE, DOUBLE]);
    module.declare_function("js_node_sqlite_database_sync_open", I32, &[I64]);
    module.declare_function("js_node_sqlite_database_sync_close", I32, &[I64]);
    module.declare_function("js_node_sqlite_database_sync_dispose", I32, &[I64]);
    module.declare_function("js_node_sqlite_database_sync_exec", I32, &[I64, DOUBLE]);
    module.declare_function(
        "js_node_sqlite_database_sync_prepare",
        I64,
        &[I64, DOUBLE, DOUBLE],
    );
    module.declare_function(
        "js_node_sqlite_database_sync_function",
        I32,
        &[I64, DOUBLE, DOUBLE, DOUBLE],
    );
    module.declare_function(
        "js_node_sqlite_database_sync_aggregate",
        I32,
        &[I64, DOUBLE, DOUBLE],
    );
    module.declare_function(
        "js_node_sqlite_database_sync_enable_defensive",
        I32,
        &[I64, DOUBLE],
    );
    module.declare_function(
        "js_node_sqlite_database_sync_set_authorizer",
        I32,
        &[I64, DOUBLE],
    );
    module.declare_function(
        "js_node_sqlite_database_sync_create_tag_store",
        I64,
        &[I64, DOUBLE],
    );
    module.declare_function(
        "js_node_sqlite_database_sync_create_session",
        I64,
        &[I64, DOUBLE],
    );
    module.declare_function(
        "js_node_sqlite_database_sync_apply_changeset",
        DOUBLE,
        &[I64, DOUBLE, DOUBLE],
    );
    module.declare_function(
        "js_node_sqlite_database_sync_enable_load_extension",
        I32,
        &[I64, DOUBLE],
    );
    module.declare_function(
        "js_node_sqlite_database_sync_load_extension",
        I32,
        &[I64, DOUBLE],
    );
    module.declare_function(
        "js_node_sqlite_database_sync_location",
        DOUBLE,
        &[I64, DOUBLE],
    );
    module.declare_function("js_node_sqlite_database_sync_is_open", DOUBLE, &[I64]);
    module.declare_function(
        "js_node_sqlite_database_sync_is_transaction",
        DOUBLE,
        &[I64],
    );
    module.declare_function("js_node_sqlite_database_sync_limits", I64, &[I64]);
    module.declare_function("js_node_sqlite_statement_sync_call", I64, &[DOUBLE, DOUBLE]);
    module.declare_function("js_node_sqlite_statement_sync_new", I64, &[DOUBLE, DOUBLE]);
    module.declare_function("js_node_sqlite_statement_sync_run", I64, &[I64, I64]);
    module.declare_function("js_node_sqlite_statement_sync_get", DOUBLE, &[I64, I64]);
    module.declare_function("js_node_sqlite_statement_sync_all", I64, &[I64, I64]);
    module.declare_function("js_node_sqlite_statement_sync_iterate", DOUBLE, &[I64, I64]);
    module.declare_function("js_node_sqlite_statement_sync_columns", I64, &[I64]);
    module.declare_function(
        "js_node_sqlite_statement_sync_set_read_bigints",
        I32,
        &[I64, DOUBLE],
    );
    module.declare_function(
        "js_node_sqlite_statement_sync_set_return_arrays",
        I32,
        &[I64, DOUBLE],
    );
    module.declare_function(
        "js_node_sqlite_statement_sync_set_allow_bare_named_parameters",
        I32,
        &[I64, DOUBLE],
    );
    module.declare_function(
        "js_node_sqlite_statement_sync_set_allow_unknown_named_parameters",
        I32,
        &[I64, DOUBLE],
    );
    module.declare_function("js_node_sqlite_statement_sync_source_sql", I64, &[I64]);
    module.declare_function("js_node_sqlite_statement_sync_expanded_sql", I64, &[I64]);
    module.declare_function("js_node_sqlite_sql_tag_store_run", I64, &[I64, I64]);
    module.declare_function("js_node_sqlite_sql_tag_store_get", DOUBLE, &[I64, I64]);
    module.declare_function("js_node_sqlite_sql_tag_store_all", I64, &[I64, I64]);
    module.declare_function("js_node_sqlite_sql_tag_store_iterate", DOUBLE, &[I64, I64]);
    module.declare_function("js_node_sqlite_sql_tag_store_clear", I32, &[I64]);
    module.declare_function("js_node_sqlite_sql_tag_store_size", DOUBLE, &[I64]);
    module.declare_function("js_node_sqlite_sql_tag_store_capacity", DOUBLE, &[I64]);
    module.declare_function("js_node_sqlite_sql_tag_store_db", I64, &[I64]);
    module.declare_function("js_node_sqlite_session_call", I64, &[DOUBLE, DOUBLE]);
    module.declare_function("js_node_sqlite_session_new", I64, &[DOUBLE, DOUBLE]);
    module.declare_function("js_node_sqlite_session_changeset", I64, &[I64]);
    module.declare_function("js_node_sqlite_session_patchset", I64, &[I64]);
    module.declare_function("js_node_sqlite_session_close", I32, &[I64]);
    module.declare_function("js_node_sqlite_session_dispose", I32, &[I64]);

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
    // arg is the NaN-unboxed options-object pointer (0 = none).
    module.declare_function("js_crypto_scrypt_bytes", I64, &[I64, I64, DOUBLE, I64]);
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

    // ========== Nanoid ==========
    module.declare_function("js_nanoid", I64, &[DOUBLE]);
    module.declare_function("js_nanoid_custom", I64, &[I64, DOUBLE]);
}
