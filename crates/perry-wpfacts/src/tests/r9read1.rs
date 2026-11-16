// R9READ1 (2026-11-16): the remaining r9 read/call sites.
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
    let mi = pf
        .module_paths
        .iter()
        .position(|p| p == "a.ts")
        .expect("a.ts") as u32;
    let v: Vec<_> = pf
        .sites
        .iter()
        .filter(|s| s.module == mi && s.key == key && !s.access.is_store())
        .map(|s| format!("{:?}", s.verdict))
        .collect();
    for verdict in &v {
        eprintln!("RR1 .{key} verdict={verdict}");
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

// b: a method value copied through a local into a field, then called.
const B: &str = "class P {
  _a() { return 1; }
  constructor(t: string) {
    let g = this._a;
    this.f = g;
  }
  run() { return this.f(); }
}
const p = new P(process.argv[2] ?? String(Math.random()));
console.log(p.run());
";

#[test]
fn r9read1_b1_method_through_local_is_proven() {
    proven(B, "f");
}

#[test]
fn r9read1_b2_method_value_direct() {
    proven(
        &B.replace("let g = this._a;\n    this.f = g;", "this.f = this._a;"),
        "f",
    );
}

#[test]
fn r9read1_b3_method_through_const() {
    proven(&B.replace("let g", "const g"), "f");
}

#[test]
fn r9read1_b4_keep_function_local() {
    proven(
        &B.replace("let g = this._a;", "let g = function () { return 2; };"),
        "f",
    );
}

// KEEP: what can take the method away or store `undefined` over it.
#[test]
fn r9read1_bk1_delete() {
    not_proven(
        &B.replace(
            "const p =",
            "if (process.argv[2]) delete (P.prototype as any)._a;\nconst p =",
        ),
        "f",
    );
}

#[test]
fn r9read1_bk2_stored_undefined() {
    not_proven(
        &B.replace(
            "const p =",
            "if (process.argv[2]) (P.prototype as any)._a = undefined;\nconst p =",
        ),
        "f",
    );
}

#[test]
fn r9read1_bk3_computed_undefined() {
    not_proven(
        &B.replace(
            "const p =",
            "(P.prototype as any)[process.argv[2] ?? \"x\"] = undefined;\nconst p =",
        ),
        "f",
    );
}

// R9READ11: `f` then holds the prototype method or `undefined` (the own
// `_a` stored over it): the call runs `_a` or throws a TypeError: decided
// per kind, never a claim codegen uses (T0Mono / T0Set).
#[test]
fn r9read1_bk4_instance_undefined() {
    let pf = facts(&B.replace(
        "let g = this._a;",
        "if (t.length > 3) (this as any)._a = undefined;\n    let g = this._a;",
    ));
    let v = verdicts(&pf, "f");
    assert!(
        !v.is_empty()
            && v.iter()
                .all(|x| x.starts_with("No(") || x.starts_with("PerKind")),
        "{v:?}"
    );
}

#[test]
fn r9read1_bk5_define_property() {
    not_proven(&B.replace("const p =", "if (process.argv[2]) Object.defineProperty(P.prototype, \"_a\", { get() { return 7; } });\nconst p ="), "f");
}

// d: a settled static `X.k = v` on a class with a well-known-symbol member
// (`*[Symbol.iterator]()`): the member key is a Symbol, never `k`.
const D: &str = "class E {
  x: number; o: any;
  constructor(o = (E as any).DEFAULT) { this.x = 1; this.o = o; }
  *[Symbol.iterator]() { yield this.x; }
}
(E as any).DEFAULT = 'XYZ';
const e = new E(process.argv[2] ? undefined : 'abc');
console.log(e.o, [...e].length);
";

#[test]
fn r9read1_d1_static_beside_symbol_member() {
    proven(D, "DEFAULT");
}

#[test]
fn r9read1_dk1_evaluated_computed_member() {
    not_proven(
        &D.replace(
            "*[Symbol.iterator]() { yield this.x; }",
            "[String(Math.random())]() { return 1; }",
        )
        .replace("[...e].length", "0"),
        "DEFAULT",
    );
}

#[test]
fn r9read1_dk2_code_runs_before_the_store() {
    not_proven(
        &D.replace(
            "(E as any).DEFAULT = ",
            "const e0 = new E();\n(E as any).DEFAULT = ",
        ),
        "DEFAULT",
    );
}
