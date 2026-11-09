//! PR-C1 (DESIGN-p4l1 §2.8-2.9): an element read's past-the-end
//! `undefined` is a solve-time constraint (`Cons::ElemUndef`), and a for-of
//! walk's element read has a site key.

use crate::FactsConfig;

struct Read {
    /// `number_element_reads`: `Some(may be undefined)`.
    number: Option<bool>,
    /// `element_floors` (check mode).
    floor: Option<u32>,
}

fn read_at(src: &str, offset: u32, cfg: &FactsConfig) -> Read {
    let module = perry_parser::parse_typescript(src, "t.ts").expect("parses");
    let hir = perry_hir::lower_module(&module, "t", "t.ts").expect("lowers");
    let pf = crate::compute(&[("t.ts".to_string(), &hir)], cfg);
    let mb = pf.bindings_for("t.ts").expect("module bindings");
    Read {
        number: mb
            .number_element_reads
            .iter()
            .find(|(o, _)| *o == offset)
            .map(|(_, u)| *u),
        floor: mb
            .element_floors
            .iter()
            .find(|(o, _)| *o == offset)
            .map(|(_, f)| *f),
    }
}

/// The read whose object starts at `at` (SWC offsets are 1-based).
fn read(src: &str, at: &str, cfg: &FactsConfig) -> Read {
    let offset = src.find(at).expect("read in source") as u32 + 1;
    read_at(src, offset, cfg)
}

fn check() -> FactsConfig {
    FactsConfig {
        verify_facts: true,
        ..Default::default()
    }
}

/// A function over a 128-element literal (a short constant loop would be
/// unrolled to literal keys); `cond` is the loop test, `extra` follows it.
fn literal_loop(cond: &str, extra: &str) -> String {
    let elems = (0..128)
        .map(|i| i.to_string())
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "function sum(): number {{\n\
           const a = [{elems}];\n\
           let s = 0;\n\
           for (let i = 0; {cond}; i++) {{ const x = a[i]; s += x; }}\n\
           {extra}\n\
           return s;\n\
         }}\n\
         console.log(sum());"
    )
}

#[test]
fn a_read_below_the_floor_has_no_undefined() {
    let src = literal_loop("i < a.length", "");
    let r = read(&src, "a[i]", &check());
    assert_eq!(r.number, Some(false), "a Number element, never undefined");
    assert_eq!(r.floor, Some(128));
}

#[test]
fn a_key_that_reaches_the_length_keeps_undefined() {
    let src = literal_loop("i <= a.length", "");
    let r = read(&src, "a[i]", &check());
    assert_eq!(r.number, Some(true));
    assert_eq!(r.floor, None);
}

/// A function over a 127-element literal read under `i & 127`: the key is
/// `[0, 127]`, one past the floor.
fn masked_one_past() -> String {
    let elems = (0..127)
        .map(|i| i.to_string())
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "function sum(): number {{\n\
           const a = [{elems}];\n\
           let s = 0;\n\
           for (let i = 0; i < 1000; i++) {{ const x = a[i & 127]; s += x === undefined ? 1 : x; }}\n\
           return s;\n\
         }}\n\
         console.log(sum());"
    )
}

#[test]
fn a_masked_key_one_past_the_floor_keeps_undefined() {
    let src = masked_one_past();
    let r = read(&src, "a[i & 127]", &check());
    assert_eq!(r.number, Some(true));
    assert_eq!(r.floor, None);
}

#[test]
fn the_negative_control_admits_a_key_at_the_length() {
    let src = masked_one_past();
    let cfg = FactsConfig {
        sabotage: Some("arrundef".into()),
        ..check()
    };
    let r = read(&src, "a[i & 127]", &cfg);
    assert_eq!(r.number, Some(false));
    assert_eq!(r.floor, Some(127));
}

#[test]
fn the_switch_restores_the_unconditional_undefined() {
    let src = literal_loop("i < a.length", "");
    let cfg = FactsConfig {
        no_elem_undef: true,
        ..check()
    };
    let r = read(&src, "a[i]", &cfg);
    assert_eq!(r.number, Some(true));
    assert_eq!(r.floor, None);
}

#[test]
fn a_length_that_moves_later_adds_undefined() {
    // The shrink follows the read in the source: the length node goes ⊤
    // after the rule first ran, and the rule watches it.
    let src = literal_loop("i < 128", "a.length = 1;");
    let r = read(&src, "a[i]", &check());
    assert_eq!(r.number, Some(true));
    assert_eq!(r.floor, None);
}

#[test]
fn a_stored_undefined_reaches_the_read() {
    let src = literal_loop(
        "i < a.length",
        "(a as (number | undefined)[])[3] = undefined;",
    );
    let r = read(&src, "a[i]", &check());
    assert_eq!(r.number, Some(true), "the element itself may be undefined");
}

#[test]
fn a_hole_reaches_the_read() {
    let src = "function f(): number {\n\
                 const a: number[] = [];\n\
                 for (let j = 0; j < 128; j++) a.push(j);\n\
                 let s = 0;\n\
                 for (let i = 0; i < 64; i++) { const x = a[i]; s += x; }\n\
                 return s;\n\
               }\n\
               console.log(f());";
    let r = read(src, "a[i]", &check());
    assert_eq!(r.number, Some(true), "a pushed array's length is ⊤");
    assert_eq!(r.floor, None);
}

#[test]
fn a_for_of_walk_read_has_a_site_key() {
    // The walk's element read is keyed by the loop binding's span start.
    let src = "const K = [-33, -31, -18, -14, 14, 18, 31, 33];\n\
               function f(square: number): number {\n\
                 let n = 0;\n\
                 for (const delta of K) { const s = square + delta; if (s > 0) n += s; }\n\
                 return n;\n\
               }\n\
               console.log(f(40));";
    let r = read(src, "delta of K", &check());
    assert_eq!(r.number, Some(false), "a Number element of K");
    assert_eq!(r.floor, Some(8));
}
