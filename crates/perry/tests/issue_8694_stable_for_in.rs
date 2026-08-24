//! Semantic and observability ratchet for #8694's stable one-key `for...in`
//! snapshot.  The executable exercises the allocation-free arm first, then
//! cases that must remain on the complete generic enumerator.  It is run again
//! with forced evacuation so the returned shape-owned key array is proven to
//! stay valid across moving collections.

use std::path::PathBuf;
use std::process::{Command, Output};

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn run_fixture(binary: &std::path::Path, force_evacuation: bool) -> Output {
    let mut command = Command::new(binary);
    command.env("PERRY_FOR_IN_DIAG", "1");
    if force_evacuation {
        command.env("PERRY_GC_FORCE_EVACUATE", "1");
    } else {
        command.env_remove("PERRY_GC_FORCE_EVACUATE");
    }
    command.output().expect("run compiled #8694 fixture")
}

fn diagnostic_counts(stderr: &str) -> (u64, u64) {
    let mut stable = 0;
    let mut fallback = 0;
    for line in stderr
        .lines()
        .filter(|line| line.starts_with("FOR-IN-DIAG "))
    {
        for field in line.split_whitespace() {
            let Some((name, value)) = field.split_once('=') else {
                continue;
            };
            match name {
                "stable_single" => stable = stable.max(value.parse().unwrap_or(0)),
                "fallback" => fallback = fallback.max(value.parse().unwrap_or(0)),
                _ => {}
            }
        }
    }
    (stable, fallback)
}

#[test]
fn stable_one_key_for_in_reuses_shape_snapshot_with_generic_fallback() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    let binary = dir.path().join("main_bin");
    std::fs::write(
        &entry,
        r#"
const groups: any = {};
groups[3] = [];

function pushEntity(entity: any) {
  for (const groupHash in groups) groups[groupHash].push(entity);
}
function removeEntity(entity: any) {
  for (const groupHash in groups) {
    const entities = groups[groupHash];
    const index = entities.indexOf(entity);
    if (index !== -1) entities.splice(index, 1);
  }
}

for (let i = 0; i < 2000; i++) {
  const entity = { id: i };
  pushEntity(entity);
  removeEntity(entity);
}
console.log("stable:", groups[3].length);

// The fast path returns the receiver's shape-owned key array.  Adding a key
// in the body must fork it: additions after enumeration starts are not part of
// this snapshot, but the next invocation sees the successor shape.
const mutating: any = { only: 1 };
const first: string[] = [];
for (const key in mutating) {
  first.push(key);
  mutating.added = 2;
  const gc = (globalThis as any).gc;
  if (gc) gc();
}
const second: string[] = [];
for (const key in mutating) second.push(key);
console.log("snapshot:", first.join(","), "next:", second.join(","));

// Multi-key ordering, inherited enumerables, non-enumerable shadowing, and a
// deletion before visitation all require and exercise the generic fallback.
const proto: any = { inherited: 1, shadowed: 2 };
Object.defineProperty(proto, "hidden", { value: 3, enumerable: false });
const ordered: any = Object.create(proto);
ordered[10] = 10;
ordered[2] = 2;
ordered.word = 1;
Object.defineProperty(ordered, "shadowed", { value: 4, enumerable: false });
const orderedKeys: string[] = [];
for (const key in ordered) orderedKeys.push(key);
console.log("ordered:", orderedKeys.join(","));

// Even a one-key receiver must fall back when it has a custom prototype.
const customOne: any = Object.create({ base: 1 });
customOne.own = 2;
const customOneKeys: string[] = [];
for (const key in customOne) customOneKeys.push(key);
console.log("custom-one:", customOneKeys.join(","));

// The cached Object.prototype verdict is shape-generation keyed.  Installing
// an enumerable property after the hot arm was warmed must invalidate it.
(Object.prototype as any).lateEnumerable = 3;
const latePrototypeKeys: string[] = [];
for (const key in { own: 1 }) latePrototypeKeys.push(key);
delete (Object.prototype as any).lateEnumerable;
console.log("late-prototype:", latePrototypeKeys.join(","));

const deleting: any = { a: 1, b: 2 };
const deletionKeys: string[] = [];
for (const key in deleting) {
  deletionKeys.push(key);
  if (key === "a") {
    delete deleting.b;
    deleting.c = 3;
  }
}
console.log("mutation:", deletionKeys.join(","));

const proxyKeys: string[] = [];
const proxy = new Proxy({ target: 1 }, {
  ownKeys() { return ["target"]; },
  getOwnPropertyDescriptor(_target: any, key: string) {
    if (key === "target") return { enumerable: true, configurable: true };
    return undefined;
  }
});
for (const key in proxy) proxyKeys.push(key);
console.log("proxy:", proxyKeys.join(","));

let caught = "";
try {
  for (const key in { x: 1, y: 2 }) {
    caught = key;
    throw new Error("stop");
  }
} catch (_error) {
  console.log("exception:", caught);
}
"#,
    )
    .expect("write #8694 fixture");

    let mut compile_command = Command::new(perry_bin());
    compile_command
        .current_dir(dir.path())
        // RS4GC's Windows EH limitation is unrelated to this test's moving-GC
        // coverage and rejects the fixture's deliberate try/catch before
        // codegen, so select the shadow-root backend explicitly (#7354).
        .env("PERRY_RS4GC", "0")
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&binary)
        .arg("--no-cache")
        .arg("--no-auto-optimize");
    let compile = compile_command.output().expect("compile #8694 fixture");
    assert!(
        compile.status.success(),
        "compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );

    const EXPECTED: &str = "stable: 0\n\
snapshot: only next: only,added\n\
ordered: 2,10,word,inherited\n\
custom-one: own,base\n\
late-prototype: own,lateEnumerable\n\
mutation: a\n\
proxy: target\n\
exception: x\n";

    for force_evacuation in [false, true] {
        let run = run_fixture(&binary, force_evacuation);
        let stderr = String::from_utf8_lossy(&run.stderr);
        assert!(
            run.status.success(),
            "fixture failed (force_evacuation={force_evacuation})\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&run.stdout),
            stderr
        );
        assert_eq!(String::from_utf8_lossy(&run.stdout), EXPECTED);
        let (stable, fallback) = diagnostic_counts(&stderr);
        assert!(
            stable > 0 && fallback > 0,
            "both the stable arm and generic fallback must be observable; got \
             stable={stable}, fallback={fallback}\nstderr:\n{stderr}"
        );
    }
}
