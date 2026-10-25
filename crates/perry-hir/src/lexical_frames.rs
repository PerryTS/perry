//! A specialization is a new lexical body, not another use of its template's
//! bindings. Separately emitted frames get fresh LocalIds/FuncIds. Nested
//! closures retain their shared emission identity; only their capture sources
//! follow the enclosing frame's renaming.
use crate::ir::*;
use crate::types::FuncId;
use crate::types::LocalId;
use std::collections::BTreeSet;
use std::collections::HashMap;

/// Module-wide allocator for identities of separately emitted lexical frames.
pub struct FrameIds {
    next_local_id: LocalId,
    next_func_id: FuncId,
}

impl FrameIds {
    pub fn new(module: &Module) -> Self {
        let (local, func) = max_ids(module);
        Self {
            next_local_id: local.checked_add(1).expect("frame LocalId overflow"),
            next_func_id: func.checked_add(1000).expect("frame FuncId overflow"),
        }
    }

    pub(crate) fn fresh_func_id(&mut self) -> FuncId {
        let id = self.next_func_id;
        self.next_func_id = id.checked_add(1).expect("frame FuncId overflow");
        id
    }

    /// Relocated bodies can carry source-module ids above the destination's.
    pub fn reserve_functions(&mut self, functions: &[Function]) {
        let mut ids = Owned::default();
        for f in functions {
            ids.function(f);
            ids.funcs.insert(f.id);
        }
        if let Some(id) = ids.locals.last() {
            self.next_local_id = self
                .next_local_id
                .max(id.checked_add(1).expect("frame LocalId overflow"));
        }
        if let Some(id) = ids.funcs.last() {
            self.next_func_id = self
                .next_func_id
                .max(id.checked_add(1).expect("frame FuncId overflow"));
        }
    }
}

#[derive(Default)]
struct Owned {
    locals: BTreeSet<LocalId>,
    funcs: BTreeSet<FuncId>,
}

impl Owned {
    fn params(&mut self, params: &[Param]) {
        self.locals.extend(params.iter().map(|p| p.id));
    }

    fn expr(&mut self, e: &Expr) {
        match e {
            Expr::Closure {
                func_id, params, ..
            } => {
                self.funcs.insert(*func_id);
                self.params(params);
            }
            Expr::ScopedTemp { id, .. } => {
                self.locals.insert(*id);
            }
            Expr::ClassExprFresh {
                evaluation_owner: Some(id),
                ..
            } => {
                self.locals.insert(*id);
            }
            _ => {}
        }
    }

    fn stmt(&mut self, s: &Stmt) {
        match s {
            Stmt::Let { id, .. } => {
                self.locals.insert(*id);
            }
            Stmt::Try { catch: Some(c), .. } => {
                if let Some((id, _)) = &c.param {
                    self.locals.insert(*id);
                }
            }
            _ => {}
        }
    }

    fn expr_tree(&mut self, e: &Expr) {
        crate::tdz_check::for_each_expr_in(e, &mut |e| self.expr(e));
        crate::tdz_check::for_each_expr_stmt_deep(e, &mut |s| self.stmt(s));
    }

    fn function(&mut self, f: &Function) {
        self.params(&f.params);
        crate::tdz_check::for_each_stmts_expr(&f.body, &mut |e| self.expr(e));
        crate::tdz_check::for_each_stmt_deep(&f.body, &mut |s| self.stmt(s));
        for p in &f.params {
            if let Some(e) = &p.default {
                self.expr_tree(e);
            }
            for d in &p.decorators {
                for e in &d.args {
                    self.expr_tree(e);
                }
            }
        }
        for d in &f.decorators {
            for e in &d.args {
                self.expr_tree(e);
            }
        }
    }

    fn shared_expr(&mut self, e: &Expr) {
        if let Expr::Closure {
            params,
            body,
            func_id,
            ..
        } = e
        {
            self.funcs.insert(*func_id);
            self.params(params);
            crate::tdz_check::for_each_stmt_deep(body, &mut |s| self.stmt(s));
            crate::tdz_check::for_each_stmts_expr(body, &mut |e| self.expr(e));
            for p in params {
                if let Some(e) = &p.default {
                    self.expr_tree(e);
                }
            }
        }
    }

