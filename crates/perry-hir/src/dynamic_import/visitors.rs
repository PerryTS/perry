//! AST visitors for dynamic `import()` and `new Worker(...)` sites.
//!
//! Extracted from `dynamic_import.rs` (file-size split). Provides the
//! `for_each_dynamic_import*` / `for_each_worker_new*` walkers the driver
//! uses to find and rewrite every dynamic-import / worker-new node in a
//! freshly lowered module, plus their private statement/expression
//! recursion helpers.

use crate::ir::{Expr, Function, Module, Stmt};
use crate::walker::{walk_expr_children, walk_expr_children_mut};

/// Walk every expression in `module` (init statements, top-level functions,
/// class constructors/methods/getters/setters, field initializers, etc.)
/// and invoke `f` with each `&mut Expr::DynamicImport` node found.
///
/// Used by the driver to run [`resolve_import_path`] over every dynamic
/// import site in a freshly lowered module so it can register the
/// resolved targets in the import graph and stamp `paths` on each node.
pub fn for_each_dynamic_import_mut<F: FnMut(&mut Expr)>(module: &mut Module, f: &mut F) {
    for stmt in &mut module.init {
        visit_stmt_for_dyn_imports(stmt, f);
    }
    for func in &mut module.functions {
        visit_function_for_dyn_imports(func, f);
    }
    for cls in &mut module.classes {
        if let Some(ctor) = &mut cls.constructor {
            visit_function_for_dyn_imports(ctor, f);
        }
        for m in &mut cls.methods {
            visit_function_for_dyn_imports(m, f);
        }
        for (_, g) in &mut cls.getters {
            visit_function_for_dyn_imports(g, f);
        }
        for (_, s) in &mut cls.setters {
            visit_function_for_dyn_imports(s, f);
        }
        for m in &mut cls.static_methods {
            visit_function_for_dyn_imports(m, f);
        }
        for field in &mut cls.fields {
            if let Some(init) = &mut field.init {
                visit_expr_for_dyn_imports(init, f);
            }
        }
        for field in &mut cls.static_fields {
            if let Some(init) = &mut field.init {
                visit_expr_for_dyn_imports(init, f);
            }
        }
    }
    for global in &mut module.globals {
        if let Some(init) = &mut global.init {
            visit_expr_for_dyn_imports(init, f);
        }
    }
}

/// Immutable sibling of [`for_each_dynamic_import_mut`], used when callers
/// need to inspect dynamic imports while holding borrowed module-local consts.
pub fn for_each_dynamic_import<F: FnMut(&Expr)>(module: &Module, f: &mut F) {
    for stmt in &module.init {
        visit_stmt_for_dyn_imports_ref(stmt, f);
    }
    for func in &module.functions {
        visit_function_for_dyn_imports_ref(func, f);
    }
    for cls in &module.classes {
        if let Some(ctor) = &cls.constructor {
            visit_function_for_dyn_imports_ref(ctor, f);
        }
        for m in &cls.methods {
            visit_function_for_dyn_imports_ref(m, f);
        }
        for (_, g) in &cls.getters {
            visit_function_for_dyn_imports_ref(g, f);
        }
        for (_, s) in &cls.setters {
            visit_function_for_dyn_imports_ref(s, f);
        }
        for m in &cls.static_methods {
            visit_function_for_dyn_imports_ref(m, f);
        }
        for field in &cls.fields {
            if let Some(init) = &field.init {
                visit_expr_for_dyn_imports_ref(init, f);
            }
        }
        for field in &cls.static_fields {
            if let Some(init) = &field.init {
                visit_expr_for_dyn_imports_ref(init, f);
            }
        }
    }
    for global in &module.globals {
        if let Some(init) = &global.init {
            visit_expr_for_dyn_imports_ref(init, f);
        }
    }
}

