//! Regression coverage for source-ordered lexical declarations in classic
//! `for` heads (#9052).

#![cfg(test)]

use super::*;
use crate::ir::{Module, Stmt};

fn lower(source: &str) -> Module {
    let parsed =
        perry_parser::parse_typescript(source, "for-multi-decl.ts").expect("source should parse");
    lower_module(&parsed, "for-multi-decl", "for-multi-decl.ts").expect("source should lower")
}

fn assert_source_ordered_prelude(stmts: &[Stmt]) {
    let for_index = stmts
        .iter()
        .position(|stmt| matches!(stmt, Stmt::For { .. }))
        .expect("classic for should be present");
    let prelude = &stmts[..for_index];
    let names = prelude
        .iter()
        .filter_map(|stmt| match stmt {
            Stmt::Let { name, .. } => Some(name.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(names, ["i", "limit", "previous", "next"]);

    let i_id = prelude
        .iter()
        .find_map(|stmt| match stmt {
            Stmt::Let { id, name, .. } if name == "i" => Some(*id),
            _ => None,
        })
        .expect("i binding should be present");
    let next_init = prelude
        .iter()
        .find_map(|stmt| match stmt {
            Stmt::Let {
                name,
                init: Some(init),
                ..
            } if name == "next" => Some(init),
            _ => None,
        })
        .expect("next initializer should be present");
    let mut refs = Vec::new();
    let mut visited = std::collections::HashSet::new();
    crate::analysis::collect_local_refs_expr(next_init, &mut refs, &mut visited);
    assert!(
        refs.contains(&i_id),
        "next must read the earlier i binding: {next_init:#?}"
    );
    assert!(
        !format!("{next_init:?}").contains("GlobalGet"),
        "next must not contain an unresolved/global reference: {next_init:#?}"
    );

    assert!(matches!(stmts[for_index], Stmt::For { init: None, .. }));
}

#[test]
fn module_classic_for_lexical_declarators_lower_in_source_order() {
    let hir =
        lower("for (let i = 0, limit = 2, previous = i, next = i + 1; i < limit; i++, next++) {}");
    assert_source_ordered_prelude(&hir.init);
}

#[test]
fn function_classic_for_lexical_declarators_lower_in_source_order() {
    let hir = lower(
        "function run() { for (let i = 0, limit = 2, previous = i, next = i + 1; i < limit; i++, next++) {} }",
    );
    let function = hir
        .functions
        .iter()
        .find(|function| function.name == "run")
        .expect("run should lower");
    assert_source_ordered_prelude(&function.body);
}

#[test]
fn safe_counter_head_keeps_the_for_init_slot() {
    // #9106: `let i = 0, len = arr.length` — a literal-initialized counter
    // that no tail declarator mentions. Hoisting the tail around it is
    // unobservable, so the counter stays in `For::init` (the versioned
    // counted-loop matchers key their admission on it) and only the tail
    // moves to the loop-scoped prelude.
    let hir = lower(
        "function run(arr: number[]) { let sum = 0; for (let i = 0, len = arr.length; i < len; i++) { sum += arr[i]; } return sum; }",
    );
    let function = hir
        .functions
        .iter()
        .find(|function| function.name == "run")
        .expect("run should lower");
    let for_index = function
        .body
        .iter()
        .position(|stmt| matches!(stmt, Stmt::For { .. }))
        .expect("classic for should be present");
    let len_id = function.body[..for_index]
        .iter()
        .find_map(|stmt| match stmt {
            Stmt::Let { id, name, .. } if name == "len" => Some(*id),
            _ => None,
        })
        .expect("tail binding should be hoisted into the prelude");
    assert!(
        hir.classic_for_lexical_bindings.contains(&len_id),
        "hoisted tail binding keeps per-iteration capture semantics"
    );
    let crate::ir::Stmt::For {
        init: Some(init), ..
    } = &function.body[for_index]
    else {
        panic!(
            "counter must stay in For::init: {:#?}",
            function.body[for_index]
        );
    };
    let counter_id = match init.as_ref() {
        Stmt::Let {
            id,
            name,
            init: Some(crate::ir::Expr::Integer(0)),
            ..
        } if name == "i" => *id,
        other => panic!("counter Let expected in For::init, got {other:#?}"),
    };
    assert!(
        !hir.classic_for_lexical_bindings.contains(&counter_id),
        "the init-slot counter is handled by the For machinery, not the prelude set"
    );
}
