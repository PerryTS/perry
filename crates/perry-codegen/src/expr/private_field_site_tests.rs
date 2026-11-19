//! A compiled `this.#x` site's word hit sends a hole slot to the miss before
//! it reads or overwrites it: a class family's optional private field is
//! absent until its definition fills the slot.

fn compile(source: &str) -> String {
    let ast = perry_parser::parse_typescript(source, "pfield_site.ts").unwrap();
    let m = perry_hir::lower_module(&ast, "pfield_site", "pfield_site.ts").unwrap();
    String::from_utf8(
        crate::compile_module(
            &m,
            crate::CompileOptions {
                emit_ir_only: true,
                ..Default::default()
            },
        )
        .unwrap(),
    )
    .unwrap()
}

/// The body of the defined function whose name contains `needle`.
fn function(ir: &str, needle: &str) -> String {
    ir.lines()
        .skip_while(|l| !(l.starts_with("define ") && l.contains(needle)))
        .take_while(|l| *l != "}")
        .collect::<Vec<_>>()
        .join("\n")
}

/// The block labelled `label` (up to its next blank line).
fn block<'a>(body: &'a str, label: &str) -> &'a str {
    let start = body
        .find(&format!("\n{label}:"))
        .unwrap_or_else(|| panic!("no block {label}"))
        + 1;
    let rest = &body[start..];
    &rest[..rest.find("\n\n").unwrap_or(rest.len())]
}

/// The label of each `pfield.hit.*` (site word) block of `body`.
fn word_hits(body: &str) -> Vec<String> {
    body.lines()
        .filter_map(|l| l.strip_suffix(':'))
        .filter(|l| l.starts_with("pfield.hit."))
        .map(str::to_string)
        .collect()
}

const SOURCE: &str = r#"
class Box {
  #v = 1;
  get() { return this.#v; }
  set(x: number) { this.#v = x; }
}
const b = new Box();
b.set(2);
console.log(b.get());
"#;

#[test]
fn a_word_hit_sends_a_hole_slot_to_the_miss() {
    let ir = compile(SOURCE);
    for (method, op) in [("Box__get", "read"), ("Box__set", "write")] {
        let body = function(&ir, method);
        let hits = word_hits(&body);
        assert!(!hits.is_empty(), "{op}: the site has a word hit");
        for hit in hits {
            let hit_block = block(&body, &hit);
            let hole = hit_block
                .lines()
                .find(|l| l.contains("icmp eq i64") && l.contains(crate::nanbox::TAG_HOLE_I64))
                .unwrap_or_else(|| panic!("{op}: {hit} tests the slot for the hole:\n{hit_block}"));
            let is_hole = hole.trim_start().split(' ').next().unwrap();
            assert!(
                hit_block.lines().any(|l| {
                    let l = l.trim_start();
                    l.starts_with(&format!("br i1 {is_hole}, label %"))
                        && l.contains(", label %pfield.present")
                }),
                "{op}: a hole slot takes the other arm, the miss:\n{hit_block}"
            );
        }
    }
}
