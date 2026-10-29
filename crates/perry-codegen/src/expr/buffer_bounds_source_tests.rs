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
