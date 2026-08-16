//! The harness's own coverage.
//!
//! A parser that silently returns "no safepoints" or "no live values" would
//! make every negative assertion in [`super::mechanics`] pass for the wrong
//! reason, and nothing downstream could tell. So the parser is pinned on
//! fixtures whose answers are known by inspection, and the two "this subject
//! does not exist" paths are asserted to PANIC rather than to return empty.

use super::*;

/// One statepoint with a two-value live set and one with none — the exact two
/// shapes every mechanic below distinguishes between.
const FIXTURE: &str = r#"define double @probe() gc "statepoint-example" {
entry.0:
  %tok = call token (i64, i32, ptr, i32, i32, ...) @llvm.experimental.gc.statepoint.p0(i64 2882400000, i32 0, ptr elementtype(i64 (i32)) @js_map_alloc, i32 1, i32 0, i32 8, i32 0, i32 0)
  %tok2 = call token (i64, i32, ptr, i32, i32, ...) @llvm.experimental.gc.statepoint.p0(i64 2882400000, i32 0, ptr elementtype(i64 (i32)) @js_array_alloc, i32 1, i32 0, i32 0, i32 0, i32 0) [ "gc-live"(ptr addrspace(1) %a, ptr addrspace(1) %b) ]
  ret double 0.0
}
"#;

fn fixture_points() -> Vec<Statepoint> {
    function_slice(FIXTURE, "probe")
        .lines()
        .filter(|l| l.contains("llvm.experimental.gc.statepoint"))
        .map(super::parse_statepoint)
        .collect()
}

#[test]
fn the_statepoint_parser_reads_callee_and_live_set() {
    let points = fixture_points();
    assert_eq!(points.len(), 2, "{points:?}");
    assert_eq!(
        points[0],
        Statepoint {
            callee: "js_map_alloc".to_string(),
            live: Vec::new(),
        },
        "a statepoint with no bundle must read as zero live values, not as \
         unparsed"
    );
    assert_eq!(
        points[1],
        Statepoint {
            callee: "js_array_alloc".to_string(),
            live: vec!["%a".to_string(), "%b".to_string()],
        },
        "every operand of the `gc-live` bundle must be reported"
    );
}

/// The parser must not report a live set for a call that has none *because it
/// failed to find the bundle*. Deleting the bundle text is the one edit that
/// tells those apart.
#[test]
fn a_deleted_live_bundle_changes_the_answer() {
    let stripped = FIXTURE.replace(
        " [ \"gc-live\"(ptr addrspace(1) %a, ptr addrspace(1) %b) ]",
        "",
    );
    let points: Vec<Statepoint> = function_slice(&stripped, "probe")
        .lines()
        .filter(|l| l.contains("llvm.experimental.gc.statepoint"))
        .map(super::parse_statepoint)
        .collect();
    assert_eq!(points.len(), 2);
    assert!(
        points.iter().all(|sp| sp.live.is_empty()),
        "{points:?} — and the unmodified fixture must NOT read this way"
    );
    assert_eq!(
        fixture_points()[1].live.len(),
        2,
        "control: the unmodified fixture reports two live values, so the empty \
         result above is the edit and not the parser"
    );
}

/// `Statepoints::at` is the guard against a mechanic silently asserting about
/// the empty set. It must panic, not return `[]`.
#[test]
#[should_panic(expected = "has no subject")]
fn asking_about_an_absent_callee_is_a_failure_not_an_empty_answer() {
    let points = Statepoints {
        function: "probe".to_string(),
        points: fixture_points(),
    };
    points.at("js_closure_call1");
}

/// Same contract on the map side: a function the collector will find no roots
/// in must not read as "this function has zero roots".
#[test]
#[should_panic(expected = "no stack-map entry")]
fn a_function_missing_from_the_map_is_a_failure() {
    let target = NATIVE_TARGETS[0];
    // A module that DOES produce a map, so the panic under test is "this
    // function is absent" rather than "there is no map at all" — the two
    // failures this seam exists to keep apart.
    let module = probe_module(
        "selftest_missing.ts",
        vec![
            let_stmt(1, "a", Expr::MapNew),
            Stmt::Return(Some(Expr::LocalGet(1))),
        ],
    );
    let _pin = NativeRootsPin::native();
    let ir = native_ir(&module, target, false);
    let asm = assembly_for(&ir, target);
    map_records_for(&asm, target, "perry_fn_no_such_function");
}

