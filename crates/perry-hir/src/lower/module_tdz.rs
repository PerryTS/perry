//! Static proof for module lexical TDZ reads.

use std::collections::{HashMap, HashSet};

use crate::ir::{Expr, Function, Module, Stmt};
use crate::types::{FuncId, LocalId};

#[derive(Default)]
struct EagerFacts {
    refs: Vec<LocalId>,
    closure_refs: Vec<LocalId>,
    func_refs: HashSet<FuncId>,
    exposes_class: bool,
    may_run_user_code: bool,
}

impl EagerFacts {
    fn expr(&mut self, expr: &Expr) {
        match expr {
            // Function creation is inert. Its body becomes relevant only if
            // an earlier statement crosses a user-code boundary.
            Expr::Closure { .. } => {
                crate::analysis::collect_local_refs_expr(
                    expr,
                    &mut self.closure_refs,
                    &mut HashSet::new(),
                );
                collect_func_refs_expr(expr, &mut self.func_refs);
                return;
            }
            Expr::FuncRef(id) => {
                self.func_refs.insert(*id);
            }
            Expr::ClassRef(_) | Expr::ClassExprFresh { .. } => self.exposes_class = true,
            Expr::LocalGet(id) | Expr::Update { id, .. } => self.refs.push(*id),
            Expr::LocalSet(id, _) => self.refs.push(*id),
            Expr::ArrayPush { array_id, .. }
            | Expr::ArrayPushSpread { array_id, .. }
            | Expr::ArrayUnshift { array_id, .. }
            | Expr::ArraySplice { array_id, .. }
            | Expr::ArrayCopyWithin { array_id, .. }
            | Expr::ArrayPop(array_id)
            | Expr::ArrayShift(array_id) => self.refs.push(*array_id),
            Expr::SetAdd { set_id, .. } => self.refs.push(*set_id),
            _ => {}
        }
        // Closed allowlist: every unknown/specialized expression is treated as
        // capable of calling user code. Besides keeping new HIR variants sound
        // by default, this covers specialized callback-taking builtins and
        // implicit ToPrimitive conversions that no longer look like `Call`.
        self.may_run_user_code |= !matches!(
            expr,
            Expr::Undefined
                | Expr::Null
                | Expr::Bool(_)
                | Expr::Number(_)
                | Expr::Integer(_)
                | Expr::BigInt(_)
                | Expr::String(_)
                | Expr::WtfString(_)
                | Expr::LocalGet(_)
                | Expr::LocalSet(_, _)
                | Expr::ScopedTemp { .. }
                | Expr::FuncRef(_)
                | Expr::ExternFuncRef { .. }
                | Expr::NativeModuleRef(_)
                | Expr::Object(_)
                | Expr::Array(_)
                | Expr::Conditional { .. }
                | Expr::Logical { .. }
                | Expr::Unary {
                    op: crate::ir::UnaryOp::Not,
                    ..
                }
                | Expr::Compare {
                    op: crate::ir::CompareOp::Eq | crate::ir::CompareOp::Ne,
                    ..
                }
                | Expr::TypeOf(_)
                | Expr::Void(_)
                | Expr::Closure { .. }
                | Expr::ClassRef(_)
                | Expr::EnumMember { .. }
        );
        crate::walker::walk_expr_children(expr, &mut |child| self.expr(child));
    }

    fn stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Let { init, .. } => init.iter().for_each(|expr| self.expr(expr)),
            Stmt::Expr(expr) | Stmt::Throw(expr) => self.expr(expr),
            Stmt::Return(expr) => expr.iter().for_each(|expr| self.expr(expr)),
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                self.expr(condition);
                then_branch.iter().for_each(|stmt| self.stmt(stmt));
                else_branch
                    .iter()
                    .flatten()
                    .for_each(|stmt| self.stmt(stmt));
            }
            Stmt::While { condition, body } | Stmt::DoWhile { condition, body } => {
                self.expr(condition);
                body.iter().for_each(|stmt| self.stmt(stmt));
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                init.iter().for_each(|stmt| self.stmt(stmt));
                condition
                    .iter()
                    .chain(update)
                    .for_each(|expr| self.expr(expr));
                body.iter().for_each(|stmt| self.stmt(stmt));
            }
            Stmt::Labeled { body, .. } => self.stmt(body),
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                body.iter().for_each(|stmt| self.stmt(stmt));
                catch
                    .iter()
                    .flat_map(|catch| &catch.body)
                    .for_each(|stmt| self.stmt(stmt));
                finally.iter().flatten().for_each(|stmt| self.stmt(stmt));
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                self.expr(discriminant);
                for case in cases {
                    case.test.iter().for_each(|expr| self.expr(expr));
                    case.body.iter().for_each(|stmt| self.stmt(stmt));
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
}

