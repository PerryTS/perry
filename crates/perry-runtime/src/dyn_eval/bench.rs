//! Measurement harness for the #6559 interpreter (#6693 perf work).
//!
//! Not a correctness test — a stopwatch. It mirrors the real TypeBox / ajv /
//! fastify load: construct many `new Function(source)` validators, then run
//! each many times. It reports the two costs the issue's `sample` split apart
//! — CONSTRUCTION (SWC parse + prepass, `swc_ecma_parser` frames) vs
//! EXECUTION (the tree-walk, `get_field_by_name` frames) — so an optimization
//! can be pointed at the dominant one and re-measured.
//!
//! Run explicitly (it is `#[ignore]`d so normal `cargo test` skips it):
//! ```text
//! cargo test --release -p perry-runtime --lib -- --ignored --nocapture \
//!     --test-threads=1 dyn_eval::bench
//! ```
//! `PERRY_BENCH_N` (distinct validators, default 200) and `PERRY_BENCH_M`
//! (calls per validator, default 50) tune the load.

use std::time::Instant;

use super::dyn_function_from_strings;
use super::{root_get, root_push, roots_truncate};

fn call(f: f64, args: &[f64]) -> f64 {
    unsafe { crate::closure::js_native_call_value(f, args.as_ptr(), args.len()) }
}

fn env_usize(key: &str, default: usize) -> usize {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

/// A TypeBox-`TypeCompiler`-shaped object validator: the exact construct mix
/// the real workload emits — scope-chain identifier reads (`value`, `ok`),
/// named property access (`value.fN`), typeof guards, relational/logical ops,
/// a `for-in` excess-key scan. `seed` makes every source textually distinct
/// (real schemas are), so nothing dedups the parse away.
fn make_validator_source(seed: usize, fields: usize) -> String {
    let mut s = String::new();
    s.push_str(&format!("/* validator {seed} */\n"));
    s.push_str("return function check(value) {\n");
    s.push_str("  let ok = true;\n");
    s.push_str("  ok = ok && (typeof value === 'object' && value !== null);\n");
    for i in 0..fields {
        match i % 3 {
            0 => s.push_str(&format!(
                "  ok = ok && (typeof value.f{i} === 'number' && value.f{i} >= 0);\n"
            )),
            1 => s.push_str(&format!(
                "  ok = ok && (typeof value.f{i} === 'string' && value.f{i}.length < 64);\n"
            )),
            _ => s.push_str(&format!(
                "  ok = ok && (typeof value.f{i} === 'boolean');\n"
            )),
        }
    }
    s.push_str("  let extra = 0;\n");
    s.push_str("  for (const k in value) { extra = extra + 1; }\n");
    s.push_str("  return ok && extra >= 0;\n");
    s.push_str("}\n");
    s
}

/// A matching sample input `{ f0: 1, f1: "x", f2: true, … }`, built via the
/// interpreter itself so no test-only object plumbing is needed.
fn make_sample(fields: usize) -> f64 {
    let mut src = String::from("return {");
    for i in 0..fields {
        if i > 0 {
            src.push(',');
        }
        match i % 3 {
            0 => src.push_str(&format!("f{i}:{i}")),
            1 => src.push_str(&format!("f{i}:\"s{i}\"")),
            _ => src.push_str(&format!("f{i}:true")),
        }
    }
    src.push('}');
    let maker = dyn_function_from_strings(&[src]);
    call(maker, &[])
}

/// Measure CONSTRUCTION (SWC parse + prepass) on the REAL TypeBox validator
/// bodies captured from `pi-bundle.mjs` (the accel-ON bundle) via a
/// `Function`-constructor hook. Point `PERRY_BENCH_SRC_DIR` at a directory of
/// `<hash>.body` (+ optional `<hash>.params`) files. Reports cold parse time
/// and the warm (parse-cache-hit) time per source. Execution is not measured
/// here — the real bodies close over host scope params we don't have.
#[test]
#[ignore = "benchmark: set PERRY_BENCH_SRC_DIR and run with --ignored --nocapture"]
fn bench_real_sources() {
    let Ok(dir) = std::env::var("PERRY_BENCH_SRC_DIR") else {
        eprintln!("PERRY_BENCH_SRC_DIR not set — skipping real-source bench");
        return;
    };
    let mut entries: Vec<_> = std::fs::read_dir(&dir)
        .expect("read src dir")
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().map(|x| x == "body").unwrap_or(false))
        .collect();
    entries.sort();

    let mut total_cold = std::time::Duration::ZERO;
    let mut total_warm = std::time::Duration::ZERO;
    eprintln!(
        "── #6693 real-source construction bench ({} bodies) ──",
        entries.len()
    );
    for body_path in &entries {
        let body = std::fs::read_to_string(body_path).unwrap_or_default();
        let params_path = body_path.with_extension("params");
        let params = std::fs::read_to_string(&params_path).unwrap_or_default();
        let mut args: Vec<String> = params
            .split(',')
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .collect();
        args.push(body.clone());

        // Cold: first construction of this exact source (full parse + prepass).
        let t = Instant::now();
        let f = dyn_function_from_strings(&args);
        let cold = t.elapsed();
        std::hint::black_box(f);
        // Warm: identical source again — should hit the parse cache.
        let t = Instant::now();
        let f2 = dyn_function_from_strings(&args);
        let warm = t.elapsed();
        std::hint::black_box(f2);

        total_cold += cold;
        total_warm += warm;
        eprintln!(
            "  {:>7} bytes : cold {:>8.3} ms   warm {:>8.4} ms   ({})",
            body.len(),
            cold.as_secs_f64() * 1e3,
            warm.as_secs_f64() * 1e3,
            body_path.file_name().unwrap().to_string_lossy()
        );
    }
    eprintln!(
        "TOTAL cold parse: {:>8.2} ms   TOTAL warm (cached): {:>8.4} ms",
        total_cold.as_secs_f64() * 1e3,
        total_warm.as_secs_f64() * 1e3
    );
}

