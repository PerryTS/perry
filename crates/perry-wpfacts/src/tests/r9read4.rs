// R9READ4 (2026-11-16): a class-body method read on the prototype object
// itself (`X.prototype.m`) is present: no `undefined` joins the read.
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

/// Verdicts of the call sites `<recv>.<key>(...)`.
fn calls(pf: &crate::ProgramFacts, key: &str) -> Vec<String> {
    let mi = pf.module_paths.iter().position(|p| p == "a.ts").expect("a.ts") as u32;
    let v: Vec<_> = pf
        .sites
        .iter()
        .filter(|s| s.module == mi && s.key == key && !s.access.is_store())
        .map(|s| format!("{:?}", s.verdict))
        .collect();
    for verdict in &v {
        eprintln!("RR4 .{key} verdict={verdict}");
    }
    v
}

fn proven(src: &str, key: &str) {
    let pf = facts(src);
    let v = calls(&pf, key);
    assert!(
        !v.is_empty() && v.iter().all(|x| !x.starts_with("No(")),
        "{v:?}\n{}",
        pf.dump_text()
    );
}

fn not_proven(src: &str, key: &str) {
    let pf = facts(src);
    let v = calls(&pf, key);
    assert!(v.iter().any(|x| x.starts_with("No(")), "{v:?}");
}

const A: &str = "class P {
  g: any;
  constructor(public n: number) { this.g = this.a; }
  a(b: number[]) { b[0] = this.n; }
  b(b: number[]) { b[0] = this.n + 1; }
  bind() {
    let t = 0;
    if ((process.argv[2] || \"\") === \"x\") t = 1;
    this.g = G[t];
    this.g([1]);
  }
}
const G = [P.prototype.a, P.prototype.b];
/*EXTRA*/
new P(Number(process.argv[3] || \"1\")).bind();
";

// a1: `P.prototype.a` reads the class-body method (15.7.14, 10.2.8): the
// element read under {0, 1} of the 2-element literal holds only methods.
#[test]
fn r9read4_a1_proto_method_read_present() {
    proven(A, "g");
}

// KEEP a2: a `delete` of the method may leave it absent.
#[test]
fn r9read4_a2_deleted_method_keeps_undefined() {
    not_proven(&A.replace("/*EXTRA*/", "if (process.argv[4]) delete (P.prototype as any).a;"), "g");
}

// KEEP a3: a stored `undefined` reaches the read.
#[test]
fn r9read4_a3_stored_undefined_kept() {
    not_proven(&A.replace("/*EXTRA*/", "if (process.argv[4]) (P.prototype as any).a = undefined;"), "g");
}

// KEEP a4: an index past the end reads `undefined`.
#[test]
fn r9read4_a4_index_past_end_kept() {
    not_proven(&A.replace("t = 1;", "t = 2;"), "g");
}

// KEEP a5: an accessor under the key on the prototype.
#[test]
fn r9read4_a5_accessor_kept() {
    not_proven(
        &A.replace("/*EXTRA*/", "if (process.argv[4]) Object.defineProperty(P.prototype, \"a\", { get() { return undefined; } });"),
        "g",
    );
}

const B6: &str = "class P {
  g: any; s: any;
  constructor(public n: number) { this.g = this.u; this.s = this.u; }
  a(b: number[]) { b[0] = this.n; }
  b(b: number[]) { b[0] = this.n + 1; }
  c(b: number[]) { this.n = b[0]; }
  d(b: number[]) { this.n = b[0] * 2; }
  u(b: number[]) { this.bind(); this.g(b); this.s(b); }
  bind() {
    const self: any = this;
    let v = self.V.None;
    if (this.n > 3) v = self.V.Up;
    let t = self.T.A;
    if ((process.argv[2] || \"\") === \"x\") t = self.T.B;
    this.g = self.G[t];
    this.s = self.S[t][v];
  }
}
(P.prototype as any).T = { A: 0, B: 1 };
(P.prototype as any).V = { None: 0, Up: 1 };
(P.prototype as any).G = [P.prototype.a, P.prototype.b];
(P.prototype as any).S = [[P.prototype.c, P.prototype.d], [P.prototype.d, P.prototype.c]];
const p = new P(Number(process.argv[3] || \"1\"));
p.u([5]);
";

// a6: the settled prototype shape: index read from settled prototype
// constants `{ A: 0, B: 1 }`, arrays of class-body methods stored on the
// prototype (nested for `s`).
#[test]
fn r9read4_a6_settled_proto_tables() {
    proven(B6, "g");
    proven(B6, "s");
}
