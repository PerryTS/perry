//! The strided constant-fill kernel (`js_array_fill_range_strided_tagged`).
//!
//! `for (j = A; j < B; j += S) arr[j] = <boolean/null/undefined literal>` — the
//! sieve idiom — paid the inline dense-store guard chain per element for a
//! receiver that cannot change mid-loop. The kernel validates the receiver
//! once and stores in a tight native loop; `11_prime_sieve` went 12 ms ->
//! 5 ms against node's 8 (the sieve nest itself 9 ms -> 1 ms).
//!
//! What these tests pin is every DECLINE route producing node's exact
//! semantics through the generic fallback: a raw-f64 receiver (tag bits would
//! corrupt unboxed-double storage — the generic path downgrades the layout
//! instead), an out-of-range window (node grows the array), and the plain
//! strided-gap behavior, under normal and forced-evacuation runs.

use std::path::PathBuf;
use std::process::{Command, Output};

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

const SOURCE: &str = r#"
const a: any[] = [];
for (let i = 0; i < 20; i++) a[i] = i;
for (let j = 2; j < 20; j = j + 3) a[j] = false;
console.log("gaps:" + JSON.stringify(a.slice(0, 8)));
const b: number[] = [];
for (let i = 0; i < 10; i++) b.push(i * 1.5);
for (let j = 0; j < 10; j = j + 2) (b as any[])[j] = false;
console.log("mixed:" + JSON.stringify(b));
const c: any[] = [];
for (let i = 0; i < 5; i++) c[i] = 1;
for (let j = 3; j < 9; j = j + 2) c[j] = null;
console.log("grow:" + c.length + ":" + JSON.stringify(c));
const N = 30; const S = 4;
const d: any[] = [];
for (let i = 0; i < N; i++) d[i] = true;
for (let j = 1; j < N; j = j + S) d[j] = null;
let t = 0; for (let i = 0; i < N; i++) if (d[i] === null) t++;
console.log("locals:" + t);
"#;

const EXPECTED: &str = "gaps:[0,1,false,3,4,false,6,7]\nmixed:[false,1.5,false,4.5,false,7.5,false,10.5,false,13.5]\ngrow:8:[1,1,1,null,1,null,null,null]\nlocals:8\n";

#[test]
fn strided_fills_and_every_decline_route_match_node() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    let output = dir.path().join("main_bin");
    std::fs::write(&entry, SOURCE).expect("write entry");
    let compile = Command::new(perry_bin())
        .current_dir(dir.path())
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .env("PERRY_NO_CACHE", "1")
        .env("PERRY_LLVM_KEEP_IR", "1")
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stderr)
    );
    // The kernel is actually reached (not vacuously passing via the generic
    // path everywhere).
    let stderr = String::from_utf8_lossy(&compile.stderr);
    let ir_path = stderr
        .lines()
        .find_map(|line| line.split("kept LLVM IR: ").nth(1))
        .map(str::trim)
        .expect("kept IR path");
    let ir = std::fs::read_to_string(ir_path).expect("read IR");
    assert!(
        ir.contains("js_array_fill_range_strided_tagged"),
        "the strided-fill matcher must claim at least one loop"
    );

    for moving_gc in [false, true] {
        let mut command = Command::new(&output);
        command.current_dir(dir.path());
        if moving_gc {
            command
                .env("PERRY_GC_FORCE_EVACUATE", "1")
                .env("PERRY_GC_VERIFY_EVACUATION", "1");
        }
        let run: Output = command.output().expect("run binary");
        assert!(
            run.status.success(),
            "binary failed moving_gc={moving_gc}\nstderr:\n{}",
            String::from_utf8_lossy(&run.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&run.stdout),
            EXPECTED,
            "divergence with moving_gc={moving_gc}"
        );
    }
}
