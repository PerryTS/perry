//! Preserve source names for checked reads of forward lexical boxes.
use std::collections::{HashMap, HashSet};

use perry_hir::{Expr, Function, Module, Stmt};

#[derive(Default)]
struct Names {
    bindings: HashMap<u32, String>,
    tdz: HashSet<u32>,
}

pub(super) fn collect(module: &Module) -> HashMap<u32, String> {
    let mut names = Names::default();
    names.stmts(&module.init);
    for function in &module.functions {
        names.function(function);
    }
    for class in &module.classes {
        for function in class
            .methods
            .iter()
            .chain(&class.static_methods)
            .chain(class.getters.iter().map(|(_, f)| f))
            .chain(class.setters.iter().map(|(_, f)| f))
            .chain(class.constructor.iter())
            .chain(class.computed_members.iter().map(|member| &member.function))
        {
            names.function(function);
        }
        for field in class.fields.iter().chain(&class.static_fields) {
            for expr in field.init.iter().chain(&field.key_expr) {
                names.expr(expr);
            }
        }
    }
    for global in &module.globals {
        if let Some(init) = &global.init {
            names.expr(init);
        }
    }
    names.bindings.retain(|id, _| names.tdz.contains(id));
    names.bindings
}

impl Names {
    fn function(&mut self, function: &Function) {
        self.stmts(&function.body);
        for param in &function.params {
            if let Some(default) = &param.default {
                self.expr(default);
            }
        }
    }

    fn expr(&mut self, expr: &Expr) {
        if let Expr::Closure { body, .. } = expr {
            self.stmts(body);
        }
        perry_hir::walker::walk_expr_children(expr, &mut |child| self.expr(child));
    }

    fn stmts(&mut self, stmts: &[Stmt]) {
        for stmt in stmts {
            match stmt {
                Stmt::Let { id, name, init, .. } => {
                    self.bindings.insert(*id, name.clone());
                    if let Some(init) = init {
                        self.expr(init);
                    }
                }
                Stmt::PreallocateTdzBoxes(ids) => self.tdz.extend(ids),
                Stmt::Expr(expr) | Stmt::Throw(expr) => self.expr(expr),
                Stmt::Return(expr) => {
                    if let Some(expr) = expr {
                        self.expr(expr);
                    }
                }
                Stmt::If {
                    condition,
                    then_branch,
                    else_branch,
                } => {
                    self.expr(condition);
                    self.stmts(then_branch);
                    if let Some(branch) = else_branch {
                        self.stmts(branch);
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
                    if let Some(init) = init {
                        self.stmts(std::slice::from_ref(init));
                    }
                    for expr in condition.iter().chain(update) {
                        self.expr(expr);
                    }
                    self.stmts(body);
                }
                Stmt::Labeled { body, .. } => self.stmts(std::slice::from_ref(body)),
                Stmt::Try {
                    body,
                    catch,
                    finally,
                } => {
                    self.stmts(body);
                    if let Some(catch) = catch {
                        self.stmts(&catch.body);
                    }
                    if let Some(finally) = finally {
                        self.stmts(finally);
                    }
                }
                Stmt::Switch {
                    discriminant,
                    cases,
                } => {
                    self.expr(discriminant);
                    for case in cases {
                        if let Some(test) = &case.test {
                            self.expr(test);
                        }
                        self.stmts(&case.body);
                    }
                }
                Stmt::Break
                | Stmt::Continue
                | Stmt::LabeledBreak(_)
                | Stmt::LabeledContinue(_)
                | Stmt::PreallocateBoxes(_)
                | Stmt::ReleaseBoxes(_) => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use perry_hir::types::Type;

    #[test]
    fn collects_nested_lexical_names_without_naming_ordinary_boxes() {
        let local = |id, name: &str| Stmt::Let {
            id,
            name: name.into(),
            ty: Type::Any,
            mutable: true,
            init: None,
        };
        let mut hir = Module::new("names");
        hir.init = vec![
            Stmt::PreallocateBoxes(vec![0]),
            Stmt::PreallocateTdzBoxes(vec![1]),
            local(0, "ordinary"),
            local(1, "later"),
            Stmt::Expr(Expr::Closure {
                func_id: 0,
                params: Vec::new(),
                return_type: Type::Any,
                body: vec![Stmt::PreallocateTdzBoxes(vec![2]), local(2, "nested")],
                captures: Vec::new(),
                mutable_captures: Vec::new(),
                captures_this: false,
                captures_new_target: false,
                enclosing_class: None,
                is_arrow: true,
                is_async: false,
                is_generator: false,
                is_strict: true,
            }),
        ];
        let mut names = super::collect(&hir).into_values().collect::<Vec<_>>();
        names.sort();
        assert_eq!(names, ["later", "nested"]);
    }
}
