//! Module-singleton literal prototypes use O3 storage at birth. Waiting for
//! a minor leaves allocation-light clients on generic reads/method calls.

use perry_hir::walker::{stmt_any_expr, walk_expr_children};
use perry_hir::{Expr, Module, Stmt};
use std::collections::HashSet;

pub(crate) fn run(module: &mut Module) {
    let mut prototypes = HashSet::new();
    fn inspect(expr: &Expr, prototypes: &mut HashSet<u32>) {
        if let Expr::ObjectCreate(proto, _) = expr {
            if let Expr::LocalGet(id) = proto.as_ref() {
                prototypes.insert(*id);
            }
        }
        walk_expr_children(expr, &mut |e| inspect(e, prototypes));
        if let Expr::Closure { body, .. } = expr {
            inspect_body(body, prototypes);
        }
    }
    fn inspect_body(body: &[Stmt], prototypes: &mut HashSet<u32>) {
        for stmt in body {
            stmt_any_expr(stmt, &mut |e| {
                inspect(e, prototypes);
                false
            });
        }
    }
    inspect_body(&module.init, &mut prototypes);
    for function in &module.functions {
        inspect_body(&function.body, &mut prototypes);
    }
    // Follow immutable aliases back to their allocation. Only direct module
    // statements are rewritten: a factory/loop birth has no singleton proof.
    loop {
        let before = prototypes.len();
        for stmt in &module.init {
            if let Stmt::Let {
                id,
                mutable: false,
                init: Some(Expr::LocalGet(source)),
                ..
            } = stmt
            {
                if prototypes.contains(id) {
                    prototypes.insert(*source);
                }
            }
        }
        if before == prototypes.len() {
            break;
        }
    }
    for stmt in &mut module.init {
        if let Stmt::Let {
            id,
            mutable: false,
            init: Some(init),
            ..
        } = stmt
        {
            if prototypes.contains(id) {
                pretenure_literal_seed(init);
            }
        }
    }
}

fn pretenure_literal_seed(expr: &mut Expr) {
    match expr {
        Expr::Object(props) if props.is_empty() => {
            *expr = Expr::Call {
                byte_end: 0,
                callee: Box::new(Expr::ExternFuncRef {
                    name: "js_object_literal_prototype_birth".into(),
                    param_types: vec![],
                    return_type: perry_hir::types::Type::Any,
                }),
                args: vec![],
                type_args: vec![],
                byte_offset: 0,
            };
        }
        // Computed keys, accessors and spreads use a source-ordered builder.
        // Touch only its seed, preserving every observable operation.
        Expr::Call { callee, args, .. } if args.len() == 1 => {
            if let Expr::Closure { params, body, .. } = callee.as_ref() {
                if params.len() == 1
                    && matches!(body.last(), Some(Stmt::Return(Some(Expr::LocalGet(id))))
                        if *id == params[0].id)
                {
                    pretenure_literal_seed(&mut args[0]);
                }
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use perry_hir::types::Type;

    fn binding(id: u32, init: Expr) -> Stmt {
        Stmt::Let {
            id,
            name: format!("binding{id}"),
            ty: Type::Any,
            mutable: false,
            init: Some(init),
        }
    }

    #[test]
    fn singleton_prototype_alias_replaces_only_its_birth() {
        let mut module = Module::new("singleton");
        module.init = vec![
            binding(1, Expr::Object(vec![])),
            binding(2, Expr::LocalGet(1)),
            binding(3, Expr::ObjectCreate(Box::new(Expr::LocalGet(2)), None)),
            binding(4, Expr::Object(vec![])),
        ];
        run(&mut module);
        assert!(
            matches!(&module.init[0], Stmt::Let { init: Some(Expr::Call { callee, .. }), .. }
            if matches!(callee.as_ref(), Expr::ExternFuncRef { name, .. }
                if name == "js_object_literal_prototype_birth"))
        );
        assert!(matches!(
            &module.init[3],
            Stmt::Let {
                init: Some(Expr::Object(_)),
                ..
            }
        ));
        let once = format!("{:?}", module.init);
        run(&mut module);
        assert_eq!(once, format!("{:?}", module.init));
    }

    #[test]
    fn repeated_birth_has_no_singleton_proof() {
        let body = vec![
            binding(1, Expr::Object(vec![])),
            binding(2, Expr::ObjectCreate(Box::new(Expr::LocalGet(1)), None)),
        ];
        let mut module = Module::new("repeated");
        module.init = vec![Stmt::While {
            condition: Expr::Bool(false),
            body,
        }];
        let before = format!("{:?}", module.init);
        run(&mut module);
        assert_eq!(before, format!("{:?}", module.init));
    }
}
