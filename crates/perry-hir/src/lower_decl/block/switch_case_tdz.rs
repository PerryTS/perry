//! #11826: dead zone for `let`/`const` declared in a `switch` case.
//!
//! Every case of a switch shares one lexical scope, but control enters at the
//! matching case and skips the declarations of the cases before it. A binding
//! declared in case `i` is visible in every later case (and in their tests),
//! where it can be read before its declaration ever ran:
//!
//! ```js
//! switch (k) { case 0: let x = 1; break; case 1: x; }  // k = 1: ReferenceError
//! ```
//!
//! Such a binding gets the TDZ-seeded cell the forward-capture path already
//! gives a binding a closure can read early, so every read of it checks.
//! A binding no later case names is untouched.

use std::collections::HashSet;

use swc_ecma_ast as ast;
use swc_ecma_visit::{Visit, VisitWith};

use crate::lower::LoweringContext;

/// Register the case-declared `let`/`const` bindings a later case names. Call
/// before the cases' `rebind_nested_forward_scope_lets`, which then allocates
/// their cells at switch entry.
pub(crate) fn register_switch_case_tdz_lets(ctx: &mut LoweringContext, cases: &[ast::SwitchCase]) {
    struct Names(HashSet<String>);
    impl Visit for Names {
        fn visit_ident(&mut self, ident: &ast::Ident) {
            self.0.insert(ident.sym.to_string());
        }
    }
    // `later[i]`: every identifier the cases after `i` mention (a superset of
    // the references; an extra name only costs a cell).
    let mut later: Vec<HashSet<String>> = vec![HashSet::new(); cases.len() + 1];
    for index in (0..cases.len()).rev() {
        let mut names = Names(later[index + 1].clone());
        if let Some(test) = &cases[index].test {
            test.visit_with(&mut names);
        }
        for stmt in &cases[index].cons {
            stmt.visit_with(&mut names);
        }
        later[index] = names.0;
    }
    for (index, case) in cases.iter().enumerate() {
        let after = &later[index + 1];
        if after.is_empty() {
            continue;
        }
        for stmt in &case.cons {
            let ast::Stmt::Decl(ast::Decl::Var(var_decl)) = stmt else {
                continue;
            };
            if !matches!(
                var_decl.kind,
                ast::VarDeclKind::Let | ast::VarDeclKind::Const
            ) {
                continue;
            }
            for decl in &var_decl.decls {
                if crate::lower::ambient::declarator_binds_nothing(var_decl, decl) {
                    continue;
                }
                let mut binding_idents: Vec<(String, u32)> = Vec::new();
                super::collect_pat_forward_idents(&decl.name, &mut binding_idents);
                for (name, span_lo) in binding_idents {
                    if !after.contains(&name) || ctx.lexical_forward_decls.contains_key(&span_lo) {
                        continue;
                    }
                    let id = ctx.fresh_local();
                    ctx.var_hoisted_ids.insert(id);
                    ctx.tdz_forward_ids.insert(id);
                    ctx.nested_forward_scope_ids.insert(id);
                    ctx.lexical_forward_decls.insert(span_lo, id);
                }
            }
        }
    }
}
