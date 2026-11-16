//! ROOTS1: an escape made possible by an earlier escape names it. Class
//! `A` is passed to code outside the program (a true root: no
//! earlier escape made it). Unknown code may then call `A.prototype.go` on
//! any receiver: in that entry run `this` is ⊤, so `this.use(b)` hands `b`
//! (a `B`) to unknown code. `B`'s escape is a consequence of `A`'s: its
//! escape-first record's cause chain must lead to `A`'s escape, which is
//! itself cause-less (its root is external).

use super::*;

const SRC: &str = r#"
declare function external(x: unknown): void;
class B { v = 1; }
class A {
    go() { const b = new B(); this.use(b); return b.v; }
    use(x: any) { return x; }
}
external(A);
"#;

fn field<'a>(l: &'a str, k: &str) -> &'a str {
    let pat = format!("\"{k}\":");
    let i = l.find(&pat).map(|i| i + pat.len()).unwrap_or(l.len());
    let rest = &l[i..];
    let mut depth = 0;
    for (j, ch) in rest.char_indices() {
        match ch {
            '{' | '[' => depth += 1,
            '}' | ']' if depth > 0 => depth -= 1,
            ',' | '}' if depth == 0 => return &rest[..j],
            _ => {}
        }
    }
    rest
}

#[test]
fn an_escape_inside_an_entry_run_names_the_escape_that_made_the_run() {
    let pf = facts_with(
        SRC,
        FactsConfig {
            diagnostics: true,
            ..Default::default()
        },
    );
    let lines: Vec<&String> = pf.diagnostics.iter().flat_map(|d| d.1.iter()).collect();
    let tok_name = |id: &str| -> String {
        let needle = format!("\"record\":\"token\",\"id\":{id},");
        lines
            .iter()
            .find(|l| l.contains(&needle))
            .map(|l| field(l, "name").trim_matches('"').to_string())
            .unwrap_or_default()
    };
    let first = |id: &str| -> Option<&String> {
        let needle = format!("\"record\":\"escape-first\",\"token\":{id},");
        lines.iter().copied().find(|l| l.contains(&needle))
    };
    let firsts: Vec<&String> = lines
        .iter()
        .copied()
        .filter(|l| l.contains("\"record\":\"escape-first\""))
        .collect();
    let dump = firsts
        .iter()
        .map(|l| format!("{} {l}", tok_name(field(l, "token"))))
        .collect::<Vec<_>>()
        .join("\n");
    // `b`: the escaped `B` instance.
    let b = firsts
        .iter()
        .find(|l| {
            let n = tok_name(field(l, "token"));
            n.contains('B') && !n.contains('A') && !n.contains("class") && !n.contains("proto")
        })
        .unwrap_or_else(|| {
            let mut kinds = std::collections::BTreeMap::new();
            for l in &lines {
                *kinds.entry(field(l, "record").to_string()).or_insert(0) += 1;
            }
            let toks: Vec<&&String> = lines.iter().filter(|l| l.contains("\"record\":\"token\"")).collect();
            panic!("B never escaped:\n{dump}\n{kinds:?}\n{toks:#?}")
        });
    // Follow the cause chain to its end.
    let mut chain = vec![tok_name(field(b, "token"))];
    let mut at: &String = b;
    for _ in 0..32 {
        let c = field(at, "cause");
        let Some(t) = c.strip_prefix("{\"token\":").map(|s| s.trim_end_matches('}')) else {
            break;
        };
        chain.push(tok_name(t));
        match first(t) {
            Some(n) => at = n,
            None => break,
        }
    }
    assert!(
        chain.len() > 1 && chain[1..].iter().any(|n| n.contains('A')),
        "B's escape does not name A's: chain {chain:?}\n{dump}"
    );
    // The chain ends at a root: an escape no earlier escape made (no `via`).
    assert_eq!(field(at, "via"), "null", "root {at}\n{dump}");
    assert!(
        lines.iter().any(|l| l.contains("\"record\":\"entry-run\"")),
        "no entry-run record\n{dump}"
    );
}

/// ROOTS2: `HOLDER.prototype` is passed to code outside the program (the
/// root). A field of it read later holds ⊤ ("field of escaped prototype")
/// and is called with argument `r`: the `VICTIM` instance escapes because
/// the callee is unknown. `r` also holds `v.f`, which holds ⊤ once the
/// instance escaped (a field of its own): that later ⊤ is not the cause.
/// The instance's escape must name the prototype's.
const SRC2: &str = r#"
declare function external(x: unknown): void;
class HOLDER { }
class VICTIM { f: any = null; }
external(HOLDER.prototype);
const v = new VICTIM();
const r: any = Math.random() < 0.5 ? v : v.f;
const g = (HOLDER.prototype as any).k;
g(r);
"#;

#[test]
fn a_top_seeded_into_a_field_of_an_escaped_prototype_names_the_prototype() {
    let pf = facts_with(
        SRC2,
        FactsConfig {
            diagnostics: true,
            ..Default::default()
        },
    );
    let lines: Vec<&String> = pf.diagnostics.iter().flat_map(|d| d.1.iter()).collect();
    let tok_name = |id: &str| -> String {
        let needle = format!("\"record\":\"token\",\"id\":{id},");
        lines
            .iter()
            .find(|l| l.contains(&needle))
            .map(|l| field(l, "name").trim_matches('"').to_string())
            .unwrap_or_default()
    };
    let first = |id: &str| -> Option<&String> {
        let needle = format!("\"record\":\"escape-first\",\"token\":{id},");
        lines.iter().copied().find(|l| l.contains(&needle))
    };
    let firsts: Vec<&String> = lines
        .iter()
        .copied()
        .filter(|l| l.contains("\"record\":\"escape-first\""))
        .collect();
    let dump = firsts
        .iter()
        .map(|l| format!("{} {l}", tok_name(field(l, "token"))))
        .collect::<Vec<_>>()
        .join("\n");
    // Each `VICTIM` instance (the program's and unknown code's) escapes
    // because the callee `g` is unknown, and `g` holds ⊤ only as a field
    // of the escaped `HOLDER.prototype`: that escape is the cause, the
    // first hop, not a later ⊤ that `r` holds.
    let victims: Vec<&&String> = firsts
        .iter()
        .filter(|l| tok_name(field(l, "token")).starts_with("obj:") && tok_name(field(l, "token")).contains("VICTIM"))
        .collect();
    assert!(!victims.is_empty(), "the VICTIM instance never escaped:\n{dump}");
    for v in victims {
        let via = tok_name(field(v, "via"));
        assert!(
            via.contains("HOLDER") && via.starts_with("proto:"),
            "a VICTIM instance's escape names {via:?}, not HOLDER.prototype's: {v}\n{dump}"
        );
        assert!(first(field(v, "via")).is_some(), "no escape-first for the cause\n{dump}");
    }
    // Every call an escape names has a call record.
    assert!(
        lines.iter().any(|l| l.contains("\"record\":\"call\"")),
        "no call record\n{dump}"
    );
}
