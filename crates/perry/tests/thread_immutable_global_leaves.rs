//! perry/thread agents share user-module globals, but each agent owns a moving
//! heap. A String/BigInt module global initialized on main must never be read
//! by a worker out of main's slot: main's collector rewrites that slot, not the
//! worker's retained copy. These programs retain the values on a worker, let
//! main collect with forced evacuation and from-space protection, then consume
//! them. Before the immutable-global transfer every moving cell crashed on a
//! retired from-space object; the ordinary cells passed.
//!
//! Each program runs ordinary, under forced evacuation + verification +
//! from-space protection, and under the seeded schedule. A timeout or a signal
//! is a failure, never a pass.

use std::path::{Path, PathBuf};
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

const MOVING: [(&str, &str); 4] = [
    ("PERRY_GC_FORCE_EVACUATE", "1"),
    ("PERRY_GC_VERIFY_EVACUATION", "1"),
    ("PERRY_GC_PROTECT_FROMSPACE", "1"),
    ("PERRY_GC_DIAG", "1"),
];
const SEED: [(&str, &str); 3] = [
    ("PERRY_GC_SCHEDULE_SEED", "1"),
    ("PERRY_GC_SCHEDULE_RATE", "0.25"),
    ("PERRY_GC_SCHEDULE_ALLOC_KB", "0"),
];

fn clean(cmd: &mut Command) -> &mut Command {
    for (key, _) in std::env::vars() {
        if key.starts_with("PERRY_") || key == "NODE_OPTIONS" {
            cmd.env_remove(key);
        }
    }
    cmd
}

/// Compile `files` (first is the entry) and return the stdout of each mode,
/// after asserting every run exited 0 with no retired from-space report.
fn run_all_modes(files: &[(&str, &str)]) -> Vec<String> {
    let dir = tempfile::tempdir().expect("tempdir");
    for (name, source) in files {
        std::fs::write(dir.path().join(name), source).unwrap();
    }
    let exe = dir.path().join("main_bin");
    let compile = clean(&mut Command::new(perry_bin()))
        .current_dir(dir.path())
        .env("PERRY_GC_INSTRUMENTS", "1")
        .env("PERRY_NO_CACHE", "1")
        .arg("compile")
        .arg(dir.path().join(files[0].0))
        .arg("--no-auto-optimize")
        .arg("-o")
        .arg(&exe)
        .output()
        .expect("compile");
    assert!(
        compile.status.success(),
        "compile failed\n{}",
        String::from_utf8_lossy(&compile.stderr)
    );
    [
        ("ordinary", Vec::new()),
        ("manual", MOVING.to_vec()),
        ("seed1", MOVING.iter().chain(SEED.iter()).copied().collect()),
    ]
    .into_iter()
    .map(|(mode, knobs)| run_once(&exe, dir.path(), mode, &knobs))
    .collect()
}

fn run_once(exe: &Path, dir: &Path, mode: &str, knobs: &[(&str, &str)]) -> String {
    let mut cmd = Command::new(exe);
    clean(&mut cmd).current_dir(dir);
    for (key, value) in knobs {
        cmd.env(key, value);
    }
    let run = cmd.output().expect("run");
    let stderr = String::from_utf8_lossy(&run.stderr);
    assert!(
        run.status.success() && !stderr.contains("RETIRED FROM-SPACE"),
        "{mode}: status {:?}\nstdout:\n{}\nstderr:\n{stderr}",
        run.status,
        String::from_utf8_lossy(&run.stdout)
    );
    String::from_utf8(run.stdout).unwrap()
}

fn assert_every_mode(files: &[(&str, &str)], expected: &str) {
    for stdout in run_all_modes(files) {
        assert_eq!(stdout, expected);
    }
}

