//! Stream literals use canonical keys, with no private address-keyed roots.

#[test]
fn stream_literal_key_is_canonical_and_survives_gc() {
    let scope = crate::gc::RuntimeHandleScope::new();
    let bytes = b"__perryHiddenKeyRootTestB";
    let key = crate::node_stream::hidden_key_for_test(bytes);
    let key = scope.root_string_ptr(key);
    let hash = crate::object::key_bytes_hash(bytes.as_ptr(), bytes.len());
    let atom = crate::string::js_string_pool_atom(bytes.as_ptr(), bytes.len() as u32, hash, 0);
    assert_eq!(
        key.with_mut_ptr::<crate::StringHeader, _>(|key| key == atom),
        true,
        "stream literal must use the canonical atom"
    );
    crate::gc::js_gc_collect();
    let after = crate::node_stream::hidden_key_for_test(bytes);
    assert_eq!(
        key.with_mut_ptr::<crate::StringHeader, _>(|key| key == after),
        true,
        "canonical key must survive collection"
    );
}

#[test]
fn fresh_key_sabotage_reddens_canonical_witness() {
    let out = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "gc::tests::runtime_roots::hidden_keys::stream_literal_key_is_canonical_and_survives_gc", "--nocapture"])
        .env("PERRY_TEST_STREAM_LITERAL_KEY", "1").output().unwrap();
    assert!(!out.status.success(), "fresh-key sabotage must fail");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("stream literal must use the canonical atom")
    );
}
