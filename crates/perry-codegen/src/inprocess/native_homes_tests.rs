use super::*;

fn fixture(n: usize, calls: usize) -> String {
    let mut ir = String::from(
        "declare void @collect()\ndeclare ptr addrspace(1) @make(i64, i32) \"gc-leaf-function\"\ndefine i64 @homes(i64 %arg) gc \"statepoint-example\" {\nentry:\n",
    );
    for i in 0..n {
        ir.push_str(&format!(
            "  %slot{i} = alloca ptr addrspace(1)\n  %p{i} = call ptr addrspace(1) @make(i64 %arg, i32 {i})\n  store ptr addrspace(1) %p{i}, ptr %slot{i}\n"
        ));
    }
    for _ in 0..calls {
        ir.push_str("  call void @collect()\n");
    }
    let mut sum = String::from("0");
    for i in 0..n {
        ir.push_str(&format!(
            "  %v{i} = load ptr addrspace(1), ptr %slot{i}\n  %bits{i} = ptrtoint ptr addrspace(1) %v{i} to i64\n  %sum{i} = xor i64 {sum}, %bits{i}\n"
        ));
        sum = format!("%sum{i}");
    }
    ir.push_str(&format!("  ret i64 {sum}\n}}\n"));
    ir
}

#[test]
fn native_homes_publish_every_word_at_every_statepoint() {
    let pieces = compile_ll_to_object_inprocess(
        &fixture(32, 64),
        "x86_64-unknown-linux-gnu",
        &["-S".into(), "-Os".into()],
        "native_homes_test",
        true,
    )
    .expect("native homes emit");
    let asm = String::from_utf8(pieces.concat()).expect("assembly is text");
    let maps = crate::gc_map::decode_stack_map_roots(&asm, "x86_64-unknown-linux-gnu")
        .expect("direct alloca range decodes and roundtrips");
    let records = &maps
        .iter()
        .find(|(name, _)| name == "homes")
        .expect("function map")
        .1;
    assert_eq!(records.len(), 64);
    for record in records {
        assert_eq!(
            record.len(),
            32,
            "every native home is visible to moving GC"
        );
        assert_eq!(
            record, &records[0],
            "consecutive calls keep the same physical homes"
        );
    }
}

#[test]
fn merging_first_entry_alloca_keeps_the_builder_position_valid() {
    let context = Context::create();
    let module = parse_ir_text(
        &context,
        &fixture(
            native_homes::SMALL_HOME_SET + 1,
            native_homes::HOME_CALL_SPAN + 1,
        ),
        "entry_home_test",
    )
    .unwrap();
    retain(&module);
    global_init(&[]);
    let triple = TargetTriple::create("x86_64-unknown-linux-gnu");
    let tm = Target::from_triple(&triple)
        .unwrap()
        .create_target_machine(
            &triple,
            "",
            "",
            OptimizationLevel::Default,
            RelocMode::PIC,
            CodeModel::Default,
        )
        .unwrap();
    module
        .run_passes(STATEPOINT_REWRITE_PASSES, &tm, PassBuilderOptions::create())
        .unwrap();
    module
        .run_passes("default<Os>", &tm, PassBuilderOptions::create())
        .unwrap();
    publish(&module).expect("first managed alloca can be erased safely");
    module.verify().expect("merged homes verify");
    let text = module.print_to_string().to_string();
    assert_eq!(text.matches("alloca [9 x ptr addrspace(1)]").count(), 1);
    assert_eq!(
        text.matches("\"gc-live\"(ptr %gc.homes)").count(),
        native_homes::HOME_CALL_SPAN + 1
    );
    assert!(!text.contains("@llvm.experimental.gc.relocate"));
}

#[test]
fn sabotage_without_native_homes_restores_relocation_fanout() {
    let context = Context::create();
    global_init(&[]);
    let triple = TargetTriple::create("x86_64-unknown-linux-gnu");
    let tm = Target::from_triple(&triple)
        .unwrap()
        .create_target_machine(
            &triple,
            "",
            "",
            OptimizationLevel::Default,
            RelocMode::PIC,
            CodeModel::Default,
        )
        .unwrap();
    let count = |n| {
        let module = parse_ir_text(&context, &fixture(n, n), "sabotage").unwrap();
        // Deliberately omit retain: mem2reg makes every home an SSA value.
        module
            .run_passes(STATEPOINT_REWRITE_PASSES, &tm, PassBuilderOptions::create())
            .unwrap();
        module
            .print_to_string()
            .to_string()
            .lines()
            .filter(|line| {
                line.contains(" = call") && line.contains("@llvm.experimental.gc.relocate")
            })
            .count()
    };
    let small = count(25);
    let large = count(50);
    assert!(small >= 25 * 25);
    assert!(
        large >= small * 3,
        "without native homes the relocation count must be quadratic"
    );
}