    fn retain_frame(&mut self, shared: &Owned) {
        self.locals.retain(|id| !shared.locals.contains(id));
        self.funcs.retain(|id| !shared.funcs.contains(id));
    }

    fn function_frame(&mut self, f: &Function) {
        self.function(f);
        let mut shared = Owned::default();
        crate::tdz_check::for_each_function_expr(f, &mut |e| shared.shared_expr(e));
        self.retain_frame(&shared);
    }

    fn class_frame(&mut self, c: &Class) {
        self.class(c);
        let mut shared = Owned::default();
        crate::tdz_check::for_each_class_expr(c, &mut |e| shared.shared_expr(e));
        self.retain_frame(&shared);
    }

    fn class(&mut self, c: &Class) {
        for f in c
            .constructor
            .iter()
            .chain(&c.methods)
            .chain(&c.static_methods)
            .chain(c.getters.iter().chain(&c.setters).map(|(_, f)| f))
            .chain(c.computed_members.iter().map(|m| &m.function))
        {
            self.funcs.insert(f.id);
            self.function(f);
        }
        if let Some(e) = &c.extends_expr {
            self.expr_tree(e);
        }
        for d in &c.decorators {
            for e in &d.args {
                self.expr_tree(e);
            }
        }
        for m in &c.computed_members {
            self.expr_tree(&m.key_expr);
        }
        for field in c.fields.iter().chain(&c.static_fields) {
            for d in &field.decorators {
                for e in &d.args {
                    self.expr_tree(e);
                }
            }
            for e in field.key_expr.iter().chain(field.init.iter()) {
                self.expr_tree(e);
            }
        }
    }
}

pub fn max_ids(module: &Module) -> (LocalId, FuncId) {
    let mut ids = Owned::default();
    for f in &module.functions {
        ids.funcs.insert(f.id);
        ids.function(f);
    }
    for c in &module.classes {
        ids.class(c);
    }
    for g in &module.globals {
        if let Some(e) = &g.init {
            ids.expr_tree(e);
        }
    }
    crate::tdz_check::for_each_stmt_deep(&module.init, &mut |s| ids.stmt(s));
    crate::tdz_check::for_each_module_expr(module, &mut |e| {
        ids.expr(e);
        match e {
            Expr::LocalGet(id) | Expr::LocalSet(id, _) | Expr::Update { id, .. } => {
                ids.locals.insert(*id);
            }
            Expr::FuncRef(id) => {
                ids.funcs.insert(*id);
            }
            Expr::Closure {
                captures,
                mutable_captures,
                ..
            } => {
                ids.locals
                    .extend(captures.iter().chain(mutable_captures).copied());
            }
            _ => {}
        }
    });
    (
        ids.locals.last().copied().unwrap_or(0),
        ids.funcs.last().copied().unwrap_or(0),
    )
}

pub struct Remap {
    locals: HashMap<LocalId, LocalId>,
    funcs: HashMap<FuncId, FuncId>,
}

impl Remap {
    fn new(ids: Owned, ctx: &mut FrameIds) -> Self {
        Self {
            locals: ids
                .locals
                .into_iter()
                .map(|old| {
                    let new = ctx.next_local_id;
                    ctx.next_local_id = new.checked_add(1).expect("monomorph LocalId overflow");
                    (old, new)
                })
                .collect(),
            funcs: ids
                .funcs
                .into_iter()
                .map(|old| (old, ctx.fresh_func_id()))
                .collect(),
        }
    }

    fn local(&self, id: &mut LocalId) {
        if let Some(new) = self.locals.get(id) {
            *id = *new;
        }
    }

    fn func(&self, id: &mut FuncId) {
        if let Some(new) = self.funcs.get(id) {
            *id = *new;
        }
    }