#[test]
fn a_computed_string_read_by_an_imported_function_and_method_survives_main_evacuation() {
    let helper = r#"// The load is runtime work, even though the fresh SAB's value is deterministically zero.
// Keep this initializer computed: an exported literal is folded into its readers.
const zeroView = new Int32Array(new SharedArrayBuffer(4));
console.log("string-module-once");
export const sharedString = "shared-computed-string-main-owned-payload-over-sixty-four-bytes-for-relocation-probe-" + Atomics.load(zeroView, 0);

export function readSharedComputedString(): string { return sharedString; }
export class SharedComputedStringReader {
    read(): string { return readSharedComputedString(); }
}
"#;
    let main = r#"import { spawn } from "perry/thread";
import { readSharedComputedString, SharedComputedStringReader } from "./helper";

declare function gc(): void;
const shared = new SharedArrayBuffer(8);
const main = new Int32Array(shared);
const result = spawn((): string => {
    const worker = new Int32Array(shared);
    const fromFunction = readSharedComputedString();
    const fromMethod = new SharedComputedStringReader().read();
    // Both original heap values must be retained before main can collect.
    Atomics.store(worker, 0, 1);
    Atomics.notify(worker, 0);
    while (Atomics.load(worker, 1) === 0) {
        if (Atomics.wait(worker, 1, 0, 10000) === "timed-out") {
            throw new Error("main did not finish collection");
        }
    }
    // Read the retained headers/payloads before allocating the returned string.
    const length = fromFunction.length + fromMethod.length;
    const last = fromFunction.charCodeAt(fromFunction.length - 1) + fromMethod.charCodeAt(fromMethod.length - 1);
    return fromFunction + "|" + fromMethod + "|" + length + "|" + last;
});
while (Atomics.load(main, 0) === 0) {
    if (Atomics.wait(main, 0, 0, 10000) === "timed-out") {
        throw new Error("worker did not retain computed strings");
    }
}
// Parent-owned diagnostic arm uses forced evacuation; verify THIS target moved.
// Avoid allocation-pressure loops before gc(): they can tenure the target early.
gc();
Atomics.store(main, 1, 1);
Atomics.notify(main, 1);
console.log(await result);
"#;
    let payload =
        "shared-computed-string-main-owned-payload-over-sixty-four-bytes-for-relocation-probe-0";
    assert_every_mode(
        &[("main.ts", main), ("helper.ts", helper)],
        &format!("string-module-once\n{payload}|{payload}|172|96\n"),
    );
}

#[test]
fn a_computed_bigint_read_by_an_imported_function_and_method_survives_main_evacuation() {
    let helper = r#"const zeroView = new Int32Array(new SharedArrayBuffer(4));
console.log("bigint-module-once");
// Arithmetic allocates the final BigInt; it is not a pooled literal handle.
export const sharedBigInt = 900719925474099312345678901234567890n + BigInt(Atomics.load(zeroView, 0));

export function readSharedComputedBigInt(): bigint { return sharedBigInt; }
export class SharedComputedBigIntReader {
    read(): bigint { return readSharedComputedBigInt(); }
}
"#;
    let main = r#"import { spawn } from "perry/thread";
import { readSharedComputedBigInt, SharedComputedBigIntReader } from "./helper";

declare function gc(): void;
const shared = new SharedArrayBuffer(8);
const main = new Int32Array(shared);
const result = spawn((): string => {
    const worker = new Int32Array(shared);
    const fromFunction = readSharedComputedBigInt();
    const fromMethod = new SharedComputedBigIntReader().read();
    Atomics.store(worker, 0, 1);
    Atomics.notify(worker, 0);
    while (Atomics.load(worker, 1) === 0) {
        if (Atomics.wait(worker, 1, 0, 10000) === "timed-out") {
            throw new Error("main did not finish collection");
        }
    }
    return (fromFunction + 1n).toString() + "|" + (fromMethod + 2n).toString();
});
while (Atomics.load(main, 0) === 0) {
    if (Atomics.wait(main, 0, 0, 10000) === "timed-out") {
        throw new Error("worker did not retain computed bigints");
    }
}
gc();
Atomics.store(main, 1, 1);
Atomics.notify(main, 1);
console.log(await result);
"#;
    assert_every_mode(
        &[("main.ts", main), ("helper.ts", helper)],
        "bigint-module-once\n\
         900719925474099312345678901234567891|900719925474099312345678901234567892\n",
    );
}