#[test]
fn loop_carried_homes_are_retained_before_source_order_sees_a_call() {
    let context = Context::create();
    let ir = r#"declare void @collect()
declare void @read(i64) "gc-leaf-function"
define void @loop(ptr addrspace(1) %arg, i1 %again) gc "statepoint-example" {
entry:
  %slot = alloca ptr addrspace(1)
  store ptr addrspace(1) %arg, ptr %slot
  br label %header
header:
  %loaded = load ptr addrspace(1), ptr %slot
  %bits = ptrtoint ptr addrspace(1) %loaded to i64
  call void @read(i64 %bits)
  br label %body
body:
  call void @collect()
  call void @collect()
  call void @collect()
  call void @collect()
  call void @collect()
  call void @collect()
  call void @collect()
  call void @collect()
  call void @collect()
  call void @collect()
  call void @collect()
  call void @collect()
  call void @collect()
  call void @collect()
  call void @collect()
  call void @collect()
  br i1 %again, label %header, label %exit
exit:
  ret void
}"#;
    let extra_slots = (0..native_homes::SMALL_HOME_SET).map(|i| format!("  %extra{i} = alloca ptr addrspace(1)\n  store ptr addrspace(1) %arg, ptr %extra{i}\n")).collect::<String>();
    let extra_loads = (0..native_homes::SMALL_HOME_SET)
        .map(|i| format!("  %extra_loaded{i} = load ptr addrspace(1), ptr %extra{i}\n"))
        .collect::<String>();
    let ir = ir.replace(
        "  br label %header\nheader:",
        &format!("{extra_slots}  br label %header\nheader:\n{extra_loads}"),
    );
    let ir = ir.replace(
        "body:\n",
        &format!(
            "body:\n{}",
            "  call void @collect()\n".repeat(native_homes::HOME_CALL_SPAN)
        ),
    );
    let module = parse_ir_text(&context, &ir, "loop_homes").unwrap();
    retain(&module);
    let text = module.print_to_string().to_string();
    assert!(
        text.contains("load volatile ptr addrspace(1)"),
        "loop carries must not fan out through SSA: {text}"
    );
}

#[test]
fn a_cross_block_read_without_intervening_calls_stays_ssa() {
    let context = Context::create();
    let ir = r#"declare void @collect()
define ptr addrspace(1) @short(ptr addrspace(1) %arg) gc "statepoint-example" {
entry:
  %slot = alloca ptr addrspace(1)
  call void @collect()
  call void @collect()
  store ptr addrspace(1) %arg, ptr %slot
  br label %exit
exit:
  %result = load ptr addrspace(1), ptr %slot
  ret ptr addrspace(1) %result
}"#;
    let module = parse_ir_text(&context, ir, "short_home").unwrap();
    retain(&module);
    assert!(!module.print_to_string().to_string().contains("volatile"));
}

#[test]
fn hundreds_of_long_lived_homes_are_one_alloca_before_optimization() {
    let context = Context::create();
    let module = parse_ir_text(&context, &fixture(200, 200), "linear_alloca_walk").unwrap();
    retain(&module);
    module.verify().unwrap();
    let text = module.print_to_string().to_string();
    assert_eq!(text.matches(" = alloca ").count(), 1);
    assert!(text.contains("alloca [200 x ptr addrspace(1)]"));
    assert!(!text.contains("disable-tail-calls"));
}

#[test]
fn bounded_long_lived_root_sets_use_ordinary_ssa_statepoints() {
    let context = Context::create();
    let module = parse_ir_text(
        &context,
        &fixture(native_homes::SMALL_HOME_SET, 200),
        "bounded_ssa",
    )
    .unwrap();
    retain(&module);
    module.verify().unwrap();
    assert!(!module.print_to_string().to_string().contains("volatile"));
    let module = parse_ir_text(
        &context,
        &fixture(native_homes::SMALL_HOME_SET + 1, 200),
        "large_homes",
    )
    .unwrap();
    retain(&module);
    module.verify().unwrap();
    assert!(module.print_to_string().to_string().contains("volatile"));
}

#[test]
fn acyclic_single_block_homes_release_their_last_value() {
    let context = Context::create();
    let module = parse_ir_text(
        &context,
        &fixture(native_homes::SMALL_HOME_SET + 1, 200),
        "last_home_access",
    )
    .unwrap();
    retain(&module);
    module.verify().unwrap();
    let text = module.print_to_string().to_string();
    assert_eq!(
        text.matches("store volatile ptr addrspace(1) null").count(),
        native_homes::SMALL_HOME_SET + 1
    );
}

#[test]
fn cyclic_homes_do_not_clear_a_value_needed_on_the_backedge() {
    let context = Context::create();
    let mut ir = fixture(native_homes::SMALL_HOME_SET + 1, 200);
    ir = ir.replace("entry:\n", "entry:\n  %again = icmp ne i64 %arg, 0\n");
    ir = ir.replacen(
        "  call void @collect()\n",
        "  br label %loop\nloop:\n  call void @collect()\n",
        1,
    );
    ir = ir.replace(
        "  ret i64 %sum8",
        "  br i1 %again, label %loop, label %exit\nexit:\n  ret i64 %sum8",
    );
    let module = parse_ir_text(&context, &ir, "cyclic_home_access").unwrap();
    retain(&module);
    module.verify().unwrap();
    assert!(!module
        .print_to_string()
        .to_string()
        .contains("store volatile ptr addrspace(1) null"));
}

