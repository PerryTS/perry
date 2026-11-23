//! `Math.max` / `Math.min` on the web target.

use super::run_web_source;

// A non-number argument converts with ToNumber, as the variadic forms do; two numbers
// keep their result, -0 and NaN included. Expected output is Node's.
#[test]
fn two_argument_math_max_and_min_convert_their_arguments() {
    assert_eq!(
        run_web_source(
            r#"
function id(v: any): any { return v; }
const show = (v: any) => v + ":" + typeof v;
const nul: any = id(null);
const und: any = id(undefined);
const tru: any = id(true);
const s5: any = id("5");
const s9: any = id("9");
console.log([show(Math.max(1, nul)), show(Math.min(1, nul)), show(Math.max(1, und)), show(Math.max(tru, 0)), show(Math.max(1, s5)), show(Math.max(s9, s5)), show(Math.min(s9, s5))].join(" "));
function two(a: number, b: number) { return Math.max(a, b) + "," + Math.min(a, b); }
console.log(two(3, 9) + " " + two(-0, 0) + " " + two(NaN, 1) + " " + (1 / Math.min(0, -0)) + " " + Math.max(1, 2, nul));
"#
        ),
        "1:number 0:number NaN:number 1:number 5:number 9:number 5:number\n9,3 0,0 NaN,NaN -Infinity 2"
    );
}

// One argument converts with ToNumber too; zero arguments give the identity.
#[test]
fn one_and_zero_argument_math_max_and_min_convert_their_arguments() {
    assert_eq!(
        run_web_source(
            r#"
function id(v: any): any { return v; }
const show = (v: any) => v + ":" + typeof v;
const nul: any = id(null);
const s5: any = id("5");
const tru: any = id(true);
console.log([show(Math.max(nul)), show(Math.min(s5)), show(Math.max(tru)), show(Math.min(id(undefined))), show(Math.max()), show(Math.min()), 1 / Math.max(-0), show(Math.max(nul, s5, tru))].join(" "));
"#
        ),
        "0:number 5:number 1:number NaN:number -Infinity:number Infinity:number -Infinity 5:number"
    );
}
