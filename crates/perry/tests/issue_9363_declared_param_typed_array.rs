//! #9363/#5525: a DECLARED typed-array parameter earns the inline checked
//! element load, and a declaration that lies is still answered correctly.
//!
//! `receiver_class_name` answers only from `proven_local_types`, which is
//! runtime-derived and therefore empty for a parameter — its value arrives
//! from outside the body. So the shape this machinery exists for was the one
//! shape it never served: bcryptjs's
//! `_encipher(lr, off, P: Int32Array, S: Int32Array)` does ~600M `S[i]` reads
//! through parameters and emitted a `js_typed_array_get` CALL for every one,
//! while the identical loop over a module-global receiver took the inline
//! load (measured: 0 `ctaf.get` blocks vs 66).
//!
//! The fix reads the DECLARED type through `local_type_hint`, the audited
//! escape hatch for "sites whose independent representation proof or runtime
//! guard validates the current value". That is exactly this site: the emitted
//! guard re-derives the truth from `PERRY_TA_KIND_CACHE`, so a wrong
//! declaration misses the cache and defers to the memory-safe helper. A lying
//! annotation therefore costs a missed speedup, never a wrong answer — which
//! is what `wrong_declared_type_still_reads_correctly` pins, because that is
//! the only claim holding the optimism up.
//!
//! Measured on the u8 twin (`buf_ctx` fixture, SIZE=1e6 x 50): a
//! `Uint8Array` parameter receiver 576 -> 235 ms. On
//! `bench_typed_array_untyped_access` the same change fires (0 -> 66 blocks)
//! but is flat, because that benchmark's cost is its accumulator's dynamic
//! add and rooting, not its reads (#9361).

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn compile(dir: &Path, source: &str) -> (PathBuf, String) {
    let entry = dir.join("main.ts");
    let output = dir.join("main_bin");
    std::fs::write(&entry, source).expect("write entry");
    let compile = Command::new(perry_bin())
        .current_dir(dir)
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
        "perry compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );
    (
        output,
        String::from_utf8_lossy(&compile.stderr).into_owned(),
    )
}

fn kept_ir(stderr: &str) -> String {
    let path = stderr
        .lines()
        .find_map(|line| line.split("kept LLVM IR: ").nth(1))
        .map(str::trim)
        .map(PathBuf::from)
        .unwrap_or_else(|| panic!("PERRY_LLVM_KEEP_IR did not report an IR path\n{stderr}"));
    std::fs::read_to_string(path).expect("read kept LLVM IR")
}

fn function_body(ir: &str, suffix: &str) -> String {
    let mut out = String::new();
    let mut inside = false;
    for line in ir.lines() {
        if line.starts_with("define ") {
            inside = line.contains(&format!("__{suffix}("));
        } else if inside {
            if line.starts_with('}') {
                break;
            }
            out.push_str(line);
            out.push('\n');
        }
    }
    assert!(!out.is_empty(), "no emitted body found for `{suffix}`");
    out
}

fn run(bin: &Path, dir: &Path, gc_stress: bool) -> Output {
    let mut command = Command::new(bin);
    command.current_dir(dir);
    if gc_stress {
        command
            .env("PERRY_GC_HEAP_LIMIT", "8")
            .env("PERRY_GC_FORCE_EVACUATE", "1");
    }
    command.output().expect("run compiled binary")
}

