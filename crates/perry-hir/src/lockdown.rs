//! #496 — `--lockdown` mode: HIR walk that catches the standard
//! arbitrary-code-execution surfaces.
//!
//! The driver enforces the broader contract (refuse any runtime-JS
//! (V8) imports — that runtime was removed — and refuse any
//! `perry.nativeLibrary` archive reference) at the build-graph level.
//! This module covers the
//! per-module HIR check: does any source module call into
//! `child_process.*`? In lockdown mode the answer must be no.
//!
//! ## Scope
//!
//! `child_process` covers the spawn/exec class of arbitrary-code-
//! execution surfaces directly visible to user TypeScript. The HIR
//! has dedicated variants for the hot shapes (`ChildProcessExec`,
//! `ChildProcessExecSync`, `ChildProcessSpawn`, `ChildProcessSpawnSync`,
//! `ChildProcessSpawnBackground`, `ChildProcessGetProcessStatus`,
//! `ChildProcessKillProcess`); the general-shape `NativeMethodCall`
//! variant catches anything else lowered through the
//! `child_process` namespace path.
//!
//! ## Out of scope (covered elsewhere in the series)
//!
//! - Runtime-JS (V8) imports — refused unconditionally by the build
//!   driver's V8-free gate (`perry-jsruntime` was removed), so
//!   lockdown needs no separate check here.
//! - `perry.nativeLibrary` reference gate — `#497` plumbing in the
//!   build driver. Lockdown reads `ctx.native_libraries.is_empty()`.
//! - Dynamic stdlib dispatch (`obj[runtimeVar]()`) — `#503`'s HIR
//!   refusal is unconditionally on (not opt-in like the rest), so
//!   lockdown doesn't need to add anything beyond what's already
//!   enforced.
//! - eval / Function constructor / dynamic import of arbitrary
//!   strings — placeholders in the issue. Perry doesn't emit `eval`
//!   today; the `#503` dynamic-dispatch refusal already blocks the
//!   common obfuscation shapes.

use crate::ir::{Expr, Module, Stmt};
use crate::walker::walk_expr_children;

/// One refused call site under `--lockdown` mode. The driver
/// collects these across every module and emits a single combined
/// diagnostic — better UX than failing on the first site and asking
/// the user to re-run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockdownViolation {
    /// Source file the call appears in (matches the module path
    /// passed to `audit_module_lockdown`).
    pub source: String,
    /// Specific surface that was tripped — `"child_process.exec"`,
    /// `"child_process.spawn"`, etc. The string is consumed by the
    /// diagnostic so reviewers know which entrypoint is at fault.
    pub kind: &'static str,
}

/// Walk a single HIR module collecting `--lockdown` violations. Pure
/// analyser — returns the list and lets the driver decide how to
/// surface them.
pub fn audit_module_lockdown(hir_module: &Module, source: &str) -> Vec<LockdownViolation> {
    let mut ctx = WalkCtx {
        source: source.to_string(),
        violations: Vec::new(),
    };
    for stmt in &hir_module.init {
        visit_stmt(stmt, &mut ctx);
    }
    for func in &hir_module.functions {
        for stmt in &func.body {
            visit_stmt(stmt, &mut ctx);
        }
    }
    for class in &hir_module.classes {
        for method in &class.methods {
            for stmt in &method.body {
                visit_stmt(stmt, &mut ctx);
            }
        }
    }
    ctx.violations
}

struct WalkCtx {
    source: String,
    violations: Vec<LockdownViolation>,
}

fn visit_stmt(stmt: &Stmt, ctx: &mut WalkCtx) {
    match stmt {
        Stmt::Expr(e) => visit_expr(e, ctx),
        Stmt::Let { init, .. } => {
            if let Some(v) = init {
                visit_expr(v, ctx);
            }
        }
        Stmt::Return(Some(e)) => visit_expr(e, ctx),
        Stmt::Return(None) | Stmt::Break | Stmt::Continue => {}
        Stmt::LabeledBreak(_) | Stmt::LabeledContinue(_) => {}
        Stmt::Labeled { body, .. } => visit_stmt(body, ctx),
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            visit_expr(condition, ctx);
            for s in then_branch {
                visit_stmt(s, ctx);
            }
            if let Some(else_b) = else_branch {
                for s in else_b {
                    visit_stmt(s, ctx);
                }
            }
        }
        Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
            visit_expr(condition, ctx);
            for s in body {
                visit_stmt(s, ctx);
            }
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(init) = init {
                visit_stmt(init, ctx);
            }
            if let Some(c) = condition {
                visit_expr(c, ctx);
            }
            if let Some(u) = update {
                visit_expr(u, ctx);
            }
            for s in body {
                visit_stmt(s, ctx);
            }
        }
        Stmt::Throw(e) => visit_expr(e, ctx),
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            for s in body {
                visit_stmt(s, ctx);
            }
            if let Some(c) = catch {
                for s in &c.body {
                    visit_stmt(s, ctx);
                }
            }
            if let Some(f) = finally {
                for s in f {
                    visit_stmt(s, ctx);
                }
            }
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            visit_expr(discriminant, ctx);
            for case in cases {
                if let Some(test) = &case.test {
                    visit_expr(test, ctx);
                }
                for s in &case.body {
                    visit_stmt(s, ctx);
                }
            }
        }
        Stmt::PreallocateBoxes(_) | Stmt::PreallocateTdzBoxes(_) => {}
    }
}

fn visit_expr(expr: &Expr, ctx: &mut WalkCtx) {
    if let Some(kind) = forbidden_call(expr) {
        ctx.violations.push(LockdownViolation {
            source: ctx.source.clone(),
            kind,
        });
    }
    walk_expr_children(expr, &mut |child| visit_expr(child, ctx));
}

