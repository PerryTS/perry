//! Regression for #10545: a BigInt literal wider than 64 bits lost its high
//! bits whenever its module was compiled as more than one codegen unit.
//!
//! Split modules are the only default user of the in-process IR reader
//! (`perry_codegen::native_emit::native_units_mode`), and every gap/parity
//! fixture is a single unit, so the gap suite cannot see this class. Large real
//! modules split on their own; `PERRY_CODEGEN_UNITS=2` forces it here.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn runtime_dir() -> PathBuf {
    std::env::var_os("PERRY_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            perry_bin()
                .parent()
                .expect("compiler directory")
                .to_path_buf()
        })
}

const SOURCE: &str = r#"
function f() { const x = 2n ** 70n; return x === 1180591620717411303424n; }
function g() { return 1180591620717411303424n; }
console.log(f(), g() === 2n ** 70n, String(g()));
const small = 12345678901234567890n;
const neg = -98765432109876543210987654321n;
console.log(String(small), String(neg), 2n ** 64n === 18446744073709551616n);
const edges = [
  170141183460469231731687303715884105727n,
  -170141183460469231731687303715884105727n,
  0x1ffffffffffffffffn,
  -0x10000000000000000n,
];
console.log(edges.map(String).join(" "), edges[0] === 2n ** 127n - 1n);
const p = 0xfffffffffffffffffffffffffffffffffffffffffffffffffffffffefffffc2fn;
console.log(String(p % 1000000007n), p === 2n ** 256n - 2n ** 32n - 977n);
"#;

/// `node --experimental-strip-types` on the pinned oracle (26.5.1).
const EXPECTED: &str = "true true 1180591620717411303424
12345678901234567890 -98765432109876543210987654321 true
170141183460469231731687303715884105727 -170141183460469231731687303715884105727 36893488147419103231 -18446744073709551616 true
497877021 true
";

#[test]
fn wide_bigint_literals_survive_a_split_module() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    let output = dir.path().join("main_bin");
    let ll_dir = dir.path().join("ll");
    std::fs::create_dir(&ll_dir).expect("create IR dump dir");
    std::fs::write(&entry, SOURCE).expect("write entry");

    let compile = Command::new(perry_bin())
        .current_dir(dir.path())
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .arg("--no-cache")
        .env("PERRY_NO_AUTO_OPTIMIZE", "1")
        .env("PERRY_CODEGEN_UNITS", "2")
        .env_remove("PERRY_LLVM_INPROCESS")
        .env("PERRY_SAVE_LL", &ll_dir)
        .env("PERRY_RUNTIME_DIR", runtime_dir())
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );

    // The subject must be live: a build that stopped splitting, or stopped
    // routing split units through native construction, would pass without
    // testing the reader at all.
    for unit in 0..2 {
        let path = ll_dir.join(format!("main_ts.unit{unit}.native.ll"));
        assert!(
            path.exists(),
            "expected natively constructed unit {} at {}; the module was not split \
             through the in-process reader",
            unit,
            path.display()
        );
    }

    let run = Command::new(&output)
        .current_dir(dir.path())
        .output()
        .expect("run compiled binary");
    assert!(
        run.status.success(),
        "compiled binary failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), EXPECTED);
}
