//! A protocol touch the solver runs on a token (`Model::implicit`: the
//! runtime calling any method of a value handed to an opaque builtin) has
//! no program site: what a method births there is one object per birth
//! site (`BORN`), before the summaries as after. A birth per touch call
//! made each copy a new token whose own touch births again (a typed
//! array's copy touched, copied, touched ...) until the stack overflowed
//! (DECL-B: 23 parity programs, `Buffer.compare` of typed arrays).

use super::*;

const SRC: &str = r#"
import { Buffer } from "node:buffer";
class Box { witness = 1; }
const box = new Box();
console.log(Buffer.compare(new Uint8Array([1, 2]), new Uint8Array([1, 3])), Buffer.compare(Buffer.from([1, 2, 3]), Buffer.from([1, 2])));
console.log(box.witness);
"#;

#[test]
fn a_protocol_touch_births_one_object_per_site() {
    let pf = facts(SRC);
    let d = pf.dump_text();
    let sites: Vec<_> = pf.sites.iter().filter(|s| s.key == "witness").collect();
    assert!(!sites.is_empty(), "{d}");
    assert!(sites.iter().all(|s| !s.top), "{d}");
}