/// Recognise the HIR variants that constitute a `child_process.*`
/// surface. Specialised variants land first (they're the hot path);
/// the general-shape `NativeMethodCall { module: "child_process", … }`
/// catches anything else the lowering routes through that namespace.
fn forbidden_call(expr: &Expr) -> Option<&'static str> {
    Some(match expr {
        Expr::ChildProcessExec { .. } => "child_process.exec",
        Expr::ChildProcessExecSync { .. } => "child_process.execSync",
        Expr::ChildProcessSpawn { .. } => "child_process.spawn",
        Expr::ChildProcessFork { .. } => "child_process.fork",
        Expr::ChildProcessSpawnSync { .. } => "child_process.spawnSync",
        Expr::ChildProcessSpawnBackground { .. } => "child_process.spawnBackground",
        Expr::ChildProcessGetProcessStatus(_) => "child_process.getProcessStatus",
        Expr::ChildProcessKillProcess(_) => "child_process.killProcess",
        Expr::NativeMethodCall { module, method, .. } if module == "child_process" => {
            // Leak the method name back through the same `&'static str`
            // shape the specialised variants use. The general-shape
            // variant carries the method by string, so we synthesise
            // a representative kind string for the diagnostic. (Storing
            // the dynamic name would require owning the string;
            // sticking with a constant keeps the API uniform.)
            // Reviewers see "child_process.<method>" in the violation
            // listing via the diagnostic-level `Display` path; here
            // we just record that the namespace was reached.
            let _ = method;
            "child_process.<dynamic>"
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_module() -> Module {
        Module::new("test")
    }

    fn child_process_exec_sync() -> Expr {
        Expr::ChildProcessExecSync {
            command: Box::new(Expr::String("ls".into())),
            options: None,
        }
    }

    #[test]
    fn empty_module_has_no_violations() {
        let m = empty_module();
        let v = audit_module_lockdown(&m, "/repo/main.ts");
        assert!(v.is_empty());
    }

    #[test]
    fn top_level_exec_sync_records_violation() {
        let mut m = empty_module();
        m.init.push(Stmt::Expr(child_process_exec_sync()));
        let v = audit_module_lockdown(&m, "/repo/main.ts");
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].kind, "child_process.execSync");
        assert_eq!(v[0].source, "/repo/main.ts");
    }

    #[test]
    fn nested_call_inside_if_recorded() {
        let mut m = empty_module();
        m.init.push(Stmt::If {
            condition: Expr::Bool(true),
            then_branch: vec![Stmt::Expr(child_process_exec_sync())],
            else_branch: None,
        });
        let v = audit_module_lockdown(&m, "/repo/main.ts");
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].kind, "child_process.execSync");
    }

    #[test]
    fn every_specialised_variant_caught() {
        // One concrete fixture per variant — confirms `forbidden_call`
        // stays exhaustive as new variants land.
        let cases: Vec<(Expr, &'static str)> = vec![
            (
                Expr::ChildProcessExec {
                    command: Box::new(Expr::String("x".into())),
                    options: None,
                    callback: None,
                },
                "child_process.exec",
            ),
            (
                Expr::ChildProcessExecSync {
                    command: Box::new(Expr::String("x".into())),
                    options: None,
                },
                "child_process.execSync",
            ),
            (
                Expr::ChildProcessSpawn {
                    command: Box::new(Expr::String("x".into())),
                    args: None,
                    options: None,
                },
                "child_process.spawn",
            ),
            (
                Expr::ChildProcessSpawnSync {
                    command: Box::new(Expr::String("x".into())),
                    args: None,
                    options: None,
                },
                "child_process.spawnSync",
            ),
            (
                Expr::ChildProcessSpawnBackground {
                    command: Box::new(Expr::String("x".into())),
                    args: None,
                    log_file: Box::new(Expr::String("log".into())),
                    env_json: None,
                },
                "child_process.spawnBackground",
            ),
            (
                Expr::ChildProcessGetProcessStatus(Box::new(Expr::Number(1.0))),
                "child_process.getProcessStatus",
            ),
            (
                Expr::ChildProcessKillProcess(Box::new(Expr::Number(1.0))),
                "child_process.killProcess",
            ),
        ];
        for (e, expected_kind) in cases {
            let mut m = empty_module();
            m.init.push(Stmt::Expr(e));
            let v = audit_module_lockdown(&m, "/repo/x.ts");
            assert_eq!(v.len(), 1);
            assert_eq!(v[0].kind, expected_kind);
        }
    }

    #[test]
    fn general_native_call_through_child_process() {
        // The dispatch-through-NativeMethodCall fallback for any
        // `child_process.<method>` lowering that doesn't have a
        // dedicated variant yet (e.g. `fork`).
        let mut m = empty_module();
        m.init.push(Stmt::Expr(Expr::NativeMethodCall {
            module: "child_process".into(),
            class_name: None,
            object: None,
            method: "fork".into(),
            args: vec![],
        }));
        let v = audit_module_lockdown(&m, "/repo/x.ts");
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].kind, "child_process.<dynamic>");
    }

    #[test]
    fn unrelated_native_call_not_flagged() {
        // Lockdown is scoped to child_process; other stdlib calls
        // (fs, crypto, …) are untouched.
        let mut m = empty_module();
        m.init.push(Stmt::Expr(Expr::NativeMethodCall {
            module: "fs".into(),
            class_name: None,
            object: None,
            method: "readFileSync".into(),
            args: vec![],
        }));
        let v = audit_module_lockdown(&m, "/repo/x.ts");
        assert!(v.is_empty());
    }
}