fn collect_func_refs_expr(expr: &Expr, refs: &mut HashSet<FuncId>) {
    match expr {
        Expr::FuncRef(id) => {
            refs.insert(*id);
        }
        Expr::Closure { body, params, .. } => {
            for default in params.iter().filter_map(|param| param.default.as_ref()) {
                collect_func_refs_expr(default, refs);
            }
            for stmt in body {
                collect_func_refs_stmt(stmt, refs);
            }
            return;
        }
        _ => {}
    }
    crate::walker::walk_expr_children(expr, &mut |child| collect_func_refs_expr(child, refs));
}

fn collect_func_refs_stmt(stmt: &Stmt, refs: &mut HashSet<FuncId>) {
    crate::walker::stmt_any_expr(stmt, &mut |expr| {
        collect_func_refs_expr(expr, refs);
        false
    });
}

fn collect_function_refs(
    function: &Function,
    refs: &mut Vec<LocalId>,
    visited: &mut HashSet<usize>,
) {
    for stmt in &function.body {
        crate::analysis::collect_local_refs_stmt(stmt, refs, visited);
    }
    for default in function
        .params
        .iter()
        .filter_map(|param| param.default.as_ref())
    {
        crate::analysis::collect_local_refs_expr(default, refs, visited);
    }
}

