//! Path identities for the generated CommonJS registry operations.

use crate::gen::Gen;
use crate::model::*;
use perry_hir::{CompareOp, Expr, ImportSpecifier, Stmt};

impl<'a> Gen<'a> {
    /// `require("<literal>")` in a CommonJS-wrapped module. The wrapper's
    /// synthetic `require(specifier)` resolved that specifier at compile
    /// time into one arm, `if (specifier === "<literal>") { ... "<absolute
    /// target path>" ... }`; the computed-specifier registry lookup behind
    /// the arms serves only specifiers the wrapper could not resolve. The
    /// call is worth that one target: its publication under the path (the
    /// record's live `exports`, or a raw publication) and the hoisted import
    /// binding of the same path. `None` for any other shape of call.
    pub(crate) fn cjs_static_require(
        &mut self,
        mi: u32,
        callee: &Expr,
        spec: &str,
    ) -> Option<NodeId> {
        let module = self.modules[mi as usize].1;
        let wrapped = module.is_commonjs_wrap();
        if !wrapped {
            if self.m.explain {
                eprintln!("perry: wpfacts explain: static require {spec:?}: module not CJS-wrapped (imports {:?})", module.imports.iter().map(|i| i.source.clone()).collect::<Vec<_>>());
            }
            return None;
        }
        // The wrapper's `require(specifier)`: a module-level function in a
        // hand-written module, a local bound to the helper closure inside the
        // module factory in a generated wrapper.
        let body: &[Stmt] = match callee {
            Expr::FuncRef(func_id) => {
                match module
                    .functions
                    .iter()
                    .find(|f| f.id == *func_id && f.name == "require")
                {
                    Some(f) => &f.body,
                    None => {
                        if self.m.explain {
                            eprintln!("perry: wpfacts explain: static require {spec:?}: callee fn {func_id} is not the wrapper's require");
                        }
                        return None;
                    }
                }
            }
            Expr::LocalGet(id) => match self.require_helper_body(mi, *id) {
                Some(b) => b,
                None => {
                    if self.m.explain {
                        eprintln!("perry: wpfacts explain: static require {spec:?}: local {id} is not bound to a `require` closure");
                    }
                    return None;
                }
            },
            _ => return None,
        };
        let arm = body.iter().find_map(|s| match s {
            Stmt::If {
                condition:
                    Expr::Compare {
                        op: CompareOp::Eq,
                        left,
                        right,
                    },
                then_branch,
                ..
            } if is_spec_test(&left, &right, spec) => Some(then_branch),
            _ => None,
        });
        let Some(arm) = arm else {
            if self.m.explain {
                let heads: Vec<String> = body
                    .iter()
                    .take(6)
                    .map(|st| {
                        let d = format!("{st:?}");
                        d[..d.len().min(160)].to_string()
                    })
                    .collect();
                eprintln!("perry: wpfacts explain: static require {spec:?}: no arm in require body; first statements {heads:?}");
            }
            return None;
        };
        let mut path: Option<String> = None;
        for s in arm {
            perry_hir::walker::stmt_any_expr(s, &mut |e| {
                expr_strings(e, &mut |x| {
                    if path.is_none() && std::path::Path::new(x).is_absolute() {
                        path = Some(x.to_string());
                    }
                });
                false
            });
        }
        let Some(path) = path else {
            if self.m.explain {
                eprintln!("perry: wpfacts explain: static require {spec:?}: arm has no absolute path literal");
            }
            return None;
        };
        if self.m.explain {
            eprintln!("perry: wpfacts explain: static require {spec:?} -> {path}");
        }
        let out = self.tmp("require-static");
        let rec = self.cjs_path_node(&path);
        // The path node holds what `require` returns (`registerPathModule`).
        self.m.edge(rec, out);
        self.m.add_prim(out, prim::UNDEF);
        let canon = canonical_path(&path);
        let locals: Vec<String> = module
            .imports
            .iter()
            .filter(|i| {
                i.resolved_path
                    .as_deref()
                    .is_some_and(|p| canonical_path(p) == canon)
            })
            .flat_map(|i| i.specifiers.iter().map(spec_local))
            .collect();
        for l in locals {
            if let Some(n) = self.import_value(mi, &l) {
                self.m.edge(n, out);
            }
        }
        Some(out)
    }

    /// The body of the `require` closure a module-local binding holds: the
    /// wrapper declares `function require(specifier)` inside the module
    /// factory, which lowers to a local bound to a closure. Indexed once per
    /// module over every statement, nested closures included.
    fn require_helper_body(&mut self, mi: u32, id: u32) -> Option<&'a [Stmt]> {
        if !self.require_helpers_indexed.contains(&mi) {
            self.require_helpers_indexed.insert(mi);
            let module = self.modules[mi as usize].1;
            let mut found: Vec<(u32, &'a [Stmt])> = Vec::new();
            for st in &module.init {
                collect_require_lets(st, &mut found);
            }
            for f in &module.functions {
                for st in &f.body {
                    collect_require_lets(st, &mut found);
                }
            }
            for (lid, body) in found {
                self.require_helpers.insert((mi, lid), body);
            }
        }
        self.require_helpers.get(&(mi, id)).copied()
    }

