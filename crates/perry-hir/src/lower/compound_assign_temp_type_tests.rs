//! #10718: the compound-assignment spill must carry the SOURCE binding's type.
//!
//! `a[i] += 1` is desugared by `hoist_compound_member_assign` into two
//! immutable temps so base and key are each evaluated exactly once. Those temps
//! used to be minted with `ty: Type::Any`, which erased BOTH the receiver's
//! array-ness and the index's integer-ness before codegen ever saw the
//! statement — so every element tier declined and the read and the write fell
//! to `js_object_get_index_polymorphic` / `js_object_set_index_polymorphic`,
//! the generic OBJECT property path. Measured on `for (i) a[i] += 1` over an
//! ordinary 400-element array: 948 instructions per element, against 16 for
//! `a[i] = k + i` on the same array, and a `number[]` annotation did not help
//! because the erasure happens here.
//!
//! This is a VERDICT test: the desugar is semantically correct either way and
//! prints the same numbers, so only the recorded type distinguishes
//! "optimizable" from "structurally excluded". Behaviour is covered
//! differentially against node.

#![cfg(test)]

use crate::types::Type;
use crate::{Module, Stmt};
use perry_diagnostics::SourceCache;

fn lower(src: &str) -> Module {
    let src = src.to_string();
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(move || {
            let mut cache = SourceCache::new();
            let parsed = perry_parser::parse_typescript_with_cache(
                &src,
                "compound_assign_temp_type.ts",
                &mut cache,
            )
            .expect("parse should succeed");
            crate::lower_module(&parsed.module, "test", "compound_assign_temp_type.ts")
                .expect("lower should succeed")
        })
        .expect("spawn lower thread")
        .join()
        .expect("lower thread panicked")
}

/// `(name, ty)` of every `Stmt::Let` whose name starts with `__cmpd_`.
fn cmpd_temps(stmts: &[Stmt]) -> Vec<(String, Type)> {
    let mut out = Vec::new();
    fn walk(stmts: &[Stmt], out: &mut Vec<(String, Type)>) {
        for stmt in stmts {
            match stmt {
                Stmt::Let { name, ty, .. } if name.starts_with("__cmpd_") => {
                    out.push((name.clone(), ty.clone()));
                }
                Stmt::For { init, body, .. } => {
                    if let Some(init) = init {
                        walk(std::slice::from_ref(init.as_ref()), out);
                    }
                    walk(body, out);
                }
                Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => walk(body, out),
                Stmt::If {
                    then_branch,
                    else_branch,
                    ..
                } => {
                    walk(then_branch, out);
                    if let Some(else_branch) = else_branch {
                        walk(else_branch, out);
                    }
                }
                _ => {}
            }
        }
    }
    walk(stmts, &mut out);
    out
}

fn temp(temps: &[(String, Type)], tag: &str) -> Type {
    temps
        .iter()
        .find(|(name, _)| name.starts_with(&format!("__cmpd_{tag}_")))
        .unwrap_or_else(|| panic!("no __cmpd_{tag}_* temp among {temps:?}"))
        .1
        .clone()
}

#[test]
fn declared_number_array_compound_assign_keeps_both_types() {
    let module = lower(
        "const a: number[] = new Array(4);\n\
         for (let i = 0; i < 4; i++) a[i] += 1;\n",
    );
    let temps = cmpd_temps(&module.init);
    assert_eq!(
        temp(&temps, "base"),
        Type::Array(Box::new(Type::Number)),
        "the base temp must carry the receiver's declared array type"
    );
    assert_eq!(
        temp(&temps, "key"),
        Type::Number,
        "the key temp must carry the counter's Number type, or every \
         integer-index proof is erased before codegen"
    );
}

#[test]
fn untyped_new_array_compound_assign_keeps_the_erased_array_type() {
    // `new Array(n)` records `Generic { base: "Array", type_args: [] }`. That
    // is a WEAKER claim than `number[]` but still says "an Array", which is
    // what the range-loop tier's admission keys on. `Any` said nothing.
    let module = lower(
        "const a = new Array(4);\n\
         for (let i = 0; i < 4; i++) a[i] += 1;\n",
    );
    let temps = cmpd_temps(&module.init);
    assert!(
        matches!(temp(&temps, "base"), Type::Generic { ref base, .. } if base == "Array"),
        "the base temp must stay an Array type, got {:?}",
        temp(&temps, "base")
    );
    assert_eq!(temp(&temps, "key"), Type::Number);
}

#[test]
fn a_non_local_base_still_spills_as_any() {
    // The copy is restricted to a bare `LocalGet` source, where the temp is an
    // immutable snapshot of exactly one binding and its type is the source's
    // type by construction. A call result has no binding to copy from, so the
    // temp keeps `Any` — if this ever becomes a type, the copy has been
    // widened past its proof.
    let module = lower(
        "function f() { return [1, 2, 3]; }\n\
         f()[1] += 1;\n",
    );
    let temps = cmpd_temps(&module.init);
    assert_eq!(temp(&temps, "base"), Type::Any);
}

#[test]
fn a_string_array_compound_assign_does_not_gain_a_numeric_type() {
    // The copy is exact: a `string[]` receiver must report `string[]`, never a
    // numeric array. This is the twin that fails if the copy is ever replaced
    // by a guess.
    let module = lower(
        "const a: string[] = [\"x\"];\n\
         a[0] += \"y\";\n",
    );
    let temps = cmpd_temps(&module.init);
    assert_eq!(temp(&temps, "base"), Type::Array(Box::new(Type::String)));
}
