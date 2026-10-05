//! Temporal Dead Zone for module-level `let`/`const` bindings (#11826).
//!
//! Module evaluation runs `module.init` once, in order, so a module-level
//! lexical binding `X` declared by the top-level statement at index `d(X)` is
//! uninitialized exactly until that statement completes. The rule is static:
//!
//! 1. A read or write of `X` in top-level code at index `i <= d(X)` that is
//!    not inside a function always runs in the dead zone (the statement at
//!    `d(X)` itself only reaches it from `X`'s own initializer). It becomes the
//!    ReferenceError throw. Nothing at a later index ever sees the dead zone.
//! 2. A function body can run before `d(X)` completes only if it can be
//!    reached by then. Its *reach position* `p` is the first top-level index
//!    from which it can run:
//!    - a closure is created by the statement that contains it (or by the
//!      body that contains it, transitively), so `p` is that statement's index
//!      or the enclosing body's `p`;
//!    - a hoisted function declaration exists from the start, but it can only
//!      run once something refers to it: `p` is the least index of a top-level
//!      statement, or the least `p` of a body, that refers to it. A function
//!      the module exports is reachable from the start when an importer can
//!      run first (an import cycle), and so is a script-global function. A
//!      function value exists only through a reference to it (strict module
//!      code has no `with`, a module's functions are not global properties,
//!      and perry has no runtime `eval` that could name one), so a function
//!      nothing refers to and nothing exports never runs and needs no check;
//!    - a class's members exist from its definition on (reading a class
//!      before its definition is itself a dead-zone access), so its bodies
//!      take the class's position.
//!    Only a body with `p <= d(X)` can observe `X` uninitialized, and only
//!    its reads of `X` get the check (`crate::tdz_check`). Every other read is
//!    left exactly as it was.
//!
//! The check reads the binding's slot, which a backend seeds with the TDZ
//! sentinel; nothing else changes for a binding no check names.

use std::collections::{HashMap, HashSet};

use crate::ir::*;
use crate::tdz_check::{self, for_each_stmt_part, Part};
use crate::types::{FuncId, LocalId};
use crate::walker::{walk_expr_children, walk_expr_children_mut};

/// Facts the lowering knows and the HIR does not carry.
pub(crate) struct ModuleTdzFacts<'a> {
    /// Ids of the module-level `let`/`const` bindings.
    pub lexical_ids: &'a HashSet<LocalId>,
    /// `module.init` length when each top-level class was defined. A class
    /// without an entry is treated as defined at the start.
    pub class_positions: &'a HashMap<String, usize>,
    /// True when an importer of this module can run before its body (the
    /// module takes part in, or may take part in, an import cycle).
    pub exports_may_run_early: bool,
}

pub(crate) fn apply(module: &mut Module, facts: &ModuleTdzFacts<'_>) {
    let mut decl: HashMap<LocalId, (usize, String)> = HashMap::new();
    for (index, stmt) in module.init.iter().enumerate() {
        if let Stmt::Let { id, name, .. } = stmt {
            if facts.lexical_ids.contains(id) {
                decl.entry(*id).or_insert_with(|| (index, name.clone()));
            }
        }
    }
    if decl.is_empty() {
        return;
    }
    let last_decl = decl.values().map(|(index, _)| *index).max().unwrap_or(0);
    let reach = function_positions(module, facts);

    for (index, stmt) in module.init.iter_mut().enumerate() {
        if index > last_decl {
            // Every binding is initialized; closures created here run later.
            break;
        }
        rw_stmt(
            stmt,
            &Rw {
                decl: &decl,
                threshold: index,
                direct_throws: true,
            },
        );
    }
    for function in &mut module.functions {
        // A function nothing refers to never runs (see `function_positions`).
        let Some(&threshold) = reach.get(&function.id) else {
            continue;
        };
        if threshold <= last_decl {
            rw_function(function, &Rw::checks(&decl, threshold));
        }
    }
    for class in &mut module.classes {
        let threshold = facts.class_positions.get(&class.name).copied().unwrap_or(0);
        if threshold <= last_decl {
            rw_class(class, &Rw::checks(&decl, threshold));
        }
    }
    // A checked `let x;` must end its dead zone when the declaration runs:
    // give it the `undefined` initializer it means.
    let checked = tdz_check::checked_ids(module);
    for stmt in &mut module.init {
        if let Stmt::Let { id, init, .. } = stmt {
            if init.is_none() && checked.contains(id) && decl.contains_key(id) {
                *init = Some(Expr::Undefined);
            }
        }
    }
}

