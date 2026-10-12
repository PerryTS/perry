//! Preserve a bare block's lexical activation in the HIR.
//!
//! Scope grouping uses statement-list identity. Splicing a bare block into its
//! parent list erased that identity and let a surviving outer closure retain
//! inner captured bindings. An unconditional branch carries the list through
//! the existing walkers and transforms; LLVM folds its constant condition
//! after scope grouping. Unlike a run-once loop it does not intercept break or
//! continue, so enclosing loops and switches keep their control-flow targets.

use anyhow::Result;
use swc_ecma_ast as ast;

use crate::ir::{Expr, Stmt};
use crate::lower::LoweringContext;

pub(crate) fn lower_bare_block_stmt(
    ctx: &mut LoweringContext,
    block: &ast::BlockStmt,
) -> Result<Stmt> {
    Ok(Stmt::If {
        condition: Expr::Bool(true),
        then_branch: super::lower_block_stmt_scoped(ctx, block)?,
        else_branch: None,
    })
}

/// A control statement already owns the block's statement list. Reuse that
/// list instead of adding an unconditional branch inside it. Synthesized
/// iteration bodies with a head-binding prelude still lower the source block
/// as a statement, keeping its lexical bindings separate from that prelude.
pub(crate) fn lower_control_body_stmt(
    ctx: &mut LoweringContext,
    stmt: &ast::Stmt,
) -> Result<Vec<Stmt>> {
    match stmt {
        ast::Stmt::Block(block) => super::lower_block_stmt_scoped(ctx, block),
        _ => super::lower_body_stmt(ctx, stmt),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lower(source: &str) -> crate::Module {
        let ast = perry_parser::parse_typescript(source, "lexical_block.ts").expect("parse");
        crate::lower_module(&ast, "lexical_block", "lexical_block.ts").expect("lower")
    }

    fn local(stmts: &[Stmt], name: &str) -> Option<u32> {
        stmts.iter().find_map(|s| match s {
            Stmt::Let { id, name: n, .. } if n == name => Some(*id),
            _ => None,
        })
    }

    fn inner(stmts: &[Stmt]) -> &[Stmt] {
        stmts
            .iter()
            .find_map(|s| match s {
                Stmt::If {
                    condition: Expr::Bool(true),
                    then_branch,
                    else_branch: None,
                } => Some(then_branch.as_slice()),
                _ => None,
            })
            .expect("bare block must keep its own statement list")
    }

    #[test]
    fn function_bare_block_keeps_inner_captures_out_of_outer_list() {
        let module = lower(
            r#"
            function make() {
                let count = 0;
                const tick = () => ++count;
                { let big = [7]; const grow = () => { big = big.concat([count]); }; grow(); }
                return tick;
            }
        "#,
        );
        let body = &module
            .functions
            .iter()
            .find(|f| f.name == "make")
            .expect("make")
            .body;
        assert!(local(body, "count").is_some());
        assert!(
            local(body, "big").is_none(),
            "inner binding cannot share outer activation"
        );
        assert!(local(inner(body), "big").is_some());
    }

    #[test]
    fn module_bare_block_keeps_its_own_activation() {
        let module = lower("let outer = 0; { let inner = 1; const grow = () => ++inner; grow(); }");
        assert!(local(&module.init, "outer").is_some());
        assert!(local(&module.init, "inner").is_none());
        assert!(local(inner(&module.init), "inner").is_some());
    }

    #[test]
    fn same_scope_control_keeps_bindings_in_one_list() {
        let module = lower("function make() { let a = 0; let b = 1; return () => { a++; b++; }; }");
        let body = &module
            .functions
            .iter()
            .find(|f| f.name == "make")
            .expect("make")
            .body;
        assert!(local(body, "a").is_some());
        assert!(local(body, "b").is_some());
        assert!(!body.iter().any(|s| matches!(
            s,
            Stmt::If {
                condition: Expr::Bool(true),
                ..
            }
        )));
    }
    #[test]
    fn control_bodies_reuse_their_list_and_keep_nested_bare_blocks_separate() {
        let module = lower(
            r#"
            function make(flag: boolean) {
                if (flag) { let inIf = 1; { let nested = 2; } }
                while (flag) { let inWhile = 3; break; }
                do { let inDo = 4; } while (false);
                for (let i = 0; i < 1; i++) { let inFor = 5; }
            }
        "#,
        );
        let body = &module
            .functions
            .iter()
            .find(|f| f.name == "make")
            .expect("make")
            .body;
        for stmt in body {
            match stmt {
                Stmt::If { then_branch, .. } => {
                    assert!(local(then_branch, "inIf").is_some());
                    assert!(local(then_branch, "nested").is_none());
                    assert!(local(inner(then_branch), "nested").is_some());
                }
                Stmt::While { body, .. } => assert!(local(body, "inWhile").is_some()),
                Stmt::DoWhile { body, .. } => assert!(local(body, "inDo").is_some()),
                Stmt::For { body, .. } => assert!(local(body, "inFor").is_some()),
                _ => {}
            }
        }
        assert_eq!(
            body.iter()
                .filter(|s| matches!(
                    s,
                    Stmt::If { .. } | Stmt::While { .. } | Stmt::DoWhile { .. } | Stmt::For { .. }
                ))
                .count(),
            4
        );
    }
}
