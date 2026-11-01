
use perry_hir::types::Type;
use perry_hir::{Expr, Function, Module as HirModule, Stmt, UpdateOp};

/// Compile a one-closure module to LLVM IR text.
///
/// `outer() { let x; const f = () => { x = 2; x++; return x; }; return f; }`
/// — the smallest shape that exercises all three capture accessors: a read
/// (`js_closure_get_capture_bits`), a write
/// (`js_closure_set_capture_bits`), and a read-modify-write whose coercion
/// (`js_to_numeric`) can run a user `valueOf` and therefore collect between
/// the read and the write.
fn one_capture_closure_ir() -> String {
    let closure = Expr::Closure {
        func_id: 1,
        params: Vec::new(),
        return_type: Type::Any,
        body: vec![
            Stmt::Expr(Expr::LocalSet(0, Box::new(Expr::Number(2.0)))),
            Stmt::Expr(Expr::Update {
                id: 0,
                op: UpdateOp::Increment,
                prefix: false,
            }),
            Stmt::Return(Some(Expr::LocalGet(0))),
        ],
        captures: vec![0],
        mutable_captures: vec![0],
        captures_this: false,
        captures_new_target: false,
        enclosing_class: None,
        is_arrow: true,
        is_async: false,
        is_generator: false,
        is_strict: true,
    };
    let mut hir = HirModule::new("closure_self_root_test");
    hir.functions.push(Function {
        id: 0,
        name: "outer".to_string(),
        type_params: Vec::new(),
        params: Vec::new(),
        return_type: Type::Any,
        body: vec![
            Stmt::Let {
                id: 0,
                name: "x".to_string(),
                ty: Type::Any,
                mutable: true,
                init: None,
            },
            Stmt::Let {
                id: 2,
                name: "f".to_string(),
                ty: Type::Any,
                mutable: false,
                init: Some(closure),
            },
            Stmt::Return(Some(Expr::LocalGet(2))),
        ],
        is_async: false,
        is_generator: false,
        is_strict: true,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
        was_plain_async: false,
        was_unrolled: false,
    });
    let opts = crate::CompileOptions {
        emit_ir_only: true,
        ..Default::default()
    };
    let bytes = crate::compile_module(&hir, opts).expect("closure test module compiles");
    String::from_utf8(bytes).expect("LLVM IR is UTF-8")
}

/// Same module, but the captured id has **no declaration site** in the
/// enclosing function, so `collect_boxed_vars` does not box it and the
/// closure body takes the *unboxed* capture accessors — the pair whose
/// writer (`js_closure_set_capture_bits`) does no bounds check at all.
fn unboxed_capture_update_ir() -> String {
    let closure = Expr::Closure {
        func_id: 1,
        params: Vec::new(),
        return_type: Type::Any,
        body: vec![
            Stmt::Expr(Expr::Update {
                id: 0,
                op: UpdateOp::Increment,
                prefix: false,
            }),
            Stmt::Return(Some(Expr::LocalGet(0))),
        ],
        captures: vec![0],
        mutable_captures: Vec::new(),
        captures_this: false,
        captures_new_target: false,
        enclosing_class: None,
        is_arrow: true,
        is_async: false,
        is_generator: false,
        is_strict: true,
    };
    let mut hir = HirModule::new("closure_unboxed_update_test");
    hir.functions.push(Function {
        id: 0,
        name: "outer".to_string(),
        type_params: Vec::new(),
        params: Vec::new(),
        return_type: Type::Any,
        body: vec![
            Stmt::Let {
                id: 2,
                name: "f".to_string(),
                ty: Type::Any,
                mutable: false,
                init: Some(closure),
            },
            Stmt::Return(Some(Expr::LocalGet(2))),
        ],
        is_async: false,
        is_generator: false,
        is_strict: true,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
        was_plain_async: false,
        was_unrolled: false,
    });
    let opts = crate::CompileOptions {
        emit_ir_only: true,
        ..Default::default()
    };
    let bytes = crate::compile_module(&hir, opts).expect("unboxed-capture module compiles");
    String::from_utf8(bytes).expect("LLVM IR is UTF-8")
}