/// End-to-end canary: the pipeline this module asserts through must produce
/// safepoints AND a decodable map for a trivial allocating program on both
/// shipped targets. If this goes red, every other test here is measuring
/// nothing regardless of what it reports.
#[test]
fn the_pipeline_produces_safepoints_and_a_map_on_every_shipped_target() {
    for target in NATIVE_TARGETS {
        let module = probe_module(
            "selftest_canary.ts",
            vec![
                let_stmt(1, "a", Expr::MapNew),
                let_stmt(2, "b", Expr::MapNew),
                Stmt::Return(Some(Expr::LocalGet(1))),
            ],
        );
        let _pin = NativeRootsPin::native();
        let ir = native_ir(&module, target, false);
        let symbol = probe_body_symbol(&ir, "selftest_canary.ts");
        let fn_ir = function_slice(&ir, &symbol);

        assert!(
            fn_ir.contains("gc \"statepoint-example\""),
            "[{target}] a function with root slots must carry the GC strategy, \
             or RS4GC skips it entirely:\n{fn_ir}"
        );
        assert_eq!(
            root_allocas(fn_ir),
            3,
            "[{target}] two heap locals plus the unchecked generic-ABI parameter, three root slots:\n{fn_ir}"
        );

        let points = statepoints_of(&ir, target, &symbol);
        assert!(
            points.len() >= 2,
            "[{target}] expected a safepoint per allocation, got {}",
            points.len()
        );

        let asm = assembly_for(&ir, target);
        let records = map_records_for(&asm, target, &symbol);
        assert!(
            records.len() >= 2,
            "[{target}] the compact map must carry a record per safepoint, got \
             {}",
            records.len()
        );
    }
}

/// **`mem2reg` promoting every root alloca is load-bearing, not incidental.**
///
/// RS4GC tracks `addrspace(1)` SSA values; it does not scan allocas. A root
/// slot that survives promotion is therefore a slot whose contents the
/// collector never relocates — the native-roots analogue of #7184's
/// silently-bounds-checked `js_shadow_slot_bind`, and just as invisible: the
/// IR still says the value was rooted.
///
/// **Sabotage** — `function/precise_roots.rs`, each root alloca's address
/// passed to a `gc-leaf-function` call right after its definition, so it
/// escapes and `mem2reg` must leave it in memory: RED, two `alloca ptr
/// addrspace(1)` survived the rewrite. Nothing else in the pipeline complains
/// about that IR — it verifies, it codegens, and it ships a frame whose roots
/// the collector cannot follow.
#[test]
fn no_root_alloca_survives_the_statepoint_rewrite() {
    for target in NATIVE_TARGETS {
        let module = probe_module(
            "selftest_promotion.ts",
            vec![
                let_stmt(1, "a", Expr::MapNew),
                let_stmt(2, "b", Expr::MapNew),
                console_log(vec![Expr::LocalGet(1), Expr::LocalGet(2)]),
                Stmt::Return(Some(Expr::LocalGet(1))),
            ],
        );
        let _pin = NativeRootsPin::native();
        let ir = native_ir(&module, target, false);
        let symbol = probe_body_symbol(&ir, "selftest_promotion.ts");
        assert!(
            root_allocas(function_slice(&ir, &symbol)) >= 2,
            "[{target}] control: codegen must have asked for root slots here"
        );

        let rewritten = crate::inprocess::statepoint_rewritten_ir(&ir, target, "promotion")
            .unwrap_or_else(|e| panic!("[{target}] statepoint rewrite failed: {e:#}"));
        let body = function_slice(&rewritten, &symbol);
        assert_eq!(
            root_allocas(body),
            0,
            "[{target}] a root alloca survived mem2reg. RS4GC relocates SSA \
             values, not memory, so this slot's contents are invisible to the \
             collector — the value reads as rooted and is not:\n{body}"
        );
    }
}