    pub(crate) fn cjs_path_node(&mut self, path: &str) -> NodeId {
        // Generated wrappers pass canonical absolute filenames. Canonicalize
        // existing aliases exactly as the runtime registry does; absent paths
        // keep their spelling and are conservatively nullable on lookup.
        let key = std::fs::canonicalize(path)
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|_| path.to_string());
        if let Some(&node) = self.cjs_paths.get(&key) {
            return node;
        }
        let node = self.m.node_named("cjs-path", || format!("cjs-path({key})"));
        let spelled = self.m.atom(path);
        let canon = self.m.atom(&key);
        self.m.cjs_paths.insert(spelled, node);
        self.m.cjs_paths.insert(canon, node);
        self.cjs_paths.insert(key, node);
        node
    }
}

/// `specifier === "<spec>"` (either order) on the wrapper's parameter.
fn is_spec_test(l: &Expr, r: &Expr, spec: &str) -> bool {
    matches!((l, r), (Expr::LocalGet(_), Expr::String(s)) | (Expr::String(s), Expr::LocalGet(_)) if s == spec)
}

/// Every string literal and literal property name in `e`, outermost
/// first (`require.cache["/abs/x.js"]` lowers to a named property read).
fn expr_strings(e: &Expr, f: &mut dyn FnMut(&str)) {
    match e {
        Expr::String(s) => f(s),
        Expr::PropertyGet { property, .. } => f(property),
        _ => {}
    }
    let mut kids: Vec<&Expr> = Vec::new();
    perry_hir::walker::walk_expr_children(e, &mut |c| kids.push(c));
    for k in kids {
        expr_strings(k, f);
    }
}

fn spec_local(s: &ImportSpecifier) -> String {
    match s {
        ImportSpecifier::Named { local, .. }
        | ImportSpecifier::Default { local }
        | ImportSpecifier::Namespace { local } => local.clone(),
    }
}

fn canonical_path(p: &str) -> String {
    std::fs::canonicalize(p)
        .map(|x| x.to_string_lossy().into_owned())
        .unwrap_or_else(|_| p.to_string())
}

/// `let require = <closure>` statements in `st`, nested bodies included.
fn collect_require_lets<'a>(st: &'a Stmt, out: &mut Vec<(u32, &'a [Stmt])>) {
    let mut exprs: Vec<&'a Expr> = Vec::new();
    match st {
        Stmt::Let { id, name, init, .. } => {
            if let Some(e) = init {
                if let Expr::Closure { body, .. } = e {
                    if name == "require" {
                        out.push((*id, body));
                    }
                }
                exprs.push(e);
            }
        }
        Stmt::Expr(e) | Stmt::Throw(e) => exprs.push(e),
        Stmt::Return(Some(e)) => exprs.push(e),
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            exprs.push(condition);
            for s in then_branch {
                collect_require_lets(s, out);
            }
            if let Some(b) = else_branch {
                for s in b {
                    collect_require_lets(s, out);
                }
            }
        }
        Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
            exprs.push(condition);
            for s in body {
                collect_require_lets(s, out);
            }
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(i) = init {
                collect_require_lets(i, out);
            }
            if let Some(c) = condition {
                exprs.push(c);
            }
            if let Some(u) = update {
                exprs.push(u);
            }
            for s in body {
                collect_require_lets(s, out);
            }
        }
        Stmt::Labeled { body, .. } => collect_require_lets(body, out),
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            for s in body {
                collect_require_lets(s, out);
            }
            if let Some(c) = catch {
                for s in &c.body {
                    collect_require_lets(s, out);
                }
            }
            if let Some(f) = finally {
                for s in f {
                    collect_require_lets(s, out);
                }
            }
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            exprs.push(discriminant);
            for c in cases {
                for s in &c.body {
                    collect_require_lets(s, out);
                }
            }
        }
        _ => {}
    }
    for e in exprs {
        collect_require_closures(e, out);
    }
}

/// Closure bodies inside `e`, searched for `let require = <closure>`.
fn collect_require_closures<'a>(e: &'a Expr, out: &mut Vec<(u32, &'a [Stmt])>) {
    if let Expr::Closure { body, .. } = e {
        for s in body {
            collect_require_lets(s, out);
        }
    }
    let mut kids: Vec<&'a Expr> = Vec::new();
    perry_hir::walker::walk_expr_children(e, &mut |c| kids.push(c));
    for k in kids {
        collect_require_closures(k, out);
    }
}
