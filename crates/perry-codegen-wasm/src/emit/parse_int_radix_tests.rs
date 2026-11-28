//! `parseInt(string, radix)` and `parseFloat` on the web target.

use super::run_web_source;

// The radix selects the base; an omitted or zero radix detects a `0x` prefix; a radix
// outside 2..=36 gives NaN. Expected output is Node's.
#[test]
fn parse_int_reads_its_radix() {
    assert_eq!(
        run_web_source(
            r#"
const r: number = Number("16");
console.log([parseInt("ff", 16), parseInt("0x1F", 16), parseInt("777", 8), parseInt("9", 8), parseInt("z", 36), parseInt("101", 2), parseInt("ff", r)].join(" "));
console.log([parseInt("0x1F"), parseInt("0x1F", 0), parseInt("42"), parseInt("12", 10), parseInt("ff", 1), parseInt("ff", 37)].join(" "));
"#
        ),
        "255 31 511 NaN 35 5 255\n31 31 42 12 NaN NaN"
    );
}

// An async body, which is emitted as JavaScript, calls the same bridges as a
// synchronous one. Expected output is Node's.
#[test]
fn async_body_parse_int_and_parse_float_match_sync_code() {
    assert_eq!(
        run_web_source(
            r#"
async function parse(s: string, r: number): Promise<string> {
  return [parseInt(s), parseInt("ff", 16), parseInt(s, r), parseInt("0x1F"), parseInt("ff", 37), parseFloat("3.5abc"), parseFloat("x")].join(" ");
}
parse("42", 8).then((v) => console.log(v));
"#
        ),
        "42 255 34 31 NaN 3.5 NaN"
    );
}
