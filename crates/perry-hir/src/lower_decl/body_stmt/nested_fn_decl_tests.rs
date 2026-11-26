use crate::ir::{Expr, Stmt};
use crate::lower::expr_function::closure_type_params_tests::{
    closure_with_param, function_body, let_closure, lower_src, type_param_names,
};
use crate::types::{LocalId, Type};
use crate::Module;

fn param_local(module: &Module, func: &str, param: &str) -> LocalId {
    module
        .functions
        .iter()
        .find(|f| f.name == func)
        .and_then(|f| f.params.iter().find(|p| p.name == param))
        .map(|p| p.id)
        .unwrap_or_else(|| panic!("`{func}` has no parameter `{param}`"))
}

fn type_var(name: &str) -> Type {
    Type::TypeVar(name.to_string())
}

const ISSUE_EXAMPLE: &str = r#"
function outer<T>(saved: T) {
  function first<T>(items: T[]): T {
    console.log(saved);
    return items[0];
  }
  return first(["x"]);
}
"#;

#[test]
fn nested_generic_function_keeps_its_declared_signature() {
    let module = lower_src(&format!("{ISSUE_EXAMPLE}outer(42);"));
    let Expr::Closure {
        params,
        type_params,
        return_type,
        captures,
        ..
    } = let_closure(function_body(&module, "outer"), "first")
    else {
        unreachable!()
    };
    assert_eq!(type_param_names(type_params), ["T"]);
    assert_eq!(params.len(), 1);
    assert_eq!(params[0].ty, Type::Array(Box::new(type_var("T"))));
    assert_eq!(*return_type, type_var("T"));
    assert!(captures.contains(&param_local(&module, "outer", "saved")));
}

#[test]
fn specializing_the_outer_function_leaves_the_inner_signature_generic() {
    let mut module = lower_src(&format!("{ISSUE_EXAMPLE}outer<number>(42);"));
    crate::monomorph::monomorphize_module(&mut module);
    let specialized: Vec<_> = module
        .functions
        .iter()
        .filter(|f| f.name.starts_with("outer") && f.type_params.is_empty())
        .collect();
    assert!(!specialized.is_empty(), "`outer` was not specialized");
    for outer in specialized {
        let Expr::Closure {
            params,
            return_type,
            ..
        } = let_closure(&outer.body, "first")
        else {
            unreachable!()
        };
        assert_eq!(
            params[0].ty,
            Type::Array(Box::new(type_var("T"))),
            "in `{}`",
            outer.name
        );
        assert_eq!(*return_type, type_var("T"));
    }
}

#[test]
fn nested_function_without_annotations_stays_any() {
    let module = lower_src(
        r#"
function outer() {
  function id(x) {
    return x;
  }
  return id(1);
}
outer();
"#,
    );
    let Expr::Closure {
        params,
        return_type,
        ..
    } = let_closure(function_body(&module, "outer"), "id")
    else {
        unreachable!()
    };
    assert_eq!(params[0].ty, Type::Any);
    assert_eq!(*return_type, Type::Any);
}

#[test]
fn an_unconstrained_inner_type_parameter_shadows_a_constrained_outer_one() {
    let module = lower_src(
        r#"
function outer<T extends number>(n: T) {
  function id<T>(x: T): T {
    return x;
  }
  return id("s");
}
outer(1);
"#,
    );
    let Expr::Closure {
        params,
        return_type,
        ..
    } = let_closure(function_body(&module, "outer"), "id")
    else {
        unreachable!()
    };
    assert_eq!(params[0].ty, type_var("T"));
    assert_eq!(*return_type, type_var("T"));
}

#[test]
fn generic_function_expression_and_object_method_resolve_their_type_parameters() {
    let module = lower_src(
        r#"
function outer(saved: number) {
  const id = function <T>(x: T) {
    return x;
  };
  const o = {
    first<U>(xs: U[]): U {
      console.log(saved);
      return xs[0];
    },
  };
  return [id(1), o.first(["x"])];
}
outer(1);
"#,
    );
    let body = function_body(&module, "outer");
    let Expr::Closure { params, .. } = let_closure(body, "id") else {
        unreachable!()
    };
    assert_eq!(params[0].ty, type_var("T"));
    let method = body
        .iter()
        .find_map(|stmt| match stmt {
            Stmt::Let {
                init: Some(init), ..
            } => closure_with_param(init, "xs"),
            _ => None,
        })
        .expect("the method lowers to a closure");
    let Expr::Closure { params, .. } = method else {
        unreachable!()
    };
    assert_eq!(params[0].ty, Type::Array(Box::new(type_var("U"))));
}
