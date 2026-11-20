//! Rest parameters on the web target.

use super::run_web_source;

// A rest parameter receives the arguments from its position on as one array: in a function
// declaration, an arrow, an `any`-typed arrow, an instance, a static and a computed method,
// a generator, an array, promise and timer callback and an async function, and from a call
// in an async body.
// Expected output is Node's.
#[test]
fn a_rest_parameter_receives_its_arguments_as_an_array() {
    assert_eq!(
        run_web_source(
            r#"
function count(...r: number[]) { return r.length; }
function first(a: number, ...r: number[]) { return a + ":" + r.join(","); }
function anyRest(...r: any[]) { return r.length + "|" + r.join(","); }
const arrow = (...r: number[]) => r.length;
const anyArrow: any = (a: number, ...r: number[]) => a + "/" + r.join(",");
class K { m(...r: number[]) { return r.length; } static s(a: number, ...r: number[]) { return a + r.length; } }
async function countAsync(...r: number[]) { return r.length; }
console.log([count(1, 2, 3), count(), first(1, 2, 3), first(1), anyRest("a", 2), arrow(1, 2, 3), arrow(), anyArrow(1, 2, 3), anyArrow(1), new K().m(1, 2, 3), K.s(10, 1, 1)].join(" "));
console.log([1, 2].map((...r: number[]) => r.length).join(","));
countAsync(1, 2, 3).then((v) => console.log("async " + v));
const key = "c" + "";
class C2 { [key](...r: number[]) { return r.length; } }
function* gen(...r: number[]) { yield r.length; }
let seen = 0;
[7].forEach((...r: any[]) => { seen = r.length; });
console.log([(new C2() as any).c(1, 2, 3), gen(1, 2, 3).next().value, seen].join(" "));
async function fromAsync(xs: number[]) { return count(1, 2, 3) + " " + first(1) + " " + count(...xs); }
fromAsync([4, 5]).then((v) => console.log("from async " + v));
Promise.resolve(7).then((...r: any[]) => console.log("then " + r.length));
setTimeout((...r: any[]) => console.log("timer " + r.length), 0);
"#
        ),
        "3 0 1:2,3 1: 2|a,2 3 0 1/2,3 1/ 3 12\n3,3\n3 3 3\nasync 3\nfrom async 3 1: 2\nthen 1\ntimer 0"
    );
}
