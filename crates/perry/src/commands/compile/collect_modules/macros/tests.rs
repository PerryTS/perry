use super::*;
use serde_json::json;
fn values(input: Vec<serde_json::Value>) -> Vec<ast::Expr> {
    input
        .into_iter()
        .map(|input| {
            serde_json::from_str::<super::value::Value>(&input.to_string())
                .unwrap()
                .0
        })
        .collect()
}

fn parse(source: &str) -> ast::Module {
    perry_parser::parse_typescript(source, "macro-test.ts").unwrap()
}

#[test]
fn attributed_alias_calls_become_json_and_remove_the_runtime_import() {
    let module = parse(
        r#"import { read as load, type Stack } from './input.ts' with { type: 'macro' }; const value = await load();"#,
    );
    let expanded = expand_with(&module, |_, calls| {
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].export, "read");
        Ok(values(vec![
            json!({"deno":{"main.ts":"export default 7;"},"__proto__":3}),
        ]))
    })
    .unwrap();
    assert!(!expanded.body.iter().any(|item| matches!(
        item,
        ast::ModuleItem::ModuleDecl(ast::ModuleDecl::Import(_))
    )));
    let ast::ModuleItem::Stmt(ast::Stmt::Decl(ast::Decl::Var(decl))) = &expanded.body[0] else {
        panic!("variable")
    };
    let ast::Expr::Await(value) = decl.decls[0].init.as_deref().unwrap() else {
        panic!("await")
    };
    let ast::Expr::Object(value) = value.arg.as_ref() else {
        panic!("inlined object")
    };
    assert_eq!(value.props.len(), 2);
    assert!(value.props.iter().all(|prop| matches!(prop, ast::PropOrSpread::Prop(prop) if matches!(prop.as_ref(), ast::Prop::KeyValue(prop) if matches!(prop.key, ast::PropName::Computed(_))))));
}

#[test]
fn shadowed_import_names_are_ordinary_runtime_calls() {
    let module = parse(
        r#"import { value } from './macro.ts' with { type: 'macro' }; function f(value: () => number) { return value(); } console.log(value(), f(() => 9));"#,
    );
    expand_with(&module, |_, calls| {
        assert_eq!(calls.len(), 1);
        Ok(values(vec![json!(7)]))
    })
    .unwrap();
}

#[test]
fn nested_namespace_default_and_static_const_arguments() {
    let module = parse(
        r#"import f from './one.ts' with { type: 'macro' }; import * as m from './two.ts' with { type: 'macro' }; const x = 2; console.log(m['twice'](f(x), { a: [true, null] }));"#,
    );
    expand_with(&module, |_, calls| {
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].export, "default");
        assert_eq!(calls[0].arguments, ["2.0"]);
        assert_eq!(calls[1].export, "twice");
        assert_eq!(calls[1].arguments[0], "JSON.parse(__macroValues[0])");
        Ok(values(vec![json!(2), json!(4)]))
    })
    .unwrap();
}

#[test]
fn dynamic_arguments_name_the_macro_import() {
    let module = parse(
        r#"import { f } from './dynamic.ts' with { type: 'macro' }; console.log(f(Math.random()));"#,
    );
    let error = expand_with(&module, |_, _| panic!("dynamic call must not execute")).unwrap_err();
    let error = format!("{error:#}");
    assert!(
        error.contains("dynamic.ts") && error.contains("not statically known"),
        "{error}"
    );
}

#[test]
fn escaped_bindings_and_mutated_object_constants_are_refused() {
    for source in [
        r#"import { f } from './m.ts' with { type: 'macro' }; const alias = f;"#,
        r#"import * as m from './m.ts' with { type: 'macro' }; const alias = m;"#,
        r#"import { f } from './m.ts' with { type: 'macro' }; const x = {a:1}; x.a = 2; f(x);"#,
    ] {
        let error = expand_with(&parse(source), |_, _| {
            panic!("unsafe call must not execute")
        })
        .unwrap_err();
        assert!(format!("{error:#}").contains("macro"));
    }
}

#[test]
fn unused_macros_do_not_execute_or_enter_the_runtime_graph() {
    let module =
        parse(r#"import { f } from './unused.ts' with { type: 'macro' }; console.log('ok');"#);
    let result = expand_with(&module, |_, _| panic!("unused macro must not execute")).unwrap();
    assert_eq!(result.body.len(), 1);
}

#[test]
fn distinct_calls_with_equal_source_spans_keep_independent_values() {
    let mut module = parse(
        r#"import { f } from './macro.ts' with { type: 'macro' }; const result = [f(), f()];"#,
    );
    struct EqualSpans;
    impl VisitMut for EqualSpans {
        fn visit_mut_call_expr(&mut self, call: &mut ast::CallExpr) {
            call.visit_mut_children_with(self);
            call.span = swc_common::DUMMY_SP;
        }
    }
    module.visit_mut_with(&mut EqualSpans);
    let expanded = expand_with(&module, |_, calls| {
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].span, calls[1].span);
        assert_ne!(calls[0].identity, calls[1].identity);
        Ok(values(vec![json!(1), json!(2)]))
    })
    .unwrap();
    let ast::ModuleItem::Stmt(ast::Stmt::Decl(ast::Decl::Var(decl))) = &expanded.body[0] else {
        panic!("variable")
    };
    let ast::Expr::Array(array) = decl.decls[0].init.as_deref().unwrap() else {
        panic!("inlined array")
    };
    let numbers: Vec<_> = array
        .elems
        .iter()
        .map(|value| {
            let ast::Expr::Lit(ast::Lit::Num(number)) = value.as_ref().unwrap().expr.as_ref()
            else {
                panic!("inlined number")
            };
            number.value
        })
        .collect();
    assert_eq!(numbers, [1.0, 2.0]);
    struct SelfContained;
    impl Visit for SelfContained {
        fn visit_syntax_context(&mut self, context: &SyntaxContext) {
            assert_eq!(*context, SyntaxContext::empty());
        }
    }
    expanded.visit_with(&mut SelfContained);
}
