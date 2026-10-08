//! The array IteratorRecord cursor remains integral by construction: a literal
//! zero seed and successful-step increments. Its compiler proof is attached to
//! the indexed read rather than a signed i32 shadow, which would wrap at 2^31.
//! These verdict tests cover both lowering entry points and reject an unproven
//! Number seed as well as a return to the old signed-counter initializer.
//! Codegen tests verify the unsigned read and full-range numeric increment.

#![cfg(test)]

use crate::{Expr, Module, Stmt};
use perry_diagnostics::SourceCache;

fn lower(src: &str) -> Module {
    let src = src.to_string();
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(move || {
            let mut cache = SourceCache::new();
            let parsed =
                perry_parser::parse_typescript_with_cache(&src, "for_of_counter.ts", &mut cache)
                    .expect("parse should succeed");
            crate::lower_module(&parsed.module, "test", "for_of_counter.ts")
                .expect("lower should succeed")
        })
        .expect("spawn lower thread")
        .join()
        .expect("lower thread panicked")
}

/// Init expressions of every `Stmt::Let` named `__idx_*` anywhere in `stmts`
/// — the synthetic counters the for-of / for-in desugars mint.
fn synthetic_counter_inits(stmts: &[Stmt]) -> Vec<Expr> {
    fn walk(stmts: &[Stmt], out: &mut Vec<Expr>) {
        for s in stmts {
            if let Stmt::Let {
                name,
                init: Some(init),
                ..
            } = s
            {
                if name.starts_with("__idx_") {
                    out.push(init.clone());
                }
            }
            match s {
                Stmt::If {
                    then_branch,
                    else_branch,
                    ..
                } => {
                    walk(then_branch, out);
                    if let Some(eb) = else_branch {
                        walk(eb, out);
                    }
                }
                Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => walk(body, out),
                Stmt::For { init, body, .. } => {
                    if let Some(init) = init {
                        walk(std::slice::from_ref(init.as_ref()), out);
                    }
                    walk(body, out);
                }
                Stmt::Try {
                    body,
                    catch,
                    finally,
                } => {
                    walk(body, out);
                    if let Some(c) = catch {
                        walk(&c.body, out);
                    }
                    if let Some(f) = finally {
                        walk(f, out);
                    }
                }
                Stmt::Switch { cases, .. } => {
                    for c in cases {
                        walk(&c.body, out);
                    }
                }
                Stmt::Labeled { body, .. } => walk(std::slice::from_ref(body.as_ref()), out),
                _ => {}
            }
        }
    }
    let mut out = Vec::new();
    walk(stmts, &mut out);
    out
}

fn unsigned_integer_seed(expr: &Expr) -> bool {
    matches!(expr, Expr::NativeMethodCall { module, method, args,
        class_name: None, object: None }
        if module == "__perry_runtime" && method == "arrayRecordIndex"
            && matches!(args.as_slice(), [Expr::Integer(0)]))
}

#[test]
fn module_level_for_of_counter_has_unsigned_integer_proof() {
    let m = lower(
        "class P { constructor(public x: number) {} }\n\
         const a: P[] = [new P(1)];\n\
         let s = 0;\n\
         for (const p of a) { s += p.x; }\n\
         console.log(s);\n",
    );
    let inits = synthetic_counter_inits(&m.init);
    assert!(
        !inits.is_empty(),
        "the for-of desugar should mint a `__idx_*` counter; found none"
    );
    assert!(
        inits.iter().all(unsigned_integer_seed),
        "each array record must seed its unsigned proof with Integer(0); got {inits:?}"
    );
}

#[test]
fn function_body_for_of_counter_has_unsigned_integer_proof() {
    // The function-body desugar is a SECOND emission site
    // (`lower_decl/body_stmt.rs`). It drifted independently before, and the
    // parameter case #7766 is about lives here, not in module init.
    let m = lower(
        "class P { constructor(public x: number) {} }\n\
         function total(ps: P[]): number {\n\
           let s = 0;\n\
           for (const p of ps) { s += p.x; }\n\
           return s;\n\
         }\n\
         console.log(total([new P(1)]));\n",
    );
    let f = m
        .functions
        .iter()
        .find(|f| f.name == "total")
        .expect("`total` should be lowered");
    let inits = synthetic_counter_inits(&f.body);
    assert!(
        !inits.is_empty(),
        "the function-body for-of desugar should mint a `__idx_*` counter"
    );
    assert!(
        inits.iter().all(unsigned_integer_seed),
        "each array record must seed its unsigned proof with Integer(0); got {inits:?}"
    );
}
