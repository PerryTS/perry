use super::*;

fn async_generator_with_delegations(id: FuncId, delegations: usize) -> Function {
    Function {
        id,
        name: "many_delegations".to_string(),
        type_params: Vec::new(),
        params: Vec::new(),
        return_type: Type::Any,
        body: (0..delegations)
            .map(|_| {
                Stmt::Expr(Expr::Yield {
                    value: Some(Box::new(Expr::GlobalGet(0))),
                    delegate: true,
                })
            })
            .collect(),
        is_async: true,
        is_generator: true,
        is_strict: false,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
        was_plain_async: false,
        was_unrolled: false,
    }
}

fn transformed_body_size(delegations: usize) -> usize {
    let mut module = Module::new("delegation_growth");
    module
        .functions
        .push(async_generator_with_delegations(1, delegations));
    transform_generators(&mut module);
    format!("{:?}", module.functions[0].body).len()
}

#[test]
fn async_generator_yield_star_dispatch_growth_is_linear() {
    let size_16 = transformed_body_size(16);
    let size_32 = transformed_body_size(32);

    assert!(
        size_32 < size_16 * 3,
        "doubling yield* sites grew transformed HIR from {size_16} to {size_32} bytes; \
         every delegation route must share the state dispatcher instead of cloning it"
    );
}
