//! Tests for per-function fast-emit containment (`fast_emit_split`).

use super::super::*;
use super::{split_emission_supported, with_split_declined, PROMOTED_INFIX};

fn host_target() -> String {
    crate::codegen::default_target_triple()
}

/// Whether this host can run the split tests. On the hosts CI and developers
/// use (macOS, Linux) the split MUST be supported for the host triple —
/// otherwise a regression in `split_emission_supported` would turn every
/// containment test below into a silent skip.
fn split_host() -> bool {
    if cfg!(any(target_os = "macos", target_os = "linux")) {
        assert!(
            split_emission_supported(&host_target()),
            "containment must be supported for the host triple {}",
            host_target()
        );
        true
    } else {
        false
    }
}

/// One function over the budget, one ordinary function that is nowhere
/// near it, and one external callee so nothing folds away. `narrow`
/// holds three values across three calls, which is what makes its
/// machine code differ between the optimized and the O0 register
/// allocators.
fn sibling_cost_fixture(with_wide: bool) -> String {
    let wide = r#"
define i64 @wide(i64 %n) {
entry:
  %a = call i64 @src(i64 %n)
  %b = call i64 @src(i64 %a)
  %c = call i64 @src(i64 %b)
  %d = call i64 @src(i64 %c)
  %s = add i64 %a, %b
  %t = add i64 %s, %c
  %u = add i64 %t, %d
  ret i64 %u
}
"#;
    format!(
        r#"
declare i64 @src(i64)

define i64 @narrow(i64 %x, i64 %y) {{
entry:
  %a = call i64 @src(i64 %x)
  %b = call i64 @src(i64 %y)
  %c = call i64 @src(i64 %a)
  %s = add i64 %a, %b
  %t = add i64 %s, %c
  ret i64 %t
}}
{}"#,
        if with_wide { wide } else { "" }
    )
}

/// The assembly of one function, from its label to the end of its body.
/// Tolerates ELF (`narrow:` / `.size`) and Mach-O (`_narrow:`) spelling.
fn function_assembly(asm: &str, name: &str) -> String {
    let label_elf = format!("{name}:");
    let label_macho = format!("_{name}:");
    let mut body: Vec<&str> = Vec::new();
    let mut inside = false;
    for line in asm.lines() {
        let trimmed = line.trim();
        if !inside {
            inside = trimmed == label_elf || trimmed == label_macho;
            continue;
        }
        let next_symbol = trimmed.ends_with(':')
            && !trimmed.starts_with('.')
            && !trimmed.starts_with('L')
            && !trimmed.contains(' ');
        if trimmed.starts_with(".size") || trimmed == ".cfi_endproc" || next_symbol {
            break;
        }
        body.push(line);
    }
    assert!(
        body.len() > 3,
        "no body extracted for `{name}` — the assertion below would be vacuous:\n{asm}"
    );
    body.join("\n")
}

/// Emit `ir` at `-Os -S` under `budget`; one assembly text per emitted part
/// (two when containment split the unit), the functions over the budget,
/// and whether they were contained.
fn emit_assembly(
    ir: &str,
    module_name: &str,
    budget: FastEmitBudget,
) -> (Vec<String>, Vec<String>, bool) {
    global_init(&[]);
    let target = crate::codegen::default_target_triple();
    let context = Context::create();
    let module = parse_ir_text(&context, ir, module_name).expect("fixture parses");
    let mut stats = UnitCodegenStats::default();
    let asm = with_test_fast_emit_budget_value(budget, || {
        optimize_and_emit_module_with_stats(
            &module,
            &target,
            &["-Os".into(), "-S".into()],
            false,
            Some(&mut stats),
        )
    })
    .expect("the fixture emits");
    (
        asm.into_iter()
            .map(|part| String::from_utf8(part).expect("LLVM emits UTF-8 assembly"))
            .collect(),
        stats
            .fast_emit_fallbacks
            .iter()
            .map(|f| f.name.clone())
            .collect(),
        stats.fast_emit_fallbacks.iter().all(|f| f.contained),
    )
}

fn defines(asm: &str, name: &str) -> bool {
    asm.lines().any(|line| {
        let t = line.trim();
        t == format!("{name}:") || t == format!("_{name}:")
    })
}

