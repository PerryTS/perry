//! Regression coverage for #8690: stable packed Array and Array-subclass
//! counted loops get a fallback-free fast copy and resume the generic copy at
//! the current index whenever a mutation invalidates the admission proof.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn compile(dir: &Path, source: &str, retain_artifacts: bool) -> (PathBuf, String) {
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
    if retain_artifacts {
        cmd.env("PERRY_LLVM_KEEP_IR", "1")
            .env("PERRY_NATIVE_REPS", "1")
            .env("PERRY_NATIVE_REPS_DIR", dir.join("native-reps"));
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

fn run(bin: &Path, dir: &Path, moving_gc: bool) -> Output {
    let mut command = Command::new(bin);
    command.current_dir(dir);
    if moving_gc {
        command
            .env("PERRY_GC_FORCE_EVACUATE", "1")
            .env("PERRY_GC_VERIFY_EVACUATION", "1");
    }
    command.output().expect("run compiled binary")
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "compiled binary failed\nstatus: {:?}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
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
fn read_only_loops_have_preheader_proofs_and_fallback_free_fast_blocks() {
    let dir = tempfile::tempdir().expect("tempdir");
    let source = r#"
class Query extends Array {}
class Archetype extends Array {}

const entityCount = 10_000;
const iterations = 2_000;
const query = new Query();
const archetype = new Archetype();
for (let i = 0; i < entityCount; i++) archetype.push(i);
query.push(archetype);

function scan(): number {
  let sum = 0;
  for (let i = 0, length = query.length; i < length; i++) {
    const current = query[i];
    for (let j = 0, length = current.length; j < length; j++) {
      sum = (sum + current[j]) | 0;
    }
  }
  return sum;
}

let checksum = 0;
for (let i = 0; i < iterations; i++) checksum ^= scan();
console.log(checksum);
"#;
    let (bin, stderr) = compile(dir.path(), source, true);
    let output = run(&bin, dir.path(), false);
    assert_success(&output);
    assert_eq!(String::from_utf8_lossy(&output.stdout), "0\n");

    let ll_path = stderr
        .lines()
        .find_map(|line| line.split("kept LLVM IR: ").nth(1))
        .map(str::trim)
        .map(PathBuf::from)
        .unwrap_or_else(|| panic!("PERRY_LLVM_KEEP_IR did not report an IR path\n{stderr}"));
    let ir = std::fs::read_to_string(&ll_path).expect("read kept LLVM IR");
    let read_only = function_ir(&ir, "__scan(");
    assert_eq!(
        read_only
            .matches("call i32 @js_packed_arraylike_loop_guard(")
            .count(),
        3,
        "the outer fast/slow copies each own a preheader-versioned inner loop"
    );
    assert!(read_only.contains("stable_packed.loop.fast.preheader"));
    assert!(read_only.contains("stable_packed.loop.slow.preheader"));
    assert!(read_only.contains("for.stable_packed_fast"));
    assert!(
        !read_only.contains("stable_packed.iteration.fast"),
        "the admitted call-free clone must not repeat header revalidation"
    );

    let fast_blocks = named_blocks(read_only, &["stable_packed", "for.stable_packed_fast"]);
    assert!(
        fast_blocks.contains("load double"),
        "the fast copy must contain a private direct element load\n{fast_blocks}"
    );
    assert!(
        !fast_blocks.contains("js_object_get_index_polymorphic")
            && !fast_blocks.contains("js_packed_arraylike_index_get"),
        "the fast copy must not contain an indexed-read runtime fallback\n{fast_blocks}"
    );
    for forbidden in [
        "js_dynamic_string_or_number_add",
        "js_number_coerce",
        "js_gc_loop_safepoint",
    ] {
        assert!(
            !fast_blocks.contains(forbidden),
            "the call-free fast copy must not contain `{forbidden}`\n{fast_blocks}"
        );
    }

    let artifact_dir = dir.path().join("native-reps");
    let artifact_text = std::fs::read_dir(&artifact_dir)
        .expect("read native-reps directory")
        .map(|entry| {
            let path = entry.expect("native-reps entry").path();
            std::fs::read_to_string(path).expect("read native-reps artifact")
        })
        .collect::<String>();
    for required in [
        "proof=preheader_scalar_layout",
        "revalidation=none_call_free_clone",
        "side_exit=current_index",
        "loop_versioning=stable_packed_arraylike_fallback",
    ] {
        assert!(
            artifact_text.contains(required),
            "native-region artifact must identify `{required}`"
        );
    }
}

#[test]
fn mutations_and_moving_gc_resume_with_generic_semantics() {
    let dir = tempfile::tempdir().expect("tempdir");
    let source = r#"
class Dense extends Array {}

function live(a: any, mutate: (value: any) => void): string {
  let out = "";
  for (let i = 0; i < a.length; i++) {
    const value = a[i];
    if (i === 0) mutate(a);
    out += String(value) + ";";
  }
  return out;
}

function snapshot(a: any, mutate: (value: any) => void): string {
  const length = a.length;
  let out = "";
  for (let i = 0; i < length; i++) {
    const value = a[i];
    if (i === 0) mutate(a);
    out += String(value) + ";";
  }
  return out;
}

function noChange(_value: any): void {}

function packedSum(a: any): number {
  const length = a.length;
  let sum = 0;
  for (let i = 0; i < length; i++) sum = (sum + a[i]) | 0;
  return sum;
}

// A declared Number is not a runtime proof: callers may still provide a
// String or BigInt through `any`. This exercises the assignment-side `| 0`
// lowering used by the numeric clone without letting it skip ToNumber or the
// required mixed-BigInt TypeError.
function declaredToInt32(value: number): number {
  let result = 0;
  result = value | 0;
  return result;
}

function breakAfterEffect(a: any): string {
  let out = "";
  for (let i = 0; i < a.length; i++) {
    const value = a[i];
    out += String(value);
    break;
  }
  return out;
}

console.log("break=" + breakAfterEffect([31, 32]));
console.log("declared-string=" + declaredToInt32("7" as any));
try { declaredToInt32(1n as any); console.log("declared-bigint=no-throw"); }
catch (_error) { console.log("declared-bigint=throw"); }

const grow: any[] = [1, 2];
console.log("grow=" + live(grow, (a: any) => a.push(3)));

const shrink: any[] = [1, 2, 3];
console.log("shrink=" + live(shrink, (a: any) => { a.length = 1; }));

const fixed: any[] = [1, 2];
console.log("snapshot=" + snapshot(fixed, (a: any) => a.push(3)));

const mixed: any[] = [1, 2, 3];
console.log("kind=" + live(mixed, (a: any) => { a[1] = "mixed"; }));

const aliased: any[] = [4, 5, 6];
const alias: any = aliased;
console.log("alias=" + live(aliased, (_a: any) => { alias.pop(); }));

const described: any[] = [7, 8, 9];
Object.defineProperty(described, "1", { get() { return 41; }, configurable: true });
console.log("descriptor=" + live(described, noChange));

const hole: any = new Dense();
hole.push(10); hole.push(11); hole.push(12);
delete hole[1];
const replacement: any = {};
Object.defineProperty(replacement, "1", { get() { return 77; }, configurable: true });
Object.setPrototypeOf(hole, replacement);
console.log("hole-prototype=" + live(hole, noChange));

const dense: any = new Dense();
dense.push(13); dense.push(14); dense.push(15);
const proxied: any = new Proxy(dense, {
  get(target: any, key: any) {
    if (String(key) === "1") return 88;
    return target[key];
  }
});
console.log("proxy=" + live(proxied, noChange));

// Establish the call-free clone's preheader proof, then mutate between calls.
// These cases exercise proof retirement itself; the callback cases above
// deliberately stay on the mutation-capable generic clone.
const betweenKind: any = new Dense();
betweenKind.push(1); betweenKind.push(2); betweenKind.push(3);
console.log("between-kind-before=" + packedSum(betweenKind));
betweenKind[1] = "9";
console.log("between-kind-after=" + packedSum(betweenKind));

const betweenHole: any = new Dense();
betweenHole.push(2); betweenHole.push(3); betweenHole.push(4);
console.log("between-hole-before=" + packedSum(betweenHole));
delete betweenHole[1];
const betweenPrototype: any = {};
Object.defineProperty(betweenPrototype, "1", { get() { return 50; }, configurable: true });
Object.setPrototypeOf(betweenHole, betweenPrototype);
console.log("between-hole-after=" + packedSum(betweenHole));

const betweenDescriptor: any = new Dense();
betweenDescriptor.push(5); betweenDescriptor.push(6); betweenDescriptor.push(7);
console.log("between-descriptor-before=" + packedSum(betweenDescriptor));
Object.defineProperty(betweenDescriptor, "1", { get() { return 40; }, configurable: true });
console.log("between-descriptor-after=" + packedSum(betweenDescriptor));

const betweenSize: any = new Dense();
betweenSize.push(8); betweenSize.push(9);
console.log("between-grow-before=" + packedSum(betweenSize));
betweenSize.push(10);
console.log("between-grow-after=" + packedSum(betweenSize));
betweenSize.length = 1;
console.log("between-shrink-after=" + packedSum(betweenSize));

const betweenProxyTarget: any = new Dense();
betweenProxyTarget.push(11); betweenProxyTarget.push(12);
console.log("between-proxy-before=" + packedSum(betweenProxyTarget));
const betweenProxy: any = new Proxy(betweenProxyTarget, {
  get(target: any, key: any) {
    if (String(key) === "1") return 60;
    return target[key];
  }
});
console.log("between-proxy-after=" + packedSum(betweenProxy));

const betweenThrow: any = new Dense();
betweenThrow.push(13); betweenThrow.push(14);
console.log("between-throw-before=" + packedSum(betweenThrow));
Object.defineProperty(betweenThrow, "1", { get() { throw new Error("between-getter"); } });
try { packedSum(betweenThrow); }
catch (error) { console.log("between-throw-after=" + error.message); }

const throwing: any[] = [16, 17];
Object.defineProperty(throwing, "0", { get() { throw new Error("getter"); } });
try { live(throwing, noChange); } catch (error) { console.log("thrown-getter=" + error.message); }
try { live([18, 19], (_a: any) => { throw new Error("callback"); }); }
catch (error) { console.log("thrown-callback=" + error.message); }

const moved: any = new Dense();
moved.push(20); moved.push(21); moved.push(22);
console.log("moving-proof-before=" + packedSum(moved));
console.log("moving=" + live(moved, (_a: any) => {
  for (let i = 0; i < 2000; i++) { const garbage = [i, i + 1, i + 2]; }
  gc();
}));
console.log("moving-proof-after=" + packedSum(moved));
"#;
    let (bin, _stderr) = compile(dir.path(), source, false);
    let expected = "break=31\n\
                    declared-string=7\n\
                    declared-bigint=throw\n\
                    grow=1;2;3;\n\
                    shrink=1;\n\
                    snapshot=1;2;\n\
                    kind=1;mixed;3;\n\
                    alias=4;5;\n\
                    descriptor=7;41;9;\n\
                    hole-prototype=10;77;12;\n\
                    proxy=13;88;15;\n\
                    between-kind-before=6\n\
                    between-kind-after=22\n\
                    between-hole-before=9\n\
                    between-hole-after=56\n\
                    between-descriptor-before=18\n\
                    between-descriptor-after=52\n\
                    between-grow-before=17\n\
                    between-grow-after=27\n\
                    between-shrink-after=8\n\
                    between-proxy-before=23\n\
                    between-proxy-after=71\n\
                    between-throw-before=27\n\
                    between-throw-after=between-getter\n\
                    thrown-getter=getter\n\
                    thrown-callback=callback\n\
                    moving-proof-before=63\n\
                    moving=20;21;22;\n\
                    moving-proof-after=63\n";
    for moving_gc in [false, true] {
        let output = run(&bin, dir.path(), moving_gc);
        assert_success(&output);
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            expected,
            "semantic output diverged with moving_gc={moving_gc}"
        );
    }
}
