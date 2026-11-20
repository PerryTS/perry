//! A function declaration used as a value on the web target (`function_value` in
//! `wasm_runtime.js`).

use super::run_web_source;

// A function declaration used as a value is one object, with the `prototype` an
// ordinary function has; `F.prototype = x` stores on it. Expected output is Node's.
#[test]
fn a_function_value_keeps_its_identity_and_has_a_prototype() {
    assert_eq!(
        run_web_source(
            r#"
function F(this: any) { this.v = 1; }
function G() {}
const fp: any = (F as any).prototype;
const before = fp;
(F as any).tag = "t";
(F as any).prototype = { replaced: true };
console.log([typeof fp, fp.constructor === F, Object.keys(fp).length, (F as any) === (F as any), (F as any).tag, (F as any).prototype.replaced, before !== (F as any).prototype, (G as any).prototype !== fp, typeof (() => 1 as any).prototype].join(","));
"#
        ),
        "object,true,0,true,t,true,true,true,undefined"
    );
}
