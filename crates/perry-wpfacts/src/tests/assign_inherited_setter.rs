//! `Object.assign` [[Set]]s every own enumerable key of each source on the
//! target (ECMA-262 20.1.2.1 step 3.a.iii.2.b). A computed source key with a
//! known shape (`s["s" + i] = v`) is stored apart from the any-key field;
//! the copy must still meet every inherited setter that shape can name, or
//! the setter's body is "known inactive" and traps when the runtime calls it.

use crate::FactsConfig;

/// Whether the body of function `hit` is active in `src`.
fn hit_runs(src: &str) -> bool {
    let src = format!("function hit(v: number) {{ console.log('set', v); }}\n{src}");
    let module = perry_parser::parse_typescript(&src, "t.ts").expect("parses");
    let mut hir = perry_hir::lower_module(&module, "t", "t.ts").expect("lowers");
    perry_hir::monomorphize_module(&mut hir);
    let cfg = FactsConfig {
        max_work: Some(50_000_000),
        ..FactsConfig::default()
    };
    let pf = crate::compute(&[("t.ts".to_string(), &hir)], &cfg);
    assert!(pf.body_reach.complete, "activated");
    pf.body_reach
        .bodies
        .iter()
        .any(|(_, i, _)| hir.functions.iter().any(|f| f.id == *i && f.name == "hit"))
}

const LOOP_SOURCE: &str = "const s: any = {}; for (let i = 0; i < 40; i++) s['s' + i] = i;\n";

#[test]
fn a_shaped_source_key_runs_the_inherited_setter_it_can_name() {
    for (what, target) in [
        (
            "literal prototype",
            "const o: any = Object.create({ set s20(v: number) { hit(v); } });\n",
        ),
        (
            "class instance",
            "class W { set s20(v: number) { hit(v); } }\nconst o: any = new W();\n",
        ),
    ] {
        for (copy, call) in [
            ("assign", "Object.assign(o, s);\n"),
            ("assign of a spread", "Object.assign(o, { ...s });\n"),
        ] {
            let src = format!("{target}{LOOP_SOURCE}{call}");
            assert!(hit_runs(&src), "{what}, {copy}:\n{src}");
        }
    }
}

#[test]
fn a_source_key_stored_after_the_copy_runs_the_setter_too() {
    let src = "class W { set s20(v: number) { hit(v); } }\n\
               const o: any = new W();\n\
               const s: any = {};\n\
               function copy() { Object.assign(o, s); }\n\
               for (let i = 0; i < 40; i++) s['s' + i] = i;\n\
               copy();\n";
    assert!(hit_runs(src), "{src}");
}

/// Control: a source with only named keys meets only the setters it names.
#[test]
fn a_named_source_key_leaves_another_setter_inactive() {
    let src = "class W { set t20(v: number) { hit(v); } }\n\
               const o: any = new W();\n\
               Object.assign(o, { s20: 1 });\n";
    assert!(!hit_runs(src), "{src}");
}
