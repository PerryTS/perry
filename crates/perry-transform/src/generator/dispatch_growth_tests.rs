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

#[test]
fn sync_yield_star_forwards_return_and_closes_on_missing_throw() {
    let mut module = Module::new("sync_delegation");
    let mut function = async_generator_with_delegations(1, 1);
    function.is_async = false;
    module.functions.push(function);
    transform_generators(&mut module);
    let body = format!("{:?}", module.functions[0].body);
    assert!(body.contains("js_iterator_delegate_return"));
    assert!(body.contains("js_iterator_delegate_throw"));
    assert!(
        !body.contains("__yield_star_close_m"),
        "close protocol must be shared"
    );
    assert!(
        !body.contains("ReflectApply"),
        "protocol calls must not allocate an argument array"
    );
    assert!(
        !body.contains("Await("),
        "sync delegation must call without awaiting"
    );
}

#[test]
fn return_before_start_does_not_enter_finally() {
    let mut module = Module::new("unstarted_return");
    let mut function = async_generator_with_delegations(1, 0);
    function.is_async = false;
    function.body = vec![Stmt::Try {
        body: vec![Stmt::Expr(Expr::Yield {
            value: Some(Box::new(Expr::Number(1.0))),
            delegate: false,
        })],
        catch: None,
        finally: Some(vec![Stmt::Expr(Expr::GlobalSet(
            0,
            Box::new(Expr::Number(1.0)),
        ))]),
    }];
    module.functions.push(function);
    transform_generators(&mut module);
    let body = format!("{:?}", module.functions[0].body);
    assert!(
        body.contains("op: Gt, left: LocalGet("),
        "pending finally must exclude the initial state"
    );
    assert!(
        body.contains("right: Number(0.0)"),
        "finally interval must start after state zero"
    );
}

#[test]
fn sync_abrupt_resumes_share_the_private_next_dispatcher() {
    let mut module = Module::new("sync_shared_resume");
    let mut function = async_generator_with_delegations(1, 4);
    function.is_async = false;
    module.functions.push(function);
    transform_generators(&mut module);
    let body = format!("{:?}", module.functions[0].body);
    assert!(body.contains("__gen_resume"));
    // Only .next owns a state-dispatch loop. .return/.throw forward their
    // successful continuation to the captured closure.
    assert_eq!(body.matches("While {").count(), 1);
}
