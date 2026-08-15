//! #8064: unknown-`FuncRef` fallback wrappers are scoped to their source
//! module. Split codegen promotes internal functions for cross-unit calls, so
//! a process-global fallback name makes otherwise independent module objects
//! collide at the application link.

use crate::{compile_module, CompileOptions};
use perry_hir::{Expr, Module, Stmt};

fn fallback_ir(module_name: &str) -> String {
    let mut module = Module::new(module_name);
    module.init = vec![Stmt::Expr(Expr::FuncRef(0x8064))];
    let options = CompileOptions {
        emit_ir_only: true,
        output_type: "executable".to_string(),
        ..CompileOptions::default()
    };
    String::from_utf8(compile_module(&module, options).expect("fallback module compiles"))
        .expect("LLVM IR is UTF-8")
}

#[test]
fn unknown_funcref_uses_its_module_scoped_wrapper() {
    let alpha = fallback_ir("alpha.ts");
    let beta = fallback_ir("beta.ts");
    let alpha_wrapper = "__perry_wrap_perry_unknown_func_alpha_ts";
    let beta_wrapper = "__perry_wrap_perry_unknown_func_beta_ts";

    assert!(
        alpha.contains(&format!("define internal double @{alpha_wrapper}(")),
        "alpha must define its own internal fallback wrapper"
    );
    assert!(
        alpha.contains(&format!("ptr @{alpha_wrapper}")),
        "alpha's unresolved FuncRef must reference alpha's wrapper"
    );
    assert!(
        beta.contains(&format!("define internal double @{beta_wrapper}(")),
        "beta must define its own internal fallback wrapper"
    );
    assert!(
        beta.contains(&format!("ptr @{beta_wrapper}")),
        "beta's unresolved FuncRef must reference beta's wrapper"
    );
    assert!(!alpha.contains(beta_wrapper));
    assert!(!beta.contains(alpha_wrapper));
    assert!(
        !alpha.contains("@__perry_wrap_perry_unknown_func("),
        "the process-global fallback symbol must not be emitted"
    );
}
