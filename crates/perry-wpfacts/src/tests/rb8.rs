//! RAYBOX8. An element read's NUM fact (`number_element_reads`) holds at
//! every summary-instance copy of its value node; a copy whose node holds
//! nothing (no run of that instance yields a value there) constrains
//! nothing, so it does not drop the read. A copy that holds a non-Number
//! still does.
use crate::FactsConfig;

fn number_at(src: &str, at: &str) -> Option<bool> {
    let module = perry_parser::parse_typescript(src, "t.ts").expect("parses");
    let hir = perry_hir::lower_module(&module, "t", "t.ts").expect("lowers");
    let cfg = FactsConfig {
        verify_facts: true,
        ..Default::default()
    };
    let pf = crate::compute(&[("t.ts".to_string(), &hir)], &cfg);
    let mb = pf.bindings_for("t.ts").expect("module bindings");
    let offset = src.find(at).expect("read in source") as u32 + 1;
    mb.number_element_reads
        .iter()
        .find(|(o, _)| *o == offset)
        .map(|(_, u)| *u)
}

/// `extra`: a further call of `apply`.
fn src(extra: &str) -> String {
    format!(
        "class M {{ elements: any; constructor() {{ this.elements = [1, 0, 0, 1]; }} }}
         class V {{ x: number; constructor() {{ this.x = Number(process.argv.length); }}
           apply(m: any): V {{ const e = m.elements; this.x = e[ 0 ] * this.x + e[ 3 ]; return this; }} }}
         function never(): any {{ throw new Error(String(Math.random())); }}
         const v = new V();
         v.apply(new M());
         {extra}
         console.log(v.x);"
    )
}

#[test]
fn rb8_live_calls_alone_prove_the_read() {
    assert_eq!(number_at(&src(""), "e[ 0 ]"), Some(false));
}

#[test]
fn rb8_a_call_whose_argument_holds_nothing_keeps_the_read_proven() {
    let s = src("try { new V().apply(never()); } catch (_) { console.log('t'); }");
    assert_eq!(number_at(&s, "e[ 0 ]"), Some(false));
    assert_eq!(number_at(&s, "e[ 3 ]"), Some(false));
}

#[test]
fn rb8_keep_a_live_string_element_drops_the_read() {
    let s = src("new V().apply({ elements: ['a', 'b', 'c', String(Math.random())] });");
    assert_eq!(number_at(&s, "e[ 0 ]"), None);
}

#[test]
fn rb8_keep_a_hole_keeps_undefined() {
    let s = src("new V().apply({ elements: [1, , 2] });");
    assert_eq!(number_at(&s, "e[ 3 ]"), Some(true));
}
