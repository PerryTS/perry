//! The closures of a module whose body runs at most once per evaluation of
//! the module, and only while it evaluates (RPG1b): the order its stores run
//! in is the order the walk meets them (`StoreSite::in_loop`, `derive`'s
//! ordered writers).
//!
//! A body is *once* when it is
//!
//! * the module's top level (`<init>`, the caller's part), or
//! * a closure called directly (`(function(){…})()`) from a position of a
//!   once body that is not inside a loop, or
//! * a closure bound by a declaration of a once body whose binding is never
//!   written and read exactly once, as the callee of a call at such a
//!   position (the CommonJS wrapper's `function __perry_cjs_factory(){…}
//!   return __perry_cjs_factory();`).
//!
//! Async and generator closures are never once (their body is re-entered by
//! a step function), nor a closure that names `callee` (sloppy
//! `arguments.callee` reaches the running function again), nor any closure
//! of a module with a `with` statement (its references are by name).
//!
//! One read is exempt: the factory a wrapper hands the runtime's path
//! registry (`registerPathModulePartial`). The runtime calls it only to
//! evaluate the module again after its `require.cache` entry was deleted.
//! Such a body is once *with reload*: `derive` takes it as once only while
//! the program's cache has no deletion (it is not HARD, escaped or ⊤).
//!
//! The same shape `perry_hir::lower::run_once` admits for class definitions,
//! computed here on the lowered HIR. Nothing of it reaches the binary.

use perry_hir::walker::walk_expr_children;
use perry_hir::{Expr, Module, Stmt};

use crate::pts::FxHashMap;

/// The once closures of `module` by function id: `true` when the body is
/// once with reload. Empty for a module with top-level await (its
/// evaluation interleaves with other modules') or with a `with` statement.
pub(crate) fn once_closures(module: &Module) -> FxHashMap<u32, bool> {
    let mut out = FxHashMap::default();
    if module.has_top_level_await {
        return out;
    }
    let mut refs = Refs::default();
    let mut with = false;
    perry_hir::tdz_check::for_each_module_expr(module, &mut |e| match e {
        Expr::LocalGet(id) => *refs.reads.entry(*id).or_default() += 1,
        Expr::LocalSet(id, _) | Expr::Update { id, .. } => {
            *refs.writes.entry(*id).or_default() += 1
        }
        Expr::NativeMethodCall { method, args, .. } if method == "registerPathModulePartial" => {
            if let Some(Expr::LocalGet(id)) = args.get(2) {
                *refs.registry.entry(*id).or_default() += 1;
            }
        }
        Expr::WithGet { .. } | Expr::WithSet { .. } => with = true,
        _ => {}
    });
    if with {
        return out;
    }
    let mut w = Walk {
        refs: &refs,
        out: &mut out,
        lets: FxHashMap::default(),
    };
    w.body(&module.init, false);
    out
}

#[derive(Default)]
struct Refs {
    reads: FxHashMap<u32, u32>,
    writes: FxHashMap<u32, u32>,
    /// Reads as the registry's factory argument (also counted in `reads`).
    registry: FxHashMap<u32, u32>,
}

struct Walk<'r, 'a> {
    refs: &'r Refs,
    out: &'r mut FxHashMap<u32, bool>,
    /// Closures bound by a declaration of the body being walked.
    lets: FxHashMap<u32, &'a Expr>,
}

impl<'r, 'a> Walk<'r, 'a> {
    /// A once body (`reload`: once with reload).
    fn body(&mut self, stmts: &'a [Stmt], reload: bool) {
        let saved = std::mem::take(&mut self.lets);
        collect_lets(stmts, &mut self.lets);
        self.stmts(stmts, false, reload);
        self.lets = saved;
    }

    fn stmts(&mut self, stmts: &'a [Stmt], in_loop: bool, reload: bool) {
        for s in stmts {
            self.stmt(s, in_loop, reload);
        }
    }