/// Walk every expression in `module` and invoke `f` with each
/// `&mut Expr::WorkerNew` node found. Worker filenames use the same
/// deterministic resolver as dynamic `import()`, but they lower to a
/// different runtime shape at codegen.
pub fn for_each_worker_new_mut<F: FnMut(&mut Expr)>(module: &mut Module, f: &mut F) {
    for stmt in &mut module.init {
        visit_stmt_for_worker_new(stmt, f);
    }
    for func in &mut module.functions {
        visit_function_for_worker_new(func, f);
    }
    for cls in &mut module.classes {
        if let Some(ctor) = &mut cls.constructor {
            visit_function_for_worker_new(ctor, f);
        }
        for m in &mut cls.methods {
            visit_function_for_worker_new(m, f);
        }
        for (_, g) in &mut cls.getters {
            visit_function_for_worker_new(g, f);
        }
        for (_, s) in &mut cls.setters {
            visit_function_for_worker_new(s, f);
        }
        for m in &mut cls.static_methods {
            visit_function_for_worker_new(m, f);
        }
        for field in &mut cls.fields {
            if let Some(init) = &mut field.init {
                visit_expr_for_worker_new(init, f);
            }
        }
        for field in &mut cls.static_fields {
            if let Some(init) = &mut field.init {
                visit_expr_for_worker_new(init, f);
            }
        }
    }
    for global in &mut module.globals {
        if let Some(init) = &mut global.init {
            visit_expr_for_worker_new(init, f);
        }
    }
}

/// Immutable sibling of [`for_each_worker_new_mut`].
pub fn for_each_worker_new<F: FnMut(&Expr)>(module: &Module, f: &mut F) {
    for stmt in &module.init {
        visit_stmt_for_worker_new_ref(stmt, f);
    }
    for func in &module.functions {
        visit_function_for_worker_new_ref(func, f);
    }
    for cls in &module.classes {
        if let Some(ctor) = &cls.constructor {
            visit_function_for_worker_new_ref(ctor, f);
        }
        for m in &cls.methods {
            visit_function_for_worker_new_ref(m, f);
        }
        for (_, g) in &cls.getters {
            visit_function_for_worker_new_ref(g, f);
        }
        for (_, s) in &cls.setters {
            visit_function_for_worker_new_ref(s, f);
        }
        for m in &cls.static_methods {
            visit_function_for_worker_new_ref(m, f);
        }
        for field in &cls.fields {
            if let Some(init) = &field.init {
                visit_expr_for_worker_new_ref(init, f);
            }
        }
        for field in &cls.static_fields {
            if let Some(init) = &field.init {
                visit_expr_for_worker_new_ref(init, f);
            }
        }
    }
    for global in &module.globals {
        if let Some(init) = &global.init {
            visit_expr_for_worker_new_ref(init, f);
        }
    }
}

fn visit_function_for_dyn_imports<F: FnMut(&mut Expr)>(func: &mut Function, f: &mut F) {
    for stmt in &mut func.body {
        visit_stmt_for_dyn_imports(stmt, f);
    }
    for param in &mut func.params {
        if let Some(default) = &mut param.default {
            visit_expr_for_dyn_imports(default, f);
        }
    }
}

fn visit_function_for_dyn_imports_ref<F: FnMut(&Expr)>(func: &Function, f: &mut F) {
    for stmt in &func.body {
        visit_stmt_for_dyn_imports_ref(stmt, f);
    }
    for param in &func.params {
        if let Some(default) = &param.default {
            visit_expr_for_dyn_imports_ref(default, f);
        }
    }
}