#[test]
#[ignore = "benchmark: run explicitly with --ignored --nocapture"]
fn bench_typebox_like_load() {
    let n = env_usize("PERRY_BENCH_N", 200);
    let m = env_usize("PERRY_BENCH_M", 50);
    let fields = env_usize("PERRY_BENCH_FIELDS", 12);

    let sample = make_sample(fields);
    let sample_idx = root_push(sample);

    // ── Scenario 1: N DISTINCT sources (parse cannot be reused across them). ─
    let sources: Vec<String> = (0..n).map(|i| make_validator_source(i, fields)).collect();

    let t0 = Instant::now();
    let mut fns_idx = Vec::with_capacity(n);
    for src in &sources {
        let f = dyn_function_from_strings(std::slice::from_ref(src));
        fns_idx.push(root_push(f));
    }
    let construct = t0.elapsed();

    let t1 = Instant::now();
    let mut sink = 0u64;
    for _ in 0..m {
        for &fi in &fns_idx {
            let r = call(root_get(fi), &[root_get(sample_idx)]);
            sink = sink.wrapping_add(r.to_bits());
        }
    }
    let execute = t1.elapsed();

    // ── Scenario 2: the SAME source constructed N times (parse-cache probe). ─
    let one = make_validator_source(0, fields);
    let t2 = Instant::now();
    for _ in 0..n {
        let f = dyn_function_from_strings(std::slice::from_ref(&one));
        std::hint::black_box(f);
    }
    let construct_same = t2.elapsed();

    roots_truncate(sample_idx);

    let per_distinct_us = construct.as_micros() as f64 / n as f64;
    let per_same_us = construct_same.as_micros() as f64 / n as f64;
    let calls = (n * m) as f64;
    let per_call_us = execute.as_micros() as f64 / calls;

    eprintln!("── #6693 dyn_eval bench (N={n} distinct, M={m} calls, fields={fields}) ──");
    eprintln!(
        "CONSTRUCT {n} distinct : {:>8.2} ms total  ({per_distinct_us:>7.2} us / new Function)",
        construct.as_secs_f64() * 1e3
    );
    eprintln!(
        "CONSTRUCT {n} SAME src : {:>8.2} ms total  ({per_same_us:>7.2} us / new Function)",
        construct_same.as_secs_f64() * 1e3
    );
    eprintln!(
        "EXECUTE  {} calls      : {:>8.2} ms total  ({per_call_us:>7.2} us / call)",
        calls as u64,
        execute.as_secs_f64() * 1e3
    );
    eprintln!("(sink={sink:x})");
}