#[test]
fn forward_multi_block_homes_release_after_their_last_component() {
    let context = Context::create();
    let ir = fixture(native_homes::SMALL_HOME_SET + 1, 200).replacen(
        "  call void @collect()\n",
        "  br label %body\nbody:\n  call void @collect()\n",
        1,
    );
    let module = parse_ir_text(&context, &ir, "split_home_access").unwrap();
    retain(&module);
    module.verify().unwrap();
    assert_eq!(
        module
            .print_to_string()
            .to_string()
            .matches("store volatile ptr addrspace(1) null")
            .count(),
        native_homes::SMALL_HOME_SET + 1
    );
}

#[test]
fn releasing_a_home_keeps_the_loaded_ssa_value_rooted() {
    let context = Context::create();
    let ir = fixture(native_homes::SMALL_HOME_SET + 1, 200).replace(
        "  ret i64 %sum8",
        "  call void @collect()\n  %final = ptrtoint ptr addrspace(1) %v8 to i64\n  ret i64 %final",
    );
    let module = parse_ir_text(&context, &ir, "loaded_root_after_release").unwrap();
    retain(&module);
    global_init(&[]);
    let triple = TargetTriple::create("x86_64-unknown-linux-gnu");
    let tm = Target::from_triple(&triple)
        .unwrap()
        .create_target_machine(
            &triple,
            "",
            "",
            OptimizationLevel::Default,
            RelocMode::PIC,
            CodeModel::Default,
        )
        .unwrap();
    module
        .run_passes(STATEPOINT_REWRITE_PASSES, &tm, PassBuilderOptions::create())
        .unwrap();
    module.verify().unwrap();
    let text = module.print_to_string().to_string();
    assert!(text.contains("store volatile ptr addrspace(1) null"));
    assert!(
        text.contains(
            "%v8.relocated = call coldcc ptr addrspace(1) @llvm.experimental.gc.relocate"
        ),
        "loaded value must remain rooted after its memory home is cleared:\n{text}"
    );
}

#[test]
fn escaped_home_addresses_are_not_released() {
    let context = Context::create();
    let ir = fixture(native_homes::SMALL_HOME_SET + 1, 200).replacen(
        "  call void @collect()",
        "  %sink = alloca ptr\n  store ptr %slot8, ptr %sink\n  call void @collect()",
        1,
    );
    let module = parse_ir_text(&context, &ir, "escaped_home").unwrap();
    retain(&module);
    module.verify().unwrap();
    assert_eq!(
        module
            .print_to_string()
            .to_string()
            .matches("store volatile ptr addrspace(1) null")
            .count(),
        native_homes::SMALL_HOME_SET
    );
}

#[test]
fn component_rank_overrides_basic_block_source_order() {
    let context = Context::create();
    let mut ir = fixture(native_homes::SMALL_HOME_SET + 1, 200);
    ir = ir.replacen("  call void @collect()", "  br label %early\nlate:\n  %latevalue = load ptr addrspace(1), ptr %slot8\n  %latebits = ptrtoint ptr addrspace(1) %latevalue to i64\n  ret i64 %latebits\nearly:\n  call void @collect()", 1);
    ir = ir.replace("  ret i64 %sum8", "  br label %late");
    let module = parse_ir_text(&context, &ir, "block_order").unwrap();
    retain(&module);
    module.verify().unwrap();
    let text = module.print_to_string().to_string();
    let late = text
        .split("late:")
        .nth(1)
        .unwrap()
        .split("early:")
        .next()
        .unwrap();
    assert!(late.contains("store volatile ptr addrspace(1) null"));
    let early = text.split("early:").nth(1).unwrap();
    assert_eq!(
        early
            .matches("store volatile ptr addrspace(1) null")
            .count(),
        native_homes::SMALL_HOME_SET
    );
}

#[test]
fn explicit_release_does_not_gain_a_duplicate_volatile_store() {
    let context = Context::create();
    let clears = (0..native_homes::SMALL_HOME_SET + 1)
        .map(|i| format!("  store ptr addrspace(1) null, ptr %slot{i}\n"))
        .collect::<String>();
    let ir = fixture(native_homes::SMALL_HOME_SET + 1, 200)
        .replace("  ret i64 %sum8", &(clears + "  ret i64 %sum8"));
    let module = parse_ir_text(&context, &ir, "explicit_release").unwrap();
    retain(&module);
    module.verify().unwrap();
    assert_eq!(
        module
            .print_to_string()
            .to_string()
            .matches("store volatile ptr addrspace(1) null")
            .count(),
        native_homes::SMALL_HOME_SET + 1
    );
}