// ─── Reach positions ────────────────────────────────────────────────────────

/// The reach position of every function that can run at all. A function
/// without an entry is never referred to, exported or global: it never runs.
fn function_positions(module: &Module, facts: &ModuleTdzFacts<'_>) -> HashMap<FuncId, usize> {
    let mut by_name: HashMap<&str, Vec<FuncId>> = HashMap::new();
    for function in &module.functions {
        by_name
            .entry(function.name.as_str())
            .or_default()
            .push(function.id);
    }
    let mut reach: HashMap<FuncId, usize> = HashMap::new();
    let mut work: Vec<FuncId> = Vec::new();
    fn lower(id: FuncId, pos: usize, reach: &mut HashMap<FuncId, usize>, work: &mut Vec<FuncId>) {
        if reach.get(&id).map_or(true, |old| pos < *old) {
            reach.insert(id, pos);
            work.push(id);
        }
    }

    if facts.exports_may_run_early {
        for function in &module.functions {
            let exported = function.is_exported
                || module
                    .exported_functions
                    .iter()
                    .any(|(_, id)| *id == function.id)
                || module.exports.iter().any(|export| {
                    matches!(export, Export::Named { local, .. } if *local == function.name)
                });
            if exported {
                lower(function.id, 0, &mut reach, &mut work);
            }
        }
    }
    for (_, id) in &module.script_global_functions {
        lower(*id, 0, &mut reach, &mut work);
    }
    for (index, stmt) in module.init.iter().enumerate() {
        let mut refs = Refs::new(&by_name);
        refs.stmt(stmt);
        for id in refs.out {
            lower(id, index, &mut reach, &mut work);
        }
    }
    for class in &module.classes {
        let pos = facts.class_positions.get(&class.name).copied().unwrap_or(0);
        let mut refs = Refs::new(&by_name);
        refs.class(class);
        refs.out
            .extend(class.static_accessor_fn_ids.iter().copied());
        for id in refs.out {
            lower(id, pos, &mut reach, &mut work);
        }
    }
    let bodies: HashMap<FuncId, &Function> = module.functions.iter().map(|f| (f.id, f)).collect();
    while let Some(id) = work.pop() {
        let Some(function) = bodies.get(&id) else {
            continue;
        };
        let pos = reach[&id];
        let mut refs = Refs::new(&by_name);
        refs.function(function);
        for callee in refs.out {
            lower(callee, pos, &mut reach, &mut work);
        }
    }
    reach
}

/// Every function a piece of HIR refers to, including through the bodies of
/// the closures it creates.
struct Refs<'a> {
    by_name: &'a HashMap<&'a str, Vec<FuncId>>,
    out: Vec<FuncId>,
}

impl<'a> Refs<'a> {
    fn new(by_name: &'a HashMap<&'a str, Vec<FuncId>>) -> Self {
        Refs {
            by_name,
            out: Vec::new(),
        }
    }

    fn name(&mut self, name: &str) {
        if let Some(ids) = self.by_name.get(name) {
            self.out.extend(ids.iter().copied());
        }
    }

    fn expr(&mut self, expr: &Expr) {
        match expr {
            Expr::FuncRef(id) => self.out.push(*id),
            Expr::ExternFuncRef { name, .. } => self.name(name),
            Expr::Closure { body, .. } => self.stmts(body),
            _ => {}
        }
        walk_expr_children(expr, &mut |child| self.expr(child));
    }

