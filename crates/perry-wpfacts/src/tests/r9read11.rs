// R9READ11 (2026-11-16): a builtin prototype taken as a value
// (`Array.prototype`) is that builtin prototype object only (ECMA-262
// 23.1.3: Array.prototype is an Array exotic object whose [[Prototype]] is
// %Object.prototype%; it is not %Object.prototype%), and a write through it
// is still the collapsed builtin-prototype write; `F.call( x )` with F
// exactly a builtin method value calls the builtin Function.prototype.call
// (20.2.3.3).
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

/// The verdicts of the non-store sites of `key` in a.ts.
fn sites(pf: &crate::ProgramFacts, key: &str) -> Vec<String> {
    let mi = pf.module_paths.iter().position(|p| p == "a.ts").expect("a.ts") as u32;
    let v: Vec<_> = pf
        .sites
        .iter()
        .filter(|s| s.module == mi && s.key == key && !s.access.is_store())
        .map(|s| format!("{:?} {:?}", s.verdict, s.receivers))
        .collect();
    for verdict in &v {
        eprintln!("RR11 .{key} verdict={verdict}");
    }
    v
}

fn proven(v: &[String]) -> bool {
    !v.is_empty() && v.iter().all(|x| !x.starts_with("No("))
}

const SLICE: &str = "const n = Number(process.argv[2] ?? \"3\");
const a: any = n > 2 ? new Float32Array(n) : [1, 2, 3];
";
const USE: &str = "console.log(Array.prototype.slice.call(a).length);\n";

/// s1 (RED before): `.slice` reads the builtin Array.prototype.slice (the
/// value of `Array.prototype` is Array.prototype only); `.call` is the
/// builtin Function.prototype.call on that builtin method value.
#[test]
fn r9read11_s1_slice_call_proven() {
    let pf = facts(&format!("{SLICE}{USE}"));
    assert!(proven(&sites(&pf, "slice")), "slice");
    assert!(proven(&sites(&pf, "call")), "call");
}

/// KEEP s2: `Array.prototype.slice` is replaced somewhere.
#[test]
fn r9read11_s2_slice_written_kept() {
    let pf = facts(&format!("{SLICE}(Array.prototype as any).slice = function () {{ return [n]; }};\n{USE}"));
    assert!(!proven(&sites(&pf, "slice")), "slice");
}

/// KEEP s3: `Function.prototype.call` is replaced somewhere.
#[test]
fn r9read11_s3_call_written_kept() {
    let pf = facts(&format!("{SLICE}(Function.prototype as any).call = function () {{ return [n]; }};\n{USE}"));
    assert!(!proven(&sites(&pf, "call")), "call");
}

/// KEEP s4: a method stored through `Array.prototype` is seen by a read on
/// an array and on a plain object (the collapsed builtin prototype).
#[test]
fn r9read11_s4_store_through_array_prototype_seen() {
    let pf = facts(&format!(
        "{SLICE}(Array.prototype as any).zz = function () {{ return n; }};\nconsole.log(a.zz(), ([n] as any).zz(), ({{}} as any).zz);\n"
    ));
    let v = sites(&pf, "zz");
    assert!(!v.iter().any(|x| x.contains("Absent") || x.contains("Dead")), "{v:?}");
}

/// KEEP s5: an accessor defined through `Array.prototype` is seen.
#[test]
fn r9read11_s5_define_through_array_prototype_seen() {
    let pf = facts(&format!(
        "{SLICE}Object.defineProperty(Array.prototype, \"zz\", {{ get() {{ return n; }} }});\nconsole.log(([n] as any).zz, ({{}} as any).zz);\n"
    ));
    let v = sites(&pf, "zz");
    assert!(!v.iter().any(|x| x.contains("Absent") || x.contains("Dead")), "{v:?}");
}

/// KEEP s6: `delete Array.prototype.slice`: `slice` is a locked member
/// (DEC 250, `members::locked_read`: the runtime refuses the delete), so
/// the read may still be decided, but never as a claim codegen uses
/// (NativeMember): the site stays the generic read.
#[test]
fn r9read11_s6_delete_slice_kept() {
    let pf = facts(&format!("{SLICE}delete (Array.prototype as any).slice;\n{USE}"));
    assert!(!sites(&pf, "slice").iter().any(|v| v.starts_with("NativeMember")), "slice");
}

/// KEEP s7: `Array.prototype` escapes to unknown code (a callback the
/// program does not see is impossible: closed world; here it is stored on
/// globalThis and read back under an unknown key).
#[test]
fn r9read11_s7_escape_kept() {
    let pf = facts(&format!(
        "{SLICE}(globalThis as any)[String(n)] = Array.prototype;\n((globalThis as any)[process.argv[3] ?? \"x\"] as any).zz = 1;\nconsole.log(({{}} as any).zz);\n"
    ));
    let v = sites(&pf, "zz");
    assert!(!v.iter().any(|x| x.contains("Absent") || x.contains("Dead")), "{v:?}");
}

/// KEEP s8: a read of a key through the value sees a method stored on
/// `Object.prototype` and one stored through `Array.prototype`, and the
/// member `Array.prototype` inherits (`hasOwnProperty`).
#[test]
fn r9read11_s8_reads_through_value_see_writes() {
    let pf = facts(&format!(
        "{SLICE}(Object.prototype as any).yy = function () {{ return n; }};\n(Array.prototype as any).zz = function () {{ return n; }};\nconsole.log((Array.prototype as any).yy(), (Array.prototype as any).zz(), (Array.prototype as any).ww, Array.prototype.hasOwnProperty(\"x\"));\n"
    ));
    for k in ["yy", "zz"] {
        let v = sites(&pf, k);
        assert!(!v.iter().any(|x| x.contains("Absent") || x.contains("Dead")), "{k} {v:?}");
    }
    let v = sites(&pf, "hasOwnProperty");
    assert!(!v.is_empty(), "{v:?}");
}

/// KEEP s9: `Array.prototype` stored on as an Array exotic object under an
/// index name and its `slice` replaced through a computed key.
#[test]
fn r9read11_s9_computed_store_kept() {
    let pf = facts(&format!("{SLICE}(Array.prototype as any)[process.argv[3] ?? \"q\"] = function () {{ return [n]; }};\n{USE}"));
    assert!(!proven(&sites(&pf, "slice")), "slice");
}

/// KEEP s10: a method value called through `.call` after `F.call = g` on it.
#[test]
fn r9read11_s10_own_call_on_fn_kept() {
    let pf = facts(&format!("{SLICE}(Array.prototype.slice as any).call = function () {{ return [n]; }};\n{USE}"));
    assert!(!proven(&sites(&pf, "call")), "call");
}
