//! A function's `arguments` object is no Array (ECMA-262 10.4.4): its
//! prototype is `%Object.prototype%`, and `length` is an ordinary own data
//! property, writable and configurable. A store or `defineProperty` gives it
//! any value, which a later read of `arguments.length` returns.

use super::*;

/// `arguments.length = o` stores `o`: the read returns it, so `n.x = 5`
/// writes `A.x` after construction (node prints 5). With `arguments` an
/// Array, the store was a ToNumber length store, `f` returned a Number and
/// `A.x` was proven read-only.
#[test]
fn arguments_length_store_keeps_the_value() {
    let pf = facts(
        r#"
        class A { x: number; constructor() { this.x = 1; } }
        function f(o: any) { (arguments as any).length = o; return arguments.length; }
        const a = new A();
        const n: any = f(a);
        n.x = 5;
        console.log(a.x);
    "#,
    );
    let x = field(tmpl(&pf, "A"), "x");
    assert!(!x.readonly && x.writers_post > 0, "{}", pf.dump_text());
}

/// `Object.defineProperty(arguments, "length", { value: o })` redefines
/// the own `length` (it is configurable): the read returns `o`.
#[test]
fn arguments_length_redefined_keeps_the_value() {
    let pf = facts(
        r#"
        class A { x: number; constructor() { this.x = 1; } }
        function f(o: any) {
          Object.defineProperty(arguments, "length", { value: o });
          return arguments.length;
        }
        const a = new A();
        const n: any = f(a);
        n.x = 5;
        console.log(a.x);
    "#,
    );
    let x = field(tmpl(&pf, "A"), "x");
    assert!(!x.readonly && x.writers_post > 0, "{}", pf.dump_text());
}

/// An unwritten `arguments.length` reads the count only: it does not read
/// the arguments, so `n.x = 5` on it writes no argument's field.
#[test]
fn arguments_length_read_is_not_the_arguments() {
    let pf = facts(
        r#"
        class A { x: number; constructor() { this.x = 1; } }
        function f(o: any) { return arguments.length; }
        const a = new A();
        const n: any = f(a);
        n.x = 5;
        console.log(a.x);
    "#,
    );
    let x = field(tmpl(&pf, "A"), "x");
    assert!(x.readonly && x.writers_post == 0, "{}", pf.dump_text());
}
