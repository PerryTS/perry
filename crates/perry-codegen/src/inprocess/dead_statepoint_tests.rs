//! A dead post-call use must not make a value a statepoint root.

#[test]
fn dead_post_call_users_do_not_add_gc_live_operands() {
    let target = crate::codegen::default_target_triple();
    let ir = r#"
declare void @may_collect()
define ptr addrspace(1) @probe(ptr addrspace(1) %live, ptr addrspace(1) %dead) gc "statepoint-example" {
entry:
  call void @may_collect()
  %unused = ptrtoint ptr addrspace(1) %dead to i64
  ret ptr addrspace(1) %live
}
"#;
    let after = super::statepoint_rewritten_ir(ir, &target, "dead_statepoint").unwrap();
    let statepoint = after
        .lines()
        .find(|line| line.contains("\"gc-live\""))
        .unwrap();
    assert!(
        statepoint.contains("%live") && !statepoint.contains("%dead"),
        "{after}"
    );
    assert_eq!(
        after
            .matches("call coldcc ptr addrspace(1) @llvm.experimental.gc.relocate")
            .count(),
        1,
        "one relocation per live value: {after}"
    );

    let before = super::statepoint_rewritten_ir_with_passes(
        ir,
        &target,
        "dead_statepoint_control",
        "always-inline,function(mem2reg,sccp),rewrite-statepoints-for-gc",
    )
    .unwrap();
    let statepoint = before
        .lines()
        .find(|line| line.contains("\"gc-live\""))
        .unwrap();
    assert!(
        statepoint.contains("%dead"),
        "negative control must spill the dead value: {before}"
    );

    let emit = |ir: &str| {
        let context = inkwell::context::Context::create();
        let module = super::parse_ir_text(&context, ir, "spill_cost").unwrap();
        let pieces = super::optimize_and_emit_module(
            &module,
            "x86_64-unknown-linux-gnu",
            &["-O3".into(), "-S".into()],
            false,
        )
        .unwrap();
        String::from_utf8(super::single_piece(pieces)).unwrap()
    };
    let stack_moves = |asm: &str| {
        asm.lines()
            .filter(|line| line.trim_start().starts_with("mov") && line.contains("(%rsp)"))
            .count()
    };
    let after_asm = emit(&after);
    let before_asm = emit(&before);
    assert!(
        stack_moves(&after_asm) <= 2,
        "one live spill and reload: {after_asm}"
    );
    // Late LLVM optimization also removes this simple dead root. The early
    // pass eliminates the extra gc-live/relocate before that work is needed,
    // and must not increase the final spill sequence.
    assert_eq!(stack_moves(&before_asm), stack_moves(&after_asm));
    let start = after_asm.find("probe:").unwrap();
    let end = after_asm[start..].find(".Lfunc_end").unwrap();
    let instructions = after_asm[start..start + end]
        .lines()
        .map(|line| line.split('#').next().unwrap_or_default().trim())
        .filter(|line| !line.is_empty() && !line.starts_with('.') && !line.ends_with(':'))
        .count();
    assert!(instructions <= 6, "one live root: {after_asm}");
}
