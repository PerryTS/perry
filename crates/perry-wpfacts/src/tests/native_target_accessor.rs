//! An accessor `Object.defineProperty` installs on a value from unmodeled
//! code (a native module's class, its prototype, a stream or emitter
//! instance: all ⊤ to the model) is that code's to run, as a value stored
//! there is: every read of the key there, by compiled code or by the
//! runtime, may call it. SOUND2 (CXCHK group G2):
//! test_gap_streambirth2_emitter, test_gap_stream_proto_writes,
//! test_gap_native_constructor_static_get, pipe-listener-protocol.
//! An accessor on a holder the model knows keeps its own rules: one on a
//! program object runs only where a read finds it, and DEC 250's lock
//! still decides what a sealed intrinsic prototype member is.
use super::*;

fn hit_runs(src: &str) -> bool {
    let module = perry_parser::parse_typescript(src, "t.ts").expect("parses");
    let hir = perry_hir::lower_module(&module, "t", "t.ts").expect("lowers");
    let cfg = FactsConfig {
        max_work: Some(50_000_000),
        ..FactsConfig::default()
    };
    let pf = crate::compute(&[("t.ts".to_string(), &hir)], &cfg);
    assert!(pf.body_reach.complete, "activated");
    let id = hir
        .functions
        .iter()
        .find(|f| f.name == "hit")
        .expect("hit")
        .id;
    pf.body_reach.bodies.iter().any(|(_, i, _)| *i == id)
}

#[test]
fn native_target_accessor_runs() {
    for (name, src) in [
        (
            "emitter prototype getter, read on an instance",
            r#"import { EventEmitter } from "events";
function hit() { return 1; }
Object.defineProperty(EventEmitter.prototype, "y", { get() { return hit(); }, configurable: true });
const e: any = new EventEmitter(); console.log(e.y);"#,
        ),
        (
            "emitter prototype getter, read by the runtime",
            r#"import { EventEmitter } from "events";
function hit() { return 1; }
const proto: any = EventEmitter.prototype;
Object.defineProperty(proto, "_maxListeners", { get() { return hit(); }, configurable: true });
const e: any = new EventEmitter(); console.log(e.getMaxListeners());"#,
        ),
        (
            "emitter prototype setter, written by the runtime",
            r#"import { EventEmitter } from "events";
function hit(v: any) { return v; }
const proto: any = EventEmitter.prototype;
Object.defineProperty(proto, "_maxListeners", { get() { return 1; }, set(v: any) { hit(v); }, configurable: true });
console.log(typeof new EventEmitter());"#,
        ),
        (
            "native constructor static getter, read through a subclass",
            r#"import { EventEmitter } from "events";
function hit() { return 1; }
class C extends EventEmitter {}
const E: any = EventEmitter; const CC: any = C;
Object.defineProperty(E, "g", { get() { return hit(); }, configurable: true });
console.log(CC.g);"#,
        ),
        (
            "stream prototype getter",
            r#"import { Readable } from "stream";
function hit() { return 1; }
Object.defineProperty(Readable.prototype, "y", { get() { return hit(); }, configurable: true });
const r: any = new Readable({ read() {} }); console.log(r.y);"#,
        ),
        (
            "stream constructor `call` getter",
            r#"import { Duplex } from "stream";
function hit() { return function () { return 1; }; }
Object.defineProperty(Duplex, "call", { get() { return hit(); }, configurable: true });
console.log((Duplex as any).call({}));"#,
        ),
    ] {
        assert!(hit_runs(src), "{name}: hit inactive, Node runs it");
    }
}

#[test]
fn known_holder_accessor_controls() {
    for (name, src) in [
        (
            "a program object's getter nobody reads",
            r#"function hit() { return 1; }
const o: any = { x: 1 };
Object.defineProperty(o, "y", { get() { return hit(); }, configurable: true });
console.log(o.x);"#,
        ),
        (
            "a native value read, no accessor installed there",
            r#"import { EventEmitter } from "events";
function hit() { return 1; }
const o: any = { get y() { return hit(); } };
const e: any = new EventEmitter(); console.log(e.y, typeof o);"#,
        ),
    ] {
        assert!(!hit_runs(src), "{name}: hit active, Node never runs it");
    }
}

/// The getter escapes, the reads stay: an accessor on a ⊤ holder under a
/// key a sealed intrinsic prototype owns (`map`, an `Image` key the DEC 250
/// lock refuses to redefine there, perry_abi::lock::refuses) or under a key
/// a program object owns leaves those reads proven: the lock (`final_on`)
/// and the own field decide them, not the escaped getter.
#[test]
fn locked_member_and_own_field_stay_proven() {
    let src = r#"import { EventEmitter } from "events";
function hit() { return (f: any) => [f]; }
class Box { witness = 1; }
const box = new Box();
const proto: any = EventEmitter.prototype;
Object.defineProperty(proto, "map", { get() { return hit(); }, configurable: true });
Object.defineProperty(proto, "witness", { get() { return hit(); }, configurable: true });
const xs = [1].map((x) => x + 1);
console.log(xs.length, box.witness);"#;
    assert!(hit_runs(src), "the getter on a ⊤ holder escapes");
    let pf = facts(src);
    for key in ["length", "witness"] {
        let s: Vec<_> = pf.sites.iter().filter(|s| s.key == key).collect();
        assert!(!s.is_empty(), "{key}: {}", pf.dump_text());
        for s in s {
            assert!(
                !matches!(s.verdict, Verdict::No(_)),
                "{key}: {:?}",
                s.verdict
            );
        }
    }
}
