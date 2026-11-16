// R9READ2 (2026-11-16): rule I, chain reads in the construction phase that run no code.
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

fn verdicts(pf: &crate::ProgramFacts, key: &str) -> Vec<String> {
    let mi = pf.module_paths.iter().position(|p| p == "a.ts").expect("a.ts") as u32;
    let v: Vec<_> = pf
        .sites
        .iter()
        .filter(|s| s.module == mi && s.key == key && !s.access.is_store())
        .map(|s| format!("{:?}", s.verdict))
        .collect();
    for verdict in &v {
        eprintln!("RR2 .{key} verdict={verdict}");
    }
    v
}

fn proven(src: &str, key: &str) {
    let pf = facts(src);
    let v = verdicts(&pf, key);
    assert!(
        !v.is_empty() && v.iter().all(|x| !x.starts_with("No(")),
        "{v:?}\n{}",
        pf.dump_text()
    );
}

fn not_proven(src: &str, key: &str) {
    let pf = facts(src);
    let v = verdicts(&pf, key);
    assert!(v.iter().any(|x| x.starts_with("No(")), "{v:?}");
}

// i: a construction-phase read of a prototype data key the program defines
// no accessor under (KeyframeTrack `this.TimeBufferType`), before the store
// of the key a later read takes `.length` of.
const I: &str = "function conv(a: any, t: any) {
  if (!a || a.constructor === t) return a;
  return new t(a);
}
function sz(o: any) { return o.times.length; }
class K {
  constructor(n: string, times: number[]) {
    if (times === undefined || times.length === 0) throw new Error(\"no keys\");
    this.name = n;
    this.times = conv(times, this.TB);
  }
}
K.prototype.TB = Float32Array;
const k = new K(process.argv[2] ?? String(Math.random()), [1, 2, 3]);
console.log(sz(k));
";

const GET: &str = "{ get() { if (Math.random() < 0) sz(this); return Float32Array; } }";

#[test]
fn r9read2_i1_inert_chain_read_keeps_key_present() {
    proven(I, "length");
}

#[test]
fn r9read2_i2_subclass_without_constructor() {
    let src = I.replace("const k = new K(", "class V extends K {}\nconst k = new V(");
    proven(&src, "length");
}

// KEEP: an accessor under the key runs with `this` before the store.
#[test]
fn r9read2_ik1_accessor_under_the_key() {
    let src = I.replace("K.prototype.TB = Float32Array;", &format!("Object.defineProperty(K.prototype, \"TB\", {GET});"));
    not_proven(&src, "length");
}

// KEEP: an accessor under a computed key that may spell it.
#[test]
fn r9read2_ik2_accessor_under_unknown_key() {
    let src = I.replace(
        "K.prototype.TB = Float32Array;",
        &format!("Object.defineProperty(K.prototype, process.argv[3] ?? \"q\", {GET});\nK.prototype.TB = Float32Array;"),
    );
    not_proven(&src, "length");
}

// KEEP: a subclass getter under the key (its instances read it first).
#[test]
fn r9read2_ik3_subclass_getter() {
    let src = I.replace(
        "const k = new K(",
        "class S extends K { get TB() { if (Math.random() < 0) sz(this); return Float32Array; } }\nconst k = new S(",
    );
    not_proven(&src, "length");
}
