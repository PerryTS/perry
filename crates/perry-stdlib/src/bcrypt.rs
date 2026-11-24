//! Bcrypt password hashing through the shared native binding.
//!
//! Bundled and separately linked builds use the same providers, including
//! boolean promise results from `compare` and the sync salt generator.

pub use perry_ext_bcrypt::*;

#[cfg(test)]
mod tests {
    use super::*;

    use perry_ffi::{alloc_string, read_string, JsString, JsValue};

    fn text(ptr: *mut perry_ffi::StringHeader) -> String {
        read_string(unsafe { JsString::from_raw(ptr) })
            .expect("string result")
            .to_string()
    }

    #[test]
    fn bundled_gen_salt_sync_returns_requested_cost() {
        let salt = text(js_bcrypt_gen_salt_sync(4.0));
        assert_eq!(salt.len(), 29);
        assert!(salt.starts_with("$2b$04$"));
    }

    #[test]
    fn gen_salt_sync_normalizes_rounds_like_node() {
        assert!(text(js_bcrypt_gen_salt_sync(0.0)).starts_with("$2b$10$"));
        assert!(text(js_bcrypt_gen_salt_sync(f64::NAN)).starts_with("$2b$10$"));
        assert!(text(js_bcrypt_gen_salt_sync(2.0)).starts_with("$2b$04$"));
        assert!(text(js_bcrypt_gen_salt_sync(20.0)).starts_with("$2b$20$"));
        assert!(text(js_bcrypt_gen_salt_sync(f64::INFINITY)).starts_with("$2b$04$"));
        assert!(text(js_bcrypt_gen_salt_sync(f64::NEG_INFINITY)).starts_with("$2b$04$"));
        assert!(text(js_bcrypt_gen_salt_sync(-5.0)).starts_with("$2b$31$"));
        assert!(text(js_bcrypt_gen_salt_sync(-0.5)).starts_with("$2b$04$"));
        assert!(text(js_bcrypt_gen_salt_sync(-252.0)).starts_with("$2b$04$"));
        assert!(text(js_bcrypt_gen_salt_sync(266.0)).starts_with("$2b$10$"));
        assert!(text(js_bcrypt_gen_salt_sync(4294967308.0)).starts_with("$2b$12$"));
    }

    #[test]
    fn hash_sync_accepts_a_salt_string_or_a_cost() {
        // A fresh string per call: a held raw pointer would go stale across the
        // allocating calls below.
        fn password() -> *mut perry_ffi::StringHeader {
            alloc_string("pw").as_raw()
        }
        let salt = js_bcrypt_gen_salt_sync(4.0);
        let salt_bits = JsValue::from_string_ptr(salt).bits() as i64;
        let salted = unsafe { js_bcrypt_hash_sync(password(), salt_bits) };
        assert!(text(salted).starts_with(&text(salt)));
        let matches = unsafe { js_bcrypt_compare_sync(password(), salted) };
        assert_eq!(matches.to_bits(), JsValue::from_bool(true).bits());

        let cost_bits = JsValue::from_number(5.0).bits() as i64;
        assert!(text(unsafe { js_bcrypt_hash_sync(password(), cost_bits) }).starts_with("$2b$05$"));

        let undefined = JsValue::UNDEFINED.bits() as i64;
        assert!(unsafe { js_bcrypt_hash_sync(password(), undefined) }.is_null());
        let bad_salt = JsValue::from_string_ptr(alloc_string("$2y$04$abc").as_raw()).bits() as i64;
        assert!(unsafe { js_bcrypt_hash_sync(password(), bad_salt) }.is_null());
    }
}