fn visit_stmt_for_dyn_imports<F: FnMut(&mut Expr)>(stmt: &mut Stmt, f: &mut F) {
    match stmt {
        Stmt::Let { init, .. } => {
            if let Some(e) = init {
                visit_expr_for_dyn_imports(e, f);
            }
        }
        Stmt::Expr(e) => visit_expr_for_dyn_imports(e, f),
        Stmt::Return(opt) => {
            if let Some(e) = opt {
                visit_expr_for_dyn_imports(e, f);
            }
        }
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            visit_expr_for_dyn_imports(condition, f);
            for s in then_branch {
                visit_stmt_for_dyn_imports(s, f);
            }
            if let Some(eb) = else_branch {
                for s in eb {
                    visit_stmt_for_dyn_imports(s, f);
                }
            }
        }
        Stmt::While { condition, body } => {
            visit_expr_for_dyn_imports(condition, f);
            for s in body {
                visit_stmt_for_dyn_imports(s, f);
            }
        }
        Stmt::DoWhile { body, condition } => {
            for s in body {
                visit_stmt_for_dyn_imports(s, f);
            }
            visit_expr_for_dyn_imports(condition, f);
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(i) = init {
                visit_stmt_for_dyn_imports(i, f);
            }
            if let Some(c) = condition {
                visit_expr_for_dyn_imports(c, f);
            }
            if let Some(u) = update {
                visit_expr_for_dyn_imports(u, f);
            }
            for s in body {
                visit_stmt_for_dyn_imports(s, f);
            }
        }
        Stmt::Labeled { body, .. } => visit_stmt_for_dyn_imports(body, f),
        Stmt::Throw(e) => visit_expr_for_dyn_imports(e, f),
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            for s in body {
                visit_stmt_for_dyn_imports(s, f);
            }
            if let Some(c) = catch {
                for s in &mut c.body {
                    visit_stmt_for_dyn_imports(s, f);
                }
            }
            if let Some(fb) = finally {
                for s in fb {
                    visit_stmt_for_dyn_imports(s, f);
                }
            }
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            visit_expr_for_dyn_imports(discriminant, f);
            for c in cases {
                if let Some(t) = &mut c.test {
                    visit_expr_for_dyn_imports(t, f);
                }
                for s in &mut c.body {
                    visit_stmt_for_dyn_imports(s, f);
                }
            }
        }
        Stmt::Break
        | Stmt::Continue
        | Stmt::LabeledBreak(_)
        | Stmt::LabeledContinue(_)
        | Stmt::PreallocateBoxes(_) => {}
    }
}

fn visit_stmt_for_dyn_imports_ref<F: FnMut(&Expr)>(stmt: &Stmt, f: &mut F) {
    match stmt {
        Stmt::Let { init, .. } => {
            if let Some(e) = init {
                visit_expr_for_dyn_imports_ref(e, f);
            }
        }
        Stmt::Expr(e) => visit_expr_for_dyn_imports_ref(e, f),
        Stmt::Return(opt) => {
            if let Some(e) = opt {
                visit_expr_for_dyn_imports_ref(e, f);
            }
        }
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            visit_expr_for_dyn_imports_ref(condition, f);
            for s in then_branch {
                visit_stmt_for_dyn_imports_ref(s, f);
            }
            if let Some(eb) = else_branch {
                for s in eb {
                    visit_stmt_for_dyn_imports_ref(s, f);
                }
            }
        }
        Stmt::While { condition, body } => {
            visit_expr_for_dyn_imports_ref(condition, f);
            for s in body {
                visit_stmt_for_dyn_imports_ref(s, f);
            }
        }
        Stmt::DoWhile { body, condition } => {
            for s in body {
                visit_stmt_for_dyn_imports_ref(s, f);
            }
            visit_expr_for_dyn_imports_ref(condition, f);
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(i) = init {
                visit_stmt_for_dyn_imports_ref(i, f);
            }
            if let Some(c) = condition {
                visit_expr_for_dyn_imports_ref(c, f);
            }
            if let Some(u) = update {
                visit_expr_for_dyn_imports_ref(u, f);
            }
            for s in body {
                visit_stmt_for_dyn_imports_ref(s, f);
            }
        }
        Stmt::Labeled { body, .. } => visit_stmt_for_dyn_imports_ref(body, f),
        Stmt::Throw(e) => visit_expr_for_dyn_imports_ref(e, f),
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            for s in body {
                visit_stmt_for_dyn_imports_ref(s, f);
            }
            if let Some(c) = catch {
                for s in &c.body {
                    visit_stmt_for_dyn_imports_ref(s, f);
                }
            }
            if let Some(fb) = finally {
                for s in fb {
                    visit_stmt_for_dyn_imports_ref(s, f);
                }
            }
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            visit_expr_for_dyn_imports_ref(discriminant, f);
            for case in cases {
                if let Some(test) = &case.test {
                    visit_expr_for_dyn_imports_ref(test, f);
                }
                for stmt in &case.body {
                    visit_stmt_for_dyn_imports_ref(stmt, f);
                }
            }
        }
        Stmt::Break
        | Stmt::Continue
        | Stmt::LabeledBreak(_)
        | Stmt::LabeledContinue(_)
        | Stmt::PreallocateBoxes(_) => {}
    }
}

