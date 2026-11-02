//! Emission pins for the hint on an `if` that guards a loop nest inside a loop:
//! its condition is said to be unlikely, so the skip is the fall-through.

use crate::{compile_module, CompileOptions};

/// The IR of the functions whose symbol starts with `@perry_fn_hint__<name>`.
fn function_ir(source: &str, name: &str) -> String {
    let source = source.to_string();
    let symbol = format!("@perry_fn_hint__{name}");
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(move || {
            let mut cache = perry_diagnostics::SourceCache::new();
            let parsed =
                perry_parser::parse_typescript_with_cache(&source, "hint.ts", &mut cache).unwrap();
            let m = perry_hir::lower_module(&parsed.module, "hint", "hint.ts").unwrap();
            let opts = CompileOptions {
                emit_ir_only: true,
                ..Default::default()
            };
            let ir = String::from_utf8(compile_module(&m, opts).unwrap()).unwrap();
            let mut out = String::new();
            let mut inside = false;
            for line in ir.lines() {
                if line.starts_with("define ") {
                    inside = line.contains(&symbol);
                }
                if inside {
                    out.push_str(line);
                    out.push('\n');
                }
            }
            assert!(!out.is_empty(), "no IR for {symbol}:\n{ir}");
            out
        })
        .unwrap()
        .join()
        .unwrap()
}

fn expect_hints(ir: &str) -> usize {
    ir.matches("call i1 @llvm.expect.i1").count()
}

const NEST_IN_LOOP: &str = "function nestInLoop(n: number): number {
       let c = 0;
       for (let i = 0; i < n; i++) {
         if (i % 3 === 0) {
           for (let j = 0; j < 4; j++) c += j;
         }
       }
       return c;
     }
     console.log(nestInLoop(10));";

/// The guard of a loop nest inside a loop carries the hint.
#[test]
fn a_loop_nest_guarded_inside_a_loop_is_unlikely() {
    let ir = function_ir(NEST_IN_LOOP, "nestInLoop");
    assert!(
        expect_hints(&ir) > 0,
        "the guard of the nested loop is hinted:\n{ir}"
    );
}

/// Without a loop around it the skip runs once, so no hint.
#[test]
fn a_guarded_loop_nest_outside_any_loop_is_not_hinted() {
    let ir = function_ir(
        "function nestOutside(n: number): number {
           let c = 0;
           if (n > 3) {
             for (let j = 0; j < n; j++) c += j;
           }
           return c;
         }
         console.log(nestOutside(10));",
        "nestOutside",
    );
    assert_eq!(expect_hints(&ir), 0, "no hint outside a loop:\n{ir}");
}

/// An `else` arm means the `if` picks between two paths, not a skip.
#[test]
fn a_guard_with_an_else_arm_is_not_hinted() {
    let ir = function_ir(
        "function withElse(n: number): number {
           let c = 0;
           for (let i = 0; i < n; i++) {
             if (i % 3 === 0) {
               for (let j = 0; j < 4; j++) c += j;
             } else {
               c += 1;
             }
           }
           return c;
         }
         console.log(withElse(10));",
        "withElse",
    );
    assert_eq!(expect_hints(&ir), 0, "no hint with an else arm:\n{ir}");
}

/// A guarded block without a loop is just a branch, not a nest to skip.
#[test]
fn a_guard_without_a_loop_nest_is_not_hinted() {
    let ir = function_ir(
        "function noNest(n: number): number {
           let c = 0;
           for (let i = 0; i < n; i++) {
             if (i % 3 === 0) {
               c += 2;
             }
           }
           return c;
         }
         console.log(noNest(10));",
        "noNest",
    );
    assert_eq!(expect_hints(&ir), 0, "no hint without a nested loop:\n{ir}");
}
