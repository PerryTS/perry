//! Regression coverage for #8773: immutable closure-captured packed Arrays and
//! Array subclasses, including an inner array derived from the guarded outer
//! indexed read, receive direct-load fast loop versions with generic side exits.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn runtime_dir() -> PathBuf {
    if let Some(target_dir) = std::env::var_os("CARGO_TARGET_DIR") {
        return PathBuf::from(target_dir).join(if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        });
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("target")
        .join(if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        })
}

fn compile(dir: &Path, source: &str, retain_artifacts: bool) -> (PathBuf, String) {
    let entry = dir.join("main.ts");
    let output = dir.join("main_bin");
    std::fs::write(&entry, source).expect("write entry");

    let mut command = Command::new(perry_bin());
    command
        .current_dir(dir)
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .arg("--no-cache")
        .arg("--no-auto-optimize")
        .env("PERRY_RUNTIME_DIR", runtime_dir());
    if retain_artifacts {
        command
            .env("PERRY_LLVM_KEEP_IR", "1")
            .env("PERRY_NATIVE_REPS", "1")
            .env("PERRY_NATIVE_REPS_DIR", dir.join("native-reps"));
    }
    let compiled = command.output().expect("run perry compile");
    assert!(
        compiled.status.success(),
        "perry compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compiled.stdout),
        String::from_utf8_lossy(&compiled.stderr)
    );
    (
        output,
        String::from_utf8_lossy(&compiled.stderr).into_owned(),
    )
}

fn run(binary: &Path, dir: &Path, moving_gc: bool) -> Output {
    let mut command = Command::new(binary);
    command.current_dir(dir);
    if moving_gc {
        command
            .env("PERRY_GC_FORCE_EVACUATE", "1")
            .env("PERRY_GC_VERIFY_EVACUATION", "1");
    }
    command.output().expect("run compiled fixture")
}

