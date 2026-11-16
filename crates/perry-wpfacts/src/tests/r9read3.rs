// R9READ3 (2026-11-16): settled prototype stores of array literals whose
// elements read class-body methods (`X.prototype.T = [ X.prototype.a, ... ]`).
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

/// Verdicts of the instance reads `this.<key>` (receiver a class instance,
/// not the prototype store sites).
fn verdicts(pf: &crate::ProgramFacts, key: &str) -> Vec<String> {
    let mi = pf.module_paths.iter().position(|p| p == "a.ts").expect("a.ts") as u32;
    let v: Vec<_> = pf
        .sites
        .iter()
        .filter(|s| s.module == mi && s.key == key && !s.access.is_store())
        .map(|s| format!("{:?}", s.verdict))
        .collect();
    for verdict in &v {
        eprintln!("RR3 .{key} verdict={verdict}");
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

const B: &str = "class P {
  k: number;
  constructor(k: number) { this.k = k; }
  a(x: number) { return x + 1; }
  b(x: number) { return x * 2; }
  pick() { const t = (this as any).T; return t[this.k](3); }
}
(P.prototype as any).L = { A: 0, B: 1 };
(P.prototype as any).T = [P.prototype.a, P.prototype.b];
const p: any = new P(Number(process.argv[2] ?? \"1\") & 1);
console.log(p.pick(), p.L.A);
";

// b1: the store completes before any instance exists: `T` is present.
#[test]
fn r9read3_b1_array_of_class_methods_settles() {
    proven(B, "T");
}

// KEEP bk1: a call between the class and the store may read `T` first.
#[test]
fn r9read3_bk1_call_before_the_store() {
    let src = B.replace(
        "(P.prototype as any).L =",
        "function early() { return new P(0).pick(); }\nconsole.log(String(Math.random()).length > 99 ? early() : 0);\n(P.prototype as any).L =",
    );
    not_proven(&src, "T");
}

// KEEP bk2: an element that is an accessor of the class body runs a getter
// (which can read `T` through `P.prototype`).
#[test]
fn r9read3_bk2_element_is_an_accessor() {
    let src = B
        .replace("  b(x: number) { return x * 2; }", "  get b(): any { return (P.prototype as any).T; }")
        .replace("[P.prototype.a, P.prototype.b]", "[P.prototype.a, (P.prototype as any).b]");
    not_proven(&src, "T");
}

// KEEP bk3: an element that calls code (`f()`) is no inert value.
#[test]
fn r9read3_bk3_element_calls_code() {
    let src = B.replace(
        "[P.prototype.a, P.prototype.b]",
        "[P.prototype.a, ((): any => new P(1).pick())()]",
    );
    not_proven(&src, "T");
}