#[test]
fn a_bigint_global_read_directly_by_the_worker_survives_main_evacuation() {
    let main = r#"import { spawn } from "perry/thread";

declare function gc(): void;
console.log("direct-bigint-module-once");
export const sharedBigInt = 900719925474099312345678901234567890n;
const shared = new SharedArrayBuffer(8);
const main = new Int32Array(shared);
const result = spawn((): string => {
    const worker = new Int32Array(shared);
    const retainedA = sharedBigInt;
    const retainedB = sharedBigInt;
    Atomics.store(worker, 0, 1);
    Atomics.notify(worker, 0);
    while (Atomics.load(worker, 1) === 0) {
        if (Atomics.wait(worker, 1, 0, 10000) === "timed-out") {
            throw new Error("main did not finish collection");
        }
    }
    return (retainedA + 1n).toString() + "|" + (retainedB + 2n).toString();
});
while (Atomics.load(main, 0) === 0) {
    if (Atomics.wait(main, 0, 0, 10000) === "timed-out") {
        throw new Error("worker did not retain direct global bigints");
    }
}
gc();
Atomics.store(main, 1, 1);
Atomics.notify(main, 1);
console.log(await result);
"#;
    assert_every_mode(
        &[("main.ts", main)],
        "direct-bigint-module-once\n\
         900719925474099312345678901234567891|900719925474099312345678901234567892\n",
    );
}

/// Leaf fidelity, worker-local collection with the replicas and retained
/// locals live, a scalar global beside them, and the parallelMap route.
/// Main computes the same summary from its own canonical values, so the
/// worker's line must equal main's.
#[test]
fn replicas_keep_exact_payloads_across_main_and_worker_collections() {
    let leaves = r#"const zero = new Int32Array(new SharedArrayBuffer(4));
console.log("leaves-module-once");
export const k = Atomics.load(zero, 0);
export const lone = String.fromCharCode(0xD800 + k) + "-lone-surrogate-payload-over-sixty-four-bytes-to-avoid-interning-" + k;
export const astral = "astral-\u{1F600}-payload-longer-than-sixty-four-bytes-to-avoid-any-interning-" + k;
export const sso = "ab" + k;
export const negative = -123456789012345678901234567890n - BigInt(k);
export const zeroBig = 0n * BigInt(k + 1);
export function readLone(): string { return lone; }
export function readAstral(): string { return astral; }
export function readSso(): string { return sso; }
export function readNegative(): bigint { return negative; }
export function readZero(): bigint { return zeroBig; }
export function readK(): number { return k; }
export function summarize(a: string, b: string, c: string, d: bigint, e: bigint, n: number): string {
    return [a.length, a.charCodeAt(0), a.isWellFormed(), a.slice(1), b.length, b.codePointAt(7),
        b, c, (d - 1n).toString(), d < 0n, e.toString(), e === 0n, n].join("|");
}
"#;
    let main = r#"import { spawn, parallelMap } from "perry/thread";
import { readLone, readAstral, readSso, readNegative, readZero, readK, summarize } from "./leaves";

declare function gc(): void;
const shared = new SharedArrayBuffer(8);
const view = new Int32Array(shared);
const result = spawn((): string => {
    const v = new Int32Array(shared);
    const a = readLone();
    const b = readAstral();
    const c = readSso();
    const d = readNegative();
    const e = readZero();
    Atomics.store(v, 0, 1);
    Atomics.notify(v, 0);
    while (Atomics.load(v, 1) === 0) {
        if (Atomics.wait(v, 1, 0, 10000) === "timed-out") {
            throw new Error("main did not finish collection");
        }
    }
    // The worker's own collection: its replicas and these locals must move
    // with it.
    gc();
    return summarize(a, b, c, d, e, readK()) + "|" + (readLone() === a) + "|" + (readAstral() === b);
});
while (Atomics.load(view, 0) === 0) {
    if (Atomics.wait(view, 0, 0, 10000) === "timed-out") {
        throw new Error("worker did not retain leaves");
    }
}
gc();
Atomics.store(view, 1, 1);
Atomics.notify(view, 1);
console.log(summarize(readLone(), readAstral(), readSso(), readNegative(), readZero(), readK()) + "|true|true");
console.log(await result);
const lengths = parallelMap([1, 2, 3, 4, 5, 6, 7, 8], (x: number): number => readAstral().length + readLone().length + x);
console.log(lengths.join(","));
"#;
    for stdout in run_all_modes(&[("main.ts", main), ("leaves.ts", leaves)]) {
        let lines: Vec<&str> = stdout.lines().collect();
        assert_eq!(lines.len(), 4, "{stdout}");
        assert_eq!(lines[0], "leaves-module-once");
        assert_eq!(
            lines[1], lines[2],
            "worker replicas must equal main's values"
        );
        assert!(
            lines[1].contains("|false|"),
            "lone surrogate must stay ill-formed: {}",
            lines[1]
        );
        assert!(
            lines[1].contains("|128512|"),
            "astral code point: {}",
            lines[1]
        );
        assert!(
            lines[1].contains("|-123456789012345678901234567891|true|0|true|0|"),
            "{}",
            lines[1]
        );
        let fields: Vec<&str> = lines[1].split('|').collect();
        let base: usize = fields[0].parse::<usize>().unwrap() + fields[4].parse::<usize>().unwrap();
        let expected: Vec<String> = (1..=8).map(|x| (base + x).to_string()).collect();
        assert_eq!(lines[3], expected.join(","));
    }
}