fn assert_output(output: &Output, expected: &str, moving_gc: bool) {
    assert!(
        output.status.success(),
        "fixture failed with moving_gc={moving_gc}\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), expected);
}

fn named_blocks(ir: &str, prefixes: &[&str]) -> String {
    let mut selected = false;
    let mut result = String::new();
    for line in ir.lines() {
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
fn nested_closure_capture_uses_live_guards_and_direct_fast_reads() {
    let dir = tempfile::tempdir().expect("tempdir");
    let source = r#"
class Query extends Array {
  archetypes = this;
  ecs = 1;
}
class Archetype extends Array {
  sset = 1;
  entities = this;
  mask = 0;
  change: any[] = [];
}

function setup(entityCount: number) {
  const query = new Query();
  const archetype = new Archetype();
  for (let i = 0; i < entityCount; i++) archetype.push(i);
  query.push(archetype);
  const values = new Uint32Array(entityCount);
  const left = new Uint32Array(entityCount);
  const right = new Uint32Array(entityCount);

  function system() {
    for (let i = 0, length = query.length; i < length; i++) {
      const current = query[i];
      for (let j = 0, length = current.length; j < length; j++) {
        const temp = left[current[j]];
        left[current[j]] = right[current[j]];
        right[current[j]] = temp;
        values[current[j]] += 1;
      }
    }
  }

  return () => {
    system();
    return values[0];
  };
}

const run = setup(1_000);
let checksum = 0;
for (let i = 0; i < 2_000; i++) checksum = run();
console.log(checksum);
"#;
    let (binary, stderr) = compile(dir.path(), source, true);
    for moving_gc in [false, true] {
        assert_output(&run(&binary, dir.path(), moving_gc), "2000\n", moving_gc);
    }

    let ir_path = stderr
        .lines()
        .find_map(|line| line.split("kept LLVM IR: ").nth(1))
        .map(str::trim)
        .map(PathBuf::from)
        .unwrap_or_else(|| panic!("PERRY_LLVM_KEEP_IR did not report an IR path\n{stderr}"));
    let ir = std::fs::read_to_string(ir_path).expect("read kept LLVM IR");
    let artifact_text = std::fs::read_dir(dir.path().join("native-reps"))
        .expect("read native-reps directory")
        .map(|entry| {
            std::fs::read_to_string(entry.expect("native-reps entry").path())
                .expect("read native-reps artifact")
        })
        .collect::<String>();
    let stable_diagnostics = artifact_text
        .lines()
        .filter(|line| {
            line.contains("stable_packed")
                || line.contains("candidate_")
                || line.contains("capture_")
                || line.contains("rejection=")
        })
        .collect::<Vec<_>>()
        .join("\n");
    let fast_preheaders = ir
        .lines()
        .filter(|line| line.starts_with("stable_packed.loop.fast.preheader") && line.ends_with(':'))
        .count();
    assert!(
        fast_preheaders >= 2,
        "both captured outer and nested-derived inner loops need fast versions\n{stable_diagnostics}"
    );
    assert!(ir.contains("stable_packed.iteration.capture_valid"));
    assert!(ir.contains("call i64 @js_packed_arraylike_loop_guard_live("));
    let clean_read_blocks = named_blocks(&ir, &["stable_packed.indexed_read.proof_clean"]);
    let dirty_read_blocks = named_blocks(&ir, &["stable_packed.indexed_read.proof_dirty"]);
    assert!(
        !clean_read_blocks.is_empty()
            && !clean_read_blocks.contains("js_packed_arraylike_loop_revalidate_live"),
        "the clean nested-read path must retain its proof without a runtime call\n{clean_read_blocks}"
    );
    assert!(
        dirty_read_blocks.contains("js_packed_arraylike_loop_revalidate_live"),
        "a path dirtied by a preceding call must retain exact revalidation\n{dirty_read_blocks}"
    );
    let cache_hit_blocks = named_blocks(&ir, &["stable_packed.indexed_read.cache_hit"]);
    assert!(
        !cache_hit_blocks.is_empty()
            && !cache_hit_blocks.contains("js_packed_arraylike_loop_revalidate_live")
            && !cache_hit_blocks.contains("js_packed_arraylike_index_get"),
        "a same-counter cache hit must be a call-free exact-value load\n{cache_hit_blocks}"
    );
    assert!(
        ir.contains("packed_index.generic_fallback")
            && ir.contains("packed_index.revalidated_merge"),
        "a failed nested proof must branch to the exact-source generic read and rejoin"
    );

    let fast_blocks = named_blocks(&ir, &["stable_packed", "for.stable_packed_fast"]);
    assert!(
        fast_blocks.contains("load double"),
        "fast versions must contain direct element loads\n{fast_blocks}"
    );
    assert!(
        !fast_blocks.contains("js_packed_arraylike_index_get")
            && !fast_blocks.contains("js_object_get_index_polymorphic"),
        "fast versions must not retain indexed-read helpers\n{fast_blocks}"
    );
    assert!(
        ir.contains("js_packed_arraylike_index_get"),
        "the unchanged generic fallback must remain in the function"
    );

    for required in [
        "candidate_storage=closure_capture_slot",
        "revalidation=each_iteration_capture_reload",
        "candidate_origin=guarded_outer_index_read",
        "nested_read_miss=generic_read_without_iteration_replay",
        "same_counter_read_cache=call_invalidated",
        "guard_identity=stable_packed_arraylike:",
        "fallback_identity=stable_packed_arraylike:",
    ] {
        assert!(
            artifact_text.contains(required),
            "lowering explanation must identify `{required}`\n{stable_diagnostics}"
        );
    }
}

#[test]
fn captured_negative_shapes_preserve_generic_semantics() {
    let dir = tempfile::tempdir().expect("tempdir");
    let source = r#"
class Query extends Array {}
class Archetype extends Array {}

function make(source: any) {
  const query = source;
  return () => {
    let text = "";
    for (let i = 0, length = query.length; i < length; i++) {
      const current = query[i];
      for (let j = 0, length = current.length; j < length; j++) {
        text += current[j] + ",";
      }
    }
    return text;
  };
}

function mutableCapture() {
  let query: any = [[1, 2]];
  const scan = () => {
    let sum = 0;
    for (let i = 0, length = query.length; i < length; i++) {
      const current = query[i];
      for (let j = 0, length = current.length; j < length; j++) sum += current[j];
    }
    return sum;
  };
  query = [[7, 8]];
  return scan;
}

function prefixedCapture() {
  const query = [0, 1];
  const values = new Uint32Array(2);
  return () => {
    for (let i = 0, length = query.length; i < length; i++) values[query[i]] += 1;
    return values[0] + values[1];
  };
}

const dense: any = new Query();
const row: any = new Archetype();
row.push(1); row.push(2); row.push(3); dense.push(row);
console.log("dense=" + make(dense)());

const hole: any[] = [[4, 5, 6]];
delete hole[0][1];
console.log("hole=" + make(hole)());

const accessor: any[] = [[7, 8]];
Object.defineProperty(accessor[0], "1", { get() { return 41; } });
console.log("accessor=" + make(accessor)());

const proxy = new Proxy([[9, 10]], {
  get(target: any, key: any) { return Reflect.get(target, key); }
});
console.log("proxy=" + make(proxy)());

const resized: any[] = [[11, 12], [13]];
const resizedScan = make(resized);
resized.push([14, 15]);
resized.length = 2;
console.log("resized=" + resizedScan());

const grown: any[] = [[16]];
const grownScan = make(grown);
const grownAlias = grown;
grownAlias.push([17, 18]);
console.log("grown=" + grownScan());

const shrunk: any[] = [[19], [20, 21]];
const shrunkScan = make(shrunk);
shrunk.length = 1;
console.log("shrunk=" + shrunkScan());

console.log("rebound=" + mutableCapture()());
console.log("prefixed=" + prefixedCapture()());

const moved: any[] = [[22, 23, 24]];
const movedScan = make(moved);
gc();
console.log("moved=" + movedScan());

class PrototypeRow extends Array {}
const prototypeQuery: any = new Query();
const prototypeRow: any = new PrototypeRow();
prototypeRow.length = 1;
Object.defineProperty(PrototypeRow.prototype, "0", { get() { return 25; } });
prototypeQuery.push(prototypeRow);
console.log("prototype=" + make(prototypeQuery)());
"#;
    let (binary, _) = compile(dir.path(), source, false);
    let expected = "dense=1,2,3,\n\
                    hole=4,undefined,6,\n\
                    accessor=7,41,\n\
                    proxy=9,10,\n\
                    resized=11,12,13,\n\
                    grown=16,17,18,\n\
                    shrunk=19,\n\
                    rebound=15\n\
                    prefixed=2\n\
                    moved=22,23,24,\n\
                    prototype=25,\n";
    for moving_gc in [false, true] {
        assert_output(&run(&binary, dir.path(), moving_gc), expected, moving_gc);
    }
}

#[test]
fn captured_accessor_exception_remains_observable() {
    let dir = tempfile::tempdir().expect("tempdir");
    let source = r#"
function make() {
  const query: any[] = [[16]];
  Object.defineProperty(query[0], "0", { get() { throw new Error("capture-getter"); } });
  return () => {
    for (let i = 0, length = query.length; i < length; i++) {
      const current = query[i];
      for (let j = 0, length = current.length; j < length; j++) console.log(current[j]);
    }
  };
}
make()();
"#;
    let (binary, _) = compile(dir.path(), source, false);
    for moving_gc in [false, true] {
        let output = run(&binary, dir.path(), moving_gc);
        assert!(
            !output.status.success(),
            "throwing getter unexpectedly succeeded with moving_gc={moving_gc}"
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("capture-getter"),
            "uncaught exception lost getter identity with moving_gc={moving_gc}:\n{stderr}"
        );
    }
}

#[test]
fn nested_read_miss_does_not_replay_prior_effects() {
    let dir = tempfile::tempdir().expect("tempdir");
    let source = r#"
function make() {
  const query: any[] = [[5]];
  const effects: any[] = [0];
  let hits = 0;
  Object.defineProperty(effects, "0", {
    get() {
      hits += 1;
      delete query[0][0];
      return 7;
    }
  });

  return () => {
    let text = "";
    for (let i = 0, length = query.length; i < length; i++) {
      const current = query[i];
      for (let j = 0, length = current.length; j < length; j++) {
        const first = current[j];
        const trigger = effects[j];
        const second = current[j];
        text += first + "|" + trigger + "|" + second + "|" + hits;
      }
    }
    return text;
  };
}

console.log(make()());
"#;
    let (binary, _) = compile(dir.path(), source, false);
    for moving_gc in [false, true] {
        assert_output(
            &run(&binary, dir.path(), moving_gc),
            "5|7|undefined|1\n",
            moving_gc,
        );
    }
}
