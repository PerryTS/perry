//! `Number.prototype.toString(radix)` on the web target.

use super::run_web_source;

// The radix selects the base for literals, locals, negatives and fractions; a radix
// of 10 or none prints decimal. Expected output is Node's.
#[test]
fn number_to_string_reads_its_radix() {
    assert_eq!(
        run_web_source(
            r#"
const n = 255;
const r: number = Number("16");
console.log([(255).toString(16), (8).toString(2), (35).toString(36), (-255).toString(16), (0.5).toString(2), n.toString(r), n.toString(), n.toString(10)].join(" "));
"#
        ),
        "ff 1000 z -ff 0.1 ff 255 255"
    );
}
