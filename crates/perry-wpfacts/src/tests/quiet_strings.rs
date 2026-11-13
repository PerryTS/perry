//! Quiet sites over Strings (`quiet_sites`, `PERRY_FACTS_QUIET_STRINGS`):
//! the code unit (or `undefined`) a quiet element read of a String gives is
//! a primitive.

use crate::FactsConfig;

fn solve(sabotage: Option<&str>) -> crate::ProgramFacts {
    let module = perry_parser::parse_typescript(SRC, "t.ts").expect("parses");
    let hir = perry_hir::lower_module(&module, "t", "t.ts").expect("lowers");
    let cfg = FactsConfig {
        diagnostics: true,
        max_work: Some(50_000_000),
        sabotage: sabotage.map(str::to_string),
        ..FactsConfig::default()
    };
    crate::compute(&[("t.ts".to_string(), &hir)], &cfg)
}

/// SWC offsets are 1-based.
fn at(text: &str) -> u32 {
    assert_eq!(SRC.matches(text).count(), 1, "`{text}` names one site");
    SRC.find(text).expect("site in source") as u32 + 1
}

fn mb(pf: &crate::ProgramFacts) -> &crate::ModuleBindings {
    pf.bindings_for("t.ts").expect("module bindings")
}

/// The dump line of the element site of `kind` at `text`.
fn line(pf: &crate::ProgramFacts, kind: &str, text: &str) -> String {
    let head = format!("# quiet kind={kind} module=0 offset={} ", at(text));
    let lines: Vec<&String> = pf
        .rep_probe
        .iter()
        .filter(|l| l.starts_with(&head))
        .collect();
    assert_eq!(lines.len(), 1, "one `{head}` line: {lines:?}");
    lines[0][head.len()..].to_string()
}

fn primitive_element(pf: &crate::ProgramFacts, text: &str) -> bool {
    mb(pf).primitive_elements.contains(&at(text))
}

const SRC: &str = r#"
class V { valueOf(): number { return 1; } }
function rev(n: number): string {
  const s = String(n) + "x";
  let r = "";
  for (let m = 0; m < s.length; m++) { r = s[m] + r; }
  return r;
}
function boxed(b: any, i: number): any { return b[i]; }
function objects(vs: V[], i: number): number { return 0 + vs[i]; }
console.log(rev(12), boxed(new String("ab"), 0), boxed("cd", 1), objects([new V()], 0));
"#;

#[test]
fn a_string_code_unit_read_gives_a_primitive() {
    let pf = solve(None);
    // The element site itself is quiet: a String receiver, a Number key.
    assert!(
        mb(&pf).quiet_elements.contains(&at("s[m]")),
        "{}",
        line(&pf, "element", "s[m]")
    );
    assert_eq!(line(&pf, "elemvalue", "s[m]"), "verdict=quiet by=string");
    assert!(primitive_element(&pf, "s[m]"));
}

#[test]
fn an_element_read_of_a_string_object_gives_no_string_value() {
    let pf = solve(None);
    // `b` holds a String object and a String: not Strings alone.
    assert!(!primitive_element(&pf, "b[i]"));
    assert!(line(&pf, "elemvalue", "b[i]").starts_with("refused="));
}

#[test]
fn the_string_negative_control_claims_every_quiet_element_value() {
    // Without the claim: an Array of objects, a quiet element site whose
    // value is an object (its `valueOf` may run).
    let pf = solve(None);
    assert!(mb(&pf).quiet_elements.contains(&at("vs[i]")));
    assert!(!primitive_element(&pf, "vs[i]"));
    let pf = solve(Some("quiet-strings"));
    assert!(primitive_element(&pf, "vs[i]"));
    // The other sabotage claims stay off: a site that is not quiet stays so.
    assert!(!mb(&pf).quiet_elements.contains(&at("b[i]")));
}

#[test]
fn the_probe_counts_the_string_rule() {
    let pf = solve(None);
    let probe = pf
        .rep_probe
        .iter()
        .find(|l| l.starts_with("# quiet strings "))
        .expect("the strings probe line");
    assert!(!probe.contains("elemvalues=0"), "{probe}");
}
