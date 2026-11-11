//! Keep module bindings global while giving its TLA body the async function CPS path.
use perry_hir::{types::Type, Expr, Function, Module, Stmt};

pub fn lower_async_module_body(module: &mut Module) {
    if !module.has_top_level_await || module.async_init_function.is_some() {
        return;
    }
    let id = crate::generator::compute_max_func_id(module) + 1;
    let mut body = std::mem::take(&mut module.init);
    lift_bindings(&mut body, &mut module.init);
    module.functions.push(Function {
        id,
        name: format!("__async_module_body_{id}"),
        type_params: Vec::new(),
        params: Vec::new(),
        return_type: Type::Any,
        body,
        is_async: true,
        is_generator: false,
        is_strict: module.init_is_strict,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
        was_plain_async: false,
        was_unrolled: false,
    });
    module.async_init_function = Some(id);
}

// Destructuring introduces Try/IteratorClose scaffolding around module
// declarations. Loop/block locals retain their generator activation scope.
fn lift_bindings(body: &mut [Stmt], declarations: &mut Vec<Stmt>) {
    for stmt in body {
        if let Stmt::Try {
            body,
            catch,
            finally,
        } = stmt
        {
            lift_bindings(body, declarations);
            if let Some(catch) = catch {
                lift_bindings(&mut catch.body, declarations);
            }
            if let Some(finally) = finally {
                lift_bindings(finally, declarations);
            }
            continue;
        }
        if let Stmt::Let {
            id,
            name,
            ty,
            mutable,
            init,
        } = stmt
        {
            declarations.push(Stmt::Let {
                id: *id,
                name: name.clone(),
                ty: ty.clone(),
                mutable: *mutable,
                init: None,
            });
            *stmt = Stmt::Expr(Expr::LocalSet(
                *id,
                Box::new(init.take().unwrap_or(Expr::Undefined)),
            ));
        }
    }
}
