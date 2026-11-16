//! A class constructor called without `new` throws a TypeError before its
//! body runs, before `this` or any argument is used (ECMA-262 10.2.1
//! [[Call]] step 2; the runtime: `object/class_value.rs`
//! `js_class_constructor_called`, the code pointer of every class function
//! object). A class reached as the callee of a plain call (a value read off
//! an object, `Function.prototype.call`/`apply`, `Reflect.apply`, a
//! protocol slot: `toString`, `@@toPrimitive`, a getter) hands the receiver
//! and the arguments to no code. Controls: an unknown value beside the
//! class keeps the unknown call; `new` on the same value constructs.
use super::*;

const CLASSES: &str = r#"
    (globalThis as any).hostTake = (_x: any) => {};
    class C { constructor(a?: any) { (globalThis as any).hostTake(a); (globalThis as any).hostTake(this); } }
    class D extends C {}
    class S { static k(a: any) { (globalThis as any).hostTake(a); } constructor(a?: any) { (globalThis as any).hostTake(a); } }
    class Svc { marker = 3; }
    function g(a: any) { return a.marker; }
    function pick(x: any) { return x; }
"#;

fn proven(body: &str) -> bool {
    let src = format!("{CLASSES}\nconst o: any = new Svc();\n{body}\nconsole.log(g(o));\n");
    let pf = facts(&src);
    let v = &site(&pf, "marker").verdict;
    !matches!(v, Verdict::No(_))
}

fn plain_calls(k: &str) -> Vec<String> {
    vec![
        format!("const f: any = pick({k}); try {{ f.call(o, o); }} catch (e) {{}}"),
        format!("const f: any = pick({k}); try {{ f.apply(o, [o]); }} catch (e) {{}}"),
        format!("const f: any = pick({k}); try {{ f(o); }} catch (e) {{}}"),
        format!("const h: any = {{ k: pick({k}) }}; try {{ h.k(o); }} catch (e) {{}}"),
        format!("try {{ Reflect.apply(pick({k}), o, [o]); }} catch (e) {{}}"),
        format!("try {{ (Function.prototype.call as any).call(pick({k}), o, o); }} catch (e) {{}}"),
        format!("const p: any = {{ toString: pick({k}), v: o }}; try {{ console.log(\"\" + p); }} catch (e) {{}}"),
        format!("const p: any = {{ [Symbol.toPrimitive]: pick({k}), v: o }}; try {{ console.log(+p); }} catch (e) {{}}"),
        format!("const p: any = {{ v: o }}; Object.defineProperty(p, \"x\", {{ get: pick({k}) }}); try {{ console.log(p.x); }} catch (e) {{}}"),
    ]
}

/// (i) A class as the callee of a plain or protocol call: nothing escapes.
#[test]
fn a_class_called_without_new_throws() {
    let bad: Vec<String> = plain_calls("C").into_iter().filter(|b| !proven(b)).collect();
    assert!(bad.is_empty(), "escaped through a class [[Call]]: {bad:#?}");
}

/// (iv) A derived class, and a class with a static method under the key it
/// is called by (the value called is the class, not its static).
#[test]
fn a_derived_class_or_one_with_a_static_called_without_new_throws() {
    let bad: Vec<String> =
        plain_calls("D").into_iter().chain(plain_calls("S")).filter(|b| !proven(b)).collect();
    assert!(bad.is_empty(), "escaped through a class [[Call]]: {bad:#?}");
}

/// (ii) Control (the fixture can fail): an unknown value beside the class
/// in the callee keeps the unknown call.
#[test]
fn an_unknown_value_beside_the_class_keeps_the_unknown_call() {
    let k = "(Math.random() < 2 ? C : (globalThis as any).hostGet())";
    for b in plain_calls(k) {
        assert!(!proven(&b), "{b}");
    }
}

/// (iii) Control: `new` on the same value constructs (the body runs on its
/// arguments).
#[test]
fn new_on_the_class_value_constructs() {
    for k in ["C", "D", "S"] {
        let b = format!("const f: any = pick({k}); console.log(new f(o));");
        assert!(!proven(&b), "{b}");
        let b = format!("console.log(Reflect.construct(pick({k}), [o]));");
        assert!(!proven(&b), "{b}");
    }
}
