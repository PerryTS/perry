//! Regression: net.BlockList.addSubnet must unbox its numeric `prefix` argument.
//!
//! The net-extension dispatch passed `prefix` to `js_net_block_list_add_subnet`
//! (which takes `prefix: f64`) as the RAW nanboxed value. perry stores small
//! integers as a tagged int32 (0x7FFE high bits), so the raw bits are not the
//! real number — the subnet boundary was computed from garbage. A /24 must
//! cover its 256 addresses and nothing beyond.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

#[test]
fn block_list_add_subnet_respects_numeric_prefix() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    let out = dir.path().join("main_bin");
    std::fs::write(
        &entry,
        r#"
import { BlockList } from "net";
const b: any = new BlockList();
b.addSubnet("10.0.0.0", 24, "ipv4");
console.log("in:", b.check("10.0.0.200", "ipv4"));    // inside /24
console.log("out:", b.check("10.0.1.5", "ipv4"));     // outside /24
"#,
    )
    .unwrap();
    let c = Command::new(perry_bin())
        .current_dir(dir.path())
        .args([
            "compile",
            entry.to_str().unwrap(),
            "-o",
            out.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        c.status.success(),
        "compile failed: {}",
        String::from_utf8_lossy(&c.stderr)
    );
    let r = Command::new(&out).current_dir(dir.path()).output().unwrap();
    assert!(
        r.status.success(),
        "run failed: {}",
        String::from_utf8_lossy(&r.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&r.stdout), "in: true\nout: false\n");
}
