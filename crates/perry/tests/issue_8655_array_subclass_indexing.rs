//! Regression coverage for #8655. Numeric indexing on an object-backed
//! `class X extends Array` used to stringify every index and perform a generic
//! property lookup inside the Wolf ECS inner loop, leaving the native binary
//! 191x behind Node in the issue report.
//!
//! The runtime now caches the exact dense property-slot layout by class and
//! semantic ShapeId. These tests pin both halves of the contract: emitted hot
//! loops call the guarded packed-arraylike helper instead of
//! `js_dyn_index_get`, and every shape the guard must reject keeps ordinary JS
//! semantics.

use std::path::{Path, PathBuf};
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn compile(dir: &Path, source: &str, keep_ir: bool) -> (PathBuf, String) {
    let entry = dir.join("main.ts");
    let output = dir.join("main_bin");
    std::fs::write(&entry, source).expect("write entry");

    let mut cmd = Command::new(perry_bin());
    cmd.current_dir(dir)
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .env("PERRY_NO_CACHE", "1");
    if keep_ir {
        cmd.env("PERRY_LLVM_KEEP_IR", "1");
    }
    let compile = cmd.output().expect("run perry compile");
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

fn issue_repro_source() -> &'static str {
    r#"
class Query extends Array { archetypes = this; }
class Archetype extends Array { entities = this; }

const query = new Query();
const archetype = new Archetype();
for (let i = 0; i < 1000; i++) archetype.push(i);
query.push(archetype);
const values = new Uint32Array(1000);

function system(values: Uint32Array) {
  for (let i = 0; i < query.length; i++) {
    const current = query[i];
    for (let j = 0; j < current.length; j++) values[current[j]] += 1;
  }
}

for (let i = 0; i < 4; i++) system(values);
console.log(values[0] + "," + values[999]);
"#
}

fn function_ir<'a>(ir: &'a str, function_fragment: &str) -> &'a str {
    let start = ir
        .find(function_fragment)
        .unwrap_or_else(|| panic!("missing function `{function_fragment}` in emitted IR"));
    let body_start = ir[..start]
        .rfind("\ndefine ")
        .unwrap_or_else(|| panic!("missing definition before `{function_fragment}`"));
    let tail = &ir[body_start + 1..];
    let end = tail
        .find("\n}\n")
        .unwrap_or_else(|| panic!("unterminated definition for `{function_fragment}`"));
    &tail[..end + 2]
}

fn named_blocks(function: &str, prefixes: &[&str]) -> String {
    let mut selected = false;
    let mut result = String::new();
    for line in function.lines() {
        if !line.starts_with([' ', '\t']) && line.contains(':') {
            selected = prefixes.iter().any(|prefix| line.contains(prefix));
        }
        if selected {
            result.push_str(line);
            result.push('\n');
        }
    }
    result
}

#[test]
fn wolf_ecs_loop_has_a_fallback_free_versioned_fast_copy() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (_bin, stderr) = compile(dir.path(), issue_repro_source(), true);
    let ll_path = stderr
        .lines()
        .find_map(|line| line.split("kept LLVM IR: ").nth(1))
        .map(str::trim)
        .map(PathBuf::from)
        .unwrap_or_else(|| panic!("PERRY_LLVM_KEEP_IR did not report an IR path\n{stderr}"));
    let ir = std::fs::read_to_string(&ll_path).expect("read kept LLVM IR");
    let _ = std::fs::remove_file(&ll_path);
    let system = function_ir(&ir, "__system(double");

    assert!(
        system.contains("call i32 @js_packed_arraylike_loop_guard("),
        "the original #8655 fixture must enter through the loop preheader proof"
    );
    assert!(system.contains("stable_packed.loop.fast.preheader"));
    assert!(
        !system.contains("stable_packed.iteration.fast"),
        "the call-free clone must not repeat its complete preheader proof per iteration"
    );
    assert!(system.contains("stable_packed.loop.slow.preheader"));
    let fast_blocks = named_blocks(system, &["stable_packed", "for.stable_packed_fast"]);
    assert!(
        !fast_blocks.contains("js_object_get_index_polymorphic")
            && !fast_blocks.contains("js_packed_arraylike_index_get"),
        "the original Wolf ECS fast copy must use private direct loads\n{fast_blocks}"
    );
    assert!(
        system.contains("call double @js_packed_arraylike_index_get("),
        "the explicit generic loop copy must retain the guarded semantic fallback"
    );
    assert!(
        !system.contains("call double @js_dyn_index_get("),
        "the Wolf ECS hot loop must not call the generic dynamic index dispatcher"
    );
    assert!(
        !system.contains("call i64 @js_string_from_bytes("),
        "the hot loop must not construct numeric or length property keys"
    );
}

#[test]
fn guarded_arraylike_reads_preserve_side_exit_semantics() {
    let dir = tempfile::tempdir().expect("tempdir");
    let source = r#"
class Dense extends Array {}

function readParam(a: any): string {
  let out = "";
  for (let i = 0; i < a.length; i++) out += String(a[i]) + ";";
  return out;
}

const captured: any = new Dense();
captured.push(10); captured.push(20); captured.push(30);
function readCaptured(): string {
  let out = "";
  for (let i = 0; i < captured.length; i++) out += String(captured[i]) + ";";
  return out;
}
console.log("dense=" + readCaptured() + "|" + readParam(captured));

// Installing an own accessor after warming the exact old shape must retire
// its cached layout and invoke the getter.
Object.defineProperty(captured, "1", { get() { return 41; }, configurable: true });
console.log("descriptor=" + readCaptured());

// A hole must fall through an indexed custom prototype accessor.
const hole: any = new Dense();
hole.push(1); hole.push(2); hole.push(3);
delete hole[1];
const proto: any = {};
Object.defineProperty(proto, "1", { get() { return 77; }, configurable: true });
Object.setPrototypeOf(hole, proto);
console.log("hole-proto=" + readParam(hole));

// A Proxy must remain wholly observable, including numeric get traps.
const proxied: any = new Proxy(captured, {
  get(target: any, key: any) {
    if (String(key) === "2") return 99;
    return target[key];
  }
});
console.log("proxy=" + readParam(proxied));

// The same unknown-receiver helper also sees real Arrays. A transition from
// packed numeric to mixed elements must read the live boxed value.
const ordinary: any[] = [4, 5, 6];
console.log("array-before=" + readParam(ordinary));
ordinary[1] = "mixed";
console.log("array-after=" + readParam(ordinary));
"#;
    let (bin, _stderr) = compile(dir.path(), source, false);
    let run = Command::new(&bin)
        .current_dir(dir.path())
        .output()
        .expect("run compiled binary");
    assert!(
        run.status.success(),
        "compiled binary failed\nstatus: {:?}\nstdout:\n{}\nstderr:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "dense=10;20;30;|10;20;30;\n\
         descriptor=10;41;30;\n\
         hole-proto=1;77;3;\n\
         proxy=10;41;99;\n\
         array-before=4;5;6;\n\
         array-after=4;mixed;6;\n"
    );
}
