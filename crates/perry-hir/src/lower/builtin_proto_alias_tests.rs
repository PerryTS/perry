//! Borrowed aliases and unrelated same-named parameters both retain reflective dispatch.

#![cfg(test)]

use crate::Module;
use perry_diagnostics::SourceCache;

fn lower(src: &str) -> Module {
    let src = src.to_string();
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(move || {
            let mut cache = SourceCache::new();
            let parsed =
                perry_parser::parse_typescript_with_cache(&src, "builtin_alias.ts", &mut cache)
                    .expect("parse should succeed");
            crate::lower_module(&parsed.module, "test", "builtin_alias.ts")
                .expect("lower should succeed")
        })
        .expect("spawn lower thread")
        .join()
        .expect("lower thread panicked")
}

fn body_of(module: &Module, name: &str) -> String {
    let function = module
        .functions
        .iter()
        .find(|function| function.name == name)
        .unwrap_or_else(|| panic!("function `{name}` is lowered"));
    format!("{:?}", function.body)
}

/// Did this body keep a real `.call`/`.apply` (i.e. was NOT folded)?
fn keeps_reflective_call(body: &str) -> bool {
    body.contains("\"call\"") || body.contains("\"apply\"")
}

/// A callback parameter that merely shares its spelling with a
/// `var e = [].slice` alias in another function keeps its real `.call(...)`;
/// the alias's own function retains the selected method value too.
#[test]
fn a_same_named_parameter_is_not_folded_into_a_builtin_method_call() {
    let module = lower(
        r#"
        function tail(x: any): any {
          var e = [].slice;
          return e.call(x, 1);
        }
        function cast(e: any, node: any, parent: any): boolean {
          return !!e.call(node, node, 1, parent);
        }
        "#,
    );
    let tail = body_of(&module, "tail");
    assert!(
        keeps_reflective_call(&tail),
        "alias must stay reflective: {tail}"
    );
    assert!(!tail.contains("ArrayLikeMethod"), "{tail}");
    let cast = body_of(&module, "cast");
    assert!(
        keeps_reflective_call(&cast),
        "parameter `e` was folded: {cast}"
    );
}

/// The minified shape that failed: the parameter-using closure lowers FIRST, the
/// alias function later, both spelling the binding `e485`.
#[test]
fn a_parameter_lowered_before_the_alias_is_not_folded() {
    let module = lower(
        r#"
        function convert(e485: any): any {
          return function (this: any, node: any, index: any, parent: any) {
            return !!e485.call(this, node, index, parent);
          };
        }
        function tail(x: any): any {
          var e485 = [].slice;
          return e485.call(x, 1);
        }
        "#,
    );
    assert!(
        keeps_reflective_call(&format!("{:?}", module)),
        "convert's e485.call was folded"
    );
    assert!(keeps_reflective_call(&body_of(&module, "tail")));
}

/// #4101 alias form: `Function.prototype.toString` held in a local must stay a
/// reflective `.call` (brand check), exactly like the direct member form.
#[test]
fn a_function_prototype_to_string_alias_stays_reflective() {
    let module = lower(
        r#"
        function source(f: any): any {
          var e = Function.prototype.toString;
          return e.call(f);
        }
        "#,
    );
    assert!(keeps_reflective_call(&body_of(&module, "source")));
}

/// `Function.prototype.call` / `apply` / `bind` reached through `.call` act
/// on their `this` value's [[Call]]: the fold must not rewrite
/// `Function.prototype.call.call(t, …)` into `t.call(…)`, a by-name read that
/// a function relinked by `Object.setPrototypeOf` (or one with an own
/// `call`) answers with another method. Reflective, `t` stays the first
/// argument of the outer `.call`; folded, it becomes the receiver and the
/// argument list starts at the `thisArg`. Output parity:
/// `tests/parity/language/function/test_gap_function_proto_intrinsic_call_on_relinked.ts`.
#[test]
fn function_prototype_intrinsics_stay_reflective() {
    let module = lower(
        r#"
        function direct_call(t: any): any { return Function.prototype.call.call(t, null, 1); }
        function direct_apply(t: any): any { return Function.prototype.apply.call(t, null, [1]); }
        function direct_bind(t: any): any { return Function.prototype.bind.call(t, null); }
        function alias_call(t: any): any { var c = Function.prototype.call; return c.call(t, null, 1); }
        "#,
    );
    for name in ["direct_call", "direct_apply", "direct_bind", "alias_call"] {
        let body = body_of(&module, name);
        assert!(
            body.contains("property: \"call\"") && body.contains("args: [LocalGet("),
            "{name} was folded into a by-name call on its target: {body}"
        );
    }
}
