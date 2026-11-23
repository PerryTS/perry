//! Rule EK with an array-rest destructuring elsewhere in the program
//! (PERFNOW15, browser.ts:323 + script.ts): `const [x0, ...rest] = xs`
//! runs the iterator protocol, which must not open the protocol arm of the
//! entries loop's `[k, v]` record onto the pair. When it did, `k` read
//! every element of the pair (its values among them) and the computed
//! global store wrote under any key.
use super::*;

fn facts_of(src: &str) -> ProgramFacts {
    let path = "main.ts".to_string();
    let module = perry_parser::parse_typescript(src, &path).expect("parses");
    let hir = perry_hir::lower_module(&module, "main", &path).expect("lowers");
    let list: Vec<(String, &perry_hir::Module)> = vec![(path, &hir)];
    let mut cfg = FactsConfig::default();
    cfg.max_work.get_or_insert(50_000_000);
    crate::compute(&list, &cfg)
}

fn c_kept(rest: &str) -> (bool, String) {
    let src = format!(
        "class A {{ a = 1; }}
        class B {{ b = 2; }}
        class C {{ c = 3; }}
        const g = globalThis as any;
        const classes = {{ A, B }};
        g.C = C;
        for (const [k, v] of Object.entries(classes)) g[k] = v;
        {rest}
        const c = new (globalThis as any).C();
        console.log(/*C*/ c.c);"
    );
    let pf = facts_of(&src);
    let at = src.find("/*C*/").expect("marker") as u32;
    let s = pf
        .sites
        .iter()
        .filter(|s| s.key == "c" && s.byte_offset >= at)
        .min_by_key(|s| s.byte_offset)
        .unwrap_or_else(|| panic!("site .c in\n{}", pf.dump_text()));
    let v = format!("{:?}", s.verdict);
    let kept = v.starts_with("T0Mono") || v.contains("Field") || v.contains("Slot");
    (kept, format!("{v}\n{}", pf.dump_text()))
}

/// Control: no rest destructuring.
#[test]
fn ek_rest_control() {
    let (ok, why) = c_kept("");
    assert!(ok, "the global C keeps its value: {why}");
}

/// RED on 1080f9b211: a rest destructuring of a split result.
#[test]
fn ek_rest_split() {
    let (ok, why) = c_kept("const xs = \"a b c\".split(\" \");\n        const [x0, ...rest] = xs;\n        console.log(x0, rest.join(\" \"));");
    assert!(ok, "the global C keeps its value: {why}");
}

/// RED on 1080f9b211: a rest destructuring of an array literal.
#[test]
fn ek_rest_literal() {
    let (ok, why) =
        c_kept("const [y0, ...more] = [\"a\", \"b\"];\n        console.log(y0, more.length);");
    assert!(ok, "the global C keeps its value: {why}");
}
