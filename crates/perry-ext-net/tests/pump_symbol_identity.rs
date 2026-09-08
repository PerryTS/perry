//! Keep the optimizer regression visible to the diff-scoped integration gate.
//! The probe compiles the actual adapter extracted from dispatch.rs, not a
//! separately maintained copy. Each rustc invocation has a 30-second deadline.

use std::path::Path;
use std::process::Command;

#[test]
fn optimized_network_pump_keeps_its_private_callback() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let output = Command::new("node")
        .arg(root.join("scripts/test-net-pump-symbol-identity.mjs"))
        .current_dir(root)
        .output()
        .expect("Node must be installed to run the bounded rustc optimizer probe");
    assert!(
        output.status.success(),
        "optimizer regression failed: {}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert_eq!(
        stdout
            .lines()
            .filter(|line| line.starts_with("PASS:"))
            .count(),
        7
    );
}
