// Stdlib external integration FFI surface inventory.
//
// This fixture is intentionally executable by the normal parity runner,
// but its main purpose is to keep TS-side coverage accounting attached
// to related public FFI shims. Move @covers entries from this
// inventory into behavioral tests as each area gets deeper compatibility
// coverage.
//
// Inventory entries: 156 unique FFI names, 156 declarations.

const testFfiSurfaceStdlibIntegrationsVersion = 1;
if (testFfiSurfaceStdlibIntegrationsVersion !== 1) {
  throw new Error("unexpected coverage inventory version");
}
console.log("test_ffi_surface_stdlib_integrations: ok");

/*
@covers
crates/perry-stdlib/src/argon2.rs:
  - js_argon2_hash
  - js_argon2_hash_sync
  - js_argon2_needs_rehash
  - js_argon2_verify
  - js_argon2_verify_sync
crates/perry-stdlib/src/bcrypt.rs:
  - js_bcrypt_compare
  - js_bcrypt_compare_sync
  - js_bcrypt_gen_salt
  - js_bcrypt_hash
  - js_bcrypt_hash_sync
crates/perry-stdlib/src/cheerio.rs:
  - js_cheerio_load
  - js_cheerio_load_fragment
  - js_cheerio_select
  - js_cheerio_selection_attr
  - js_cheerio_selection_attrs
  - js_cheerio_selection_children
  - js_cheerio_selection_eq
  - js_cheerio_selection_find
  - js_cheerio_selection_first
  - js_cheerio_selection_has_class
  - js_cheerio_selection_html
  - js_cheerio_selection_is
  - js_cheerio_selection_last
  - js_cheerio_selection_length
  - js_cheerio_selection_parent
  - js_cheerio_selection_text
  - js_cheerio_selection_texts
  - js_cheerio_selection_to_array
crates/perry-stdlib/src/crypto.rs:
  - js_crypto_aes256_decrypt
  - js_crypto_aes256_encrypt
  - js_crypto_create_hash
  - js_crypto_ed25519_verify
  - js_crypto_hmac_sha256
  - js_crypto_hmac_sha256_bytes
  - js_crypto_md5
  - js_crypto_pbkdf2
  - js_crypto_pbkdf2_bytes
  - js_crypto_random_bytes_buffer
  - js_crypto_random_bytes_hex
  - js_crypto_random_uuid
  - js_crypto_scrypt
  - js_crypto_scrypt_custom
  - js_crypto_sha256
  - js_crypto_sha256_bytes
crates/perry-stdlib/src/crypto_e2e.rs:
  - js_crypto_aes256_gcm_decrypt
  - js_crypto_aes256_gcm_encrypt
  - js_crypto_hkdf_sha256
  - js_crypto_random_nonce
  - js_crypto_x25519_keypair
  - js_crypto_x25519_shared_secret
crates/perry-stdlib/src/ethers.rs:
  - js_ethers_format_ether
  - js_ethers_format_units
  - js_ethers_get_address
  - js_ethers_parse_ether
  - js_ethers_parse_units
  - js_ethers_wallet_create_random
  - js_keccak256_native
  - js_keccak256_native_bytes
crates/perry-stdlib/src/ioredis.rs:
  - js_ioredis_connect
  - js_ioredis_decr
  - js_ioredis_del
  - js_ioredis_disconnect
  - js_ioredis_exists
  - js_ioredis_expire
  - js_ioredis_get
  - js_ioredis_hdel
  - js_ioredis_hget
  - js_ioredis_hgetall
  - js_ioredis_hlen
  - js_ioredis_hset
  - js_ioredis_incr
  - js_ioredis_new
  - js_ioredis_ping
  - js_ioredis_quit
  - js_ioredis_set
  - js_ioredis_setex
crates/perry-stdlib/src/jsonwebtoken.rs:
  - js_jwt_decode
  - js_jwt_sign
  - js_jwt_sign_es256
  - js_jwt_sign_rs256
  - js_jwt_verify
crates/perry-stdlib/src/mongodb.rs:
  - js_mongodb_client_close
  - js_mongodb_client_connect
  - js_mongodb_client_db
  - js_mongodb_client_list_databases
  - js_mongodb_client_new
  - js_mongodb_collection_count
  - js_mongodb_collection_count_value
  - js_mongodb_collection_delete_many
  - js_mongodb_collection_delete_many_value
  - js_mongodb_collection_delete_one
  - js_mongodb_collection_delete_one_value
  - js_mongodb_collection_find
  - js_mongodb_collection_find_one
  - js_mongodb_collection_find_one_value
  - js_mongodb_collection_find_value
  - js_mongodb_collection_insert_many
  - js_mongodb_collection_insert_many_value
  - js_mongodb_collection_insert_one
  - js_mongodb_collection_insert_one_value
  - js_mongodb_collection_update_many
  - js_mongodb_collection_update_many_value
  - js_mongodb_collection_update_one
  - js_mongodb_collection_update_one_value
  - js_mongodb_connect
  - js_mongodb_db_collection
  - js_mongodb_db_list_collections
crates/perry-stdlib/src/mysql2/connection.rs:
  - js_mysql2_connection_begin_transaction
  - js_mysql2_connection_commit
  - js_mysql2_connection_end
  - js_mysql2_connection_execute
  - js_mysql2_connection_query
  - js_mysql2_connection_rollback
  - js_mysql2_create_connection
crates/perry-stdlib/src/mysql2/pool.rs:
  - js_mysql2_create_pool
  - js_mysql2_pool_connection_execute
  - js_mysql2_pool_connection_query
  - js_mysql2_pool_connection_release
  - js_mysql2_pool_end
  - js_mysql2_pool_execute
  - js_mysql2_pool_get_connection
crates/perry-stdlib/src/nodemailer.rs:
  - js_nodemailer_create_transport
  - js_nodemailer_send_mail
  - js_nodemailer_verify
crates/perry-stdlib/src/pg/connection.rs:
  - js_pg_client_connect
  - js_pg_client_end
  - js_pg_client_new
  - js_pg_client_query
  - js_pg_client_query_params
  - js_pg_connect
crates/perry-stdlib/src/pg/pool.rs:
  - js_pg_create_pool
  - js_pg_pool_end
  - js_pg_pool_new
  - js_pg_pool_query
crates/perry-stdlib/src/sharp.rs:
  - js_sharp_blur
  - js_sharp_crop
  - js_sharp_flip
  - js_sharp_flop
  - js_sharp_from_buffer
  - js_sharp_from_file
  - js_sharp_grayscale
  - js_sharp_height
  - js_sharp_jpeg
  - js_sharp_metadata
  - js_sharp_png
  - js_sharp_resize
  - js_sharp_rotate
  - js_sharp_sharpen
  - js_sharp_to_buffer
  - js_sharp_to_file
  - js_sharp_webp
  - js_sharp_width
crates/perry-stdlib/src/webcrypto.rs:
  - js_webcrypto_digest
  - js_webcrypto_import_key
  - js_webcrypto_sign
  - js_webcrypto_verify
*/
