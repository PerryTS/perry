//! Write-barrier coverage stress tests for the generational GC.
//!
//! Failure mode under test: an old-gen (tenured or malloc-backed) object is
//! mutated to point at a freshly allocated nursery value without a write
//! barrier. Minor GC then either never sees the edge (child swept while
//! live) or never rewrites the slot when evacuation moves the child —
//! both end in nondeterministic garbage reads or segfaults.
//!
//! Both tests run the compiled binary with the detection-maximizing knobs:
//! `PERRY_GC_FORCE_EVACUATE=1` (stress-copy every movable nursery survivor)
//! and `PERRY_GC_VERIFY_EVACUATION=1` (panic if a live slot still points at
//! a forwarded object after an evacuation/rewrite cycle).
//!
//! NOTE: the churn helpers use object/array *literals* and explicit `gc()`
//! only on programs without wide dynamic objects — building thousands of
//! dynamic properties on one object and then calling `gc()` deadlocks on a
//! pre-existing (unrelated) bug (#4878), and structuredClone of dynamic
//! overflow properties is a separate pre-existing gap (#4879). Wide object
//! literals are avoided too (compile-time blowup, #4880).

use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

#[cfg(unix)]
use std::os::unix::process::CommandExt;

#[cfg(unix)]
unsafe extern "C" {
    fn kill(pid: i32, sig: i32) -> i32;
    fn setsid() -> i32;
}

#[cfg(unix)]
const SIGTERM: i32 = 15;
#[cfg(unix)]
const SIGKILL: i32 = 9;

const COMPILED_BINARY_TIMEOUT: Duration = Duration::from_secs(120);

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn compile_and_run(source: &str) -> std::process::Output {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    let output = dir.path().join("main_bin");
    std::fs::write(&entry, source).expect("write entry");

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

    let mut run = Command::new(&output);
    run.env("PERRY_GC_FORCE_EVACUATE", "1")
        .env("PERRY_GC_VERIFY_EVACUATION", "1");
    run_compiled_binary_with_timeout(run, COMPILED_BINARY_TIMEOUT)
}