/// The ordinary function keeps the optimized machine pipeline when an
/// extreme function shares its unit: its machine code is byte-for-byte what
/// the unit emits with no budget at all, and what a unit *without* the
/// extreme function emits. The extreme function is emitted alone in the
/// second part.
#[test]
fn containment_keeps_ordinary_siblings_on_the_optimized_pipeline() {
    if !split_host() {
        return;
    }
    let with_wide = sibling_cost_fixture(true);
    let alone = sibling_cost_fixture(false);

    let (undemoted, none, _) = emit_assembly(&with_wide, "sibling_cost_ok", FastEmitBudget::Off);
    assert!(none.is_empty(), "this arm must not demote: {none:?}");
    assert_eq!(undemoted.len(), 1, "no budget, no split");
    let (solo, _, _) = emit_assembly(&alone, "sibling_cost_alone", FastEmitBudget::Off);
    assert_eq!(
        function_assembly(&undemoted[0], "narrow"),
        function_assembly(&solo[0], "narrow"),
        "an undemoted unit emits an ordinary function exactly as a unit without the extreme \
         function does"
    );

    let (parts, over, contained) =
        emit_assembly(&with_wide, "sibling_cost_contained", FastEmitBudget::Cap(7));
    assert_eq!(over, ["wide"], "only `wide` is over the budget");
    assert!(contained, "the stats must report the containment");
    assert_eq!(parts.len(), 2, "the unit must be emitted in two parts");
    assert!(defines(&parts[0], "narrow") && !defines(&parts[0], "wide"));
    assert!(defines(&parts[1], "wide") && !defines(&parts[1], "narrow"));
    assert_eq!(
        function_assembly(&parts[0], "narrow"),
        function_assembly(&undemoted[0], "narrow"),
        "`narrow` is under the budget and must keep the optimized machine pipeline"
    );
}

/// The discriminating arm for the test above: with the split declined and
/// the O0 machine (the pre-containment behaviour), the same `narrow` IS
/// compiled differently.
/// If this ever stops failing to match, the fixture no longer tells the two
/// machine pipelines apart and the containment test is vacuous.
#[test]
fn without_containment_the_sibling_is_demoted() {
    let with_wide = sibling_cost_fixture(true);
    let (undemoted, _, _) = emit_assembly(&with_wide, "sibling_cost_ok2", FastEmitBudget::Off);
    // Cap 1: both functions are far past four times the budget, so the
    // declined unit takes the O0 backstop — the machine whose effect on an
    // ordinary sibling this arm demonstrates.
    let (demoted, over, contained) = with_split_declined(|| {
        emit_assembly(&with_wide, "sibling_cost_demoted", FastEmitBudget::Cap(1))
    });
    assert_eq!(over, ["wide", "narrow"]);
    assert!(!contained);
    assert_eq!(demoted.len(), 1, "a declined split emits one part");
    assert_ne!(
        function_assembly(&demoted[0], "narrow"),
        function_assembly(&undemoted[0], "narrow"),
        "whole-unit demotion must reach `narrow`, or this fixture cannot tell the pipelines apart"
    );
}

/// Every edge the cut severs, in one program that is linked and run: an
/// internal extreme function called directly and through a global table, an
/// internal helper it calls back across the cut, a private string constant
/// and an internal mutable global it shares with the sibling side. A missing
/// promotion is an undefined symbol at link time; a wrong one (two copies of
/// the global) changes the exit code.
fn cross_cut_program() -> &'static str {
    r#"
@.msg = private unnamed_addr constant [4 x i8] c"abc\00"
@counter = internal global i64 0
@table = internal global [1 x ptr] [ptr @wide]

define internal i64 @helper(i64 %x) noinline {
entry:
  %c = load i64, ptr @counter
  %c1 = add i64 %c, 1
  store i64 %c1, ptr @counter
  %r = add i64 %x, %c1
  ret i64 %r
}

