use crate::ir::{Expr, Stmt};
use crate::lower_module;
use crate::types::{Type, TypeParam};
use crate::Module;
use perry_diagnostics::SourceCache;
use perry_parser::parse_typescript_with_cache;

pub(crate) fn lower_src(src: &str) -> Module {
    let src = src.to_string();
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(move || {
            let mut cache = SourceCache::new();
            let parsed = parse_typescript_with_cache(&src, "test.ts", &mut cache)
                .expect("parse should succeed");
            lower_module(&parsed.module, "test", "test.ts").expect("lowering should succeed")
        })
        .expect("spawn")
        .join()
        .expect("lowering thread panicked")
}

pub(crate) fn function_body<'m>(module: &'m Module, name: &str) -> &'m [Stmt] {
    &module
        .functions
        .iter()
        .find(|f| f.name == name)
        .unwrap_or_else(|| panic!("function `{name}` not lowered"))
        .body
}

/// The closure bound by `let <name> = <closure>` in `stmts`.
pub(crate) fn let_closure<'s>(stmts: &'s [Stmt], name: &str) -> &'s Expr {
    stmts
        .iter()
        .find_map(|stmt| match stmt {
            Stmt::Let {
                name: n,
                init: Some(init @ Expr::Closure { .. }),
                ..
            } if n == name => Some(init),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no closure bound to `{name}`"))
}

/// The closure in `expr` (or below it) whose first parameter is `param`.
pub(crate) fn closure_with_param<'e>(expr: &'e Expr, param: &str) -> Option<&'e Expr> {
    if let Expr::Closure { params, .. } = expr {
        if params.first().is_some_and(|p| p.name == param) {
            return Some(expr);
        }
    }
    let mut found = None;
    crate::walker::walk_expr_children(expr, &mut |child| {
        if found.is_none() {
            found = closure_with_param(child, param);
        }
    });
    found
}

pub(crate) fn type_param_names(type_params: &[TypeParam]) -> Vec<&str> {
    type_params.iter().map(|p| p.name.as_str()).collect()
}

fn declared(expr: &Expr) -> Vec<&str> {
    let Expr::Closure { type_params, .. } = expr else {
        panic!("not a closure: {expr:?}")
    };
    type_param_names(type_params)
}

#[test]
fn every_closure_form_declares_its_type_parameters() {
    let module = lower_src(
        r#"
function outer(saved: number) {
  const pair = <K, V>(k: K, v: V) => [k, v];
  const id = function <T>(x: T) {
    return x;
  };
  function first<T>(items: T[]): T {
    console.log(saved);
    return items[0];
  }
  const o = {
    last<U>(ys: U[]): U {
      console.log(saved);
      return ys[ys.length - 1];
    },
  };
  const plain = (z: number) => z;
  return [pair("a", 1), id(1), first(["x"]), o.last([1]), plain(2)];
}
outer(1);
"#,
    );
    let body = function_body(&module, "outer");
    assert_eq!(declared(let_closure(body, "pair")), ["K", "V"]);
    assert_eq!(declared(let_closure(body, "id")), ["T"]);
    assert_eq!(declared(let_closure(body, "first")), ["T"]);
    let method = body
        .iter()
        .find_map(|stmt| match stmt {
            Stmt::Let {
                init: Some(init), ..
            } => closure_with_param(init, "ys"),
            _ => None,
        })
        .expect("the method lowers to a closure");
    assert_eq!(declared(method), ["U"]);
    assert!(declared(let_closure(body, "plain")).is_empty());
}

#[test]
fn specializing_a_function_leaves_a_redeclared_type_parameter_generic() {
    let mut module = lower_src(
        r#"
function outer<T>(saved: T) {
  const first = <T>(items: T[]): T => items[0];
  const keep = (x: T): T => x;
  keep(saved);
  return first(["x"]);
}
outer<number>(42);
"#,
    );
    crate::monomorph::monomorphize_module(&mut module);
    let specialized: Vec<_> = module
        .functions
        .iter()
        .filter(|f| f.name.starts_with("outer") && f.type_params.is_empty())
        .collect();
    assert!(!specialized.is_empty(), "`outer` was not specialized");
    for outer in specialized {
        let Expr::Closure { params, .. } = let_closure(&outer.body, "first") else {
            unreachable!()
        };
        assert_eq!(
            params[0].ty,
            Type::Array(Box::new(Type::TypeVar("T".to_string()))),
            "in `{}`",
            outer.name
        );
        let Expr::Closure { params, .. } = let_closure(&outer.body, "keep") else {
            unreachable!()
        };
        assert_eq!(
            params[0].ty,
            Type::Number,
            "the enclosing `T` still applies"
        );
    }
}
