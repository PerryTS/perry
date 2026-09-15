//! Issue #10281 regression: `--platform bun` export-condition selection.
//!
//! Split out of `resolve/tests.rs` to keep that file under the 2000-line
//! repository limit.

/// Issue #10281 — `--platform bun` must select a package's `bun` export
/// condition. Perry's condition list had no `bun` entry and ranked `node`
/// first, so a package shipping both compiled its node build even when the
/// target was bun. `@opentui/core` is the shape that surfaced it: its two
/// entries load different renderer backends, so OpenCode's terminal interface
/// ran the wrong one and threw.
///
/// The two directions live in ONE test on purpose: the platform is a
/// process-wide flag (a compile is one process with one target), so separate
/// `#[test]` functions would race under the default parallel test harness.
/// The flag is restored before returning.
use super::super::resolve_exports_candidates;
use super::super::subpath_imports::{default_conditions, set_bun_platform};

fn opentui_core_exports() -> serde_json::Value {
    serde_json::json!({
        ".": {
            "types": "./index.d.ts",
            "bun": "./index.bun.js",
            "node": "./index.node.js",
            "import": "./index.node.js"
        }
    })
}

#[test]
fn bun_platform_prefers_the_bun_entry_and_node_target_is_unchanged() {
    // Other resolver tests also consult this process-wide flag. Exercise the
    // two targets in an isolated test process so parallel tests cannot observe
    // the temporary Bun setting, and assert that the selected test ran.
    const CHILD: &str = "PERRY_TEST_BUN_CONDITIONS_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let module = module_path!().split_once("::").unwrap().1;
        let name =
            format!("{module}::bun_platform_prefers_the_bun_entry_and_node_target_is_unchanged");
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", &name, "--test-threads=1", "--nocapture"])
            .env(CHILD, "1")
            .output()
            .expect("run isolated Bun condition test");
        assert!(
            child.status.success() && String::from_utf8_lossy(&child.stdout).contains("1 passed;"),
            "isolated condition test failed or did not run\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&child.stdout),
            String::from_utf8_lossy(&child.stderr)
        );
        return;
    }
    // Default (node target): the node entry wins, exactly as before.
    set_bun_platform(false);
    assert_eq!(
        default_conditions().first().copied(),
        Some("perry"),
        "an explicit perry entry must keep winning on either target"
    );
    assert!(
        !default_conditions().contains(&"bun"),
        "the node target must not consider the bun condition"
    );
    let node_first = resolve_exports_candidates(&opentui_core_exports(), ".");
    assert_eq!(
        node_first.first().map(String::as_str),
        Some("./index.node.js"),
        "node target must resolve the node entry; got {node_first:?}"
    );

    // `--platform bun`: the bun entry wins, and `perry` still outranks it.
    set_bun_platform(true);
    let conditions = default_conditions();
    assert_eq!(
        (conditions.first().copied(), conditions.get(1).copied()),
        (Some("perry"), Some("bun")),
        "bun must rank directly after perry, above node; got {conditions:?}"
    );
    let bun_first = resolve_exports_candidates(&opentui_core_exports(), ".");
    assert_eq!(
        bun_first.first().map(String::as_str),
        Some("./index.bun.js"),
        "bun target must resolve the bun entry; got {bun_first:?}"
    );
    // The node entry stays available as a fallback for the disk-existence
    // walk in `resolve_package_entry`, it is only no longer first.
    assert!(
        bun_first.iter().any(|c| c == "./index.node.js"),
        "the node entry must remain a candidate; got {bun_first:?}"
    );

    // A package with no `bun` entry is unaffected by the target.
    let plain = serde_json::json!({ ".": { "node": "./n.js", "default": "./d.js" } });
    assert_eq!(
        resolve_exports_candidates(&plain, ".")
            .first()
            .map(String::as_str),
        Some("./n.js"),
        "a package without a bun entry must resolve identically on either target"
    );

    set_bun_platform(false);
}
