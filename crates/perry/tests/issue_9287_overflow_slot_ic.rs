//! #9287: constant-key property access to a slot past the inline region
//! (overflow / spill storage) primes and hits the inline caches.
//!
//! Before the fix, `js_put_value_set_ic_miss` bailed with `idx >= alloc_limit`
//! and `js_object_get_field_ic_miss` broke out of its keys walk at the same
//! condition — so a property at index >= `live_inline_slot_count` (2 for a
//! plain `{}`) missed both ICs on EVERY access, forever: 3 ms vs 27 ms for
//! the identical loop, decided by whether the property sat at index 1 or 2.
//! A miss-handler probe put 99.9998% of 2.4M declines on that one bail.
//!
//! The fix primes such slots with `IC_SLOT_OVERFLOW_BIT` and routes emitted
//! hits through `js_put_value_set_ic_overflow_store` /
//! `js_object_get_field_ic_overflow_load` — the dynamic-key IC's audited
//! overflow path. What these tests guard is the INVALIDATION story of that
//! new hit path: every case below primes the cache hot and then changes the
//! world (delete, accessor, freeze, shape change, GC move) in a way the hit
//! must notice. A wrong answer here is the cache returning stale state — the
//! exact bug class a primed-but-unvalidated hit would ship.
//!
//! The polymorphic-rotation case pins `pic_prime_get`'s cascade guard: an
//! overflow-encoded slot must never enter the ways, whose emitted path
//! computes a raw inline address from the slot word (a wild load if the bit
//! ever reached it).

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn compile_and_run_with_env(source: &str, envs: &[(&str, &str)]) -> String {
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
        .env("PERRY_NO_CACHE", "1")
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );

    let mut run_cmd = Command::new(&output);
    run_cmd.current_dir(dir.path());
    for (k, v) in envs {
        run_cmd.env(k, v);
    }
    let run = run_cmd.output().expect("run compiled binary");
    assert!(
        run.status.success(),
        "compiled binary failed (exit {:?})\nstderr:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).trim().to_owned()
}

fn compile_and_run(source: &str) -> String {
    compile_and_run_with_env(source, &[])
}

/// Six properties: indices 0-1 inline, 2-5 in overflow for a plain `{}`.
const MK: &str = r#"
function mk(): any { const o: any = {}; for (let j = 0; j < 6; j++) o["f" + j] = j; return o; }
"#;

#[test]
fn overflow_reads_and_writes_stay_correct_when_hot() {
    let out = compile_and_run(&format!(
        r#"{MK}
const o = mk(); let s = 0;
for (let i = 0; i < 10000; i++) {{ o["f4"] = i; s += o["f4"]; }}
console.log(s + " " + o["f0"] + " " + o["f5"]);
"#
    ));
    assert_eq!(out, "49995000 0 5");
}

#[test]
fn a_deleted_overflow_property_is_not_served_from_the_cache() {
    // Primes the read+write caches hot, deletes the property, then reads.
    // A hit path that skipped the tombstone check would return the last
    // written value instead of undefined.
    let out = compile_and_run(&format!(
        r#"{MK}
const o = mk(); let s = 0;
for (let i = 0; i < 1000; i++) {{ o["f4"] = i; s += o["f4"]; }}
delete o["f4"];
console.log(o["f4"] + " " + (s === 499500 ? "sum_ok" : "sum_" + s));
"#
    ));
    assert_eq!(out, "undefined sum_ok");
}

#[test]
fn an_accessor_installed_after_priming_fires_on_the_next_read() {
    // OBJ_FLAG_HAS_DESCRIPTORS must force the primed site back to the miss
    // handler; a stale hit would read the raw slot and bypass the getter.
    let out = compile_and_run(&format!(
        r#"{MK}
const o = mk(); let s = 0;
for (let i = 0; i < 1000; i++) s += o["f4"];
Object.defineProperty(o, "f4", {{ get() {{ return 777; }} }});
console.log(o["f4"] + " " + (s === 4000 ? "ok" : "" + s));
"#
    ));
    assert_eq!(out, "777 ok");
}

#[test]
fn freeze_after_priming_blocks_the_cached_write() {
    let out = compile_and_run(&format!(
        r#"{MK}
const o = mk();
for (let i = 0; i < 1000; i++) o["f4"] = i;
Object.freeze(o);
o["f4"] = 12345;
console.log(o["f4"]);
"#
    ));
    assert_eq!(out, "999");
}

#[test]
fn a_deleted_then_rewritten_overflow_property_revives_through_the_cache() {
    let out = compile_and_run(&format!(
        r#"{MK}
const o = mk(); let s = 0;
for (let i = 0; i < 500; i++) {{ o["f4"] = i; s += o["f4"]; }}
delete o["f4"];
o["f4"] = 999;
for (let i = 0; i < 500; i++) s += o["f4"];
console.log(o["f4"] + " " + s);
"#
    ));
    assert_eq!(out, "999 624250");
}

#[test]
fn rotating_overflow_shapes_at_one_site_stays_correct() {
    // Pins `pic_prime_get`'s cascade guard: three shapes rotate through one
    // site whose property is in overflow on each of them. If an encoded slot
    // ever cascaded into a way, the emitted way path would compute an
    // inline address from it — index | (1 << 30) scaled by 8 — and the read
    // would be garbage or a crash, not a number.
    let out = compile_and_run(&format!(
        r#"{MK}
const a = mk();
const b = mk(); b["extra"] = 0;
const c = mk(); c["e1"] = 0; c["e2"] = 0;
function site(o: any): number {{ let t = 0; for (let i = 0; i < 500; i++) {{ o["f3"] = i; t += o["f3"]; }} return t; }}
let s = 0;
for (let r = 0; r < 4; r++) {{ s += site(a); s += site(b); s += site(c); }}
console.log(s);
"#
    ));
    assert_eq!(out, "1497000");
}

#[test]
fn pointer_values_through_the_overflow_write_survive_evacuating_gc() {
    // String values exercise the write barrier inside the spill store; the
    // heap limit plus forced evacuation makes the collector actually move
    // things while the site is hot.
    let out = compile_and_run_with_env(
        &format!(
            r#"{MK}
const o = mk(); const keep: string[] = [];
for (let i = 0; i < 5000; i++) {{
  o["f5"] = "str_" + (i % 7);
  keep.push("g" + i);
  if (keep.length > 64) keep.length = 0;
}}
console.log(o["f5"] + " " + o["f0"]);
"#
        ),
        &[("PERRY_GC_HEAP_LIMIT", "8"), ("PERRY_GC_FORCE_EVACUATE", "1")],
    );
    assert_eq!(out, "str_1 0");
}
