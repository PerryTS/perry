//! Emission pins for native buffer access bounds: the proof that admits a
//! native typed-array access selects the lowering, and the lowered index is
//! still checked against the length before it becomes an address.

use crate::{compile_module, CompileOptions};

fn function_ir(source: &str, name: &str) -> String {
    let source = source.to_string();
    let name = name.to_string();
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(move || {
            let mut cache = perry_diagnostics::SourceCache::new();
            let parsed =
                perry_parser::parse_typescript_with_cache(&source, "bounds.ts", &mut cache)
                    .unwrap();
            let m = perry_hir::lower_module(&parsed.module, "bounds", "bounds.ts").unwrap();
            let opts = CompileOptions {
                emit_ir_only: true,
                ..Default::default()
            };
            let ir = String::from_utf8(compile_module(&m, opts).unwrap()).unwrap();
            let prefix = format!("define double @perry_fn_bounds__{name}");
            let mut out = String::new();
            let mut inside = false;
            for line in ir.lines() {
                if line.starts_with("define ") {
                    inside = line.contains(&prefix)
                        || line.contains(&format!("@perry_fn_bounds__{name}$"));
                }
                if inside {
                    out.push_str(line);
                    out.push('\n');
                }
            }
            assert!(!out.is_empty(), "no IR for {name}:\n{ir}");
            out
        })
        .unwrap()
        .join()
        .unwrap()
}

/// The sieve's `marks[j] = 1` is proven in bounds for the JS value of `j`
/// (`j <= n`, `marks.length == n + 1`) and takes the native byte store. The
/// store is still guarded by a branch on the lowered index, and nothing turns
/// the proof into an `llvm.assume` the optimizer could use to drop it.
#[test]
fn proven_typed_array_store_keeps_a_real_bounds_branch() {
    let ir = function_ir(
        "function sieve(n: number): number {
           const marks = new Uint8Array(n + 1);
           let count = 0;
           for (let i = 2; i <= n; i++) {
             if (marks[i] === 0) {
               count++;
               for (let j = i * i; j <= n; j += i) marks[j] = 1;
             }
           }
           return count;
         }
         console.log(sieve(100));",
        "sieve",
    );
    assert!(
        ir.contains("store i8 1, ptr") || ir.contains("store i8 %"),
        "the proven store takes the native path:\n{ir}"
    );
    assert!(
        ir.contains("bufacc.oob"),
        "the native access branches on its bounds check:\n{ir}"
    );
    assert!(
        !ir.contains("@llvm.assume("),
        "a range proof must not become an assumption on the lowered index:\n{ir}"
    );
}

/// The sieve's loops are counter-versioned: one entry check on the actual
/// values selects a copy with plain `add nsw` steps and no per-access branch;
/// the checked copy keeps exact (i64) overflow-checked steps and per-access branches.
#[test]
fn counter_versioned_fast_copy_drops_only_what_its_entry_check_proved() {
    let ir = function_ir(
        "function sieve(n: number): number {
           const marks = new Uint8Array(n + 1);
           let count = 0;
           for (let i = 2; i <= n; i++) {
             if (marks[i] === 0) {
               count++;
               for (let j = i * i; j <= n; j += i) marks[j] = 1;
             }
           }
           return count;
         }
         console.log(sieve(100));",
        "sieve",
    );
    assert!(
        ir.contains("for.counter.fast.body") && ir.contains("add nsw i32"),
        "no counter-versioned fast copy:\n{ir}"
    );
    assert!(
        ir.contains(".cond.in_range"),
        "the checked copy tests its overflow flag before reading the counter:\n{ir}"
    );
    // Every entry check (the block that branches to a fast copy and its
    // checked copy) tests the step against i32 AND the largest admitted index
    // against a length; the outer loop indexes `marks[i]`, the inner
    // `marks[j]` (checked once per copy of the outer loop).
    let mut entry_checks = 0;
    for block in ir.split("\n\n") {
        if !(block.contains("label %for.counter.fast.")
            && block.contains("label %for.counter.checked."))
        {
            continue;
        }
        entry_checks += 1;
        let compares: Vec<&str> = block
            .lines()
            .filter(|l| l.contains("icmp sle i64"))
            .collect();
        assert!(
            compares.iter().any(|l| l.ends_with("2147483647")),
            "entry check without the step-fits test:\n{block}"
        );
        assert!(
            compares.iter().any(|l| !l.ends_with("2147483647")),
            "entry check without the length test:\n{block}"
        );
    }
    assert!(
        entry_checks >= 2,
        "an entry check per versioned loop:\n{ir}"
    );
    for block in ir.split("\nfor.counter.fast.body.").skip(1) {
        let body = block.split("\n\n").next().unwrap_or("");
        assert!(
            !body.contains("bufacc.oob"),
            "a fast-copy access kept its per-access branch:\n{body}"
        );
    }
}
