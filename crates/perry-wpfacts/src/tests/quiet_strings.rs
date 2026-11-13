//! Quiet sites over Strings (`quiet_sites`, `PERRY_FACTS_QUIET_STRINGS`): a
//! String's own `length`, and the code unit an element read of a String
//! gives, run no program code and are primitives.

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

/// The dump line of the site of `kind` at `text` (key `key` for a read).
fn line(pf: &crate::ProgramFacts, kind: &str, text: &str, key: &str) -> String {
    let key = if key.is_empty() {
        String::new()
    } else {
        format!("key={key} ")
    };
    let head = format!("# quiet kind={kind} module=0 offset={} {key}", at(text));
    let lines: Vec<&String> = pf
        .rep_probe
        .iter()
        .filter(|l| l.starts_with(&head))
        .collect();
    assert_eq!(lines.len(), 1, "one `{head}` line: {lines:?}");
    lines[0][head.len()..].to_string()
}

fn quiet_read(pf: &crate::ProgramFacts, text: &str) -> bool {
    mb(pf)
        .quiet_reads
        .contains(&(at(text), "length".to_string()))
}

fn primitive_read(pf: &crate::ProgramFacts, text: &str) -> bool {
    mb(pf)
        .primitive_reads
        .contains(&(at(text), "length".to_string()))
}

const SRC: &str = r#"
class L { get length(): number { return 3; } }
function rev(n: number): string {
  const s = String(n) + "x";
  let r = "";
  for (let m = 0; m < s.length; m++) { r = s[m] + r; }
  return r;
}
function lens(parts: string[]): number {
  let t = 0;
  for (let i = 0; i < parts.length; i++) { t += parts[i].length; }
  return t;
}
function mixed(q: any): number { return q.length; }
function boxed(b: any, i: number): any { return b[i]; }
console.log(rev(12), lens(["a", "bb"]), mixed("abc"), mixed(new L()),
  boxed(new String("ab"), 0), boxed("cd", 1));
"#;

#[test]
fn a_string_code_unit_read_gives_a_primitive() {
    let pf = solve(None);
    // The element site itself is quiet: a String receiver, a Number key.
    assert!(
        mb(&pf).quiet_elements.contains(&at("s[m]")),
        "{}",
        line(&pf, "element", "s[m]", "")
    );
    assert_eq!(
        line(&pf, "elemvalue", "s[m]", ""),
        "verdict=quiet by=string"
    );
    assert!(mb(&pf).primitive_elements.contains(&at("s[m]")));
}

#[test]
fn a_string_length_read_is_quiet_with_a_primitive_value() {
    let pf = solve(None);
    // `parts[i]` holds Strings alone: the element value is a primitive by
    // the solve already, and its `length` is the String's own (quiet by the
    // site's own verdict here, or by the String rule).
    assert!(mb(&pf).primitive_elements.contains(&at("parts[i].length")));
    assert!(
        quiet_read(&pf, "parts[i].length"),
        "{}",
        line(&pf, "read", "parts[i].length", "length")
    );
    assert!(primitive_read(&pf, "parts[i].length"));
}

#[test]
fn a_length_read_that_may_reach_a_getter_is_not_quiet() {
    let pf = solve(None);
    // `q` holds a String and an `L`, whose `length` is a getter.
    assert!(!quiet_read(&pf, "q.length"));
    assert!(line(&pf, "read", "q.length", "length").starts_with("refused="));
    // An element read of a String object (no primitive) gives no String
    // value by this rule.
    assert!(!mb(&pf).primitive_elements.contains(&at("b[i]")));
    assert!(line(&pf, "elemvalue", "b[i]", "").starts_with("refused="));
}

#[test]
fn the_string_negative_control_claims_every_length_read() {
    let pf = solve(Some("quiet-strings"));
    assert!(quiet_read(&pf, "q.length"));
    assert!(primitive_read(&pf, "q.length"));
    // The other sabotage claims stay off: an element site is not claimed.
    assert!(!mb(&pf).quiet_elements.contains(&at("b[i]")));
}

#[test]
fn the_probe_counts_the_string_rules() {
    let pf = solve(None);
    let probe = pf
        .rep_probe
        .iter()
        .find(|l| l.starts_with("# quiet strings "))
        .expect("the strings probe line");
    assert!(!probe.contains("reads=0"), "{probe}");
    assert!(!probe.contains("elemvalues=0"), "{probe}");
}