    fn stmts(&mut self, stmts: &[Stmt]) {
        for stmt in stmts {
            self.stmt(stmt);
        }
    }

    fn stmt(&mut self, stmt: &Stmt) {
        for_each_stmt_part(stmt, &mut |part| match part {
            Part::Expr(expr) => self.expr(expr),
            Part::Stmts(stmts) => self.stmts(stmts),
        });
    }

    fn decorators(&mut self, decorators: &[Decorator]) {
        for decorator in decorators {
            self.name(&decorator.name);
            for arg in &decorator.args {
                self.expr(arg);
            }
        }
    }

    fn function(&mut self, function: &Function) {
        self.decorators(&function.decorators);
        for param in &function.params {
            self.decorators(&param.decorators);
            if let Some(default) = &param.default {
                self.expr(default);
            }
        }
        self.stmts(&function.body);
    }

    fn field(&mut self, field: &ClassField) {
        self.decorators(&field.decorators);
        if let Some(key) = &field.key_expr {
            self.expr(key);
        }
        if let Some(init) = &field.init {
            self.expr(init);
        }
    }

    fn class(&mut self, class: &Class) {
        self.decorators(&class.decorators);
        if let Some(extends) = &class.extends_expr {
            self.expr(extends);
        }
        if let Some(ctor) = &class.constructor {
            self.function(ctor);
        }
        for function in class.methods.iter().chain(&class.static_methods) {
            self.function(function);
        }
        for (_, function) in class.getters.iter().chain(&class.setters) {
            self.function(function);
        }
        for member in &class.computed_members {
            self.expr(&member.key_expr);
            self.function(&member.function);
        }
        for field in class.fields.iter().chain(&class.static_fields) {
            self.field(field);
        }
    }
}

/// Whether an importer of this module can run before the module's body: true
/// when the module imports or re-exports another source module (an import
/// cycle needs an edge out of the module), or loads one at run time.
pub(crate) fn may_be_in_import_cycle(module: &Module) -> bool {
    if module
        .imports
        .iter()
        .any(|import| !import.type_only && !import.runtime_erased && !import.is_native)
    {
        return true;
    }
    if module.exports.iter().any(|export| {
        matches!(
            export,
            Export::ReExport { .. } | Export::ExportAll { .. } | Export::NamespaceReExport { .. }
        )
    }) {
        return true;
    }
    fn loads(expr: &Expr) -> bool {
        let mut found = matches!(expr, Expr::DynamicImport { .. });
        if let Expr::Closure { body, .. } = expr {
            found |= body.iter().any(stmt_loads);
        }
        walk_expr_children(expr, &mut |child| found |= loads(child));
        found
    }
    fn stmt_loads(stmt: &Stmt) -> bool {
        let mut found = false;
        for_each_stmt_part(stmt, &mut |part| match part {
            Part::Expr(expr) => found |= loads(expr),
            Part::Stmts(stmts) => found |= stmts.iter().any(stmt_loads),
        });
        found
    }
    let in_function = |function: &Function| {
        function.body.iter().any(stmt_loads)
            || function
                .params
                .iter()
                .any(|param| param.default.as_ref().is_some_and(loads))
    };
    module.init.iter().any(stmt_loads)
        || module.functions.iter().any(in_function)
        || module.classes.iter().any(|class| {
            class
                .constructor
                .iter()
                .chain(&class.methods)
                .chain(&class.static_methods)
                .chain(class.getters.iter().map(|(_, f)| f))
                .chain(class.setters.iter().map(|(_, f)| f))
                .chain(class.computed_members.iter().map(|m| &m.function))
                .any(in_function)
                || class
                    .fields
                    .iter()
                    .chain(&class.static_fields)
                    .any(|field| field.init.as_ref().is_some_and(loads))
        })
}

// ─── Rewrite ────────────────────────────────────────────────────────────────

