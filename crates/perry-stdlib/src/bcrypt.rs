//! Bcrypt password hashing through the shared native binding.
//!
//! Bundled and separately linked builds use the same providers, including
//! boolean promise results from `compare` and the sync salt generator.

pub use perry_ext_bcrypt::*;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_gen_salt_sync_returns_requested_cost() {
        let salt = js_bcrypt_gen_salt_sync(4.0);
        let salt = perry_ffi::read_string(unsafe { perry_ffi::JsString::from_raw(salt) })
            .expect("salt string");
        assert_eq!(salt.len(), 29);
        assert!(salt.starts_with("$2b$04$"));
    }
}
