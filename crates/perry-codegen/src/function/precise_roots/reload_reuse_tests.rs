use super::{reuse as reuse_root_reloads, switch_allows};

fn reload(x: &str, root: &str) -> String {
    format!(
        "  {x}.rs4p = load ptr addrspace(1), ptr {root}\n  {x}.rs4o = call i64 asm \"\", \"=r,0\"(ptr addrspace(1) {x}.rs4p) \"gc-leaf-function\"\n  {x} = bitcast i64 {x}.rs4o to double\n"
    )
}

fn func(body: &str) -> String {
    format!("define double @f(double %a) gc \"statepoint-example\" {{\n{body}}}\n")
}

fn reused(ir: &str, x: &str) -> bool {
    ir.contains(&format!("  {x}.rs4o = add i64 "))
}

fn launders(ir: &str) -> usize {
    ir.matches("call i64 asm \"\", \"=r,0\"").count()
}

#[test]
fn a_second_read_with_only_leaf_calls_between_reuses_the_first() {
    let ir = func(&format!(
        "entry:\n  %r1 = alloca ptr addrspace(1)\n{}  call void @leaf(double %v1) \"gc-leaf-function\"\n  %t = call i32 @llvm.aarch64.fjcvtzs(double %a)\n{}  ret double %v2\n",
        reload("%v1", "%r1"),
        reload("%v2", "%r1")
    ));
    let out = reuse_root_reloads(&ir);
    assert!(reused(&out, "%v2"), "{out}");
    assert!(out.contains("  %v2.rs4o = add i64 %v1.rs4o, 0\n"), "{out}");
    assert!(
        out.contains("  %v2 = bitcast i64 %v2.rs4o to double\n"),
        "{out}"
    );
    assert_eq!(launders(&out), 1, "{out}");
}

#[test]
fn a_collecting_call_between_two_reads_keeps_the_barrier() {
    let ir = func(&format!(
        "entry:\n{}  call void @js_gc_loop_safepoint()\n{}  ret double %v2\n",
        reload("%v1", "%r1"),
        reload("%v2", "%r1")
    ));
    let out = reuse_root_reloads(&ir);
    assert!(!reused(&out, "%v2"), "{out}");
    assert_eq!(launders(&out), 2);
}

#[test]
fn a_store_to_the_slot_between_two_reads_keeps_the_barrier() {
    let ir = func(&format!(
        "entry:\n{}  store ptr addrspace(1) %p, ptr %r1\n{}  ret double %v2\n",
        reload("%v1", "%r1"),
        reload("%v2", "%r1")
    ));
    assert!(!reused(&reuse_root_reloads(&ir), "%v2"));
}

#[test]
fn other_slots_and_their_stores_are_independent() {
    let ir = func(&format!(
        "entry:\n{}  store ptr addrspace(1) %p, ptr %r2\n{}  ret double %v2\n",
        reload("%v1", "%r1"),
        reload("%v2", "%r1")
    ));
    assert!(reused(&reuse_root_reloads(&ir), "%v2"));
}

#[test]
fn a_join_reuses_only_when_no_incoming_path_can_collect() {
    let diamond = |else_call: &str| {
        func(&format!(
            "entry:\n{}  br i1 %c, label %then, label %else\nthen:\n  br label %join\nelse:\n  {else_call}\n  br label %join\njoin:\n{}  ret double %v2\n",
            reload("%v1", "%r1"),
            reload("%v2", "%r1")
        ))
    };
    let leaf = reuse_root_reloads(&diamond("call void @leaf() \"gc-leaf-function\""));
    assert!(reused(&leaf, "%v2"), "{leaf}");
    let collect = reuse_root_reloads(&diamond("call void @js_gc_loop_safepoint()"));
    assert!(!reused(&collect, "%v2"), "{collect}");
    // An invoke that is not leaf collects on both edges.
    let inv = func(&format!(
        "entry:\n{}  invoke void @may_throw() to label %join unwind label %pad\npad:\n  %lp = landingpad {{ ptr, i32 }} cleanup\n  br label %join\njoin:\n{}  ret double %v2\n",
        reload("%v1", "%r1"),
        reload("%v2", "%r1")
    ));
    assert!(!reused(&reuse_root_reloads(&inv), "%v2"));
}