struct Rw<'a> {
    decl: &'a HashMap<LocalId, (usize, String)>,
    /// A binding declared at index `>= threshold` may be uninitialized here.
    threshold: usize,
    /// True in top-level code outside any function: an access there always
    /// runs in the dead zone, so it throws without a check.
    direct_throws: bool,
}

impl<'a> Rw<'a> {
    fn checks(decl: &'a HashMap<LocalId, (usize, String)>, threshold: usize) -> Self {
        Rw {
            decl,
            threshold,
            direct_throws: false,
        }
    }

    fn in_function(&self) -> Self {
        Rw::checks(self.decl, self.threshold)
    }

    /// The binding's name when an access to `id` here can be in its dead zone.
    fn hit(&self, id: LocalId) -> Option<&'a str> {
        self.decl
            .get(&id)
            .filter(|(index, _)| *index >= self.threshold)
            .map(|(_, name)| name.as_str())
    }
}

fn rw_stmts(stmts: &mut [Stmt], rw: &Rw<'_>) {
    for stmt in stmts {
        rw_stmt(stmt, rw);
    }
}

fn rw_stmt(stmt: &mut Stmt, rw: &Rw<'_>) {
    match stmt {
        Stmt::Let { init, .. } => {
            if let Some(init) = init {
                rw_expr(init, rw);
            }
        }
        Stmt::Expr(expr) | Stmt::Throw(expr) => rw_expr(expr, rw),
        Stmt::Return(expr) => {
            if let Some(expr) = expr {
                rw_expr(expr, rw);
            }
        }
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            rw_expr(condition, rw);
            rw_stmts(then_branch, rw);
            if let Some(else_branch) = else_branch {
                rw_stmts(else_branch, rw);
            }
        }
        Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
            rw_expr(condition, rw);
            rw_stmts(body, rw);
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(init) = init {
                rw_stmt(init, rw);
            }
            if let Some(condition) = condition {
                rw_expr(condition, rw);
            }
            if let Some(update) = update {
                rw_expr(update, rw);
            }
            rw_stmts(body, rw);
        }
        Stmt::Labeled { body, .. } => rw_stmt(body, rw),
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            rw_stmts(body, rw);
            if let Some(catch) = catch {
                rw_stmts(&mut catch.body, rw);
            }
            if let Some(finally) = finally {
                rw_stmts(finally, rw);
            }
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            rw_expr(discriminant, rw);
            for case in cases {
                if let Some(test) = &mut case.test {
                    rw_expr(test, rw);
                }
                rw_stmts(&mut case.body, rw);
            }
        }
        Stmt::Break
        | Stmt::Continue
        | Stmt::LabeledBreak(_)
        | Stmt::LabeledContinue(_)
        | Stmt::PreallocateBoxes(_)
        | Stmt::PreallocateTdzBoxes(_)
        | Stmt::ReleaseBoxes(_) => {}
    }
}

fn rw_function(function: &mut Function, rw: &Rw<'_>) {
    for param in &mut function.params {
        if let Some(default) = &mut param.default {
            rw_expr(default, rw);
        }
    }
    rw_stmts(&mut function.body, rw);
}

fn rw_class(class: &mut Class, rw: &Rw<'_>) {
    for decorator in &mut class.decorators {
        for arg in &mut decorator.args {
            rw_expr(arg, rw);
        }
    }
    if let Some(extends) = &mut class.extends_expr {
        rw_expr(extends, rw);
    }
    if let Some(ctor) = &mut class.constructor {
        rw_function(ctor, rw);
    }
    for function in class
        .methods
        .iter_mut()
        .chain(class.static_methods.iter_mut())
    {
        rw_function(function, rw);
    }
    for (_, function) in class.getters.iter_mut().chain(class.setters.iter_mut()) {
        rw_function(function, rw);
    }
    for member in &mut class.computed_members {
        rw_expr(&mut member.key_expr, rw);
        rw_function(&mut member.function, rw);
    }
    for field in class
        .fields
        .iter_mut()
        .chain(class.static_fields.iter_mut())
    {
        if let Some(key) = &mut field.key_expr {
            rw_expr(key, rw);
        }
        if let Some(init) = &mut field.init {
            rw_expr(init, rw);
        }
    }
}

