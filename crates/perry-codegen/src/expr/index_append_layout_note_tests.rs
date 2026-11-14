//! #13491: the dense-append arm of `a[i] = v` (`idxset.extend`) calls
//! `js_gc_note_slot_layout` only behind an inline gate on the stored value and
//! the array's live `_reserved` bits, never unconditionally. A `boolean[]`
//! fill loop (`benchmarks/suite/11_prime_sieve.ts`) otherwise spends a quarter
//! of its time in a call that returns without acting.

fn compile(source: &str) -> String {
    let ast = perry_parser::parse_typescript(source, "append_note.ts").unwrap();
    let m = perry_hir::lower_module(&ast, "append_note", "append_note.ts").unwrap();
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

/// Body of the first block whose label starts with `prefix`.
fn block<'a>(ir: &'a str, prefix: &str) -> Option<&'a str> {
    let start = ir.find(&format!("\n{prefix}"))? + 1;
    let rest = &ir[start..];
    let end = rest.find("\n\n").unwrap_or(rest.len());
    Some(&rest[..end])
}

const SOURCE: &str = r#"
    const flags: boolean[] = [];
    for (let i = 0; i < 1000; i++) {
        flags[i] = true;
    }
    console.log(flags.length);
"#;

#[test]
fn append_arm_layout_note_is_gated_on_value_and_header_bits() {
    let ir = compile(SOURCE);
    let note = block(&ir, "idxset.extend.laynote.")
        .unwrap_or_else(|| panic!("the append arm has no gated layout-note block:\n{ir}"));
    assert!(
        note.contains("call void @js_gc_note_slot_layout("),
        "the gated block must still make the note call:\n{note}"
    );
    // The only path into the note block is a conditional branch whose
    // condition ORs the value's pointer test with the `_reserved & 0x3800`
    // header test.
    let label = note
        .lines()
        .next()
        .unwrap()
        .trim_end_matches(':')
        .to_string();
    let branch = ir
        .lines()
        .find(|l| l.trim_start().starts_with("br i1") && l.contains(&format!("%{label},")))
        .unwrap_or_else(|| panic!("no conditional branch into {label}:\n{ir}"));
    let cond = branch
        .split_whitespace()
        .nth(2)
        .unwrap()
        .trim_end_matches(',');
    let def = ir
        .lines()
        .find(|l| l.trim_start().starts_with(&format!("{cond} = ")))
        .unwrap_or_else(|| panic!("condition {cond} has no definition"));
    assert!(def.contains("or i1"), "gate must be value || header: {def}");
    assert!(
        ir.contains("and i16") && ir.contains(", 14336"),
        "the header half of the gate must test `_reserved & 0x3800`"
    );
    // Every `js_gc_note_slot_layout` call left in the module sits in a gated
    // block: none is emitted straight-line after the append store.
    for (at, _) in ir.match_indices("call void @js_gc_note_slot_layout(") {
        let label_start = ir[..at].rfind("\n\n").map_or(0, |p| p + 2);
        let label = ir[label_start..].lines().next().unwrap();
        assert!(
            label.contains(".laynote") || label.starts_with("apush.gc_bookkeeping"),
            "ungated layout note in block `{label}`"
        );
    }
}