pub(crate) fn forward_ids(module: &Module, lexical_ids: &[LocalId]) -> Vec<LocalId> {
    let lexical: HashSet<LocalId> = lexical_ids.iter().copied().collect();
    let mut function_refs: HashMap<FuncId, HashSet<LocalId>> = HashMap::new();
    let mut function_calls: HashMap<FuncId, HashSet<FuncId>> = HashMap::new();
    for function in &module.functions {
        let mut refs = Vec::new();
        collect_function_refs(function, &mut refs, &mut HashSet::new());
        function_refs.insert(function.id, refs.into_iter().collect());
        let mut calls = HashSet::new();
        for stmt in &function.body {
            collect_func_refs_stmt(stmt, &mut calls);
        }
        function_calls.insert(function.id, calls);
    }

    // A reachable hoisted function may call another hoisted function. Resolve
    // that small call graph to a fixed point so exposing/calling one function
    // retains checks for every lexical its synchronous body can reach.
    loop {
        let mut changed = false;
        for (id, calls) in &function_calls {
            let inherited: Vec<LocalId> = calls
                .iter()
                .filter_map(|callee| function_refs.get(callee))
                .flat_map(|refs| refs.iter().copied())
                .collect();
            let refs = function_refs.entry(*id).or_default();
            let before = refs.len();
            refs.extend(inherited);
            changed |= refs.len() != before;
        }
        if !changed {
            break;
        }
    }

    let mut class_refs = HashSet::new();
    let mut visited = HashSet::new();
    for class in &module.classes {
        for function in class
            .methods
            .iter()
            .chain(&class.static_methods)
            .chain(class.getters.iter().map(|(_, function)| function))
            .chain(class.setters.iter().map(|(_, function)| function))
            .chain(class.constructor.iter())
            .chain(class.computed_members.iter().map(|member| &member.function))
        {
            let mut refs = Vec::new();
            collect_function_refs(function, &mut refs, &mut visited);
            class_refs.extend(refs);
            let mut calls = HashSet::new();
            for stmt in &function.body {
                collect_func_refs_stmt(stmt, &mut calls);
            }
            class_refs.extend(
                calls
                    .iter()
                    .filter_map(|id| function_refs.get(id))
                    .flat_map(|refs| refs.iter().copied()),
            );
        }
        for field in class.fields.iter().chain(&class.static_fields) {
            for expr in field.init.iter().chain(&field.key_expr) {
                let mut refs = Vec::new();
                crate::analysis::collect_local_refs_expr(expr, &mut refs, &mut visited);
                class_refs.extend(refs);
                let mut calls = HashSet::new();
                collect_func_refs_expr(expr, &mut calls);
                class_refs.extend(
                    calls
                        .iter()
                        .filter_map(|id| function_refs.get(id))
                        .flat_map(|refs| refs.iter().copied()),
                );
            }
        }
    }

    let mut tdz = HashSet::new();
    let mut initialized = HashSet::new();
    let mut callable_refs = HashSet::new();
    for stmt in &module.init {
        let mut eager = EagerFacts::default();
        eager.stmt(stmt);
        // A closure made by this statement can run within the same statement
        // (for example when passed to a call), or from any later statement.
        callable_refs.extend(eager.closure_refs.iter().copied());
        for id in &eager.func_refs {
            if let Some(refs) = function_refs.get(id) {
                callable_refs.extend(refs.iter().copied());
            }
        }
        if eager.exposes_class {
            callable_refs.extend(class_refs.iter().copied());
        }
        tdz.extend(
            eager
                .refs
                .into_iter()
                .filter(|id| lexical.contains(id) && !initialized.contains(id)),
        );
        if eager.may_run_user_code {
            tdz.extend(
                callable_refs
                    .iter()
                    .copied()
                    .filter(|id| lexical.contains(id) && !initialized.contains(id)),
            );
        }
        let mut declared = HashSet::new();
        crate::lower_decl::collect_top_level_let_ids_stmt(stmt, &mut declared);
        initialized.extend(declared.into_iter().filter(|id| lexical.contains(id)));
    }
    let mut tdz: Vec<_> = tdz.into_iter().collect();
    tdz.sort_unstable();
    tdz
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Type;

    fn reader(id: LocalId) -> Function {
        Function {
            id: 1,
            name: "read".into(),
            type_params: Vec::new(),
            params: Vec::new(),
            return_type: Type::Any,
            body: vec![Stmt::Return(Some(Expr::LocalGet(id)))],
            is_async: false,
            is_generator: false,
            is_strict: true,
            is_exported: false,
            captures: vec![id],
            decorators: Vec::new(),
            was_plain_async: false,
            was_unrolled: false,
        }
    }

    fn call() -> Stmt {
        Stmt::Expr(Expr::Call {
            callee: Box::new(Expr::FuncRef(1)),
            args: Vec::new(),
            type_args: Vec::new(),
            byte_offset: 0,
        })
    }

    fn declaration(id: LocalId) -> Stmt {
        Stmt::Let {
            id,
            name: "K".into(),
            ty: Type::Number,
            mutable: false,
            init: Some(Expr::Integer(3)),
        }
    }

    #[test]
    fn function_body_matters_only_when_user_code_can_run_before_init() {
        let mut safe = Module::new("safe.ts");
        safe.functions.push(reader(7));
        safe.init = vec![declaration(7), call()];
        assert!(forward_ids(&safe, &[7]).is_empty());

        let mut risky = Module::new("risky.ts");
        risky.functions.push(reader(7));
        risky.init = vec![call(), declaration(7)];
        assert_eq!(forward_ids(&risky, &[7]), vec![7]);
    }

    #[test]
    fn creating_a_closure_before_init_does_not_execute_its_body() {
        let closure = Expr::Closure {
            func_id: 2,
            params: Vec::new(),
            return_type: Type::Any,
            body: vec![Stmt::Return(Some(Expr::LocalGet(7)))],
            captures: vec![7],
            mutable_captures: Vec::new(),
            captures_this: false,
            captures_new_target: false,
            enclosing_class: None,
            is_arrow: true,
            is_async: false,
            is_generator: false,
            is_strict: true,
        };
        let mut module = Module::new("closure.ts");
        module.init = vec![Stmt::Expr(closure), declaration(7)];
        assert!(forward_ids(&module, &[7]).is_empty());
    }

    #[test]
    fn later_top_level_reads_do_not_become_nested_reads() {
        let mut module = Module::new("later-read.ts");
        module.init = vec![call(), declaration(7), Stmt::Expr(Expr::LocalGet(7))];
        assert!(forward_ids(&module, &[7]).is_empty());
    }

    #[test]
    fn implicit_conversion_can_enter_an_exposed_function() {
        let mut module = Module::new("conversion.ts");
        module.functions.push(reader(7));
        module.init = vec![
            Stmt::Let {
                id: 8,
                name: "value".into(),
                ty: Type::Any,
                mutable: false,
                init: Some(Expr::Object(vec![("valueOf".into(), Expr::FuncRef(1))])),
            },
            Stmt::Expr(Expr::Unary {
                op: crate::ir::UnaryOp::Neg,
                operand: Box::new(Expr::LocalGet(8)),
            }),
            declaration(7),
        ];
        assert_eq!(forward_ids(&module, &[7]), vec![7]);
    }
}
