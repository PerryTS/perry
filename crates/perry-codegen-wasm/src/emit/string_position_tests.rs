//! `includes` / `indexOf` / `startsWith` / `endsWith` / two-argument `slice` on the web
//! target, whose bridges are chosen by method name before the receiver's type is known.

use super::run_web_source;

// A position argument reaches String.prototype's method; any other receiver gets the
// method it has, with every argument (a closure stays the same value), and a receiver
// without one throws. Expected output is Node's.
#[test]
fn string_method_bridges_take_every_argument_and_the_receivers_own_method() {
    assert_eq!(
        run_web_source(
            r#"
const s = "hello world";
const u: any = "abcabc";
console.log([s.includes("hello", 1), s.includes("o", 5), s.startsWith("world", 6), s.endsWith("hello", 5), s.indexOf("o", 5), u.includes("a", 4), u.startsWith("c", 2), u.indexOf("b", 2)].join(","));
const b: any = [10, 20, 1];
const t = ["ab", "c", "ab"];
console.log([b.includes(2), b.includes(1, 2), b.indexOf(1), b.indexOf(20, 2), JSON.stringify(b.slice(0, 2)), t.includes("c", 2), t.indexOf("ab", 1)].join(","));
class Bag { includes(x: any, from: any) { return "own:" + x + ":" + from; } }
const bag: any = new Bag();
console.log(bag.includes(1, 2));
const fn = () => 1;
const fns: any = [fn];
const own: any = [1];
own.includes = (x: any) => "own" + x;
const n: any = 5;
let thrown: any;
try { n.includes(5); } catch (e) { thrown = e; }
console.log([fns.includes(fn), fns.indexOf(fn), own.includes(9), thrown instanceof TypeError].join(","));
async function f() { return [s.includes("hello", 1), s.indexOf("o", 5), b.includes(2), JSON.stringify(b.slice(0, 2))].join(","); }
f().then((v) => console.log("async " + v));
"#
        ),
        "false,true,true,true,7,false,true,4\nfalse,true,2,-1,[10,20],false,2\nown:1:2\ntrue,0,own9,true\nasync false,7,false,[10,20]"
    );
}
