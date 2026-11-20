//! The length floor (`solve::arr_len`, DESIGN-p4l1 §2.5): an Array's
//! length interval keeps its birth `lo` while growth raises only `hi`. Each
//! test reads `a.length` into a binding and checks its interval, one test
//! per row of the transfer table, plus the floor switched off.

use crate::{ElementRead, FactsConfig};
use std::collections::HashMap;

/// `name -> int_range` of every `let`/`const` of `t.ts` under `cfg`.
fn ranges_with(src: &str, cfg: &FactsConfig) -> HashMap<String, Option<(i64, i64)>> {
    let module = perry_parser::parse_typescript(src, "t.ts").expect("parses");
    let hir = perry_hir::lower_module(&module, "t", "t.ts").expect("lowers");
    let pf = crate::compute(&[("t.ts".to_string(), &hir)], cfg);
    let mb = pf.bindings_for("t.ts").expect("module bindings");
    let text = format!("{hir:?}");
    let mut names: HashMap<u32, String> = HashMap::new();
    let mut rest = text.as_str();
    while let Some(i) = rest.find("Let { id: ") {
        rest = &rest[i + "Let { id: ".len()..];
        let num: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        if let (Ok(id), Some(j)) = (num.parse::<u32>(), rest.find("name: \"")) {
            let name: String = rest[j + 7..].chars().take_while(|c| *c != '"').collect();
            names.entry(id).or_insert(name);
        }
    }
    let mut out = HashMap::new();
    for (id, b) in &mb.locals {
        if let Some(name) = names.get(id) {
            out.entry(name.clone()).or_insert(b.int_range);
        }
    }
    out
}

/// The interval of binding `n` (an `a.length` read) under the default
/// configuration (the floor on).
fn len_of(src: &str) -> Option<(i64, i64)> {
    len_with(src, &Default::default())
}

fn len_with(src: &str, cfg: &FactsConfig) -> Option<(i64, i64)> {
    let r = ranges_with(src, cfg);
    *r.get("n")
        .unwrap_or_else(|| panic!("no binding n in {r:?}"))
}

fn unbounded(r: Option<(i64, i64)>) -> bool {
    use crate::model::range::BIG;
    r.is_none_or(|(lo, hi)| lo <= -BIG || hi >= BIG)
}

const U32_MAX: i64 = u32::MAX as i64;

fn reads(src: &str, at: &str) -> Option<ElementRead> {
    let module = perry_parser::parse_typescript(src, "t.ts").expect("parses");
    let hir = perry_hir::lower_module(&module, "t", "t.ts").expect("lowers");
    let pf = crate::compute(&[("t.ts".to_string(), &hir)], &Default::default());
    let mb = pf.bindings_for("t.ts").expect("module bindings");
    let offset = src.find(at).expect("read in source") as u32 + 1;
    mb.element_reads
        .iter()
        .find(|(o, _)| *o == offset)
        .map(|(_, r)| *r)
}

/// `put(k)` stores into `a`; `n` reads its length after every call.
fn store_program(birth: &str, calls: &str) -> String {
    format!(
        "const a = {birth};\n\
         function put(k: number): void {{ a[k] = 1; }}\n\
         {calls}\n\
         const n = a.length;\n\
         console.log(n);"
    )
}

#[test]
fn a_literal_is_born_with_its_element_count() {
    assert_eq!(len_of(&store_program("[1, 2, 3, 4]", "")), Some((4, 4)));
}

#[test]
fn new_array_is_born_with_its_length_only_when_densely_backed() {
    let max = perry_abi::MAX_FRESH_DENSE_ARRAY_LENGTH as i64;
    let p = |k: i64| store_program(&format!("new Array<number>({k})"), "");
    assert_eq!(len_of(&p(128)), Some((128, 128)));
    assert_eq!(len_of(&p(max)), Some((max, max)));
    // Past the dense bound the store holds no slots: no birth length.
    assert!(unbounded(len_of(&p(max + 1))));
    assert!(unbounded(len_of(&p(2_000_000))));
}

#[test]
fn an_index_store_raises_only_the_upper_bound() {
    let r = len_of(&store_program("[1, 2, 3, 4]", "put(2); put(9);"));
    let (lo, hi) = r.expect("bounded");
    assert_eq!(lo, 4, "the floor stays: {r:?}");
    assert!(hi >= 10, "covers a[9] = 1: {r:?}");
    assert!(hi <= U32_MAX, "{r:?}");
}

#[test]
fn an_index_store_inside_the_length_keeps_it() {
    let r = len_of(&store_program("[1, 2, 3, 4]", "put(0); put(3);"));
    assert_eq!(r, Some((4, 4)));
}

#[test]
fn a_negative_key_changes_nothing() {
    // A negative key is a named property in the array's separate bag.
    let r = len_of(&store_program("[1, 2, 3, 4]", "put(-1); put(-16);"));
    assert_eq!(r, Some((4, 4)));
    // A key interval reaching below 0 is judged by its upper end alone.
    let r = len_of(&store_program(
        "new Array<number>(192).fill(0)",
        "for (let i = -16; i <= 16; i++) put(i);",
    ));
    assert_eq!(r, Some((192, 192)));
}