    fn params(&self, params: &mut [Param]) {
        for p in params {
            self.local(&mut p.id);
            if let Some(m) = &mut p.arguments_object {
                for (_, id) in &mut m.mapped_parameter_ids {
                    self.local(id);
                }
            }
            if let Some(e) = &mut p.default {
                self.expr(e);
            }
            for d in &mut p.decorators {
                for e in &mut d.args {
                    self.expr(e);
                }
            }
        }
    }

    fn function(&self, f: &mut Function) {
        self.func(&mut f.id);
        self.params(&mut f.params);
        for id in &mut f.captures {
            self.local(id);
        }
        self.stmts(&mut f.body);
        for d in &mut f.decorators {
            for e in &mut d.args {
                self.expr(e);
            }
        }
    }

    fn stmts(&self, body: &mut [Stmt]) {
        for s in body {
            self.stmt(s);
        }
    }

    fn stmt(&self, s: &mut Stmt) {
        match s {
            Stmt::Let { id, init, .. } => {
                self.local(id);
                if let Some(e) = init {
                    self.expr(e);
                }
            }
            Stmt::Expr(e) | Stmt::Throw(e) => self.expr(e),
            Stmt::Return(e) => {
                if let Some(e) = e {
                    self.expr(e);
                }
            }
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                self.expr(condition);
                self.stmts(then_branch);
                if let Some(b) = else_branch {
                    self.stmts(b);
                }
            }
            Stmt::While { condition, body } | Stmt::DoWhile { condition, body } => {
                self.expr(condition);
                self.stmts(body);
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(s) = init {
                    self.stmt(s);
                }
                if let Some(e) = condition {
                    self.expr(e);
                }
                if let Some(e) = update {
                    self.expr(e);
                }
                self.stmts(body);
            }
            Stmt::Labeled { body, .. } => self.stmt(body),
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                self.stmts(body);
                if let Some(c) = catch {
                    if let Some((id, _)) = &mut c.param {
                        self.local(id);
                    }
                    self.stmts(&mut c.body);
                }
                if let Some(b) = finally {
                    self.stmts(b);
                }
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                self.expr(discriminant);
                for c in cases {
                    if let Some(e) = &mut c.test {
                        self.expr(e);
                    }
                    self.stmts(&mut c.body);
                }
            }
            Stmt::PreallocateBoxes(ids)
            | Stmt::PreallocateTdzBoxes(ids)
            | Stmt::ReleaseBoxes(ids) => {
                for id in ids {
                    self.local(id);
                }
            }
            Stmt::Break | Stmt::Continue | Stmt::LabeledBreak(_) | Stmt::LabeledContinue(_) => {}
        }
    }

    fn expr(&self, e: &mut Expr) {
        match e {
            Expr::LocalGet(id)
            | Expr::LocalSet(id, _)
            | Expr::Update { id, .. }
            | Expr::ScopedTemp { id, .. } => self.local(id),
            Expr::FuncRef(id) => self.func(id),
            Expr::WithSet { fallback, .. } => match fallback {
                WithSetFallback::Local(id) | WithSetFallback::SloppyImplicit(id) => self.local(id),
                _ => {}
            },
            Expr::ArrayPush { array_id, .. }
            | Expr::ArrayPushSpread { array_id, .. }
            | Expr::ArrayUnshift { array_id, .. }
            | Expr::ArraySplice { array_id, .. }
            | Expr::ArrayCopyWithin { array_id, .. }
            | Expr::ArrayPop(array_id)
            | Expr::ArrayShift(array_id) => self.local(array_id),
            Expr::SetAdd { set_id, .. } => self.local(set_id),
            Expr::ClassExprFresh {
                evaluation_owner: Some(id),
                ..
            } => self.local(id),
            Expr::LinkGeneratorPrototype {
                i32_controls,
                i1_controls,
                ..
            } => {
                for id in i32_controls.iter_mut().chain(i1_controls.iter_mut()) {
                    self.local(id);
                }
            }
            Expr::Closure {
                func_id,
                params,
                captures,
                mutable_captures,
                body,
                ..
            } => {
                self.func(func_id);
                self.params(params);
                for id in captures.iter_mut().chain(mutable_captures.iter_mut()) {
                    self.local(id);
                }
                self.stmts(body);
                return;
            }
            _ => {}
        }
        crate::walker::walk_expr_children_mut(e, &mut |e| self.expr(e));
    }

    fn class(&self, c: &mut Class) {
        for f in c
            .constructor
            .iter_mut()
            .chain(&mut c.methods)
            .chain(&mut c.static_methods)
            .chain(c.getters.iter_mut().chain(&mut c.setters).map(|(_, f)| f))
            .chain(c.computed_members.iter_mut().map(|m| &mut m.function))
        {
            self.function(f);
        }
        for id in &mut c.static_accessor_fn_ids {
            self.func(id);
        }
        for e in c.extends_expr.iter_mut() {
            self.expr(e);
        }
        for m in &mut c.computed_members {
            self.expr(&mut m.key_expr);
        }
        for f in c.fields.iter_mut().chain(&mut c.static_fields) {
            for d in &mut f.decorators {
                for e in &mut d.args {
                    self.expr(e);
                }
            }
            for e in f.key_expr.iter_mut().chain(f.init.iter_mut()) {
                self.expr(e);
            }
        }
        for d in &mut c.decorators {
            for e in &mut d.args {
                self.expr(e);
            }
        }
    }

    pub fn metadata(&self, module: &mut Module) {
        for (&old, &new) in &self.locals {
            if module.classic_for_lexical_bindings.contains(&old) {
                module.classic_for_lexical_bindings.insert(new);
            }
            if let Some(span) = module.local_source_spans.get(&old).cloned() {
                module.local_source_spans.insert(new, span);
            }
        }
        for (&old, &new) in &self.funcs {
            if module.async_step_closures.contains(&old) {
                module.async_step_closures.insert(new);
            }
            if module.async_generator_funcs.contains(&old) {
                module.async_generator_funcs.insert(new);
            }
            if let Some(name) = module.closure_display_names.get(&old).cloned() {
                module.closure_display_names.insert(new, name);
            }
            if let Some(len) = module.gen_param_prologue_len.get(&old).copied() {
                module.gen_param_prologue_len.insert(new, len);
            }
            if let Some(source) = module.closure_source_text.get(&old).cloned() {
                module.closure_source_text.insert(new, source);
            }
        }
    }
}

