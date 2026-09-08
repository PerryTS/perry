//! A throwing exotic `slice` read must not leave its plain species result with
//! a pointer element omitted from the GC's side-table layout.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

const SOURCE: &str = r#"
const target: any[] = [];
for (let i = 0; i < 12; i++) target.push(i === 9 ? { keep: 1 } : i);

const source: any[] = [];
for (let i = 0; i < 12; i++) source.push(i);
function Species(_length: number) { gc(); return target; }
const speciesHolder: any = {};
Object.defineProperty(speciesHolder, Symbol.species, {
  get() { gc(); return Species; },
});
Object.defineProperty(source, "constructor", {
  get() { gc(); return speciesHolder; },
});
Object.defineProperty(source, "10", {
  get() { return { late: 10 }; },
  configurable: true,
});
Object.defineProperty(source, "11", {
  get() { throw new Error("stop slice"); },
  configurable: true,
});

try {
  source.slice(0, 12);
} catch (error: any) {
  console.log("caught", error.message);
}
console.log("before", target.length, target[10].late);
gc();
console.log("after", target.length, target[10].late);

const spliceTarget: any[] = [];
for (let i = 0; i < 12; i++) spliceTarget.push(i === 9 ? { keep: 1 } : i);
const spliceSource: any[] = [];
for (let i = 0; i < 12; i++) spliceSource.push(i);
function SpliceSpecies(_length: number) { gc(); return spliceTarget; }
const spliceSpeciesHolder: any = {};
Object.defineProperty(spliceSpeciesHolder, Symbol.species, {
  get() { gc(); return SpliceSpecies; },
});
Object.defineProperty(spliceSource, "constructor", {
  get() { gc(); return spliceSpeciesHolder; },
});
Object.defineProperty(spliceSource, "10", {
  get() { return { late: 10 }; },
  configurable: true,
});
Object.defineProperty(spliceSource, "11", {
  get() { throw new Error("stop splice"); },
  configurable: true,
});
try {
  spliceSource.splice(0, 12);
} catch (error: any) {
  console.log("caught", error.message);
}
console.log("before", spliceTarget.length, spliceTarget[10].late);
gc();
console.log("after", spliceTarget.length, spliceTarget[10].late);

const sliceCoerceSource: any[] = [{ first: 1 }, { second: 2 }];
const sliceCoerced = sliceCoerceSource.slice(
  { valueOf() { gc(); return 0; } } as any,
  { valueOf() { gc(); return 2; } } as any,
);
console.log("slice-coercion", sliceCoerced[0].first, sliceCoerced[1].second);

const coercionTarget: any[] = [];
const coercionSource: any[] = [0, { removed: 1 }, 2];
const inserted = { inserted: 42 };
function CoercionSpecies(_length: number) { gc(); return coercionTarget; }
const coercionSpeciesHolder: any = {};
Object.defineProperty(coercionSpeciesHolder, Symbol.species, {
  get() { gc(); return CoercionSpecies; },
});
Object.defineProperty(coercionSource, "constructor", {
  get() { gc(); return coercionSpeciesHolder; },
});
const coercionRemoved = coercionSource.splice(
  { valueOf() { gc(); return 1; } } as any,
  { valueOf() { gc(); return 1; } } as any,
  inserted,
);
console.log(
  "splice-coercion",
  coercionRemoved[0].removed,
  coercionSource[1].inserted,
);
gc();
console.log(
  "splice-coercion-after",
  coercionRemoved[0].removed,
  coercionSource[1].inserted,
);

function frozenSpeciesCase(method: "slice" | "splice") {
  const frozenSource: any[] = [1, 2];
  const frozenTarget: any[] = Object.freeze([7, 8]);
  function FrozenSpecies(_length: number) { return frozenTarget; }
  Object.defineProperty(frozenSource, "constructor", {
    value: { [Symbol.species]: FrozenSpecies },
  });
  try {
    if (method === "slice") frozenSource.slice(0, 2);
    else frozenSource.splice(0, 2);
  } catch (error: any) {
    console.log(method, error.name, frozenTarget.join(","), frozenSource.length);
  }
}
frozenSpeciesCase("slice");
frozenSpeciesCase("splice");

const replaceSource: any[] = [1, 2];
const replaceTarget: any[] = [7, 8];
Object.defineProperty(replaceTarget, "0", {
  value: 7,
  writable: false,
  enumerable: false,
  configurable: true,
});
let setterCalls = 0;
Object.defineProperty(replaceTarget, "1", {
  set(_value) { setterCalls++; },
  configurable: true,
});
function ReplaceSpecies(_length: number) { return replaceTarget; }
Object.defineProperty(replaceSource, "constructor", {
  value: { [Symbol.species]: ReplaceSpecies },
});
replaceSource.slice(0, 2);
const replaced = Object.getOwnPropertyDescriptor(replaceTarget, "0")!;
const replacedAccessor = Object.getOwnPropertyDescriptor(replaceTarget, "1")!;
console.log(
  "replaced",
  replaceTarget.join(","),
  replaced.writable,
  replaced.enumerable,
  replaced.configurable,
  replacedAccessor.value,
  setterCalls,
);
"#;

#[test]
fn array_side_mask_covers_a_pointer_stored_at_a_late_index() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    let output = dir.path().join("main_bin");
    std::fs::write(&entry, SOURCE).expect("write fixture");

    let compile = Command::new(perry_bin())
        .current_dir(dir.path())
        .env("PERRY_NO_AUTO_OPTIMIZE", "1")
        .env("PERRY_NO_CACHE", "1")
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr),
    );

    let run = Command::new(&output)
        .env("PERRY_GC_SCHEDULE_SEED", "9983")
        .env("PERRY_GC_SCHEDULE_RATE", "1")
        .env("PERRY_GC_SCHEDULE_ALLOC_KB", "0")
        .env("PERRY_GC_FORCE_EVACUATE", "1")
        .env("PERRY_GC_VERIFY_EVACUATION", "1")
        .env("PERRY_GC_PROTECT_FROMSPACE", "1")
        .env("PERRY_GC_VERIFY_MARK", "1")
        .env("PERRY_GC_DIAG", "1")
        .output()
        .expect("run compiled fixture");
    let stdout = String::from_utf8_lossy(&run.stdout);
    let stderr = String::from_utf8_lossy(&run.stderr);
    assert!(
        run.status.success(),
        "fixture exited {:?}\nstdout:\n{stdout}\nstderr:\n{stderr}",
        run.status.code(),
    );
    assert_eq!(
        stdout,
        concat!(
            "caught stop slice\nbefore 12 10\nafter 12 10\n",
            "caught stop splice\nbefore 12 10\nafter 12 10\n",
            "slice-coercion 1 2\n",
            "splice-coercion 1 42\nsplice-coercion-after 1 42\n",
            "slice TypeError 7,8 2\nsplice TypeError 7,8 2\n",
            "replaced 1,2 true true true 2 0\n",
        )
    );
    assert!(
        stderr.contains("[gc-copy-minor] ran") && stderr.contains("copied_objects="),
        "forced evacuation never ran, so the verifier inspected nothing:\n{stderr}"
    );
    assert!(
        stderr.contains("[gc-array-slots:copying-minor] OK") && !stderr.contains("UNENUMERATED"),
        "slice left a live element outside the GC layout:\n{stderr}"
    );
}
