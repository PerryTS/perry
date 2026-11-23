//! `Object.is` on the web target.

use super::run_web_source;

// `Object.is` is SameValue in a synchronous and in an async body: NaN is NaN,
// 0 is not -0, strings compare by content and objects by identity. `===` is the
// control that differs on NaN and -0. Expected output is Node's.
#[test]
fn object_is_is_same_value() {
    assert_eq!(
        run_web_source(
            r#"
const nan = NaN; const z = 0; const nz = -0;
const o = {};
function sync() { return [Object.is(0, 0), Object.is(nan, NaN), Object.is(z, nz), Object.is(nz, -0), Object.is("a", "a"), Object.is("a", "b"), Object.is(o, o), Object.is(o, {}), Object.is(null, undefined), Object.is(1, "1"), typeof Object.is(1, 1)].join(","); }
async function inAsync() { return [Object.is(0, 0), Object.is(nan, NaN), Object.is(z, nz), Object.is(o, o), Object.is(o, {}), typeof Object.is(1, 1)].join(","); }
console.log(sync() + " | " + (z === nz) + "," + (nan === nan));
inAsync().then((v) => console.log(v));
"#
        ),
        "true,true,false,true,true,false,true,false,false,false,boolean | true,false\ntrue,true,false,true,false,boolean"
    );
}