/// #7055: the closure's own `%this_closure` pointer must be a PRECISE GC
/// root, and capture accesses must read it back from that root.
///
/// `%this_closure` is an LLVM parameter, i.e. a register the collector
/// cannot see. The shipped moving young collection runs at loop back-edge
/// polls with precise roots and no conservative native-stack scan, so a
/// closure relocated while its own body runs leaves that register pointing
/// into from-space — which is reset and reused before the body's next
/// capture access. `js_closure_get_capture_bits` then reads a foreign
/// object's `capture_count`, judges the index out of range and returns 0,
/// so every later boxed-capture read yields `undefined` and every write is
/// dropped.
///
/// Teeth: with the fix reverted the body reads captures straight off the
/// parameter and the `js_closure_get_capture_bits(i64 %this_closure`
/// assertion below fails.
#[test]
fn closure_body_roots_its_own_closure_pointer_and_reads_captures_through_it() {
    // This test asserts on the SHADOW-STACK lowering. Native roots are the
    // default now, so it has to say which lowering it is testing.
    let _shadow = crate::codegen::helpers::NativeRootsPin::shadow();
    let ir = one_capture_closure_ir();
    // The public `perry_closure_*` symbol can be a typed trampoline over a
    // straight-line `$typed_f64` clone; the real body is the one that
    // carries a shadow frame. (The typed clone lowers arithmetic-only,
    // loop-free, call-free statements — `lower_typed_f64_body_*` bails on
    // anything else — so it contains no safepoint and its `%this_closure`
    // register cannot go stale.)
    let body = ir
        .split("define ")
        .find(|f| {
            let name_starts_here = f.starts_with("double @perry_closure_")
                || f.starts_with("internal double @perry_closure_");
            name_starts_here && f.contains("@js_shadow_frame_enter")
        })
        .unwrap_or_else(|| panic!("no shadow-framed closure body in IR:\n{ir}"));

    // The prologue stores `%this_closure` into an `i64` root home and binds
    // that alloca to a shadow-stack slot, so the collector marks and
    // rewrites it. The home holds the bare pointer: shadow frames decode
    // bare and NaN-boxed words alike, and the native-root lowering encodes
    // an `i64` home itself, so codegen must not box it on the way in.
    assert!(
        body.contains("store i64 %this_closure, ptr "),
        "closure prologue must store %this_closure into its rooted slot; body:\n{body}"
    );
    assert!(
        !body.contains("or i64 %this_closure,"),
        "the closure-pointer home holds the bare pointer, never a re-encoded \
             word; body:\n{body}"
    );
    assert!(
        body.contains("@js_shadow_slot_bind"),
        "closure prologue must bind the closure-pointer slot as a GC root; body:\n{body}"
    );

    // And no capture access may use the raw (unrooted) parameter.
    assert!(
        !body.contains("@js_closure_get_capture_bits(i64 %this_closure"),
        "capture reads must reload the closure pointer from its rooted \
             slot, not use the %this_closure register; body:\n{body}"
    );
    // Belt and braces, and honestly labelled: this fixture does NOT emit the
    // unboxed capture writer. `collect_boxed_vars` boxes every declared
    // local a nested closure mutates (and `collect_boxed_param_ids` covers
    // params), so a mutating capture always lowers to `get_capture_bits` +
    // `js_box_set_bits`, never to `js_closure_set_capture_bits`. This
    // assertion guards the unboxed writer against a future change to that
    // boxing rule; it is not evidence about today's output.
    assert!(
        !body.contains("@js_closure_set_capture_bits(i64 %this_closure"),
        "capture writes must reload the closure pointer from its rooted \
             slot, not use the %this_closure register; body:\n{body}"
    );

    // Not vacuous: the fixture really does read, write and read-modify-write
    // the capture, and the read-modify-write really does emit the ToNumeric
    // coercion — the collect-capable call the reload below has to survive.
    let accesses = body.matches("@js_closure_get_capture_bits(").count();
    assert!(
        accesses >= 2,
        "fixture must access the capture more than once (read + write), or \
             the per-access reload assertion below is vacuous; body:\n{body}"
    );
    assert!(
        body.contains("@js_box_set_bits"),
        "fixture must WRITE the capture, not only read it; body:\n{body}"
    );
    assert!(
        body.contains("@js_to_numeric"),
        "the captured `x++` must emit its ToNumeric coercion; body:\n{body}"
    );

    // The invariant, stated positively: EVERY capture access re-reads the
    // rooted slot, so no access can be reached through a pointer loaded
    // before an intervening collection. Pre-fix this count is 0.
    let slot = {
        let bind = body
            .find("@js_shadow_slot_bind(")
            .map(|i| &body[i..])
            .and_then(|t| t.split_once("ptr "))
            .map(|(_, rest)| rest)
            .unwrap_or_else(|| panic!("no shadow-slot bind in body:\n{body}"));
        bind.split(')').next().expect("bind operand").to_string()
    };
    let reload = format!("load i64, ptr {slot}");
    assert!(
        body.matches(reload.as_str()).count() >= accesses,
        "every one of the {accesses} capture accesses must reload the \
             closure pointer from its rooted slot {slot}; body:\n{body}"
    );

    // #7055 (CodeRabbit): the sharp case — `js_to_numeric` runs a user
    // `valueOf`, i.e. arbitrary JS that can reach a loop poll and relocate
    // this closure. The capture access that FOLLOWS it must come from a
    // fresh load, never from a pointer materialized before the call.
    let coerce_at = body.find("@js_to_numeric").expect("coercion call");
    let next_access = body[coerce_at..]
        .find("@js_closure_get_capture_bits(")
        .map(|i| coerce_at + i)
        .unwrap_or_else(|| {
            panic!("fixture must access the capture after the coercion; body:\n{body}")
        });
    assert!(
        body[coerce_at..next_access].contains(reload.as_str()),
        "a capture access after `js_to_numeric` (which can run a user \
             `valueOf` and relocate the closure) must re-read the closure \
             pointer from its rooted slot {slot}; body:\n{body}"
    );
}