fn visit_expr_for_dyn_imports<F: FnMut(&mut Expr)>(expr: &mut Expr, f: &mut F) {
    if matches!(expr, Expr::DynamicImport { .. }) {
        f(expr);
        // After f mutates the node, still descend into the (possibly
        // unchanged) `arg` so nested dynamic imports are visited.
        if let Expr::DynamicImport { arg, .. } = expr {
            visit_expr_for_dyn_imports(arg, f);
        }
        return;
    }
    // Closure bodies — descend manually (the walker intentionally
    // doesn't).
    if let Expr::Closure { body, .. } = expr {
        for s in body {
            visit_stmt_for_dyn_imports(s, f);
        }
    }
    walk_expr_children_mut(expr, &mut |child| visit_expr_for_dyn_imports(child, f));
}

fn visit_expr_for_dyn_imports_ref<F: FnMut(&Expr)>(expr: &Expr, f: &mut F) {
    if matches!(expr, Expr::DynamicImport { .. }) {
        f(expr);
    }
    if let Expr::Closure { body, .. } = expr {
        for s in body {
            visit_stmt_for_dyn_imports_ref(s, f);
        }
    }
    walk_expr_children(expr, &mut |child| visit_expr_for_dyn_imports_ref(child, f));
}

fn visit_function_for_worker_new<F: FnMut(&mut Expr)>(func: &mut Function, f: &mut F) {
    for stmt in &mut func.body {
        visit_stmt_for_worker_new(stmt, f);
    }
    for param in &mut func.params {
        if let Some(default) = &mut param.default {
            visit_expr_for_worker_new(default, f);
        }
    }
}

fn visit_function_for_worker_new_ref<F: FnMut(&Expr)>(func: &Function, f: &mut F) {
    for stmt in &func.body {
        visit_stmt_for_worker_new_ref(stmt, f);
    }
    for param in &func.params {
        if let Some(default) = &param.default {
            visit_expr_for_worker_new_ref(default, f);
        }
    }
}

fn visit_stmt_for_worker_new<F: FnMut(&mut Expr)>(stmt: &mut Stmt, f: &mut F) {
    match stmt {
        Stmt::Let { init, .. } => {
            if let Some(e) = init {
                visit_expr_for_worker_new(e, f);
            }
        }
        Stmt::Expr(e) => visit_expr_for_worker_new(e, f),
        Stmt::Return(opt) => {
            if let Some(e) = opt {
                visit_expr_for_worker_new(e, f);
            }
        }
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            visit_expr_for_worker_new(condition, f);
            for s in then_branch {
                visit_stmt_for_worker_new(s, f);
            }
            if let Some(eb) = else_branch {
                for s in eb {
                    visit_stmt_for_worker_new(s, f);
                }
            }
        }
        Stmt::While { condition, body } => {
            visit_expr_for_worker_new(condition, f);
            for s in body {
                visit_stmt_for_worker_new(s, f);
            }
        }
        Stmt::DoWhile { body, condition } => {
            for s in body {
                visit_stmt_for_worker_new(s, f);
            }
            visit_expr_for_worker_new(condition, f);
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(i) = init {
                visit_stmt_for_worker_new(i, f);
            }
            if let Some(c) = condition {
                visit_expr_for_worker_new(c, f);
            }
            if let Some(u) = update {
                visit_expr_for_worker_new(u, f);
            }
            for s in body {
                visit_stmt_for_worker_new(s, f);
            }
        }
        Stmt::Labeled { body, .. } => visit_stmt_for_worker_new(body, f),
        Stmt::Throw(e) => visit_expr_for_worker_new(e, f),
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            for s in body {
                visit_stmt_for_worker_new(s, f);
            }
            if let Some(c) = catch {
                for s in &mut c.body {
                    visit_stmt_for_worker_new(s, f);
                }
            }
            if let Some(fb) = finally {
                for s in fb {
                    visit_stmt_for_worker_new(s, f);
                }
            }
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            visit_expr_for_worker_new(discriminant, f);
            for c in cases {
                if let Some(t) = &mut c.test {
                    visit_expr_for_worker_new(t, f);
                }
                for s in &mut c.body {
                    visit_stmt_for_worker_new(s, f);
                }
            }
        }
        Stmt::Break
        | Stmt::Continue
        | Stmt::LabeledBreak(_)
        | Stmt::LabeledContinue(_)
        | Stmt::PreallocateBoxes(_) => {}
    }
}

