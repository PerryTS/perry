//! PERFNOW30: `ObjectProto` alone is the exact `Object.prototype`. A value
//! that is another builtin's prototype also holds that builtin's own token
//! (`builtins::builtin_proto_value`, `loads::family_proto_value`), so the
//! `constructor` read of `Object.prototype` is `Object` only (ECMA-262
//! 20.1.3.1), while `Function.prototype`'s stays `Function` (E14). A call
//! of the read value through `apply` is then no `Function(...)` call.

use super::*;

fn verdict(proto: &str) -> Verdict {
    let src = format!(
        r#"
        class Svc {{ size = 3; }}
        function g(a: any) {{ return a.size; }}
        function h() {{ return 1; }}
        const s = new Svc();
        const p: any = {proto};
        const d: any = Object.getOwnPropertyDescriptor(p, "constructor");
        const f: any = d.value;
        f.apply(null, [s]);
        console.log(g(s), h());
        "#
    );
    site(&facts(&src), "size").verdict.clone()
}

/// node: `f` is `Object`; `Object(s)` returns `s`, no program code runs.
#[test]
fn object_prototype_constructor_is_object() {
    let v = verdict("Object.getPrototypeOf(Object.getPrototypeOf(s))");
    assert!(!matches!(v, Verdict::No(_)), "{v:?}");
}

/// `Object.prototype` read as `Object.prototype`: the same.
#[test]
fn object_prototype_value_constructor_is_object() {
    let v = verdict("Object.prototype");
    assert!(!matches!(v, Verdict::No(_)), "{v:?}");
}

/// `Function.prototype` (a function's prototype): its `constructor` is
/// `Function`, whose call makes code from `s`.
#[test]
fn function_prototype_constructor_stays_function() {
    let v = verdict("Object.getPrototypeOf(h)");
    assert!(matches!(v, Verdict::No(_)), "{v:?}");
}

/// A namespace's `__proto__` (`Array.__proto__` is `Function.prototype`).
#[test]
fn namespace_proto_constructor_stays_function() {
    let v = verdict("(Array as any).__proto__");
    assert!(matches!(v, Verdict::No(_)), "{v:?}");
}
