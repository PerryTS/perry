//! The fill birth (`fill_birth.rs`): a fused fill the facts name is one
//! birth, inline for a small plain-double or I32 birth, else one runtime
//! entry; without the fact, and for every shape the module refuses, the
//! two-step lowering stays.

use std::sync::Arc;

use crate::codegen::{ElisionFacts, ProgramFactsView};
use crate::{compile_module, CompileOptions};
use perry_hir::{Expr, Module};

fn lower(src: &str) -> Module {
    let src = src.to_string();
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(move || {
            let mut cache = perry_diagnostics::SourceCache::new();
            let parsed = perry_parser::parse_typescript_with_cache(&src, "fb.ts", &mut cache)
                .expect("parses");
            perry_hir::lower_module(&parsed.module, "fb", "fb.ts").expect("lowers")
        })
        .expect("spawn")
        .join()
        .expect("lowering thread")
}

/// The offsets codegen keys the `fill` calls of `m` by (the member's, else
/// the call's).
fn fill_offsets(m: &Module) -> Vec<u32> {
    fn visit(e: &Expr, out: &mut Vec<u32>) {
        if let Expr::Call {
            callee,
            byte_offset,
            ..
        } = e
        {
            if let Expr::PropertyGet {
                property,
                byte_offset: member,
                ..
            } = callee.as_ref()
            {
                if property == "fill" {
                    out.push(if *member != 0 { *member } else { *byte_offset });
                }
            }
        }
        perry_hir::walker::walk_expr_children(e, &mut |c| visit(c, out));
    }
    let mut out = Vec::new();
    for f in &m.functions {
        for s in &f.body {
            perry_hir::walker::stmt_any_expr(s, &mut |e| {
                visit(e, &mut out);
                false
            });
        }
    }
    out.sort_unstable();
    out.dedup();
    out
}

/// What the facts say of the one `fill` call of the fixture.
#[derive(Clone, Copy, Default)]
struct Fact {
    len: Option<u32>,
    rest: bool,
    reserve: Option<u32>,
}

fn ir(src: &str, fact: Fact) -> String {
    let m = lower(src);
    let offsets = fill_offsets(&m);
    assert_eq!(offsets.len(), 1, "one fill call: {offsets:?}");
    let mut elision = ElisionFacts::default();
    if let Some(len) = fact.len {
        elision.fill_births.insert(offsets[0], len);
    }
    if fact.rest {
        elision.rest_births.insert(offsets[0]);
    }
    if let Some(cap) = fact.reserve {
        elision.rest_reserves.insert(offsets[0], cap);
    }
    let opts = CompileOptions {
        emit_ir_only: true,
        program_facts: ProgramFactsView {
            elision: Arc::new(elision),
            ..Default::default()
        },
        ..Default::default()
    };
    String::from_utf8(compile_module(&m, opts).expect("fixture compiles")).expect("UTF-8 IR")
}

const ZEROES: &str = "export function make(): number[] { return new Array<number>(8).fill(0); }\n";

/// The packed header word of an inline fill birth of `slots` slots.
fn header_word(slots: u64, rest: bool) -> String {
    let total = 8 + crate::target_layout::ARRAY_HEADER_SIZE_BYTES + 8 * slots;
    let word = u64::from(perry_abi::GC_TYPE_ARRAY)
        | (0x02 << 8)
        | (u64::from(perry_abi::fill_birth_layout(rest)) << 16)
        | (total << 32);
    format!("i64 {word}")
}

/// The calls of `name` in `ir` (its declaration is none).
fn calls(ir: &str, name: &str) -> usize {
    let needle = format!("@{name}(");
    ir.lines()
        .filter(|l| l.contains(&needle) && !l.trim_start().starts_with("declare"))
        .count()
}

/// A fill value no other store of the fixtures writes.
const SEVENS: &str = "export function make(): number[] { return new Array<number>(8).fill(7); }\n";

#[test]
fn a_small_number_birth_is_inline_with_one_store_per_slot() {
    let with = ir(
        ZEROES,
        Fact {
            len: Some(8),
            ..Default::default()
        },
    );
    assert_eq!(calls(&with, "js_array_constructor_single"), 0, "{with}");
    assert_eq!(calls(&with, "js_array_fill"), 0, "{with}");
    assert_eq!(calls(&with, "js_array_fill_birth"), 0, "inline\n{with}");
    assert!(with.contains("arrlit.fast"), "the bump birth\n{with}");
    assert!(
        with.contains(&header_word(8, false)),
        "the fill birth's layout bits\n{with}"
    );
    // Length 8 and capacity 8 in one word; the element-kind prefix is 8.
    assert!(
        with.contains(&format!(
            "store i64 {},",
            perry_abi::array_birth_header(8, 8)
        )),
        "{with}"
    );
    assert!(
        !with.contains(&format!("store i64 {},", crate::nanbox::TAG_HOLE)),
        "no slack, no hole store\n{with}"
    );
    let with = ir(
        SEVENS,
        Fact {
            len: Some(8),
            ..Default::default()
        },
    );
    assert_eq!(
        with.matches("store double 7.0,").count(),
        8,
        "one store per slot\n{with}"
    );
    let without = ir(SEVENS, Fact::default());
    assert_eq!(
        without.matches("store double 7.0,").count(),
        0,
        "the two-step birth stores nothing inline\n{without}"
    );
}

#[test]
fn without_the_fact_the_two_step_birth_stays() {
    let without = ir(ZEROES, Fact::default());
    assert_eq!(
        calls(&without, "js_array_constructor_single"),
        1,
        "{without}"
    );
    assert!(calls(&without, "js_array_fill") >= 1, "{without}");
    assert_eq!(calls(&without, "js_array_fill_birth"), 0, "{without}");
}

