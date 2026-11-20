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
    let mi = pf
        .module_paths
        .iter()
        .position(|p| p == "a.ts")
        .expect("a.ts") as u32;
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
    let pf = facts(&format!(
        "{SLICE}(Array.prototype as any).slice = function () {{ return [n]; }};\n{USE}"
    ));
    assert!(!proven(&sites(&pf, "slice")), "slice");
}

/// KEEP s3: `Function.prototype.call` is replaced somewhere.
#[test]
fn r9read11_s3_call_written_kept() {
    let pf = facts(&format!(
        "{SLICE}(Function.prototype as any).call = function () {{ return [n]; }};\n{USE}"
    ));
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
    assert!(
        !v.iter().any(|x| x.contains("Absent") || x.contains("Dead")),
        "{v:?}"
    );
}

/// KEEP s5: an accessor defined through `Array.prototype` is seen.
#[test]
fn r9read11_s5_define_through_array_prototype_seen() {
    let pf = facts(&format!(
        "{SLICE}Object.defineProperty(Array.prototype, \"zz\", {{ get() {{ return n; }} }});\nconsole.log(([n] as any).zz, ({{}} as any).zz);\n"
    ));
    let v = sites(&pf, "zz");
    assert!(
        !v.iter().any(|x| x.contains("Absent") || x.contains("Dead")),
        "{v:?}"
    );
}

/// KEEP s6: `delete Array.prototype.slice`: `slice` is a locked member
/// (DEC 250, `members::locked_read`: the runtime refuses the delete), so
/// the read may still be decided, but never as a claim codegen uses
/// (NativeMember): the site stays the generic read.
#[test]
fn r9read11_s6_delete_slice_kept() {
    let pf = facts(&format!(
        "{SLICE}delete (Array.prototype as any).slice;\n{USE}"
    ));
    assert!(
        !sites(&pf, "slice")
            .iter()
            .any(|v| v.starts_with("NativeMember")),
        "slice"
    );
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
    assert!(
        !v.iter().any(|x| x.contains("Absent") || x.contains("Dead")),
        "{v:?}"
    );
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
        assert!(
            !v.iter().any(|x| x.contains("Absent") || x.contains("Dead")),
            "{k} {v:?}"
        );
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
    let pf = facts(&format!(
        "{SLICE}(Array.prototype.slice as any).call = function () {{ return [n]; }};\n{USE}"
    ));
    assert!(!proven(&sites(&pf, "call")), "call");
}

const TRACK: &str = "class T {
  constructor(i: number) { this.set(i); }
  set(i: number) {
    let f: any;
    switch (i) { case 0: f = this.A; break; case 1: f = this.B; break; }
    if (f === undefined) return;
    (this as any).ci = f;
  }
  copy() { const c: any = new (this.constructor as any)(2); c.ci = (this as any).ci; return c; }
  A(x: any) { return 1; }
  B(x: any) { return 2; }
}
class U extends T {}
const n = Number(process.argv[2] ?? \"1\");
const ts: any[] = [new T(n), new U(n), new T(n).copy()];
";
const CI: &str =
    "for (const t of ts) { try { console.log(t.ci(null)); } catch (e) { console.log(\"TE\"); } }\n";

/// c1 (RED before): `t.ci( … )` where `ci` is an own key a conditional
/// store in a method (and a copy) gives, the chain lacks it: the callees
/// are the field's known methods, else the TypeError arm.
#[test]
fn r9read11_c1_own_late_key_call() {
    let pf = facts(&format!("{TRACK}{CI}"));
    assert!(proven(&sites(&pf, "ci")), "ci");
}

/// KEEP c2: the class prototype has a `ci` method too (the chain holds it).
#[test]
fn r9read11_c2_chain_has_key_kept() {
    let pf = facts(&format!(
        "{TRACK}(T.prototype as any).ci = function () {{ return 3; }};\n{CI}"
    ));
    assert!(!proven(&sites(&pf, "ci")), "ci");
}

/// c3: a computed-key store may write `ci` with another value, a known
/// function. The key is an unknown string: it may name `ci` (OrdinarySet,
/// 10.1.9.2: an own data property on the ordinary instance), but it is
/// never `__proto__` (OPTIMISTIC, owner 2026-11-16), so the instance's
/// [[Prototype]] stays its class prototype. [[Get]] of `ci` then finds
/// the own data property, whose values are every store of `ci` on the
/// token (A, B, and the stored function), or nothing: the callee set
/// {T::A, T::B, the stored function} is closed, beside the TypeError
/// arm. (Before the owner rule the unknown key might be `__proto__`, so
/// the instance was relinked and the site refused.) c3b guards that the
/// stored value is in the set.
#[test]
fn r9read11_c3_computed_store_known_fn() {
    let pf = facts(&format!(
        "{TRACK}ts[0][process.argv[3] ?? \"z\"] = function () {{ return 4; }};\n{CI}"
    ));
    assert!(proven(&sites(&pf, "ci")), "ci");
}

/// KEEP c3b: the computed-key store's value is unknown (a global read under
/// an unknown key): the callee set holds it, so it is not closed.
#[test]
fn r9read11_c3b_computed_store_unknown_value_kept() {
    let pf = facts(&format!(
        "{TRACK}ts[0][process.argv[3] ?? \"z\"] = (globalThis as any)[process.argv[4]];\n{CI}"
    ));
    assert!(!proven(&sites(&pf, "ci")), "ci");
}

/// KEEP c4: `Object.prototype.ci` written.
#[test]
fn r9read11_c4_object_prototype_kept() {
    let pf = facts(&format!(
        "{TRACK}(Object.prototype as any).ci = function () {{ return 5; }};\n{CI}"
    ));
    assert!(!proven(&sites(&pf, "ci")), "ci");
}

/// KEEP c5: an accessor `ci` on the class prototype.
#[test]
fn r9read11_c5_accessor_kept() {
    let pf = facts(&format!("{TRACK}Object.defineProperty(T.prototype, \"ci\", {{ get() {{ return () => 6; }}, set(v) {{}} }});\n{CI}"));
    assert!(!proven(&sites(&pf, "ci")), "ci");
}

/// KEEP c6: the field may hold a non-function (a number): the call of it
/// throws too, but the callee set is not a set of functions.
#[test]
fn r9read11_c6_number_value_kept() {
    let pf = facts(&format!("{TRACK}ts[1].ci = n;\n{CI}"));
    assert!(!proven(&sites(&pf, "ci")), "ci");
}
