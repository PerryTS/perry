//! A method's lexical home binding, also captured by its arrows.
use crate::ir::*;
use crate::lower::*;
use swc_ecma_ast as ast;
use swc_ecma_visit::{Visit, VisitWith};

pub(super) fn begin(ctx: &mut LoweringContext, function: &ast::Function) -> Option<u32> {
    struct SuperProperties(bool);
    impl Visit for SuperProperties {
        fn visit_super_prop_expr(&mut self, _: &ast::SuperPropExpr) {
            self.0 = true;
        }
        fn visit_class(&mut self, _: &ast::Class) {}
        fn visit_function(&mut self, _: &ast::Function) {}
    }
    let mut scan = SuperProperties(false);
    function.visit_children_with(&mut scan);
    if !scan.0 {
        return None;
    }
    let id = ctx.define_local("__perry_method_home".into(), crate::types::Type::Any);
    ctx.object_super_home_stack.push(id);
    Some(id)
}

pub(super) fn finish(
    ctx: &mut LoweringContext,
    body: &mut Vec<Stmt>,
    home: Option<u32>,
    is_static: bool,
) {
    let Some(id) = home else {
        return;
    };
    assert_eq!(ctx.object_super_home_stack.pop(), Some(id));
    body.insert(
        0,
        Stmt::Let {
            id,
            name: "__perry_method_home".into(),
            ty: crate::types::Type::Any,
            mutable: false,
            init: Some(Expr::ClassMethodHome {
                class_name: ctx.current_class.clone().expect("class method"),
                is_static,
            }),
        },
    );
}