#[test]
fn a_fractional_or_unbounded_key_makes_the_length_unknown() {
    assert!(unbounded(len_of(&store_program(
        "[1, 2, 3, 4]",
        "put(1.5);"
    ))));
    assert!(unbounded(len_of(&store_program(
        "[1, 2, 3, 4]",
        "put(Math.random() * 10);"
    ))));
}

#[test]
fn an_append_raises_the_upper_bound_to_the_largest_length() {
    // A rule runs once per token, not once per push: `hi + 1` would be a
    // false bound for this loop.
    let src = "const a: number[] = [1, 2];\n\
               for (let i = 0; i < 1664; i++) a.push(i);\n\
               const n = a.length;\n\
               console.log(n);";
    let r = len_of(src);
    assert_eq!(r, Some((2, U32_MAX)), "{r:?}");
}

#[test]
fn shrinking_events_make_the_length_unknown() {
    for event in [
        "a.pop();",
        "a.length = 1;",
        "a.splice(0, 2);",
        "a.shift();",
        "a.unshift(0);",
        "delete a[1];",
        "Object.defineProperty(a, 0, { value: 5 });",
        "Object.assign(a, [9, 9, 9, 9, 9]);",
    ] {
        let src = format!(
            "const a = [1, 2, 3, 4];\n\
             if (Math.random() < 2) {{ {event} }}\n\
             const n = a.length;\n\
             console.log(n);"
        );
        assert!(unbounded(len_of(&src)), "{event}: {:?}", len_of(&src));
    }
}

#[test]
fn a_permutation_keeps_a_point_length() {
    for m in [
        "a.reverse();",
        "a.copyWithin(0, 2);",
        "a.sort();",
        "a.fill(0);",
    ] {
        let src = format!(
            "const a = [4, 3, 2, 1];\n\
             {m}\n\
             const n = a.length;\n\
             console.log(n);"
        );
        assert_eq!(len_of(&src), Some((4, 4)), "{m}");
    }
}

#[test]
fn a_permutation_of_a_grown_array_makes_its_length_unknown() {
    // Past the floor the array may hold holes, which `reverse` and
    // `copyWithin` may move below it (Q1). The growth may come after the
    // permutation in the program: the rule watches the length.
    for m in ["a.reverse();", "a.copyWithin(0, 2);"] {
        let src = format!(
            "const a = [4, 3, 2, 1];\n\
             function put(k: number): void {{ a[k] = 1; }}\n\
             {m}\n\
             put(9);\n\
             const n = a.length;\n\
             console.log(n);"
        );
        assert!(unbounded(len_of(&src)), "{m}: {:?}", len_of(&src));
    }
    // `sort` keeps it: it publishes holes only past every value.
    let src = "const a = [4, 3, 2, 1];\n\
               function put(k: number): void { a[k] = 1; }\n\
               a.sort();\n\
               put(9);\n\
               const n = a.length;\n\
               console.log(n);";
    assert_eq!(len_of(src).map(|r| r.0), Some(4));
}

#[test]
fn the_floor_off_keeps_main_s_rules() {
    let off = FactsConfig {
        no_array_floor: true,
        ..Default::default()
    };
    let p = store_program("[1, 2, 3, 4]", "put(2); put(9);");
    assert!(unbounded(len_with(&p, &off)));
    let p = store_program("[1, 2, 3, 4]", "put(-1);");
    assert!(unbounded(len_with(&p, &off)));
    let p = store_program("[1, 2, 3, 4]", "put(0); put(3);");
    assert_eq!(len_with(&p, &off), Some((4, 4)));
    let src = "const a: number[] = [1, 2];\n\
               a.push(3);\n\
               const n = a.length;\n\
               console.log(n);";
    assert!(unbounded(len_with(src, &off)));
}

#[test]
fn a_negative_key_keeps_a_proven_element_read() {
    // `killers`' shape: keys `[-16, 16]` on a 192-element array keep its
    // constant length, so its reads are proven (with their bound, since
    // the key leaves `[0, 192)`).
    let src = "const k = new Array<number>(192).fill(0);\n\
               function put(i: number): void { k[i] = 1; }\n\
               function at(i: number): number { return k[i] ?? 0; }\n\
               let s = 0;\n\
               for (let i = -16; i <= 16; i++) { put(i); s += at(i); }\n\
               console.log(s);";
    assert_eq!(
        reads(src, "k[i] ??"),
        Some(ElementRead {
            len: 192,
            key_in_bounds: false
        })
    );
}

#[test]
fn a_grown_array_is_not_a_proven_element_read() {
    // `lo < hi`: the storage may be reallocated, so no constant length.
    let src = "const a = [1, 2, 3, 4];\n\
               function put(k: number): void { a[k] = 1; }\n\
               put(9);\n\
               console.log(a[1]);";
    assert_eq!(reads(src, "a[1]"), None);
}