fn rw_expr(expr: &mut Expr, rw: &Rw<'_>) {
    if let Expr::Closure { params, body, .. } = expr {
        // A closure body runs when the closure is called, not where it is
        // created: it is checked, never a static throw.
        let inner = rw.in_function();
        for param in params.iter_mut() {
            if let Some(default) = &mut param.default {
                rw_expr(default, &inner);
            }
        }
        rw_stmts(body, &inner);
        return;
    }
    // An existing check's binding operand tests the slot; it is not a read.
    if tdz_check::for_each_check_value_mut(expr, &mut |value| rw_expr(value, rw)) {
        return;
    }
    walk_expr_children_mut(expr, &mut |child| rw_expr(child, rw));
    guard(expr, rw);
    lift_check(expr);
}

/// The binding an expression accesses by id (the read or write itself, not a
/// sub-expression).
fn accessed_binding(expr: &Expr) -> Option<LocalId> {
    match expr {
        Expr::LocalGet(id)
        | Expr::LocalSet(id, _)
        | Expr::Update { id, .. }
        | Expr::ArrayPop(id)
        | Expr::ArrayShift(id)
        | Expr::ArrayPush { array_id: id, .. }
        | Expr::ArrayPushSpread { array_id: id, .. }
        | Expr::ArrayUnshift { array_id: id, .. }
        | Expr::ArraySplice { array_id: id, .. }
        | Expr::ArrayCopyWithin { array_id: id, .. }
        | Expr::SetAdd { set_id: id, .. } => Some(*id),
        _ => None,
    }
}

fn guard(expr: &mut Expr, rw: &Rw<'_>) {
    let Some(id) = accessed_binding(expr) else {
        return;
    };
    let Some(name) = rw.hit(id) else {
        return;
    };
    let taken = std::mem::replace(expr, Expr::Undefined);
    *expr = match (taken, rw.direct_throws) {
        // PutValue throws after the right-hand side has been evaluated.
        (Expr::LocalSet(_, value), true) => Expr::Sequence(vec![*value, tdz_check::throw(name)]),
        (Expr::LocalSet(id, value), false) => {
            Expr::LocalSet(id, Box::new(tdz_check::check_then(id, name, *value)))
        }
        // Every other access reads the binding before anything else runs.
        (_, true) => tdz_check::throw(name),
        (access, false) => Expr::Sequence(vec![tdz_check::check(id, name), access]),
    };
}

/// `object.p`, `object[k]` and `callee(args)` evaluate their first operand
/// before anything else, so a check on that operand can precede the whole
/// expression: `(check, x).p` becomes `(check, x.p)`. This keeps the access
/// itself in the shape later passes recognize (a member read or a call of a
/// binding), and repeats up a chain like `x.a.b(c)`.
fn lift_check(expr: &mut Expr) {
    let first = match expr {
        Expr::PropertyGet { object, .. } | Expr::IndexGet { object, .. } => object.as_mut(),
        Expr::Call { callee, .. } => callee.as_mut(),
        _ => return,
    };
    let Expr::Sequence(parts) = first else {
        return;
    };
    let is_plain_check = parts.len() == 2
        && matches!(&parts[0], Expr::Call { callee, args, .. }
            if args.len() == 2
                && matches!(callee.as_ref(), Expr::ExternFuncRef { name, .. } if name == tdz_check::TDZ_CHECK));
    if !is_plain_check {
        return;
    }
    let inner = parts.pop().expect("two parts");
    let check = parts.pop().expect("two parts");
    *first = inner;
    let access = std::mem::replace(expr, Expr::Undefined);
    *expr = Expr::Sequence(vec![check, access]);
}

#[cfg(test)]
#[path = "module_tdz_tests.rs"]
mod tests;