/// A function that reads the bindings computes its agent-block address once
/// per invocation, in its entry block. The same function then runs on main
/// and on two workers at once, each collecting between its reads: every
/// invocation must read through ITS agent's address, never one computed on
/// another agent or before a collection.
#[test]
fn one_reader_function_on_main_and_two_workers_reads_each_agents_own_replica() {
    let helper = r#"const zero = new Int32Array(new SharedArrayBuffer(4));
console.log("hop-module-once");
export const s = "agent-hop-string-payload-longer-than-sixty-four-bytes-to-avoid-any-interning-" + Atomics.load(zero, 0);
export const b = 98765432109876543210n + BigInt(Atomics.load(zero, 0));
// Both bindings are read in the loop, with a collection between the reads.
export function readAcross(rounds: number, between: () => void): string {
    let out = 0;
    let last = "";
    for (let i = 0; i < rounds; i++) {
        const x = s;
        between();
        const y = b;
        out += x.length + x.charCodeAt(x.length - 1) + Number(y % 1000n);
        last = x;
    }
    return out + ":" + (last === s) + ":" + b.toString();
}
"#;
    let main = r#"import { spawn } from "perry/thread";
import { readAcross } from "./helper";

declare function gc(): void;
const first = spawn((): string => readAcross(6, (): void => { gc(); }));
const second = spawn((): string => readAcross(6, (): void => { gc(); }));
const own = readAcross(6, (): void => { gc(); });
console.log(own);
console.log(await first);
console.log(await second);
"#;
    let len =
        "agent-hop-string-payload-longer-than-sixty-four-bytes-to-avoid-any-interning-0".len();
    let line = format!("{}:true:98765432109876543210", 6 * (len + 48 + 210));
    assert_every_mode(
        &[("main.ts", main), ("helper.ts", helper)],
        &format!("hop-module-once\n{line}\n{line}\n{line}\n"),
    );
}
