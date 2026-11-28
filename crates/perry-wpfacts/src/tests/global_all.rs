//! GALL: a read of the global object under a key of no shape (an unknown
//! String, an element read of a value that may be the global object) is the
//! union of all its own values: one node of the global object, so a read
//! costs an edge and what its destination then holds, not a named read of
//! every declared and program global.
use super::*;

/// Solve work of `p` program globals (each a Number) and `n` reads
/// `globalThis[k]` under an unknown String `k`.
fn work(p: usize, n: usize) -> u64 {
    let mut src = String::from("const k: string = String(Date.now());\n");
    for j in 0..p {
        src += &format!("(globalThis as any).g{j} = {j};\n");
    }
    for i in 0..n {
        src += &format!("const r{i} = (globalThis as any)[k];\nconsole.log(r{i});\n");
    }
    let pf = facts(&src);
    assert!(!pf.stats.aborted && !pf.stats.widened, "{:?}", pf.stats);
    pf.stats.work
}

/// The work one more such read adds.
fn per_read(p: usize) -> u64 {
    (work(p, 40) - work(p, 20)) / 20
}

/// Program globals that hold only Numbers add no value to the union, so
/// they add no work to a read: with a named read per global per site, 200
/// of them cost each read 200 more named reads.
#[test]
fn global_all_any_key_read_is_one_node() {
    let (a, b) = (per_read(0), per_read(200));
    eprintln!("GALL per-read work: 0 globals {a}, 200 globals {b}");
    assert!(b < a + 50, "a read costs {a} with 0 program globals, {b} with 200");
}