define internal i64 @wide(i64 %n) noinline {
entry:
  %a = call i64 @helper(i64 %n)
  %b = call i64 @helper(i64 %a)
  %c0 = call i64 @helper(i64 %b)
  %c1 = call i64 @helper(i64 %c0)
  %c2 = call i64 @helper(i64 %c1)
  %c3 = call i64 @helper(i64 %c2)
  %c4 = call i64 @helper(i64 %c3)
  %c5 = call i64 @helper(i64 %c4)
  %c = call i64 @helper(i64 %c5)
  %p = getelementptr inbounds [4 x i8], ptr @.msg, i64 0, i64 1
  %ch = load i8, ptr %p
  %chw = zext i8 %ch to i64
  %s = add i64 %c, %chw
  %k = load i64, ptr @counter
  %t = mul i64 %s, %k
  %u = xor i64 %t, %a
  %v = sub i64 %u, %b
  ret i64 %v
}

define i32 @main(i32 %argc, ptr %argv) {
entry:
  %seed = sext i32 %argc to i64
  store i64 %seed, ptr @counter
  %f = load ptr, ptr @table
  %x = call i64 %f(i64 %seed)
  %y = call i64 @wide(i64 %seed)
  %k = load i64, ptr @counter
  %sum = add i64 %x, %y
  %all = add i64 %sum, %k
  %m = urem i64 %all, 251
  %r = trunc i64 %m to i32
  ret i32 %r
}
"#
}

