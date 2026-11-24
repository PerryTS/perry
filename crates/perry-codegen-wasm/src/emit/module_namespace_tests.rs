//! `import * as m` used as a value on the web target (#15385).

use super::run_web_modules;

const LIB: &str = r#"
export const a = 1;
export function b() { return 2; }
export let counter = 0;
export function bump() { counter++; }
export class K { v = 3; }
export default "dflt";
"#;

// The namespace is one object per module: its exports sorted on a null prototype,
// tagged `Module`, with a live `let` binding; a class member is the
// class. Sync and async bodies see the same object. Expected output is Node's.
#[test]
fn a_namespace_import_used_as_a_value_is_the_module_namespace_object() {
    assert_eq!(
        run_web_modules(&[
            ("lib", LIB),
            (
                "main",
                r#"
import * as m from "./lib";
const ns: any = m;
function take(x: any) { return Object.keys(x).length + ":" + x.a + ":" + x.b(); }
console.log(typeof m, Object.keys(m).join(","), ns === m, take(m));
console.log(Object.prototype.toString.call(m), Object.getPrototypeOf(m) === null);
console.log(new m.K().v, typeof m.K, new ns.K().v, ns.default);
m.bump();
console.log(ns.counter, m.counter);
async function run() {
  const n2: any = m;
  return [typeof m, Object.keys(m).length, n2 === ns, m.a, n2.counter, new m.K().v].join(",");
}
run().then((v) => console.log(v));
"#
            ),
        ]),
        "object K,a,b,bump,counter,default true 6:1:2\n\
         [object Module] true\n\
         3 function 3 dflt\n\
         1 1\n\
         object,6,true,1,1,3"
    );
}
