//! Source-level pins for the `integer_locals` exactness invariant: a local
//! whose write can exceed i32 in exact arithmetic is not an int32 local, and
//! the bounded loop and index shapes the i32 slots exist for still are.

use std::collections::{HashMap, HashSet};

use perry_hir::Stmt;

/// `integer_locals` of the function `name` in `source`, by local name.
fn int32_local_names(source: &str, name: &str) -> HashSet<String> {
    int32_and_exit_names(source, name).0
}

/// `integer_locals` and the overflow-exit counters among them, by name.
fn int32_and_exit_names(source: &str, name: &str) -> (HashSet<String>, HashSet<String>) {
    let source = source.to_string();
    let name = name.to_string();
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(move || {
            let mut cache = perry_diagnostics::SourceCache::new();
            let parsed =
                perry_parser::parse_typescript_with_cache(&source, "exact.ts", &mut cache).unwrap();
            let m = perry_hir::lower_module(&parsed.module, "exact", "exact.ts").unwrap();
            let f = m
                .functions
                .iter()
                .find(|f| f.name == name)
                .expect("function under test");
            let empty = HashSet::new();
            let not_bigint =
                super::not_bigint_locals::NotBigIntFacts::collect(&f.body, &[], &HashMap::new());
            let (ids, exits) = super::integer_locals::collect_integer_locals_and_exits(
                &f.body,
                &empty,
                &empty,
                &empty,
                &empty,
                &empty,
                &not_bigint,
            );
            let mut names = HashMap::new();
            collect_let_names(&f.body, &mut names);
            let named = |set: &HashSet<u32>| -> HashSet<String> {
                set.iter().filter_map(|id| names.get(id).cloned()).collect()
            };
            (named(&ids), named(&exits))
        })
        .unwrap()
        .join()
        .unwrap()
}

fn collect_let_names(stmts: &[Stmt], out: &mut HashMap<u32, String>) {
    for s in stmts {
        match s {
            Stmt::Let { id, name, .. } => {
                out.insert(*id, name.clone());
            }
            Stmt::For { init, body, .. } => {
                if let Some(i) = init {
                    collect_let_names(std::slice::from_ref(i.as_ref()), out);
                }
                collect_let_names(body, out);
            }
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => collect_let_names(body, out),
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                collect_let_names(then_branch, out);
                if let Some(e) = else_branch {
                    collect_let_names(e, out);
                }
            }
            _ => {}
        }
    }
}

/// `j = i * i` and `j += i` under `j <= lim` can leave i32 only upward, past
/// the guard: `j` is int32 through the overflow-exit lowering, not statically.
#[test]
fn square_init_and_symbolic_stride_are_overflow_exit_counters() {
    let (ints, exits) = int32_and_exit_names(
        "function run(n: number, m: Uint8Array) {
           const lim = n | 0;
           let steps = 0;
           for (let i = 1; i <= lim; i++) {
             for (let j = i * i; j <= lim; j += i) { m[j] = 1; steps++; }
           }
           return steps;
         }",
        "run",
    );
    assert!(ints.contains("lim") && ints.contains("i"), "{ints:?}");
    assert!(
        ints.contains("j") && exits.contains("j"),
        "{ints:?} {exits:?}"
    );
    assert!(!exits.contains("lim"), "{exits:?}");
}

/// Unbounded int32 operands can make `a * b` overflow DOWNWARD, where the
/// guard `j <= lim` is true: no overflow-exit admission.
#[test]
fn a_possibly_negative_product_is_not_an_overflow_exit_init() {
    let (ints, exits) = int32_and_exit_names(
        "function run(x: number, y: number, n: number, m: Uint8Array) {
           const a = x | 0;
           const b = y | 0;
           const lim = n | 0;
           for (let j = a * b; j <= lim; j++) m[j & 7] = 1;
         }",
        "run",
    );
    assert!(
        !ints.contains("j") && !exits.contains("j"),
        "{ints:?} {exits:?}"
    );
}

#[test]
fn unbounded_sum_and_increment_are_not_int32() {
    let ints = int32_local_names(
        "function run(x: number, m: Uint8Array) {
           const a = x | 0;
           const b = a + 1;
           let k = 2147483640;
           for (let s = 0; s < 20; s++) { m[k] = 1; m[b] = 2; k++; }
           return k;
         }",
        "run",
    );
    assert!(ints.contains("a") && ints.contains("s"), "{ints:?}");
    assert!(
        !ints.contains("b"),
        "`a + 1` is 2^31 for a = 2^31 - 1: {ints:?}"
    );
    assert!(
        !ints.contains("k"),
        "an unguarded `k++` leaves i32: {ints:?}"
    );
}

#[test]
fn bounded_counters_and_index_chains_stay_int32() {
    let ints = int32_local_names(
        "function run(m: Uint8Array, n: number) {
           const lim = n | 0;
           const W = 100;
           for (let i = 0; i < m.length; i++) m[i] = 1;
           for (let p = 0; p < lim; p++) m[p] = 2;
           for (let q = lim; q > 0; q--) m[q] = 3;
           for (let y = 0; y < 50; y++) {
             for (let x = 0; x < W; x++) { const idx = y * W + x; m[idx] = 4; }
           }
           for (let t = 0; t <= 1000; t += 4) m[t] = 5;
         }",
        "run",
    );
    for name in ["i", "p", "q", "y", "x", "idx", "t"] {
        assert!(ints.contains(name), "`{name}` is provably int32: {ints:?}");
    }
}

/// `i++` past `i <= 2^31 - 1` is 2^31: not statically int32, but the guard
/// is false for it, so the counter is an overflow-exit counter.
#[test]
fn le_guard_on_an_unbounded_int32_needs_the_overflow_exit() {
    let (ints, exits) = int32_and_exit_names(
        "function run(m: Uint8Array, n: number) {
           const lim = n | 0;
           for (let i = 0; i <= lim; i++) m[i] = 1;
           for (let k = 0; k < lim; k++) m[k] = 2;
         }",
        "run",
    );
    assert!(
        ints.contains("i") && exits.contains("i"),
        "{ints:?} {exits:?}"
    );
    assert!(
        ints.contains("k") && !exits.contains("k"),
        "`<` proves `k++`: {exits:?}"
    );
}

#[test]
fn ushr_zero_is_not_an_int32_write() {
    let ints = int32_local_names(
        "function run(x: number, m: Uint8Array) {
           let v = 0;
           v = x >>> 0;
           m[v] = 1;
           let w = 0;
           w = x >>> 8;
           m[w] = 1;
           return v + w;
         }",
        "run",
    );
    assert!(!ints.contains("v"), "`x >>> 0` is a Uint32: {ints:?}");
    assert!(ints.contains("w"), "`x >>> 8` fits i32: {ints:?}");
}
