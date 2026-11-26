//! `Array.prototype.lastIndexOf` on the web target (#15396).

use super::run_web_source;

// fromIndex counts from the end when negative, an explicit `undefined` searches from
// index 0, and NaN is never found. Expected output is Node's.
#[test]
fn last_index_of_searches_back_from_from_index() {
    assert_eq!(
        run_web_source(
            r#"
const a = [1, 2, 3, 2, 1];
console.log([a.lastIndexOf(2), a.lastIndexOf(2, 2), a.lastIndexOf(1, -2), a.lastIndexOf(9), a.lastIndexOf(1, undefined)].join(","));
console.log([a.lastIndexOf(1, -6), a.lastIndexOf(1, 99), a.lastIndexOf(2, 1.7)].join(","));
const s = ["x", "y", "x"];
console.log([s.lastIndexOf("x"), s.lastIndexOf("x", 1), [NaN, 1].lastIndexOf(NaN)].join(","));
"#
        ),
        "3,1,0,-1,0\n-1,4,1\n2,0,-1"
    );
}