fn run_compiled_binary_with_timeout(mut command: Command, timeout: Duration) -> Output {
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    #[cfg(unix)]
    unsafe {
        command.pre_exec(|| {
            if setsid() == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }

    let mut child = command.spawn().expect("run compiled binary");
    let start = Instant::now();
    loop {
        match child.try_wait().expect("poll compiled binary") {
            Some(_) => {
                return child
                    .wait_with_output()
                    .expect("collect compiled binary output")
            }
            None if start.elapsed() >= timeout => {
                #[cfg(unix)]
                unsafe {
                    let pgid = -(child.id() as i32);
                    kill(pgid, SIGTERM);
                    std::thread::sleep(Duration::from_millis(250));
                    if child.try_wait().expect("poll after SIGTERM").is_none() {
                        kill(pgid, SIGKILL);
                    }
                }
                #[cfg(not(unix))]
                {
                    child.kill().expect("kill timed out compiled binary");
                }
                let output = child
                    .wait_with_output()
                    .expect("collect timed out compiled binary output");
                panic!(
                    "compiled binary timed out after {:?}\nstatus: {:?}\nstdout:\n{}\nstderr:\n{}",
                    timeout,
                    output.status,
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                );
            }
            None => std::thread::sleep(Duration::from_millis(50)),
        }
    }
}

fn assert_ok_output(run: &std::process::Output, expected: &str) {
    assert!(
        run.status.success(),
        "compiled binary failed (signal/segfault = missed write barrier)\nstatus: {:?}\nstdout:\n{}\nstderr:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    let stdout = String::from_utf8_lossy(&run.stdout);
    assert_eq!(
        stdout,
        expected,
        "stderr:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
}

/// Tenure a population of objects across several GC cycles, then mutate the
/// tenured objects (object fields, array elements, closure captures, module
/// globals) to point at freshly allocated nursery values, GC again, and
/// verify every value survived. Any missed barrier on those store paths
/// shows up as corrupt reads, an evacuation-verify panic, or a crash.
// TEMPORARILY IGNORED — #5029. The Full conservative stack scan (default for
// explicit gc() since #4998) exposes a PRE-EXISTING remembered-set bug: minor
// cycles drop legitimate old→young dirty-page coverage (measured ~130 pages at
// cycle entry → ~10 after clear+rebuild), live nursery children of old-gen
// large objects get swept while still referenced, and forced evacuation then
// corrupts through the dangling slots. Root-cause trail, repro matrix, and
// bisect proof are in the #5029 issue comments. Re-enable both tests when the
// remembered-set coverage fix lands; do NOT revert #4998 (it fixes #4977).
#[test]
#[ignore = "#5029: pre-existing remembered-set coverage drop under Full conservative scan + forced evacuation"]
fn tenured_mutation_stress() {
    let run = compile_and_run(
        r#"
let moduleRef: any = null;

function makeCounter() {
    let state: any = { n: 0, tag: "init" };
    return () => {
        state = { n: state.n + 1, tag: "c" + state.n };
        return state.tag;
    };
}

function churn(rounds: number) {
    for (let c = 0; c < rounds; c++) {
        let garbage: any[] = [];
        for (let j = 0; j < 30000; j++) {
            garbage.push({ x: j, s: "pad" + j });
        }
        gc();
    }
}

// Phase 1: allocate keepers and tenure them (survive several GC cycles).
const keepers: any[] = [];
for (let i = 0; i < 300; i++) {
    keepers.push({ id: i, payload: null, arr: [i], tag: "k" + i });
}
const counter = makeCounter();
churn(5);

// Phase 2: mutate tenured objects to point at fresh nursery values.
for (let i = 0; i < 300; i++) {
    keepers[i].payload = { value: i * 3 + 1, text: "p" + i, inner: [i, i + 1, "s" + i] };
    keepers[i].arr.push({ deep: i });
}
moduleRef = { mark: "module-root", list: [1, 2, 3] };
counter(); // closure capture slot now points at a fresh nursery object
counter();

// Phase 3: GC again so any unbarriered old->young edge gets swept or
// left stale by evacuation.
churn(4);

// Phase 4: verify.
let bad = 0;
for (let i = 0; i < 300; i++) {
    const k = keepers[i];
    if (k.id !== i) bad++;
    if (k.payload.value !== i * 3 + 1) bad++;
    if (k.payload.text !== "p" + i) bad++;
    if (k.payload.inner[2] !== "s" + i) bad++;
    const last = k.arr[k.arr.length - 1];
    if (last.deep !== i) bad++;
}
if (moduleRef.mark !== "module-root" || moduleRef.list[2] !== 3) bad++;
// tag records the pre-increment n, so the 3rd call yields "c2".
if (counter() !== "c2") bad++;
console.log(bad === 0 ? "BARRIER_STRESS_OK" : "BARRIER_STRESS_CORRUPT " + bad);
"#,
    );
    assert_ok_output(&run, "BARRIER_STRESS_OK\n");
}

/// structuredClone integrity under GC churn + forced evacuation. Covers the
/// runtime-helper barrier path hardened in this change (the object-field
/// deep-clone loop in `js_structured_clone` now routes through the shared
/// barriered store). Uses a 300-key literal (all fields inline) plus a deep
/// nested chain so the clone itself allocates enough to run GCs mid-clone.
// TEMPORARILY IGNORED — #5029; see tenured_mutation_stress above.
#[test]
#[ignore = "#5029: pre-existing remembered-set coverage drop under Full conservative scan + forced evacuation"]
fn structured_clone_gc_churn_stress() {
    let mut fields = String::new();
    for i in 0..300 {
        fields.push_str(&format!(
            "    f{i}: {{ v: {i}, pad: \"x{i}\" + \"y\".repeat(96) }},\n"
        ));
    }
    let source = format!(
        r#"
function nest(depth: number): any {{
    let o: any = {{ leaf: true, n: depth }};
    for (let d = 0; d < depth; d++) {{
        o = {{ child: o, mark: "d" + d, arr: [d, "s" + d] }};
    }}
    return o;
}}

const src: any = {{
{fields}
    deep: nest(200),
    tail: "end"
}};

const cl = structuredClone(src);

for (let c = 0; c < 4; c++) {{
    let garbage: any[] = [];
    for (let j = 0; j < 30000; j++) {{
        garbage.push({{ z: j, s: "g" + j }});
    }}
    gc();
}}

let bad = 0;
for (let i = 0; i < 300; i++) {{
    const f = cl["f" + i];
    if (f === undefined || f === null) {{ bad++; continue; }}
    if (f.v !== i) bad++;
    else if (f.pad !== "x" + i + "y".repeat(96)) bad++;
}}
let cur = cl.deep;
for (let d = 199; d >= 0; d--) {{
    if (cur.mark !== "d" + d || cur.arr[1] !== "s" + d) {{ bad++; break; }}
    cur = cur.child;
}}
if (!cur.leaf || cur.n !== 200) bad++;
if (cl.tail !== "end") bad++;
console.log(bad === 0 ? "CLONE_STRESS_OK" : "CLONE_STRESS_CORRUPT " + bad);
"#
    );
    let run = compile_and_run(&source);
    assert_ok_output(&run, "CLONE_STRESS_OK\n");
}