#[test]
fn a_short_birth_has_the_runtime_floor_and_its_slack_is_holes() {
    let src = "export function make(): number[] { return new Array<number>(2).fill(7); }\n";
    let with = ir(
        src,
        Fact {
            len: Some(2),
            ..Default::default()
        },
    );
    assert!(with.contains(&header_word(4, false)), "{with}");
    assert!(
        with.contains(&format!(
            "store i64 {},",
            perry_abi::array_birth_header(2, 4)
        )),
        "{with}"
    );
    assert_eq!(with.matches("store double 7.0,").count(), 2, "{with}");
    assert_eq!(
        with.matches(&format!("store i64 {},", crate::nanbox::TAG_HOLE))
            .count(),
        2,
        "the slack\n{with}"
    );
}

#[test]
fn an_at_rest_birth_stores_i32_words_and_reserves_the_facts_capacity() {
    let with = ir(
        ZEROES,
        Fact {
            len: Some(8),
            rest: true,
            reserve: Some(16),
        },
    );
    assert_eq!(calls(&with, "js_array_rest_encode"), 0, "{with}");
    assert_eq!(calls(&with, "js_array_rest_reserve"), 0, "{with}");
    assert_eq!(calls(&with, "js_array_fill"), 0, "{with}");
    assert!(with.contains(&header_word(16, true)), "{with}");
    assert!(
        with.contains(&format!(
            "store i64 {},",
            perry_abi::array_birth_header(8, 16)
        )),
        "{with}"
    );
    // The I32 word is `INT32_TAG | zext(i32)`, stored once per slot.
    let word = with
        .lines()
        .find(|l| l.contains("or i64") && l.trim_end().ends_with(", 9222809086901354496"))
        .and_then(|l| l.split('=').next())
        .map(|r| r.trim().to_string())
        .unwrap_or_else(|| panic!("no I32 word\n{with}"));
    assert_eq!(
        with.matches(&format!("store i64 {word},")).count(),
        8,
        "{with}"
    );
    assert_eq!(
        with.matches(&format!("store i64 {},", crate::nanbox::TAG_HOLE))
            .count(),
        8,
        "the reserved slots are holes\n{with}"
    );
}

#[test]
fn a_large_birth_or_a_value_that_is_no_plain_double_takes_the_runtime_entry() {
    // Past the inline bound.
    let src = "export function make(): number[] { return new Array<number>(128).fill(0); }\n";
    let with = ir(
        src,
        Fact {
            len: Some(128),
            ..Default::default()
        },
    );
    assert_eq!(calls(&with, "js_array_fill_birth"), 1, "{with}");
    assert!(
        with.contains("@js_array_fill_birth(i32 128, i32 0, double 0.0, i32 0)"),
        "{with}"
    );
    assert_eq!(calls(&with, "js_array_constructor_single"), 0, "{with}");
    assert_eq!(calls(&with, "js_array_fill"), 0, "{with}");
    // A reservation past the inline bound.
    let with = ir(
        src,
        Fact {
            len: Some(128),
            rest: true,
            reserve: Some(256),
        },
    );
    assert!(
        with.contains("@js_array_fill_birth(i32 128, i32 256, double 0.0, i32 1)"),
        "{with}"
    );
    // A value the site cannot prove a plain double.
    let src = "export function make(v: any): any[] { return new Array(8).fill(v); }\n";
    let with = ir(
        src,
        Fact {
            len: Some(8),
            ..Default::default()
        },
    );
    assert_eq!(calls(&with, "js_array_fill_birth"), 1, "{with}");
    assert!(!with.contains("arrlit.fast"), "{with}");
}

#[test]
fn a_shape_the_facts_do_not_describe_is_refused() {
    // The facts' length disagrees with the literal: the two-step birth.
    let with = ir(
        ZEROES,
        Fact {
            len: Some(9),
            ..Default::default()
        },
    );
    assert_eq!(calls(&with, "js_array_constructor_single"), 1, "{with}");
    assert_eq!(calls(&with, "js_array_fill_birth"), 0, "{with}");
    // A fill with a start is never a birth, whatever the map says.
    let src = "export function make(): number[] { return new Array<number>(8).fill(0, 2); }\n";
    let with = ir(
        src,
        Fact {
            len: Some(8),
            ..Default::default()
        },
    );
    assert_eq!(calls(&with, "js_array_constructor_single"), 1, "{with}");
    assert_eq!(calls(&with, "js_array_fill_birth"), 0, "{with}");
    // A length whose value is the literal but whose expression has an
    // effect: the facts give the literal's node (`new Array((f(), 8))`), and
    // bearing the birth would drop the effect.
    let src = "let n = 0;\nexport function make(): number[] { return new Array<number>((n++, 8)).fill(0); }\n";
    let with = ir(
        src,
        Fact {
            len: Some(8),
            ..Default::default()
        },
    );
    assert_eq!(calls(&with, "js_array_constructor_single"), 1, "{with}");
    assert_eq!(calls(&with, "js_array_fill_birth"), 0, "{with}");
    // A length that is no literal.
    let src =
        "export function make(n: number): number[] { return new Array<number>(n).fill(0); }\n";
    let with = ir(
        src,
        Fact {
            len: Some(8),
            ..Default::default()
        },
    );
    assert!(calls(&with, "js_array_fill") >= 1, "{with}");
    assert_eq!(calls(&with, "js_array_fill_birth"), 0, "{with}");
}