/// #7055 (CodeRabbit 🟠 Major): `js_to_numeric` runs a user `valueOf` —
/// arbitrary JS that can reach a `js_gc_loop_safepoint` and relocate this
/// closure — and the *unboxed* capture writer
/// `js_closure_set_capture_bits` does NOT validate its pointer (unlike the
/// reader, which bounds-checks and returns 0). Writing through a closure
/// pointer materialized before the coercion would therefore store into
/// whatever the mutator has since placed at that recycled from-space
/// address. The write must use a pointer re-read from the rooted slot.
///
/// Reachability, stated plainly: today `collect_boxed_vars` boxes every
/// declared local a nested closure mutates and `collect_boxed_param_ids`
/// covers params, so TypeScript cannot currently produce a mutating
/// *unboxed* capture — the fixture reaches this arm by omitting the
/// declaration site. The arm is live code with a memory-corrupting failure
/// mode if that boxing rule ever narrows, which is what this pins.
#[test]
fn unboxed_capture_write_reloads_the_closure_pointer_after_the_coercion() {
    // This test asserts on the SHADOW-STACK lowering. Native roots are the
    // default now, so it has to say which lowering it is testing.
    let _shadow = crate::codegen::helpers::NativeRootsPin::shadow();
    let ir = unboxed_capture_update_ir();
    let body = ir
        .split("define ")
        .find(|f| {
            let name_starts_here = f.starts_with("double @perry_closure_")
                || f.starts_with("internal double @perry_closure_");
            name_starts_here && f.contains("@js_closure_set_capture_bits(")
        })
        .unwrap_or_else(|| panic!("no unboxed capture write in IR:\n{ir}"));

    // Non-vacuous: this really is the unboxed arm, and the coercion really
    // is emitted between the capture read and the capture write.
    assert!(
        !body.contains("@js_box_set_bits"),
        "fixture must take the UNBOXED capture arm; body:\n{body}"
    );
    let coerce_at = body
        .find("@js_to_numeric")
        .unwrap_or_else(|| panic!("captured `x++` must emit its coercion; body:\n{body}"));
    let write_at = body
        .find("@js_closure_set_capture_bits(")
        .expect("capture write");
    assert!(
        coerce_at < write_at,
        "fixture must coerce before it writes; body:\n{body}"
    );

    let slot = {
        let bind = body
            .find("@js_shadow_slot_bind(")
            .map(|i| &body[i..])
            .and_then(|t| t.split_once("ptr "))
            .map(|(_, rest)| rest)
            .unwrap_or_else(|| panic!("no shadow-slot bind in body:\n{body}"));
        bind.split(')').next().expect("bind operand").to_string()
    };
    assert!(
        body[coerce_at..write_at].contains(&format!("load i64, ptr {slot}")),
        "the unboxed capture write must re-read the closure pointer from \
             its rooted slot {slot} after `js_to_numeric`; body:\n{body}"
    );
}
