//! End-to-end exercise of the handle-vs-heap-pointer classification bands
//! (`perry-runtime/src/value/addr_class.rs`), in the #4800 shape: a Web Fetch
//! `Headers` handle (POINTER_TAG payload in the fetch band, NOT a heap
//! address) flowing through for-of / spread / typeof / instanceof / brand
//! checks alongside real heap objects (Map, Set, plain object, array, Date).
//!
//! Every probe here historically reached a `GcHeader` deref: a missing or
//! too-low magnitude floor dereferenced the handle id as a heap address and
//! SIGSEGV'd on Linux (#1843, #4004, #4665, #4800 — macOS mimalloc page
//! retention masks the class, so the binary-exits-cleanly assertion is the
//! real gate on the Linux CI runners).

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

#[test]
fn handle_band_values_classify_without_deref() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    let output = dir.path().join("main_bin");

    std::fs::write(
        &entry,
        r#"
// Fetch-band handle (id in [0x40000, 0xE0000), pointer-tagged, no GcHeader).
const h = new Headers();
h.set("content-type", "application/json");
h.set("x-perry", "1");

// #4800: lazy for-of routes the handle through js_get_iterator →
// is_builtin_iterator_class_id; a low floor deref'd the id as a GcHeader.
const pairs: string[] = [];
for (const [k, v] of h) {
    pairs.push(k + "=" + v);
}
pairs.sort();
console.log(pairs.join(","));

// Spread over the handle (flat_clone handle @@iterator path).
const spread = [...h];
console.log("spread:" + spread.length);

// Classification probes that all magnitude-check before dereferencing.
console.log("typeof:" + typeof h);
console.log("isArray:" + Array.isArray(h));
console.log("instanceofMap:" + (h instanceof Map));

// Real heap collections — is_registered_map/is_registered_set brand checks
// (registry-first after #4665) must still say yes for these...
const m = new Map<string, number>();
m.set("a", 1);
const s = new Set<number>([1, 2, 3]);
console.log("map:" + (m instanceof Map) + ":" + m.get("a") + ":" + m.size);
console.log("set:" + (s instanceof Set) + ":" + s.has(2) + ":" + s.size);

// ...and plain heap objects/arrays/Dates keep their identities.
const obj = { x: 1, y: "z" };
const arr = [1, 2, 3];
const d = new Date(0);
console.log(JSON.stringify(obj));
console.log(arr.map((n: number) => n * 2).join("-"));
console.log("date:" + (d instanceof Date) + ":" + d.getTime());
"#,
    )
    .expect("write entry");

    let compile = Command::new(perry_bin())
        .current_dir(dir.path())
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );

    let run = Command::new(&output).output().expect("run compiled binary");
    assert!(
        run.status.success(),
        "compiled binary failed (signal/segfault = handle deref'd as heap pointer, \
         #4800/#4665 regression class)\nstatus: {:?}\nstdout:\n{}\nstderr:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    let stdout = String::from_utf8_lossy(&run.stdout);
    let expected = "content-type=application/json,x-perry=1\n\
                    spread:2\n\
                    typeof:object\n\
                    isArray:false\n\
                    instanceofMap:false\n\
                    map:true:1:1\n\
                    set:true:true:3\n\
                    {\"x\":1,\"y\":\"z\"}\n\
                    2-4-6\n\
                    date:true:0\n";
    assert_eq!(stdout, expected, "handle/heap classification drifted");
}
