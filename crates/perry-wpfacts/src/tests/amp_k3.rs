//! K3 (owner premises Phase B and receiver family, 2026-11-23): a callback
//! handed to a builtin or the host runs only at its contract's invoke
//! points. (A): an escape a builtin contract states runs no invoke of its
//! own; the arm binds the contract's invokes as program calls. A value
//! later handed to code that may write it gets that code's calls (⊤
//! arguments) once.
use super::*;

const DECL: &str = "declare function sink(x: any): void;\n";

fn one(src: &str) -> ProgramFacts {
    let path = "t.ts".to_string();
    let module = perry_parser::parse_typescript(src, &path).expect("parses");
    let hir = perry_hir::lower_module(&module, "t", &path).expect("lowers");
    let mut cfg = FactsConfig::default();
    cfg.max_work.get_or_insert(50_000_000);
    crate::compute(&[(path, &hir)], &cfg)
}

/// The verdict of the first `.key` site (read or call) after `marker`.
fn site_after(src: &str, marker: &str, key: &str, call: bool) -> String {
    let pf = one(src);
    let at = src.find(marker).expect("marker") as u32;
    let s = pf
        .sites
        .iter()
        .filter(|s| s.key == key && s.is_call == call && s.byte_offset >= at)
        .min_by_key(|s| s.byte_offset)
        .unwrap_or_else(|| panic!("no {key} site after {marker:?}"));
    let v = format!("{:?}", s.verdict);
    eprintln!("AMP-K3 {src:?}: {key}: {v}");
    v
}

const CB: &str = "function cb(e: any) { return /*A*/ e.n; }\n";

/// (A): the stream calls its write callback with no argument
/// (`schedule_write_callback`); nothing else calls it, so `e` is no ⊤.
#[test]
fn k3a_stream_callback_parameter_is_no_top() {
    let src = format!("{CB}process.stdout.write(\"x\", cb as any);\n");
    let v = site_after(&src, "/*A*/", "n", false);
    assert!(!v.starts_with("No("), "the contract callback's parameter is ⊤: {v}");
}

/// (A): a chunk that is no string is converted by ToString; its
/// `toString` runs with `this` = the chunk, not ⊤.
#[test]
fn k3a_stream_chunk_to_string_runs_on_the_chunk() {
    let src = "const o = { s: \"a\", toString() { return /*A*/ this.s; } };\nprocess.stdout.write(o as any);\n";
    let v = site_after(src, "/*A*/", "s", false);
    assert!(!v.starts_with("No("), "the chunk's toString ran on ⊤: {v}");
}

/// T5: an escape no contract states (fetch's `escapes(field(arg(1), body))`)
/// keeps ⊤ parameters for what it hands.
#[test]
fn k3_t5_undeclared_escape_keeps_top_params() {
    let src = "const b = { m(e: any) { return /*A*/ e.n; } };\nfetch(\"u\", { body: b } as any);\n";
    let v = site_after(src, "/*A*/", "n", false);
    assert!(v.starts_with("No("), "an undeclared escape's callback lost its ⊤ parameter: {v}");
}

/// T6: handed first by contract (stream write), then to unknown code: the
/// re-escape runs the writer part the contract escape skipped (⊤ params).
#[test]
fn k3_t6_contract_then_writer_runs_the_writer_part() {
    let src = format!("{DECL}{CB}process.stdout.write(\"x\", cb as any);\nsink(cb);\n");
    let v = site_after(&src, "/*A*/", "n", false);
    assert!(v.starts_with("No("), "contract-then-writer kept the contract view: {v}");
}

/// T6 control: the same writer escape alone gives ⊤ params.
#[test]
fn k3_t6_control_writer_alone() {
    let src = format!("{DECL}{CB}sink(cb);\n");
    let v = site_after(&src, "/*A*/", "n", false);
    assert!(v.starts_with("No("), "a writer escape kept the parameter: {v}");
}