#[test]
fn a_value_read_in_one_branch_is_not_available_at_the_join() {
    let ir = func(&format!(
        "entry:\n  br i1 %c, label %then, label %join\nthen:\n{}  br label %join\njoin:\n{}  ret double %v2\n",
        reload("%v1", "%r1"),
        reload("%v2", "%r1")
    ));
    assert!(!reused(&reuse_root_reloads(&ir), "%v2"));
}

/// The loop shape the pass exists for: reads in one iteration share a
/// launder, but the poll on the back edge forces a fresh one each iteration,
/// and a read in the header is never served by the previous iteration's.
#[test]
fn a_polled_back_edge_relaunders_every_iteration() {
    let ir = func(&format!(
        "entry:\n  br label %head\nhead:\n{}  br i1 %c, label %body, label %exit\nbody:\n{}{}  %armed = load volatile i32, ptr @PERRY_GC_POLL_ARMED\n  %on = icmp ne i32 %armed, 0\n  br i1 %on, label %poll, label %latch\npoll:\n  call void @js_gc_loop_safepoint()\n  br label %latch\nlatch:\n  br label %head\nexit:\n{}  ret double %v4\n",
        reload("%v1", "%r1"),
        reload("%v2", "%r1"),
        reload("%v3", "%r1"),
        reload("%v4", "%r1"),
    ));
    let out = reuse_root_reloads(&ir);
    assert!(!reused(&out, "%v1"), "header read must relaunder: {out}");
    assert!(reused(&out, "%v2"), "{out}");
    assert!(reused(&out, "%v3"), "{out}");
    assert!(
        reused(&out, "%v4"),
        "exit is dominated by the header read: {out}"
    );
    // Without the poll edge the header still meets the entry's empty state.
    assert_eq!(launders(&out), 1);
}

#[test]
fn the_switch_turns_the_pass_off() {
    assert!(switch_allows(None));
    assert!(switch_allows(Some("1")));
    for off in ["0", "off", "false"] {
        assert!(!switch_allows(Some(off)));
    }
}

fn reroot(x: &str, root: &str, n: u32) -> String {
    format!(
        "  %rs4gc.b{n} = bitcast double {x} to i64\n  %rs4gc.s{n} = inttoptr i64 %rs4gc.b{n} to ptr addrspace(1)\n  store ptr addrspace(1) %rs4gc.s{n}, ptr {root}\n"
    )
}

#[test]
fn a_re_root_of_a_re_read_forwards_its_bits_to_the_next_read() {
    let ir = func(&format!(
        "entry:\n{}{}{}  ret double %v2\n",
        reload("%v1", "%r1"),
        reroot("%v1", "%tmp", 1),
        reload("%v2", "%tmp")
    ));
    let out = reuse_root_reloads(&ir);
    assert!(out.contains("  %v2.rs4o = add i64 %v1.rs4o, 0\n"), "{out}");
    // A safepoint after the re-root relocates the slot: re-read it.
    let ir = func(&format!(
        "entry:\n{}{}  call void @js_gc_loop_safepoint()\n{}  ret double %v2\n",
        reload("%v1", "%r1"),
        reroot("%v1", "%tmp", 1),
        reload("%v2", "%tmp")
    ));
    assert!(!reused(&reuse_root_reloads(&ir), "%v2"));
    // A store of some other value is a kill, not a forward.
    let ir = func(&format!(
        "entry:\n{}  %rs4gc.s9 = inttoptr i64 %fresh to ptr addrspace(1)\n  store ptr addrspace(1) %rs4gc.s9, ptr %tmp\n{}  ret double %v2\n",
        reload("%v1", "%tmp"),
        reload("%v2", "%tmp")
    ));
    assert!(!reused(&reuse_root_reloads(&ir), "%v2"));
}

#[test]
fn an_unreachable_dead_arm_does_not_weaken_its_join() {
    // `ptr_shape_set` folds its finite check to an unconditional branch; the
    // fallback block stays in the text with no predecessor.
    let ir = func(&format!(
        "entry:\n{}  br label %store\nstore:\n  br label %merge\ndowngrade:\n  call void @js_class_field_set_fallback()\n  br label %merge\nmerge:\n{}  ret double %v2\n",
        reload("%v1", "%r1"),
        reload("%v2", "%r1")
    ));
    let out = reuse_root_reloads(&ir);
    assert!(reused(&out, "%v2"), "{out}");
    // The same arm, reachable, kills.
    let ir = ir.replace(
        "  br label %store\n",
        "  br i1 %c, label %store, label %downgrade\n",
    );
    assert!(!reused(&reuse_root_reloads(&ir), "%v2"));
}
