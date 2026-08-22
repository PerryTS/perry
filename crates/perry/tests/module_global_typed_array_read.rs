//! Typed-array element reads through a MODULE-GLOBAL binding are inlined to a
//! guarded native load, not a per-element `js_typed_array_get` runtime call.
//!
//! The inline checked-f64 load (`expr/ta_param_f64_read.rs`) previously fired
//! only for typed-array *locals/params* whose class the function-local type
//! analysis proved. A typed array allocated once at module scope and read
//! inside functions — the common bundled shape — has its class in the
//! module-global proven-type map, not the local one, so every read fell back
//! to `call double @js_typed_array_get` (measured ~2x slower on a dense
//! reduction). The fix consults the module-global proof; the load stays
//! guard-protected, so a wrong/stale proof only misses the runtime kind cache
//! and defers to the safe helper — never a correctness change.
//!
//! This test pins (a) the inline path now fires for a module-global receiver
//! (the emitted IR carries the inline path's cold fallback marker and no
//! `js_typed_array_get` for the read), and (b) it is bit-exact with the
//! runtime getter, including out-of-bounds → `undefined`, by differencing the
//! fast path against `PERRY_TA_PARAM_F64_READ=0`.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

const SOURCE: &str = r#"
let a = new Float64Array(4);
a[0] = 1.5;
a[1] = 2.5;
a[2] = 3.5;
a[3] = 4.5;
function read(i) { return a[i]; }
let s = 0;
for (let k = 0; k < 4; k++) { s = s + read(k); }
console.log("sum:" + s + " oob:" + read(9) + " neg:" + read(-1));
"#;

const EXPECTED: &str = "sum:12 oob:undefined neg:undefined\n";

fn compile(
    dir: &std::path::Path,
    name: &str,
    source: &str,
    fast: bool,
    save_ll: Option<&std::path::Path>,
) {
    let entry = dir.join(format!("{name}.ts"));
    let output = dir.join(name);
    std::fs::write(&entry, source).expect("write entry");
    let mut cmd = Command::new(perry_bin());
    cmd.current_dir(dir)
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .arg("--no-cache");
    if !fast {
        cmd.env("PERRY_DISABLE_BUFFER_FAST_PATH", "1");
    }
    if let Some(ll) = save_ll {
        cmd.arg("--no-link").env("PERRY_SAVE_LL", ll);
    }
    let out = cmd.output().expect("run perry compile");
    assert!(
        out.status.success(),
        "compile (fast={fast}) failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
}

fn run(bin: &std::path::Path, dir: &std::path::Path) -> String {
    let out = Command::new(bin)
        .current_dir(dir)
        .output()
        .expect("run compiled binary");
    assert!(
        out.status.success(),
        "binary failed (exit {:?}):\n{}",
        out.status.code(),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[test]
fn module_global_float64array_read_is_inlined_and_bit_exact() {
    let dir = tempfile::tempdir().expect("tempdir");

    // (a) the inline path fires: emitted IR carries its cold-fallback marker
    //     and does NOT emit a per-read js_typed_array_get.
    let ll_dir = dir.path().join("ll");
    std::fs::create_dir_all(&ll_dir).unwrap();
    compile(dir.path(), "ir_probe", SOURCE, true, Some(&ll_dir));
    let ir: String = std::fs::read_dir(&ll_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("ll"))
        .map(|e| std::fs::read_to_string(e.path()).unwrap_or_default())
        .collect();
    assert!(
        ir.contains("js_typed_array_read_f64"),
        "the inline checked-f64 load (whose guard-miss fallback is \
         js_typed_array_read_f64) must fire for the module-global read"
    );
    // The CALL, not the always-present `declare` line.
    assert!(
        !ir.contains("call double @js_typed_array_get("),
        "no per-read js_typed_array_get CALL should remain for the module-global read"
    );

    // (b) bit-exact: fast path vs the runtime getter produce identical output,
    //     in-bounds and out-of-bounds.
    compile(dir.path(), "fast", SOURCE, true, None);
    compile(dir.path(), "slow", SOURCE, false, None);
    let fast = run(&dir.path().join("fast"), dir.path());
    let slow = run(&dir.path().join("slow"), dir.path());
    assert_eq!(fast, EXPECTED, "fast-path output");
    assert_eq!(
        slow, fast,
        "fast path must be bit-exact with js_typed_array_get"
    );
}

const I32_SOURCE: &str = r#"
let a = new Int32Array(4);
a[0] = 10;
a[1] = 20;
a[2] = 30;
a[3] = 40;
function read(i) { return a[i] | 0; }
let s = 0;
for (let k = 0; k < 4; k++) { s = (s + read(k)) | 0; }
console.log("sum:" + s + " oob:" + read(9));
"#;

const I32_EXPECTED: &str = "sum:100 oob:0\n";

#[test]
fn module_global_int32array_read_is_inlined_and_bit_exact() {
    let dir = tempfile::tempdir().expect("tempdir");

    let ll_dir = dir.path().join("ll32");
    std::fs::create_dir_all(&ll_dir).unwrap();
    compile(dir.path(), "ir32", I32_SOURCE, true, Some(&ll_dir));
    let ir: String = std::fs::read_dir(&ll_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("ll"))
        .map(|e| std::fs::read_to_string(e.path()).unwrap_or_default())
        .collect();
    assert!(
        ir.contains("js_typed_array_read_int32"),
        "the inline checked-i32 load must fire for the module-global Int32Array read"
    );
    assert!(
        !ir.contains("call double @js_typed_array_get("),
        "no per-read js_typed_array_get CALL should remain"
    );

    compile(dir.path(), "fast32", I32_SOURCE, true, None);
    compile(dir.path(), "slow32", I32_SOURCE, false, None);
    let fast = run(&dir.path().join("fast32"), dir.path());
    let slow = run(&dir.path().join("slow32"), dir.path());
    assert_eq!(fast, I32_EXPECTED);
    assert_eq!(
        slow, fast,
        "i32 fast path must be bit-exact with the runtime getter"
    );
}