fn link_and_run(object: &[u8], label: &str) -> i32 {
    let dir = std::env::temp_dir().join(format!(
        "perry_fast_emit_split_{label}_{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("scratch dir");
    let obj = dir.join("prog.o");
    let exe = dir.join("prog");
    std::fs::write(&obj, object).expect("write object");
    let cc = crate::linker::find_clang().expect("a C compiler driver links the fixture");
    let linked = std::process::Command::new(cc)
        .arg(&obj)
        .arg("-o")
        .arg(&exe)
        .output()
        .expect("run the linker");
    assert!(
        linked.status.success(),
        "{label}: link failed:\n{}",
        String::from_utf8_lossy(&linked.stderr)
    );
    let status = std::process::Command::new(&exe)
        .status()
        .expect("run the fixture");
    let _ = std::fs::remove_dir_all(&dir);
    status.code().expect("the fixture exits normally")
}

fn emit_program(
    budget: FastEmitBudget,
    label: &str,
) -> (Vec<Vec<u8>>, Vec<u8>, Vec<FastEmitFallback>) {
    global_init(&[]);
    let target = host_target();
    let args: Vec<String> = vec!["-Os".into(), "-c".into()];
    let context = Context::create();
    let module = parse_ir_text(&context, cross_cut_program(), label).expect("fixture parses");
    let mut stats = UnitCodegenStats::default();
    let parts = with_test_fast_emit_budget_value(budget, || {
        optimize_and_emit_module_with_stats(&module, &target, &args, false, Some(&mut stats))
    })
    .expect("the fixture emits");
    let object =
        crate::linker::finish_native_emission(parts.clone(), &target, &args).expect("finishes");
    (parts, object, stats.fast_emit_fallbacks)
}

#[test]
fn containment_preserves_every_edge_across_the_cut() {
    if !split_host() {
        return;
    }
    let (whole, whole_object, _) = emit_program(FastEmitBudget::Off, "whole");
    assert_eq!(whole.len(), 1);
    let expected = link_and_run(&whole_object, "whole");

    let (parts, split_object, over) = emit_program(FastEmitBudget::Cap(12), "split");
    assert!(
        over.len() == 1 && over[0].name == "wide" && over[0].contained,
        "`wide` alone must be over the budget and contained: {over:?}"
    );
    assert_eq!(parts.len(), 2, "the unit must be emitted in two parts");
    assert_eq!(
        link_and_run(&split_object, "split"),
        expected,
        "the split program must compute what the whole one does"
    );
}

/// The promoted names are hidden, unit-unique, and only the severed locals
/// are promoted.
#[test]
fn promotion_is_hidden_unique_and_minimal() {
    if !split_host() {
        return;
    }
    let context = Context::create();
    let module = parse_ir_text(&context, cross_cut_program(), "promotion").expect("fixture parses");
    let contained = super::split_moved_functions(&module, &["wide".to_string()])
        .expect("the fixture splits")
        .expect("`main` and `helper` stay behind");
    let sibling_ir = module.print_to_string().to_string();
    let contained_ir = contained.print_to_string().to_string();
    for name in ["counter", ".msg", "table"] {
        let line = sibling_ir
            .lines()
            .find(|l| l.starts_with(&format!("@{name}")))
            .unwrap_or_else(|| panic!("`{name}` missing:\n{sibling_ir}"));
        let is_promoted = line.starts_with(&format!("@{name}{PROMOTED_INFIX}"));
        // `table` is referenced only from the sibling side: it stays local.
        assert_eq!(is_promoted, name != "table", "{line}");
        if is_promoted {
            assert!(line.contains(" hidden "), "promoted must be hidden: {line}");
        }
    }
    let helper = sibling_ir
        .lines()
        .find(|l| l.starts_with("define") && l.contains(&format!("@helper{PROMOTED_INFIX}")))
        .unwrap_or_else(|| panic!("`helper` not promoted:\n{sibling_ir}"));
    assert!(helper.contains(" hidden "), "{helper}");
    assert!(
        sibling_ir.contains("declare hidden i64 @wide"),
        "the sibling side declares the moved function:\n{sibling_ir}"
    );
    assert!(
        contained_ir.contains("define hidden i64 @wide"),
        "the contained side defines it:\n{contained_ir}"
    );
    assert!(
        !contained_ir.contains("define i32 @main")
            && !contained_ir.contains("@table")
            && !contained_ir.contains("@main"),
        "the contained side holds nothing it does not reference:\n{contained_ir}"
    );
    assert!(
        contained_ir.contains("@counter") && contained_ir.contains("external hidden global i64"),
        "{contained_ir}"
    );

    // A different unit (a different function set) gets different names.
    let other_ir = cross_cut_program().replace("@main(", "@main2(");
    let other = parse_ir_text(&context, &other_ir, "promotion_other").expect("parses");
    let _ = super::split_moved_functions(&other, &["wide".to_string()])
        .expect("splits")
        .expect("has siblings");
    let token = |ir: &str| {
        let at = ir.find(PROMOTED_INFIX).expect("a promoted name") + PROMOTED_INFIX.len();
        ir[at..at + 16].to_string()
    };
    assert_ne!(
        token(&sibling_ir),
        token(&other.print_to_string().to_string()),
        "two units must not share promoted names"
    );
}

/// The sibling half keeps its statepoint stack map through the cut: the
/// collector must still find the roots of every function that stayed on the
/// optimized pipeline. (A contained function carries no statepoints of its
/// own: an over-budget statepoint function is re-lowered onto a shadow frame
/// first — see the next test.)
#[test]
fn the_sibling_half_keeps_its_stack_map() {
    if !split_host() {
        return;
    }
    let ir = r#"
declare i64 @may_collect()
declare i64 @leaf(i64)

define i64 @narrow(i64 %a) gc "statepoint-example" {
entry:
  %p = inttoptr i64 %a to ptr addrspace(1)
  %t = call i64 @may_collect()
  %bits = ptrtoint ptr addrspace(1) %p to i64
  %r = add i64 %t, %bits
  ret i64 %r
}

define i64 @wide(i64 %a) {
entry:
  %t1 = call i64 @leaf(i64 %a)
  %t2 = call i64 @leaf(i64 %t1)
  %t3 = call i64 @leaf(i64 %t2)
  %t4 = call i64 @leaf(i64 %t3)
  %t5 = call i64 @leaf(i64 %t4)
  %t6 = call i64 @leaf(i64 %t5)
  %t7 = call i64 @leaf(i64 %t6)
  %t8 = call i64 @leaf(i64 %t7)
  %s1 = add i64 %t1, %t2
  %s2 = add i64 %s1, %t3
  %s3 = add i64 %s2, %t4
  %s4 = add i64 %s3, %t5
  %s5 = add i64 %s4, %t6
  %s6 = add i64 %s5, %t7
  %r = add i64 %s6, %t8
  ret i64 %r
}
"#;
    global_init(&[]);
    let target = host_target();
    let context = Context::create();
    let module = parse_ir_text(&context, ir, "gc_split").expect("fixture parses");
    let mut stats = UnitCodegenStats::default();
    let parts = with_test_fast_emit_budget_value(FastEmitBudget::Cap(12), || {
        optimize_and_emit_module_with_stats(
            &module,
            &target,
            &["-Os".into(), "-S".into()],
            true,
            Some(&mut stats),
        )
    })
    .expect("emits");
    let over: Vec<&str> = stats
        .fast_emit_fallbacks
        .iter()
        .map(|f| f.name.as_str())
        .collect();
    assert_eq!(over, ["wide"], "`wide` alone is over 12");
    assert_eq!(parts.len(), 2);
    let names: Vec<String> =
        crate::gc_map::decode_stack_map_roots(std::str::from_utf8(&parts[0]).unwrap(), &target)
            .expect("the sibling half's stack map decodes")
            .into_iter()
            .map(|(name, _)| name.trim_start_matches('_').to_string())
            .collect();
    assert_eq!(names, ["narrow"]);
    assert!(!std::str::from_utf8(&parts[1])
        .unwrap()
        .contains("llvm_stackmaps"));
}

/// Over the budget, a statepoint function is not contained as it is: the
/// backend asks codegen to re-lower it onto a shadow frame (the typed retry
/// the RS4GC budgets use), so it can keep the optimized machine pipeline.
#[test]
fn an_over_budget_statepoint_function_requests_a_shadow_frame_relowering() {
    let ir = r#"
declare i64 @may_collect()

define i64 @wide(i64 %a) gc "statepoint-example" {
entry:
  %p = inttoptr i64 %a to ptr addrspace(1)
  %t1 = call i64 @may_collect()
  %t2 = call i64 @may_collect()
  %bits = ptrtoint ptr addrspace(1) %p to i64
  %s = add i64 %t1, %t2
  %r = add i64 %s, %bits
  ret i64 %r
}
"#;
    global_init(&[]);
    let target = host_target();
    let context = Context::create();
    let module = parse_ir_text(&context, ir, "relower").expect("fixture parses");
    let err = with_test_fast_emit_budget_value(FastEmitBudget::Cap(1), || {
        optimize_and_emit_module(&module, &target, &["-Os".into(), "-S".into()], true)
    })
    .expect_err("an over-budget statepoint function must request a re-lowering");
    let retry = rs4gc_budget_retry(&err).expect("the request is typed");
    assert_eq!(retry.len(), 1);
    assert_eq!(retry[0].name, "wide");
    assert!(
        matches!(retry[0].cause, Rs4gcBudgetCause::MachineBudget { instructions } if instructions > 1),
        "{:?}",
        retry[0].cause
    );
}

/// The bounded machine is chosen by how far past the budget the function
/// is: FastISel on the optimized pipeline up to four times over (inclusive),
/// O0 only beyond. The shipped path reports the tier it used.
#[test]
fn the_bounded_machine_grows_with_the_excess() {
    assert_eq!(MachineTier::for_function(601, 600), MachineTier::FastIsel);
    assert_eq!(MachineTier::for_function(2400, 600), MachineTier::FastIsel);
    assert_eq!(MachineTier::for_function(2401, 600), MachineTier::O0);

    let with_wide = sibling_cost_fixture(true);
    let tiers = |budget| {
        global_init(&[]);
        let target = host_target();
        let context = Context::create();
        let module = parse_ir_text(&context, &with_wide, "tiers").expect("fixture parses");
        let mut stats = UnitCodegenStats::default();
        let parts = with_test_fast_emit_budget_value(budget, || {
            optimize_and_emit_module_with_stats(
                &module,
                &target,
                &["-Os".into(), "-S".into()],
                false,
                Some(&mut stats),
            )
        })
        .expect("emits");
        let asm = String::from_utf8(parts.last().unwrap().clone()).unwrap();
        (stats.fast_emit_fallbacks, function_assembly(&asm, "wide"))
    };
    let (near, near_asm) = tiers(FastEmitBudget::Cap(7));
    assert_eq!(near.len(), 1);
    assert_eq!(near[0].tier, MachineTier::FastIsel, "{near:?}");
    let (far, far_asm) = tiers(FastEmitBudget::Cap(1));
    let wide = far.iter().find(|f| f.name == "wide").expect("wide is over");
    assert_eq!(wide.tier, MachineTier::O0, "{far:?}");
    assert_ne!(
        near_asm, far_asm,
        "the two tiers must emit different machine code, or the tier is not live"
    );
    // Code size is a property of large functions (the tier table on
    // `MachineTier`); a fixture this small cannot show it, so only liveness
    // of both tiers is asserted here.
}