fn visit_stmt_for_worker_new_ref<F: FnMut(&Expr)>(stmt: &Stmt, f: &mut F) {
    match stmt {
        Stmt::Let { init, .. } => {
            if let Some(e) = init {
                visit_expr_for_worker_new_ref(e, f);
            }
        }
        Stmt::Expr(e) => visit_expr_for_worker_new_ref(e, f),
        Stmt::Return(opt) => {
            if let Some(e) = opt {
                visit_expr_for_worker_new_ref(e, f);
            }
        }
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            visit_expr_for_worker_new_ref(condition, f);
            for s in then_branch {
                visit_stmt_for_worker_new_ref(s, f);
            }
            if let Some(eb) = else_branch {
                for s in eb {
                    visit_stmt_for_worker_new_ref(s, f);
                }
            }
        }
        Stmt::While { condition, body } => {
            visit_expr_for_worker_new_ref(condition, f);
            for s in body {
                visit_stmt_for_worker_new_ref(s, f);
            }
        }
        Stmt::DoWhile { body, condition } => {
            for s in body {
                visit_stmt_for_worker_new_ref(s, f);
            }
            visit_expr_for_worker_new_ref(condition, f);
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(i) = init {
                visit_stmt_for_worker_new_ref(i, f);
            }
            if let Some(c) = condition {
                visit_expr_for_worker_new_ref(c, f);
            }
            if let Some(u) = update {
                visit_expr_for_worker_new_ref(u, f);
            }
            for s in body {
                visit_stmt_for_worker_new_ref(s, f);
            }
        }
        Stmt::Labeled { body, .. } => visit_stmt_for_worker_new_ref(body, f),
        Stmt::Throw(e) => visit_expr_for_worker_new_ref(e, f),
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            for s in body {
                visit_stmt_for_worker_new_ref(s, f);
            }
            if let Some(c) = catch {
                for s in &c.body {
                    visit_stmt_for_worker_new_ref(s, f);
                }
            }
            if let Some(fb) = finally {
                for s in fb {
                    visit_stmt_for_worker_new_ref(s, f);
                }
            }
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            visit_expr_for_worker_new_ref(discriminant, f);
            for case in cases {
                if let Some(test) = &case.test {
                    visit_expr_for_worker_new_ref(test, f);
                }
                for stmt in &case.body {
                    visit_stmt_for_worker_new_ref(stmt, f);
                }
            }
        }
        Stmt::Break
        | Stmt::Continue
        | Stmt::LabeledBreak(_)
        | Stmt::LabeledContinue(_)
        | Stmt::PreallocateBoxes(_) => {}
    }
}

fn visit_expr_for_worker_new<F: FnMut(&mut Expr)>(expr: &mut Expr, f: &mut F) {
    if matches!(expr, Expr::WorkerNew { .. }) {
        f(expr);
        if let Expr::WorkerNew {
            filename, options, ..
        } = expr
        {
            visit_expr_for_worker_new(filename, f);
            if let Some(options) = options {
                visit_expr_for_worker_new(options, f);
            }
        }
        return;
    }
    if let Expr::Closure { body, .. } = expr {
        for s in body {
            visit_stmt_for_worker_new(s, f);
        }
    }
    walk_expr_children_mut(expr, &mut |child| visit_expr_for_worker_new(child, f));
}

fn visit_expr_for_worker_new_ref<F: FnMut(&Expr)>(expr: &Expr, f: &mut F) {
    if matches!(expr, Expr::WorkerNew { .. }) {
        f(expr);
    }
    if let Expr::Closure { body, .. } = expr {
        for s in body {
            visit_stmt_for_worker_new_ref(s, f);
        }
    }
    walk_expr_children(expr, &mut |child| visit_expr_for_worker_new_ref(child, f));
}