/// A declared `Int32Array` parameter takes the inline checked load, matching
/// the module-global receiver that already did. The module-global body is the
/// control: asserting it first means a regression that disables BOTH lanes
/// cannot pass this test by making the comparison vacuous.
#[test]
fn declared_typed_array_param_earns_the_inline_load() {
    const SOURCE: &str = r#"
const S = new Int32Array(256);
for (let i = 0; i < 256; i++) S[i] = (i * 2654435761) | 0;

function viaParam(a: Int32Array): number {
    let n = 0;
    for (let i = 0; i < 256; i++) n += a[i & 255];
    return n;
}

function viaGlobal(): number {
    let n = 0;
    for (let i = 0; i < 256; i++) n += S[i & 255];
    return n;
}

console.log(viaParam(S) + "," + viaGlobal());
"#;
    let dir = tempfile::tempdir().expect("tempdir");
    let (bin, stderr) = compile(dir.path(), SOURCE);
    let ir = kept_ir(&stderr);

    let global = function_body(&ir, "viaGlobal");
    assert!(
        global.contains("ctaf.get"),
        "control: the module-global receiver must take the inline checked \
         load — if it does not, this test can no longer distinguish anything"
    );

    let param = function_body(&ir, "viaParam");
    assert!(
        param.contains("ctaf.get"),
        "a declared `Int32Array` PARAMETER must take the same inline checked \
         load as the module-global receiver; without it every element read is \
         a `js_typed_array_get` call"
    );

    let out = run(&bin, dir.path(), false);
    assert!(out.status.success(), "binary failed");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let (a, b) = stdout.trim().split_once(',').expect("two sums");
    assert_eq!(a, b, "param and global receivers must sum identically");
}

/// **The claim the optimism rests on.** A declaration is not a lifetime
/// proof, so the lane is only sound because the emitted guard re-derives the
/// truth. Hand a function declared to take an `Int32Array` something that is
/// not one and the answers must still match node exactly — the guard misses
/// `PERRY_TA_KIND_CACHE` and defers to the memory-safe helper.
#[test]
fn wrong_declared_type_still_reads_correctly() {
    const LYING: &str = r#"
function sum3(a: Int32Array): string {
    let out = "";
    for (let i = 0; i < 3; i++) out += String(a[i]) + "|";
    return out;
}

const real = new Int32Array([10, 20, 30]);
const plainArray: any = [40, 50, 60];
const plainObject: any = { 0: 70, 1: 80, 2: 90 };
const notIndexable: any = 12345;
const shortArray: any = new Int32Array([1]);

console.log("real:" + sum3(real));
console.log("array:" + sum3(plainArray as Int32Array));
console.log("object:" + sum3(plainObject as Int32Array));
console.log("scalar:" + sum3(notIndexable as Int32Array));
console.log("short:" + sum3(shortArray as Int32Array));
"#;
    let dir = tempfile::tempdir().expect("tempdir");
    let (bin, _stderr) = compile(dir.path(), LYING);

    // Node is the oracle: whatever it prints for each lying receiver is the
    // answer the guard must reproduce, including `undefined` for the reads
    // that fall off the end or off a non-indexable value.
    let node = Command::new("node")
        .current_dir(dir.path())
        .arg("--experimental-strip-types")
        .arg(dir.path().join("main.ts"))
        .output()
        .expect("run node");
    assert!(
        node.status.success(),
        "node failed on the oracle fixture:\n{}",
        String::from_utf8_lossy(&node.stderr)
    );

    for stress in [false, true] {
        let out = run(&bin, dir.path(), stress);
        assert!(out.status.success(), "binary failed (gc_stress={stress})");
        assert_eq!(
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&node.stdout),
            "a lying `Int32Array` annotation changed the ANSWER (gc_stress={stress}) \
             — the declared-type hint must only ever cost a missed speedup"
        );
    }
}

/// A reassigned parameter is excluded, matching `receiver_class_name`'s #6906
/// rule: a later write can replace the binding with anything, so the
/// declaration describes at most its first value.
#[test]
fn reassigned_param_is_not_admitted() {
    const REASSIGNED: &str = r#"
function f(a: Int32Array, swap: boolean): number {
    if (swap) a = new Int32Array([7, 7, 7]) as Int32Array;
    let n = 0;
    for (let i = 0; i < 3; i++) n += a[i];
    return n;
}
const base = new Int32Array([1, 2, 3]);
console.log(f(base, false) + "," + f(base, true));
"#;
    let dir = tempfile::tempdir().expect("tempdir");
    let (bin, _stderr) = compile(dir.path(), REASSIGNED);
    let out = run(&bin, dir.path(), false);
    assert!(out.status.success(), "binary failed");
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "6,21\n",
        "a reassigned parameter must still read the value it actually holds"
    );
}