    fn stmt(&mut self, s: &'a Stmt, in_loop: bool, reload: bool) {
        match s {
            Stmt::Let { init: Some(e), .. } => self.expr(e, in_loop, reload),
            Stmt::Expr(e) | Stmt::Throw(e) | Stmt::Return(Some(e)) => self.expr(e, in_loop, reload),
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                self.expr(condition, in_loop, reload);
                self.stmts(then_branch, in_loop, reload);
                if let Some(e) = else_branch {
                    self.stmts(e, in_loop, reload);
                }
            }
            Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
                self.expr(condition, true, reload);
                self.stmts(body, true, reload);
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(i) = init {
                    self.stmt(i, in_loop, reload);
                }
                if let Some(c) = condition {
                    self.expr(c, true, reload);
                }
                if let Some(u) = update {
                    self.expr(u, true, reload);
                }
                self.stmts(body, true, reload);
            }
            Stmt::Labeled { body, .. } => self.stmt(body, in_loop, reload),
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                self.stmts(body, in_loop, reload);
                if let Some(c) = catch {
                    self.stmts(&c.body, in_loop, reload);
                }
                if let Some(f) = finally {
                    self.stmts(f, in_loop, reload);
                }
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                self.expr(discriminant, in_loop, reload);
                for c in cases {
                    if let Some(t) = &c.test {
                        self.expr(t, in_loop, reload);
                    }
                    self.stmts(&c.body, in_loop, reload);
                }
            }
            // No expression, or none this walk follows: its closures stay
            // not once.
            _ => {}
        }
    }

    fn expr(&mut self, e: &'a Expr, in_loop: bool, reload: bool) {
        if let (Expr::Call { callee, .. }, false) = (e, in_loop) {
            match callee.as_ref() {
                Expr::Closure { .. } => self.once(callee, reload),
                Expr::LocalGet(id) => {
                    if let Some(&c) = self.lets.get(id) {
                        let r = self.refs;
                        let reads = r.reads.get(id).copied().unwrap_or(0);
                        let registry = r.registry.get(id).copied().unwrap_or(0);
                        if r.writes.get(id).copied().unwrap_or(0) == 0 && reads == 1 + registry {
                            self.once(c, reload || registry > 0);
                        }
                    }
                }
                _ => {}
            }
        }
        // A closure's body is not this body's: walked only when once.
        walk_expr_children(e, &mut |c| self.expr(c, in_loop, reload));
    }

    fn once(&mut self, c: &'a Expr, reload: bool) {
        let Expr::Closure {
            func_id,
            body,
            is_async,
            is_generator,
            ..
        } = c
        else {
            return;
        };
        if *is_async || *is_generator || names_callee(body) || self.out.contains_key(func_id) {
            return;
        }
        self.out.insert(*func_id, reload);
        self.body(body, reload);
    }
}

/// The closures `let`/`const`/function declarations of `stmts` bind, in
/// nested blocks too (not in nested closures).
fn collect_lets<'a>(stmts: &'a [Stmt], out: &mut FxHashMap<u32, &'a Expr>) {
    for s in stmts {
        collect_let(s, out);
    }
}

fn collect_let<'a>(s: &'a Stmt, out: &mut FxHashMap<u32, &'a Expr>) {
    match s {
        Stmt::Let {
            id,
            init: Some(c @ Expr::Closure { .. }),
            ..
        } => {
            out.insert(*id, c);
        }
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            collect_lets(then_branch, out);
            if let Some(e) = else_branch {
                collect_lets(e, out);
            }
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => collect_lets(body, out),
        Stmt::For { init, body, .. } => {
            if let Some(i) = init {
                collect_let(i, out);
            }
            collect_lets(body, out);
        }
        Stmt::Labeled { body, .. } => collect_let(body, out),
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            collect_lets(body, out);
            if let Some(c) = catch {
                collect_lets(&c.body, out);
            }
            if let Some(f) = finally {
                collect_lets(f, out);
            }
        }
        Stmt::Switch { cases, .. } => {
            for c in cases {
                collect_lets(&c.body, out);
            }
        }
        _ => {}
    }
}

/// Does the body read a property named `callee` (`arguments.callee`)?
fn names_callee(body: &[Stmt]) -> bool {
    let mut found = false;
    perry_hir::tdz_check::for_each_stmts_expr(body, &mut |e| {
        if let Expr::PropertyGet { property, .. } = e {
            found |= property == "callee";
        }
    });
    found
}
