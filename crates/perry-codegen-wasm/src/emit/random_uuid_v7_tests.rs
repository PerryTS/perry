//! `crypto.randomUUIDv7()` on the web target.

use super::run_web_source;

// A version-7 UUID: the RFC 9562 layout, the version and variant nibbles, a leading
// 48-bit millisecond time within the run, and a fresh value per call.
#[test]
fn random_uuid_v7_has_the_v7_layout() {
    assert_eq!(
        run_web_source(
            r#"
import crypto from "node:crypto";
const before = Date.now();
const v = crypto.randomUUIDv7();
const after = Date.now();
const ms = Number("0x" + v.slice(0, 8) + v.slice(9, 13));
console.log(typeof v, /^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/.test(v));
console.log(ms >= before && ms <= after, crypto.randomUUIDv7() !== v);
"#
        ),
        "string true\ntrue true"
    );
}
