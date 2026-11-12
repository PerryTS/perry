//! `F.call(thisArg, ...args)` / `F.apply(thisArg, argArray)` of a builtin
//! function value whose member the solver names (`Array.prototype.slice`,
//! `Object.prototype.hasOwnProperty`) is that member run on `thisArg`
//! (ECMA-262 20.2.3.3 / 20.2.3.1): `Array.prototype.slice.call(x)` is the
//! slice algorithm with O = ToObject(x), a NEW array of x's elements. A
//! function value the solver cannot name, or an unknown receiver, stays
//! the unknown call it was.

use super::*;

fn proven(v: &Verdict) -> bool {
    !matches!(v, Verdict::No(_))
}

/// The verdict of the one `.m()` call site.
fn m_site(body: &str) -> Verdict {
    let src = format!(
        r#"
        class A {{ m() {{ return 1; }} }}
        class B {{ m() {{ return 2; }} }}
        console.log(new B().m === undefined);
        {body}
        "#
    );
    let pf = facts(&src);
    let s = pf
        .sites
        .iter()
        .filter(|s| s.key == "m" && s.is_call)
        .last()
        .unwrap_or_else(|| panic!("no .m() call\n{}", pf.dump_text()));
    s.verdict.clone()
}

/// node: prints `1`.
#[test]
fn slice_call_runs_the_slice_model() {
    let v = m_site(r#"
        const a = [new A(), new A()];
        const b: any = Array.prototype.slice.call(a);
        console.log(b[0].m());
    "#);
    assert!(proven(&v), "{v:?}");
}

/// node: prints `1`. The slice is a new array: a store into it is not a
/// store into `a`.
#[test]
fn slice_call_result_is_a_new_array() {
    let v = m_site(r#"
        const a = [new A(), new A()];
        const b: any = Array.prototype.slice.call(a);
        b.push(new B());
        console.log(a[0].m());
    "#);
    assert!(proven(&v), "{v:?}");
}

/// node: prints `[1, 1]`. `map` runs its callback on `a`'s elements.
#[test]
fn map_call_reaches_the_callback_with_the_elements() {
    let v = m_site(r#"
        const a = [new A(), new A()];
        console.log(Array.prototype.map.call(a, (x: any) => x.m()));
    "#);
    assert!(proven(&v), "{v:?}");
}

/// node: prints `1`. `apply`: the elements of the second argument are
/// slice's arguments.
#[test]
fn slice_apply_runs_the_slice_model() {
    let v = m_site(r#"
        const a = [new A(), new A()];
        const b: any = Array.prototype.slice.apply(a, [1]);
        console.log(b[0].m());
    "#);
    assert!(proven(&v), "{v:?}");
}

/// node: prints `1`. The member read as a value first.
#[test]
fn a_member_read_as_a_value_then_called() {
    let v = m_site(r#"
        const a = [new A(), new A()];
        const s = Array.prototype.slice;
        const b: any = s.call(a);
        console.log(b[0].m());
    "#);
    assert!(proven(&v), "{v:?}");
}

/// node: prints `true 2`. `hasOwnProperty` yields a boolean and hands the
/// receiver to no one.
#[test]
fn has_own_property_call_keeps_the_receiver() {
    let pf = facts(
        r#"
        class O { k = 1; pa = 2; }
        const o = new O();
        const h = Object.prototype.hasOwnProperty.call(o, "k");
        console.log(h, o.pa);
        "#,
    );
    let t = tmpl(&pf, "O");
    assert!(t.escape.is_none(), "{:?}\n{}", t.escape, pf.dump_text());
}

/// Control: the function is unknown code's value: the call stays unknown.
#[test]
fn an_unknown_function_value_stays_unknown() {
    let v = m_site(r#"
        const a = [new A(), new A()];
        const f: any = new Function(process.argv[2])();
        const b: any = f.call(a);
        console.log(b[0].m());
    "#);
    assert!(!proven(&v), "{v:?}");
}

/// Control: the receiver is unknown code's value: its elements are unknown.
#[test]
fn an_unknown_receiver_stays_unknown() {
    let v = m_site(r#"
        const r: any = new Function(process.argv[2])();
        const b: any = Array.prototype.slice.call(r);
        console.log(b[0].m());
    "#);
    assert!(!proven(&v), "{v:?}");
}

/// Control: a computed member may be any method. Reports the verdict.
#[test]
fn a_computed_member_call() {
    let v = m_site(r#"
        const a = [new A(), new A()];
        const b: any = (Array.prototype as any)[process.argv[2]].call(a);
        console.log(b[0].m());
    "#);
    eprintln!("COMPUTED-MEMBER verdict: {v:?}");
    assert!(!proven(&v), "{v:?}");
}

/// `convertArray`'s shape (three.core.js:41727): one function, receivers
/// of several kinds. The result of each is a new Array of its elements.
fn conv(other: &str) -> Verdict {
    m_site(&format!(
        r#"
        function conv(x: any) {{ return Array.prototype.slice.call(x); }}
        {other}
        const b: any = conv([new A(), new A()]);
        console.log(b[0].m());
        "#
    ))
}

/// node: prints `1`; `conv(new Float32Array(2))` is `[0, 0]`.
#[test]
fn slice_call_on_a_typed_array() {
    let v = conv("console.log(conv(new Float32Array(2)));");
    assert!(proven(&v), "{v:?}");
}

/// node: prints `1`; `conv("ab")` is `["a", "b"]`.
#[test]
fn slice_call_on_a_string() {
    let v = conv(r#"console.log(conv("ab"));"#);
    assert!(proven(&v), "{v:?}");
}

/// node: prints `1`; `conv(5)` is `[]` (ToObject(5) has no length).
#[test]
fn slice_call_on_a_number() {
    let v = conv("console.log(conv(5));");
    assert!(proven(&v), "{v:?}");
}

/// node: prints `1`; `conv({ 0: 1, length: 1 })` is `[1]`.
#[test]
fn slice_call_on_an_array_like() {
    let v = conv("console.log(conv({ 0: 1, length: 1 }));");
    assert!(proven(&v), "{v:?}");
}

/// node: prints `1`; the receiver is one of an array or `7` in one
/// expression: ToObject(7) has no `length`, its share is a new empty array.
#[test]
fn slice_call_on_array_or_number_in_one_expression() {
    let v = m_site(
        r#"
        function conv(x: any) { return Array.prototype.slice.call(x); }
        const arr: any = [new A()];
        const b: any = conv(process.argv.length > 99 ? 7 : arr);
        console.log(b[0].m());
        "#,
    );
    assert!(proven(&v), "{v:?}");
}

/// node: prints `1`; a boolean and a string share beside the array.
#[test]
fn slice_call_on_a_boolean_and_a_string() {
    let v = conv(r#"console.log(conv(true), conv("ab"));"#);
    assert!(proven(&v), "{v:?}");
}

/// Control: `slice.call(undefined)` throws (ToObject): its share adds
/// nothing, the array share stays proven.
#[test]
fn slice_call_on_undefined_adds_nothing() {
    let v = conv("try { conv(undefined); } catch (e) {}");
    assert!(proven(&v), "{v:?}");
}

/// Control: a receiver the program cannot see (`new Function(...)()`)
/// keeps today's unknown share: the result holds ⊤, the site is unproven.
#[test]
fn slice_call_on_an_unknown_value_stays_unknown() {
    let v = conv("console.log(conv(new Function(process.argv[2])()));");
    assert!(!proven(&v), "{v:?}");
}
