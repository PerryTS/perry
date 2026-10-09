use super::*;

#[test]
fn a_split_regexp_site_returns_the_generic_body_before_arguments() {
    // Worker tests permanently close the process-wide method-site gate.
    // Exercise this primary-only path in a fresh process so test order
    // cannot make the specialized entry unreachable.
    const CHILD: &str = "PERRY_TEST_SPLIT_REGEXP_SITE_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "object::method_site::regex_split_tests::a_split_regexp_site_returns_the_generic_body_before_arguments",
                "--test-threads=1",
            ])
            .env(CHILD, "1")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "fresh-process fixture failed: {}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    std::thread::Builder::new()
        .stack_size(16 << 20)
        .spawn(|| unsafe {
            let _stable = crate::gc::GcSuppressScope::new();
            let re = crate::regex::test_construct_regexp_and_exec_once("a", "");
            let recv = crate::value::js_nanbox_pointer(re as i64);
            let mut slot: MethodSiteSlot = std::ptr::null_mut();
            prime(&mut slot, recv, b"test", 1);
            assert!(!slot.is_null(), "the site must be primed");
            let word = receiver_word((re as *const u64).read());
            let entry = (*slot)
                .entries
                .iter()
                .find(|entry| entry.word == word)
                .expect("receiver's inherited entry");
            assert_eq!(
                entry.code,
                crate::object::regex_proto_thunks::regex_proto_test_direct as *const () as u64,
                "the fused entry must actually use the specialized body"
            );
            let (_, code) = memo_hit(&mut slot, recv.to_bits()).expect("split lookup must hit");
            assert_eq!(code,
            crate::object::regex_proto_thunks::regex_proto_test_thunk as *const () as u64,
            "arguments may change exec after lookup, so the split call must use the generic body");
        })
        .unwrap()
        .join()
        .unwrap();
}
