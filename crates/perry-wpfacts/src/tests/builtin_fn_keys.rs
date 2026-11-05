//! Rule R2 (STR1 design §3): a property read of a builtin function value is
//! ⊤ only under a key some store into a builtin function value may have
//! named (`Model::bf_written`, `bf_key_written`), not under every key. The
//! sites read a program object through a native function value's field
//! (rule R1: `f.cache` under a key no native declares), so they are proven
//! only while no store may have named `cache`. Twins: the store names
//! `cache`, a computed key, or comes after the read.
use super::*;

/// The read sites under `key` on the line holding `marker`.
fn at<'a>(pf: &'a ProgramFacts, src: &str, marker: &str, key: &str) -> Vec<&'a SiteFacts> {
    let m = src
        .find(marker)
        .unwrap_or_else(|| panic!("marker {marker}"));
    let start = src[..m].rfind('\n').map_or(0, |i| i + 1) as u32;
    let s: Vec<_> = (pf.sites.iter())
        .filter(|s| !s.is_call && s.key == key && (start..m as u32).contains(&s.byte_offset))
        .collect();
    assert!(!s.is_empty(), "no `{key}` site on the {marker} line");
    s
}

fn verdicts(pf: &ProgramFacts, src: &str, marker: &str, key: &str) -> String {
    let s = at(pf, src, marker, key);
    format!(
        "{:?}",
        s.iter()
            .map(|s| (&s.receivers, &s.verdict))
            .collect::<Vec<_>>()
    )
}

fn proven(pf: &ProgramFacts, src: &str, marker: &str, key: &str) -> bool {
    (at(pf, src, marker, key).iter()).all(|s| !matches!(s.verdict, Verdict::No(_)))
}

fn unproven(pf: &ProgramFacts, src: &str, marker: &str, key: &str) -> bool {
    (at(pf, src, marker, key).iter()).all(|s| matches!(s.verdict, Verdict::No(_)))
}

/// A program object kept on a native function value (an R1 field).
const BOX: &str = "class Box { n: number; constructor() { this.n = 1; } }\n\
    const f: any = [1, 2].map;\n\
    f.cache = new Box();\n";

/// What an array's computed read may be: an element or an `Array.prototype`
/// method (the anonymous native), the receiver of tsc's `node.jsDoc = v`.
const NODES: &str = "class Node { kind: number; constructor() { this.kind = 0; } }\n\
    const nodes: any = [new Node()];\n\
    const node = nodes[process.argv[2] ?? \"0\"];\n";

/// Site test 1: a declared key of one native (`Math.max.call`) replaced;
/// the read of another key on native function values (`cache`) keeps
/// the program's object. Main: every builtin function value is written,
/// `f.cache` is ⊤.
#[test]
fn a_declared_key_store_on_a_native_leaves_other_keys() {
    let src = format!(
        "{BOX}(Math.max as any).call = function () {{ return 0; }};\n\
         console.log(f.cache.n); // SITE-A\n"
    );
    let pf = facts(&src);
    assert!(
        proven(&pf, &src, "// SITE-A", "n"),
        "{}",
        verdicts(&pf, &src, "// SITE-A", "n")
    );
}

/// Site test 2 (tsc's shape, `node.jsDoc = v`): a store under a named key
/// into what may be an anonymous native writes that key only.
#[test]
fn a_named_store_into_a_maybe_native_leaves_other_keys() {
    let src = format!(
        "{BOX}{NODES}node.jsDoc = [1];\n\
         console.log(f.cache.n); // SITE-B\n"
    );
    let pf = facts(&src);
    assert!(
        proven(&pf, &src, "// SITE-B", "n"),
        "{}",
        verdicts(&pf, &src, "// SITE-B", "n")
    );
}

/// Twin: the store into what may be an anonymous native names `cache`
/// itself: the value went to the runtime, the read is ⊤.
#[test]
fn twin_the_stored_key_is_read() {
    let src = format!(
        "{BOX}{NODES}node.cache = [1];\n\
         console.log(f.cache.n); // SITE-C\n"
    );
    let pf = facts(&src);
    assert!(
        unproven(&pf, &src, "// SITE-C", "n"),
        "{}",
        verdicts(&pf, &src, "// SITE-C", "n")
    );
}

/// Twin: a computed key may name `cache`.
#[test]
fn twin_a_computed_key_store_may_name_every_key() {
    let src = format!(
        "{BOX}{NODES}node[process.argv[3] ?? \"k\"] = [1];\n\
         console.log(f.cache.n); // SITE-D\n"
    );
    let pf = facts(&src);
    assert!(
        unproven(&pf, &src, "// SITE-D", "n"),
        "{}",
        verdicts(&pf, &src, "// SITE-D", "n")
    );
}

/// Twin: the store naming `cache` comes after the read in program order,
/// in a later iteration and from a later call: the read made before turns
/// ⊤ once the key is written (`bf_reads` by key).
#[test]
fn twin_a_later_store_reaches_an_earlier_read() {
    let src = format!(
        "{BOX}{NODES}function later(x: any) {{ x.cache = [1]; }}\n\
         for (let i = 0; i < 2; i++) {{\n\
           console.log(f.cache.n); // SITE-E\n\
           if (i > 0) later(node);\n\
         }}\n"
    );
    let pf = facts(&src);
    assert!(
        unproven(&pf, &src, "// SITE-E", "n"),
        "{}",
        verdicts(&pf, &src, "// SITE-E", "n")
    );
}

/// Site test 3: an element store into what may be an anonymous native
/// writes its element (numeric) keys, never `cache`. (Its twin, a computed
/// read of a native, is unproven without any store: the read may yield a
/// `Function.prototype` member.)
#[test]
fn an_element_store_into_a_maybe_native_leaves_named_keys() {
    let src = format!(
        "{BOX}{NODES}node[0] = [1];\n\
         console.log(f.cache.n); // SITE-F\n"
    );
    let pf = facts(&src);
    assert!(
        proven(&pf, &src, "// SITE-F", "n"),
        "{}",
        verdicts(&pf, &src, "// SITE-F", "n")
    );
}