pub fn function(f: &mut Function, original_id: FuncId, ctx: &mut FrameIds) -> Remap {
    let mut ids = Owned::default();
    ids.function_frame(f);
    let mut map = Remap::new(ids, ctx);
    // The driver already allocated the root's new id. Include its origin so
    // recursive references and module metadata follow the copied body too.
    map.funcs.insert(original_id, f.id);
    map.function(f);
    map
}

pub fn class(c: &mut Class, ctx: &mut FrameIds) -> Remap {
    let mut ids = Owned::default();
    ids.class_frame(c);
    let map = Remap::new(ids, ctx);
    map.class(c);
    map
}

/// Literal-layout constructors use positional scratch ids at lowering. They
/// are not source lexical declarations, so give their frame its own ids too;
/// their scratch ids must never acquire another body's source metadata.
pub fn literal_constructor(f: &mut Function, ctx: &mut FrameIds) {
    let mut ids = Owned::default();
    ids.function(f);
    ids.funcs.insert(f.id);
    Remap::new(ids, ctx).function(f);
}

#[cfg(test)]
#[path = "monomorph/freshen_tests.rs"]
mod tests;

/// Cross-module relocation already assigns fresh function identities. All
/// lexical bindings in those newly emitted bodies must be destination-owned.
pub fn relocated_function(f: &mut Function, ids: &mut FrameIds) -> Remap {
    let mut owned = Owned::default();
    owned.function(f);
    owned.funcs.clear();
    let map = Remap::new(owned, ids);
    map.function(f);
    map
}
