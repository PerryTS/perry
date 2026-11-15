//! `int32_element_reads` (`canonical_numbers`): the Number element reads
//! whose value node holds only int32s at every copy.

use crate::FactsConfig;

/// Is the read whose object starts at `at` an int32 read (SWC offsets are
/// 1-based)?
fn int32_read(src: &str, at: &str, cfg: &FactsConfig) -> bool {
    let module = perry_parser::parse_typescript(src, "t.ts").expect("parses");
    let hir = perry_hir::lower_module(&module, "t", "t.ts").expect("lowers");
    let pf = crate::compute(&[("t.ts".to_string(), &hir)], cfg);
    let mb = pf.bindings_for("t.ts").expect("module bindings");
    let offset = src.find(at).expect("read in source") as u32 + 1;
    mb.int32_element_reads.contains(&offset)
}

const INTS: &str = r#"
const t: number[] = [];
let seed = 12345;
for (let i = 0; i < 64; i++) { seed = (seed * 1103515245 + 12345) | 0; t.push(seed); }
let h = 0;
for (let i = 0; i < 80; i++) h ^= t[i] ?? 0;
console.log(h);
"#;

#[test]
fn a_table_of_int32s_is_read_as_int32s() {
    assert!(int32_read(INTS, "t[i] ??", &FactsConfig::default()));
}

#[test]
fn a_fraction_or_a_wide_value_in_the_table_is_no_int32_read() {
    for extra in [
        "t.push(1.5);",
        "t.push(4294967301);",
        "t.push(-0);",
        "(t as any)[-1] = 0.5;",
    ] {
        let src = INTS.replace("let h = 0;", &format!("{extra}\nlet h = 0;"));
        assert!(
            !int32_read(&src, "t[i] ??", &FactsConfig::default()),
            "{extra}"
        );
    }
}

#[test]
fn a_read_that_is_no_number_read_is_no_int32_read() {
    let src = INTS.replace("let h = 0;", "(t as any).push(\"x\");\nlet h = 0;");
    assert!(!int32_read(&src, "t[i] ??", &FactsConfig::default()));
}

#[test]
fn the_negative_control_claims_every_number_read() {
    let src = INTS.replace("let h = 0;", "t.push(1.5);\nlet h = 0;");
    let cfg = FactsConfig {
        sabotage: Some("int32read".to_string()),
        ..Default::default()
    };
    assert!(int32_read(&src, "t[i] ??", &cfg));
}

#[test]
fn the_probe_names_each_int32_read() {
    let module = perry_parser::parse_typescript(INTS, "t.ts").expect("parses");
    let hir = perry_hir::lower_module(&module, "t", "t.ts").expect("lowers");
    let cfg = FactsConfig {
        diagnostics: true,
        ..Default::default()
    };
    let pf = crate::compute(&[("t.ts".to_string(), &hir)], &cfg);
    let lines: Vec<_> = pf
        .rep_probe
        .iter()
        .filter(|l| l.starts_with("# int32_read "))
        .collect();
    assert_eq!(lines.len(), 1, "{lines:#?}");
    assert!(lines[0].ends_with("undef=1"), "{lines:#?}");
}
