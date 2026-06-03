//! Phase D: e2e tests that compile + run real Perry programs against
//! a live OCI runtime. Gated on `PERRY_E2E_TESTS=1`.

use perry_container_e2e::{assert_e2e_pass, e2e_enabled, run_e2e};

#[test]
fn e2e_redis_smoke() {
    if !e2e_enabled() {
        eprintln!("[skipped] PERRY_E2E_TESTS=1 not set");
        return;
    }
    let port = std::process::id().to_string()[..5].parse::<u16>().unwrap_or(57399);
    let port_str = port.to_string();
    let result = run_e2e("redis-smoke", &[("PERRY_E2E_PORT", port_str.as_str())]);
    assert_e2e_pass("redis-smoke", &result);
}

#[test]
fn e2e_forgejo_stack() {
    if !e2e_enabled() {
        eprintln!("[skipped] PERRY_E2E_TESTS=1 not set");
        return;
    }
    if std::env::var("PERRY_E2E_FORGEJO").as_deref() != Ok("1") {
        // Forgejo deploy pulls ~250MB of images; off by default even
        // when PERRY_E2E_TESTS=1 is set. Set PERRY_E2E_FORGEJO=1 to
        // include this in the run.
        eprintln!("[skipped] PERRY_E2E_FORGEJO=1 not set");
        return;
    }
    let result = run_e2e("forgejo-stack", &[]);
    assert_e2e_pass("forgejo-stack", &result);
}
