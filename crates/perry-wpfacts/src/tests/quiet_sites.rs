//! Quiet sites (`quiet_sites`): the reads, element sites and values that
//! run no program code.

use crate::bindings::Rep;
use crate::FactsConfig;

struct Solved {
    pf: crate::ProgramFacts,
    hir: perry_hir::Module,
}

fn solve(sabotage: Option<&str>) -> Solved {
    let module = perry_parser::parse_typescript(SRC, "t.ts").expect("parses");
    let hir = perry_hir::lower_module(&module, "t", "t.ts").expect("lowers");
    let cfg = FactsConfig {
        diagnostics: true,
        max_work: Some(50_000_000),
        sabotage: sabotage.map(str::to_string),
        ..FactsConfig::default()
    };
    let pf = crate::compute(&[("t.ts".to_string(), &hir)], &cfg);
    Solved { pf, hir }
}

/// SWC offsets are 1-based.
fn at(text: &str) -> u32 {
    assert_eq!(SRC.matches(text).count(), 1, "`{text}` names one site");
    SRC.find(text).expect("site in source") as u32 + 1
}

impl Solved {
    fn mb(&self) -> &crate::ModuleBindings {
        self.pf.bindings_for("t.ts").expect("module bindings")
    }

    /// The facts of parameter or top-level `let` `local` of function `func`.
    fn local(&self, func: &str, local: &str) -> &crate::BindingFacts {
        let f = self
            .hir
            .functions
            .iter()
            .find(|f| f.name == func)
            .unwrap_or_else(|| panic!("function {func}"));
        let id = f
            .params
            .iter()
            .find(|p| p.name == local)
            .map(|p| p.id)
            .or_else(|| {
                f.body.iter().find_map(|s| match s {
                    perry_hir::Stmt::Let { id, name, .. } if name == local => Some(*id),
                    _ => None,
                })
            })
            .unwrap_or_else(|| panic!("local {local} of {func}"));
        &self
            .mb()
            .locals
            .iter()
            .find(|(l, _)| *l == id)
            .unwrap_or_else(|| panic!("facts of local {local} of {func}"))
            .1
    }

    /// The dump line of the site of `kind` at `text`: its verdict or refusal.
    fn line(&self, kind: &str, text: &str) -> String {
        let head = format!("# quiet kind={kind} module=0 offset={} ", at(text));
        let lines: Vec<&String> = self
            .pf
            .rep_probe
            .iter()
            .filter(|l| l.starts_with(&head))
            .collect();
        assert!(!lines.is_empty(), "no `{head}` line");
        lines
            .iter()
            .map(|l| l[head.len()..].to_string())
            .collect::<Vec<_>>()
            .join(" | ")
    }

    fn element(&self, text: &str) -> bool {
        self.mb().quiet_elements.contains(&at(text))
    }
}

const SRC: &str = r#"
class P {
  xs: number[] = [1, 2, 3];
  plain: number = 0;
  set w(v: number) { this.xs.pop(); }
  get g(): number { this.xs.pop(); return 1; }
}
function plain(n: number): number {
  const arr: number[] = [n, 2, 3];
  const out: number[] = [0, 0, 0];
  let s = 0;
  for (let i = 0; i < arr.length; i++) { out[i] = arr[i] + 1; s += out[i]; }
  return s;
}
function accessor(n: number): number {
  const ys: number[] = [n, 0];
  Object.defineProperty(ys, "0", { get() { return 1; }, set(v: number) {} });
  let s = 0;
  for (let i = 0; i < 2; i++) { ys[i] = 1; s += i; }
  return s;
}
function setter(q: any): void { q["w"] = 1; }
function objectKey(m: number[], k: any): number { return m[k]; }
function fields(p: P): number { const a = p.plain; const b = p.g; return a + b; }
function text(str: string, n: number): string { return str[n]; }
setter(new P());
console.log(plain(1), accessor(1), objectKey([4, 5], { toString() { return "0"; } }),
  fields(new P()), text("ab", 1));
"#;

#[test]
fn an_element_site_of_plain_arrays_under_a_number_key_is_quiet() {
    let s = solve(None);
    // The precondition: both receivers are Array locals, so the fact was
    // decided on them and not withheld for want of a representation.
    assert!(matches!(s.local("plain", "arr").rep, Rep::Arr(_)));
    assert!(matches!(s.local("plain", "out").rep, Rep::Arr(_)));
    assert!(s.element("arr[i] + 1"), "{}", s.line("element", "arr[i] + 1"));
    assert!(s.element("out[i] = "), "{}", s.line("element", "out[i] = "));
    // The value read from an Array of Numbers is a primitive.
    assert!(s.mb().primitive_elements.contains(&at("arr[i] + 1")));
}

#[test]
fn an_array_with_an_accessor_defined_on_it_is_not_quiet() {
    let s = solve(None);
    // The receiver holds an Array, and the refusal names the definition.
    assert!(s.local("accessor", "ys").other);
    assert!(!s.element("ys[i] = 1"));
    assert_eq!(s.line("element", "ys[i] = 1"), "refused=receiver:defined-key");
}

#[test]
fn a_store_into_an_object_that_may_have_a_setter_is_not_quiet() {
    let s = solve(None);
    // `q` holds a `P`, whose `w` is a setter.
    assert!(!s.local("setter", "q").class_ids.is_empty());
    assert!(!s.element("q[\"w\"] = 1"));
    assert_eq!(
        s.line("element", "q[\"w\"] = 1"),
        "refused=receiver:not-array"
    );
}

#[test]
fn a_key_that_may_be_an_object_is_not_quiet() {
    let s = solve(None);
    // The receiver alone would be quiet: it is an Array local.
    assert!(matches!(s.local("objectKey", "m").rep, Rep::Arr(_)));
    assert!(s.local("objectKey", "k").other);
    assert!(!s.element("m[k]"));
    assert_eq!(s.line("element", "m[k]"), "refused=key:not-number");
}

#[test]
fn a_data_field_read_is_quiet_and_a_getter_read_is_not() {
    let s = solve(None);
    assert!(!s.local("fields", "p").class_ids.is_empty());
    let read = |text: &str, key: &str| s.mb().quiet_reads.contains(&(at(text), key.to_string()));
    assert!(read("p.plain", "plain"), "{}", s.line("read", "p.plain"));
    assert!(!read("p.g;", "g"), "{}", s.line("read", "p.g;"));
    // The field holds Numbers only: a primitive value.
    assert!(s
        .mb()
        .primitive_reads
        .contains(&(at("p.plain"), "plain".to_string())));
}

#[test]
fn a_string_index_is_quiet() {
    let s = solve(None);
    assert_eq!(s.local("text", "str").prims, crate::model::prim::STR);
    assert!(s.element("str[n]"), "{}", s.line("element", "str[n]"));
}

#[test]
fn the_negative_control_claims_every_site() {
    let s = solve(Some("quiet"));
    assert!(s.element("q[\"w\"] = 1"));
    assert!(s.element("ys[i] = 1"));
    assert!(s.element("m[k]"));
    assert!(s
        .mb()
        .quiet_reads
        .contains(&(at("p.g;"), "g".to_string())));
}
