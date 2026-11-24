//! `Array.prototype.keys` / `entries` / `values` and array literal holes on the web target.

use super::run_web_source;

// The iterators an array's `keys()`, `entries()` and `values()` return, spread, walked by
// `for...of` and stepped with `next()`; a hole in a literal counts in its length and is
// not an element, with and without a spread beside it. Expected output is Node's.
#[test]
fn array_iterators_and_holes_match_node() {
    assert_eq!(
        run_web_source(
            r#"
const xs = [10, 20, 30];
console.log([...xs.keys()].join(","), [...xs.entries()].map((e) => e.join(":")).join(","), [...xs.values()].join(","));
let sum = 0;
for (const [i, v] of xs.entries()) sum += i * v;
const it = xs.values();
console.log(sum, it.next().value, it.next().value, typeof it.next);
const h = [1, , 3];
console.log(h.length, 0 in h, 1 in h, 2 in h, h.join("-"));
const s = [...xs, , 4];
console.log(s.length, 3 in s, s.join("-"));
"#
        ),
        "0,1,2 0:10,1:20,2:30 10,20,30\n80 10 20 function\n3 true false true 1--3\n5 false 10-20-30--4"
    );
}
