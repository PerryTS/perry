use super::*;

fn panic_message(payload: Box<dyn std::any::Any + Send>) -> String {
    match payload.downcast::<String>() {
        Ok(message) => *message,
        Err(payload) => match payload.downcast::<&'static str>() {
            Ok(message) => (*message).to_owned(),
            Err(_) => "non-string panic payload".to_owned(),
        },
    }
}

#[test]
fn stale_forwarded_reference_panic_names_parent_slot_and_coverage() {
    let message = std::thread::spawn(|| {
        let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = CopyingNurseryTestGuard::new(1);
            let _verify = VerifyEvacuationTestGuard::on();
            let _trigger = GcTriggerThresholdTestGuard::suppress_automatic_triggers();

            let child = young_leaf();
            let (_parent, field) = unsafe { alloc_old_test_object(1) };
            unsafe {
                // Deliberate sabotage: publish the old -> young field without
                // its write barrier, so neither the page nor the owner enters
                // the remembered snapshot this minor walks.
                *field = ptr_bits(child);
            }
            js_shadow_slot_set(0, ptr_bits(child));

            let _ = collect_minor_trace(GcTriggerKind::Direct);
            panic!("the sabotaged parent must leave a stale forwarded field");
        }));
        panic_message(failure.expect_err("the evacuation verifier must reject the stale field"))
    })
    .join()
    .expect("worker thread must return the caught verifier panic");

    for field in [
        "parent_type=",
        "parent_space=old_page",
        "slot_index=0",
        "remembered=no",
        "child_type=",
        "minor=",
        "trigger=",
    ] {
        assert!(
            message.contains(field),
            "verifier panic omitted {field:?}: {message}"
        );
    }
}

#[test]
fn evacuation_verifier_pass_line_counts_parents_and_slots() {
    const CHILD_ENV: &str = "PERRY_TEST_VERIFY_PASS_LINE_CHILD";
    let thread = std::thread::current();
    let name = thread.name().expect("libtest must name the test thread");
    if std::env::var(CHILD_ENV).ok().as_deref() == Some(name) {
        let _guard = CopyingNurseryTestGuard::new(1);
        let _verify = VerifyEvacuationTestGuard::on();
        let _trigger = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        js_shadow_slot_set(0, ptr_bits(young_leaf()));
        let trace = collect_minor_trace(GcTriggerKind::Direct);
        assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
        println!("evacuation verifier pass-line child completed");
        return;
    }

    let output = std::process::Command::new(std::env::current_exe().expect("current test binary"))
        .args(["--exact", name, "--nocapture", "--test-threads=1"])
        .env(CHILD_ENV, name)
        .env("PERRY_GC_DIAG", "1")
        .output()
        .expect("launch isolated diagnostic witness");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success() && stdout.contains("evacuation verifier pass-line child completed"),
        "diagnostic witness failed: {}\nstdout:\n{stdout}\nstderr:\n{stderr}",
        output.status
    );

    let lines: Vec<_> = stderr
        .lines()
        .filter(|line| line.starts_with("[gc-verify] minor=") && line.contains(" evacuation_ok "))
        .collect();
    assert_eq!(
        lines.len(),
        1,
        "expected exactly one copied-minor verifier pass line; stderr:\n{stderr}"
    );
    let parents = lines[0]
        .split_whitespace()
        .find_map(|word| word.strip_prefix("parents="))
        .and_then(|value| value.parse::<usize>().ok())
        .expect("pass line must contain a numeric parents count");
    assert!(
        parents > 0,
        "pass line must prove the heap walk ran: {}",
        lines[0]
    );
}
