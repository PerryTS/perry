//! A class declared in a function that extends a built-in, on the web target.

use super::run_web_source;

// Each evaluation's instances are the built-in's own objects, and an implicit constructor
// forwards its arguments to the built-in (an Error's message, an Array's length, a Map's
// or Set's entries), as for a class declared at module level. Expected output is Node's.
#[test]
fn evaluated_builtin_subclass_takes_its_arguments() {
    assert_eq!(
        run_web_source(
            r#"
function local(tag: string) {
  class E1 extends Error {}
  class E2 extends Error { constructor(m: string) { super(m + "!"); } }
  class E3 extends RangeError {}
  class A1 extends Array<number> {}
  class M1 extends Map<string, number> {}
  class S1 extends Set<number> { first() { return [...this][0]; } }
  const e1 = new E1(tag), e2 = new E2(tag), e3 = new E3(tag);
  const a = new A1(2), m = new M1([["k", 1]]), s = new S1([7, 8]);
  console.log(e1.message, e1 instanceof Error, e2.message, e3.message, e3 instanceof RangeError);
  console.log(Array.isArray(a), a.length, a instanceof A1, m.size, m.get("k"), m instanceof M1, s.size, s.first());
}
local("x");
local("y");
"#
        ),
        "x true x! x true\ntrue 2 true 1 1 true 2 7\ny true y! y true\ntrue 2 true 1 1 true 2 7"
    );
}
