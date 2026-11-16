// R9READ8 (2026-11-16): typed-array constructor reads; the Vector3 arm of
// mixed receivers; builtin function receivers.
use super::*;
use perry_hir::Module;

fn hirs(src: &str) -> Vec<(String, Module)> {
    let path = "a.ts".to_string();
    let module = perry_parser::parse_typescript(src, &path).expect("parses");
    let hir = perry_hir::lower_module(&module, "a", &path).expect("lowers");
    vec![(path, hir)]
}

fn facts(src: &str) -> crate::ProgramFacts {
    let h = hirs(src);
    let list: Vec<(String, &Module)> = h.iter().map(|(p, m)| (p.clone(), m)).collect();
    let cfg = FactsConfig {
        unit_io: true,
        ..Default::default()
    };
    crate::compute(&list, &cfg)
}

fn reads(pf: &crate::ProgramFacts, key: &str) -> Vec<String> {
    let mi = pf.module_paths.iter().position(|p| p == "a.ts").expect("a.ts") as u32;
    let v: Vec<_> = pf
        .sites
        .iter()
        .filter(|s| s.module == mi && s.key == key && !s.access.is_store())
        .map(|s| format!("{:?} {:?}", s.verdict, s.receivers))
        .collect();
    for verdict in &v {
        eprintln!("RR8 .{key} verdict={verdict}");
    }
    v
}

fn proven(src: &str, key: &str) {
    let pf = facts(src);
    let v = reads(&pf, key);
    assert!(
        !v.is_empty() && v.iter().all(|x| !x.starts_with("No(")),
        "{v:?}\n{}",
        pf.dump_text()
    );
}

fn not_proven(src: &str, key: &str) {
    let pf = facts(src);
    let v = reads(&pf, key);
    assert!(v.iter().any(|x| x.starts_with("No(")), "{v:?}");
}

/// R9READ8 c1: a typed array's `constructor` read: no own `constructor`
/// on the receiver, no write onto a builtin prototype: each kind's
/// prototype's own data property (23.2.7.2), one of the kinds'
/// constructors.
#[test]
fn r9read8_c1_typed_array_constructor() {
    proven(
        "const n = Number(process.argv[2] ?? \"3\");
const a = new Float32Array(n);
const b = n > 2 ? new Int16Array(n) : new Uint8Array(n);
console.log(a.constructor === Float32Array, b.constructor === Int16Array);",
        "constructor",
    );
}

/// KEEP c2: an own `constructor` stored on the typed array decides the read.
#[test]
fn r9read8_c2_own_constructor_kept() {
    not_proven(
        "const n = Number(process.argv[2] ?? \"3\");
const a: any = new Float32Array(n);
a.constructor = Array;
console.log(a.constructor === Float32Array);",
        "constructor",
    );
}

/// KEEP c3: a write onto a kind's prototype under `constructor`.
#[test]
fn r9read8_c3_proto_constructor_written_kept() {
    not_proven(
        "const n = Number(process.argv[2] ?? \"3\");
(Float32Array.prototype as any).constructor = Array;
const a = new Float32Array(n);
console.log(a.constructor === Float32Array);",
        "constructor",
    );
}

/// KEEP c4: a call of `constructor` (no described method: reads only).
#[test]
fn r9read8_c4_constructor_call_kept() {
    not_proven(
        "const n = Number(process.argv[2] ?? \"3\");
const a: any = new Float32Array(n);
console.log(a.constructor(2));",
        "constructor",
    );
}
