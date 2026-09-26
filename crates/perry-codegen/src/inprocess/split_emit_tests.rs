//! #10586: the split halves must link and run as the whole unit did.
//!
//! Each fixture unit holds an over-budget function that reaches its siblings
//! only through LOCAL symbols — an internal helper, an internal mutable
//! global, a private string — which is the shape the promotion exists for:
//! left local, the offender half would reference symbols no object defines.
//! Two units promote the same local names (`@.str`, `@helper`, `@counter`),
//! so a promoted name that was not unit-unique would fail the final link as
//! a duplicate symbol.

use super::super::*;

/// One unit: `@wide_<tag>` (the offender, internal) reads `@.str` and
/// `@counter` and calls `@helper`; `@entry_<tag>` (external) is its sibling
/// and calls `@wide_<tag>` twice, so the internal global's state must be
/// shared across the halves for the result to come out right.
fn unit_ir(tag: &str) -> String {
    format!(
        r#"
@.str = private unnamed_addr constant [4 x i8] c"abc\00"
@counter = internal global i64 5

define internal i64 @helper(i64 %x) noinline {{
entry:
  %c = load volatile i64, ptr @counter
  %r = add i64 %x, %c
  store volatile i64 %r, ptr @counter
  ret i64 %r
}}

define internal i64 @wide_{tag}(i64 %n) noinline {{
entry:
  %p = getelementptr inbounds [4 x i8], ptr @.str, i64 0, i64 1
  %ch = load volatile i8, ptr %p
  %b = zext i8 %ch to i64
  %a0 = call i64 @helper(i64 %n)
  %a1 = mul i64 %a0, 3
  %a2 = xor i64 %a1, %b
  %a3 = call i64 @helper(i64 %a2)
  %a4 = add i64 %a3, %a1
  %a5 = mul i64 %a4, 7
  %a6 = call i64 @helper(i64 %a5)
  %a7 = xor i64 %a6, %a2
  %a8 = add i64 %a7, %b
  ret i64 %a8
}}

define i64 @entry_{tag}(i64 %n) {{
entry:
  %x = call i64 @wide_{tag}(i64 %n)
  %y = call i64 @wide_{tag}(i64 %x)
  %z = call i64 @helper(i64 %y)
  ret i64 %z
}}
"#
    )
}

/// The same computation in Rust, so the expected value is not a constant
/// copied from a run.
fn model(n: i64) -> i64 {
    let mut counter = 5i64;
    let mut helper = |x: i64| {
        counter = x.wrapping_add(counter);
        counter
    };
    let b = i64::from(b'b');
    let wide = |n: i64, helper: &mut dyn FnMut(i64) -> i64| {
        let a1 = helper(n).wrapping_mul(3);
        let a2 = a1 ^ b;
        let a4 = helper(a2).wrapping_add(a1);
        let a7 = helper(a4.wrapping_mul(7)) ^ a2;
        a7.wrapping_add(b)
    };
    let x = wide(n, &mut helper);
    let y = wide(x, &mut helper);
    helper(y)
}

fn emit_split(tag: &str) -> (Vec<u8>, Vec<FastEmitFallback>) {
    global_init(&[]);
    let target = crate::codegen::default_target_triple();
    let context = Context::create();
    let module =
        parse_ir_text(&context, &unit_ir(tag), &format!("split_{tag}")).expect("fixture parses");
    let mut stats = UnitCodegenStats::default();
    let args = ["-O2".to_string(), "-c".to_string()];
    let pieces = with_test_fast_emit_budget(8, || {
        optimize_and_emit_module_with_stats(&module, &target, &args, false, Some(&mut stats))
    })
    .expect("the split unit emits");
    assert_eq!(
        pieces.len(),
        2,
        "the offender must be emitted apart from its sibling: {:?}",
        stats.fast_emit_fallbacks
    );
    let object = crate::linker::finish_native_pieces(pieces, &target, &args)
        .expect("the halves partial-link into one object");
    (object, stats.fast_emit_fallbacks)
}

#[test]
fn split_halves_share_promoted_locals_and_link_across_units() {
    let (alpha, over_alpha) = emit_split("alpha");
    let (beta, _) = emit_split("beta");
    let names: Vec<&str> = over_alpha.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(
        names,
        ["wide_alpha"],
        "only the wide function is over the budget"
    );
    assert_eq!(over_alpha[0].whole_unit, None);

    let dir = std::env::temp_dir().join(format!("perry_split_emit_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let alpha_o = dir.join("alpha.o");
    let beta_o = dir.join("beta.o");
    let main_c = dir.join("main.c");
    let exe = dir.join("split_run");
    std::fs::write(&alpha_o, &alpha).unwrap();
    std::fs::write(&beta_o, &beta).unwrap();
    std::fs::write(
        &main_c,
        "#include <stdio.h>\n\
         long long entry_alpha(long long);\n\
         long long entry_beta(long long);\n\
         int main(int argc, char **argv) {\n\
           (void)argv;\n\
           printf(\"%lld %lld\\n\", entry_alpha(argc), entry_beta(argc + 1));\n\
           return 0;\n\
         }\n",
    )
    .unwrap();
    let link = std::process::Command::new("cc")
        .arg(&main_c)
        .arg(&alpha_o)
        .arg(&beta_o)
        .arg("-o")
        .arg(&exe)
        .output()
        .expect("cc runs");
    assert!(
        link.status.success(),
        "two split units must link together (unique promoted names):\n{}",
        String::from_utf8_lossy(&link.stderr)
    );
    let run = std::process::Command::new(&exe)
        .output()
        .expect("binary runs");
    assert!(run.status.success());
    assert_eq!(
        String::from_utf8_lossy(&run.stdout).trim(),
        format!("{} {}", model(1), model(2)),
        "the split halves must compute what the whole unit computes"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
