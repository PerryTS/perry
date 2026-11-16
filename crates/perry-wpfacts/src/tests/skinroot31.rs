//! SKINROOT31 D2: `Array.prototype.flat` / `flatMap` (ECMA-262 23.1.3.13.1
//! FlattenIntoArray) read an element that IsArray at its INDEX keys only
//! (`Get(source, ToString(k))`, k < length); any other element is added
//! itself. So nothing of an inner array's prototype chain (its methods) and
//! nothing of a non-array element (the members of a builtin function, the
//! chain of a string) enters the result: a later call through an element
//! stays the program body. KEEP: a program function given a prototype by
//! `Object.setPrototypeOf` reads the inherited member (k1); `Object.assign`
//! copies the own enumerable method of a program object (k2).
use super::*;

const OBJ: &str = "const o = { m() { return 1; } };\n";

fn one(src: &str) -> ProgramFacts {
    let path = "t.ts".to_string();
    let module = perry_parser::parse_typescript(src, &path).expect("parses");
    let hir = perry_hir::lower_module(&module, "t", &path).expect("lowers");
    crate::compute(&[(path, &hir)], &FactsConfig::default())
}

/// The `m` call site: receivers, targets, top.
fn m_site(src: &str) -> (Vec<String>, Vec<String>, bool) {
    let pf = one(&format!("{OBJ}{src}"));
    let s = pf.sites.iter().find(|s| s.key == "m" && s.is_call).expect("a call site of m");
    (s.receivers.clone(), s.targets.clone(), s.top)
}

/// The call reaches `o.m` and only through `o`: no builtin member of an
/// inner array or of a non-array element became an element.
fn only_o(body: &str) {
    let (r, t, top) = m_site(body);
    let bad: Vec<_> = r.iter().filter(|x| x.starts_with("builtin")).collect();
    assert!(!top && bad.is_empty() && r.iter().any(|x| x.contains("AnonShape")), "{body}: builtin receivers {bad:?} (recv={r:?} targets={t:?} top={top})");
}

/// KEEP: the receiver stays the program object (no ⊤); the verdict oracle
/// refuses these shapes for unrelated reasons, so this guards regressions.
fn reaches(body: &str) {
    let (r, t, top) = m_site(body);
    assert!(!top && !r.is_empty(), "{body}: recv={r:?} targets={t:?} top={top}");
}

#[test]
fn d2_flat_reads_inner_array_indices_only() {
    only_o("const a = [[o]].flat();\na[0].m();\n")
}
#[test]
fn d2_flat_map_reads_returned_array_indices_only() {
    only_o("const a = [1].flatMap(() => [o]);\na[0].m();\n")
}
#[test]
fn d2_flat_of_a_string_element_adds_it_alone() {
    only_o("const a: any[] = [\"ab\", [o]].flat();\na[1].m();\n")
}
#[test]
fn k1_set_prototype_of_program_function_is_seen() {
    reaches("function f() {}\nObject.setPrototypeOf(f, o);\n(f as any).m();\n")
}
#[test]
fn k2_assign_copies_own_enumerable_method() {
    reaches("const t: any = {};\nObject.assign(t, o);\nt.m();\n")
}
