//! #8175 — recursion-participating specialized clones take LLVM's
//! `preserve_none` calling convention.
//!
//! The measured cause (fib40): a param-derived value live across a call is
//! materialized into a callee-saved register in the entry block, which pins
//! the CSR save/restore there and defeats shrink-wrapping — ~45% of wall
//! time spent on a frame the ~165M leaf invocations never use. With
//! `preserve_nonecc` there are no CSRs to pin, so the frame sinks into the
//! recursive path and the leaf runs frameless.
//!
//! The gate below is a LIVENESS gate, not just a rendering check: a future
//! change that silently stops applying the convention (or stops emitting the
//! clone at all) must go red here instead of quietly reverting the win. And
//! because a call site whose convention disagrees with its callee is UB —
//! not a verifier error — `assert_preserve_none_consistency` scans the whole
//! module for define/call agreement in both directions.

use crate::{compile_module, CompileOptions};
use perry_hir::types::Type;
use perry_hir::{BinaryOp, CompareOp, Expr, Function, Module, Param, Stmt};

const CLONE_DEFINE: &str =
    "define internal preserve_nonecc double @perry_fn_spec_preserve_none_ts__f$spec_i32(i32";
const CLONE_SYMBOL: &str = "perry_fn_spec_preserve_none_ts__f$spec_i32";

fn number_param(id: u32, name: &str) -> Param {
    Param {
        id,
        name: name.to_string(),
        ty: Type::Number,
        default: None,
        decorators: Vec::new(),
        is_rest: false,
        arguments_object: None,
    }
}

fn call(fid: u32, args: Vec<Expr>) -> Expr {
    Expr::Call {
        callee: Box::new(Expr::FuncRef(fid)),
        args,
        type_args: Vec::new(),
        byte_offset: 0,
    }
}

fn sub(id: u32, n: i64) -> Expr {
    Expr::Binary {
        op: BinaryOp::Sub,
        left: Box::new(Expr::LocalGet(id)),
        right: Box::new(Expr::Integer(n)),
    }
}

fn plain_function(id: u32, name: &str, param: u32, body: Vec<Stmt>) -> Function {
    Function {
        id,
        name: name.to_string(),
        type_params: Vec::new(),
        params: vec![number_param(param, "n")],
        return_type: Type::Number,
        body,
        is_async: false,
        is_generator: false,
        is_strict: true,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
        was_plain_async: false,
        was_unrolled: false,
    }
}

/// `function f(n: number): number { return n < 2 ? n : f(n-1) + f(n-2); }`
/// plus a module-init `f(40)` — the fib shape the campaign measured.
fn recursive_module() -> Module {
    let f = plain_function(
        1,
        "f",
        10,
        vec![Stmt::Return(Some(Expr::Conditional {
            condition: Box::new(Expr::Compare {
                op: CompareOp::Lt,
                left: Box::new(Expr::LocalGet(10)),
                right: Box::new(Expr::Integer(2)),
            }),
            then_expr: Box::new(Expr::LocalGet(10)),
            else_expr: Box::new(Expr::Binary {
                op: BinaryOp::Add,
                left: Box::new(call(1, vec![sub(10, 1)])),
                right: Box::new(call(1, vec![sub(10, 2)])),
            }),
        }))],
    );
    let mut module = Module::new("spec_preserve_none.ts");
    module.functions.push(f);
    module
        .init
        .push(Stmt::Expr(call(1, vec![Expr::Integer(40)])));
    module
}

/// The same body SHAPE as the recursive fixture — guard, two derived call
/// edges, an i32-literal init site — but the edges target a sibling, so `f`
/// is on no cycle. The clone exists and must KEEP the default convention
/// (the boundary prologue a normal-CC caller pays is a pure pessimization
/// without a recursive tree under it).
fn non_recursive_module() -> Module {
    let f = plain_function(
        1,
        "f",
        10,
        vec![Stmt::Return(Some(Expr::Conditional {
            condition: Box::new(Expr::Compare {
                op: CompareOp::Lt,
                left: Box::new(Expr::LocalGet(10)),
                right: Box::new(Expr::Integer(2)),
            }),
            then_expr: Box::new(Expr::LocalGet(10)),
            else_expr: Box::new(Expr::Binary {
                op: BinaryOp::Add,
                left: Box::new(call(2, vec![sub(10, 1)])),
                right: Box::new(call(2, vec![sub(10, 2)])),
            }),
        }))],
    );
    let g = plain_function(2, "g", 20, vec![Stmt::Return(Some(Expr::LocalGet(20)))]);
    let mut module = Module::new("spec_preserve_none.ts");
    module.functions.push(f);
    module.functions.push(g);
    module
        .init
        .push(Stmt::Expr(call(1, vec![Expr::Integer(40)])));
    module
}

fn compile_ir_for(module: &Module, target: Option<&str>) -> String {
    let opts = CompileOptions {
        emit_ir_only: true,
        output_type: "executable".to_string(),
        target: target.map(str::to_string),
        ..Default::default()
    };
    String::from_utf8(compile_module(module, opts).expect("module compiles"))
        .expect("LLVM IR is UTF-8")
}

fn compile_ir(module: &Module) -> String {
    compile_ir_for(module, None)
}

/// The UB tripwire: for every `preserve_nonecc` define, every call/invoke of
/// that symbol must carry the token, and every call/invoke carrying the token
/// must target a symbol whose define carries it. Returns how many convention-
/// carrying call sites were seen so callers can assert the subject was live.
fn assert_preserve_none_consistency(ir: &str) -> usize {
    let mut cc_defines: Vec<String> = Vec::new();
    for line in ir.lines() {
        if line.starts_with("define ") && line.contains("preserve_nonecc ") {
            let name = line
                .split_once('@')
                .and_then(|(_, tail)| tail.split_once('('))
                .map(|(name, _)| name.to_string())
                .expect("define line names a symbol");
            cc_defines.push(name);
        }
    }
    let mut cc_call_sites = 0usize;
    for line in ir.lines() {
        if line.starts_with("define ") || line.starts_with("declare ") {
            continue;
        }
        let is_call_like = line.contains("call ") || line.contains("invoke ");
        if !is_call_like {
            continue;
        }
        let carries_token = line.contains("preserve_nonecc ");
        let targets_cc_define = cc_defines
            .iter()
            .any(|name| line.contains(&format!("@{name}(")));
        assert_eq!(
            carries_token, targets_cc_define,
            "call-site convention must agree with the callee's define \
             (a mismatch is UB, not a verifier error):\n{line}"
        );
        if carries_token {
            cc_call_sites += 1;
        }
    }
    cc_call_sites
}

#[test]
fn recursive_clone_takes_preserve_none_and_every_call_site_agrees() {
    let ir = compile_ir(&recursive_module());

    // Subject liveness first: the clone exists, with the convention.
    assert!(
        ir.contains(CLONE_DEFINE),
        "expected a preserve_nonecc raw-i32 clone:\n{ir}"
    );
    // Both recursive edges re-enter the clone (behind #8167's range test)
    // WITH the call-site convention, and so does the module-init literal
    // site — three convention-carrying call sites minimum. If the dispatch
    // tiers stop stamping the token, or the clone stops being recursive,
    // this goes red rather than quietly reverting the win.
    let cc_call_sites = assert_preserve_none_consistency(&ir);
    assert!(
        cc_call_sites >= 3,
        "expected the init site plus both recursive edges to carry the \
         convention, saw {cc_call_sites}:\n{ir}"
    );
    // The out-of-range fallback arm still reaches the boxed public symbol
    // with the DEFAULT convention — the permanent ABI is untouched.
    assert!(
        ir.contains("call double @perry_fn_spec_preserve_none_ts__f(double"),
        "boxed fallback must keep the default convention:\n{ir}"
    );
}

#[test]
fn a_non_recursive_clone_keeps_the_default_convention() {
    let ir = compile_ir(&non_recursive_module());
    assert!(
        ir.contains("__f$spec_i32(i32"),
        "the clone must still exist, or this asserts nothing:\n{ir}"
    );
    assert!(
        !ir.contains("preserve_nonecc"),
        "a non-recursive clone must not pay the boundary prologue:\n{ir}"
    );
}

#[test]
fn mutually_recursive_clones_take_the_convention_too() {
    // even/odd: f(n) = n == 0 ? 1 : g(n - 1); g(n) = n == 0 ? 0 : f(n - 1);
    // both with literal init sites so both get raw-i32 plans. Recursion
    // participation is the SCC, not just a self-edge.
    let f = plain_function(
        1,
        "f",
        10,
        vec![Stmt::Return(Some(Expr::Conditional {
            condition: Box::new(Expr::Compare {
                op: CompareOp::Eq,
                left: Box::new(Expr::LocalGet(10)),
                right: Box::new(Expr::Integer(0)),
            }),
            then_expr: Box::new(Expr::Integer(1)),
            else_expr: Box::new(call(2, vec![sub(10, 1)])),
        }))],
    );
    let g = plain_function(
        2,
        "g",
        20,
        vec![Stmt::Return(Some(Expr::Conditional {
            condition: Box::new(Expr::Compare {
                op: CompareOp::Eq,
                left: Box::new(Expr::LocalGet(20)),
                right: Box::new(Expr::Integer(0)),
            }),
            then_expr: Box::new(Expr::Integer(0)),
            else_expr: Box::new(call(1, vec![sub(20, 1)])),
        }))],
    );
    let mut module = Module::new("spec_preserve_none.ts");
    module.functions.push(f);
    module.functions.push(g);
    module
        .init
        .push(Stmt::Expr(call(1, vec![Expr::Integer(40)])));
    module
        .init
        .push(Stmt::Expr(call(2, vec![Expr::Integer(40)])));
    let ir = compile_ir(&module);

    for sym in ["__f$spec_i32(i32", "__g$spec_i32(i32"] {
        assert!(
            ir.contains(&format!(
                "define internal preserve_nonecc double \
                                  @perry_fn_spec_preserve_none_ts{sym}"
            )),
            "both members of the cycle must take the convention:\n{ir}"
        );
    }
    assert_preserve_none_consistency(&ir);
}

#[test]
fn a_call_inside_a_try_region_invokes_with_the_convention() {
    // The clone is `invoke`d from a protected region: the call-site
    // convention must ride the unwind-edge form exactly like the plain form.
    let mut module = recursive_module();
    module.functions.push(plain_function(
        2,
        "g",
        30,
        vec![
            Stmt::Try {
                body: vec![Stmt::Return(Some(call(1, vec![Expr::Integer(7)])))],
                catch: Some(perry_hir::CatchClause {
                    param: None,
                    body: vec![Stmt::Return(Some(Expr::Integer(0)))],
                }),
                finally: None,
            },
            Stmt::Return(Some(Expr::Integer(0))),
        ],
    ));
    module
        .init
        .push(Stmt::Expr(call(2, vec![Expr::Integer(1)])));
    let ir = compile_ir(&module);

    assert!(
        ir.contains(CLONE_DEFINE),
        "the clone must still exist, or this asserts nothing:\n{ir}"
    );
    assert!(
        ir.contains(&format!(
            "invoke preserve_nonecc double @{CLONE_SYMBOL}(i32 7)"
        )),
        "a protected-region call must carry the convention on its invoke:\n{ir}"
    );
    assert_preserve_none_consistency(&ir);
}

#[test]
fn unsupported_targets_keep_the_default_convention() {
    // Same predicate family as the RS4GC target-awareness: watchOS arm64_32
    // and ARM64 Windows never see the convention, everything the runtime can
    // walk does.
    for triple in [
        "arm64_32-apple-watchos",
        "aarch64-pc-windows-msvc",
        "aarch64-w64-mingw32",
    ] {
        let ir = compile_ir_for(&recursive_module(), Some(triple));
        assert!(
            ir.contains("$spec_i32(i32"),
            "the clone must still exist on {triple}, or this asserts nothing"
        );
        assert!(
            !ir.contains("preserve_nonecc"),
            "{triple} must not receive preserve_nonecc"
        );
    }
    // And a supported non-host pair, so the gate isn't accidentally
    // host-shaped.
    for triple in [
        "x86_64-unknown-linux-gnu",
        "aarch64-unknown-linux-gnu",
        "x86_64-w64-mingw32",
    ] {
        let ir = compile_ir_for(&recursive_module(), Some(triple));
        assert!(
            ir.contains("preserve_nonecc double @perry_fn_spec_preserve_none_ts__f$spec_i32"),
            "{triple} supports the convention and must apply it"
        );
    }
}

/// The promoted (`force_external`) define and the cross-unit declare BOTH
/// carry the convention: codegen-unit splitting changes visibility, never
/// the ABI. A mismatch across a unit boundary is UB that will not
/// necessarily crash near the cause, so this is pinned at the renderer.
#[test]
fn unit_promotion_carries_the_convention() {
    use crate::module::LlModule;
    use crate::types::DOUBLE;

    let mut llmod = LlModule::new("arm64-apple-darwin");
    llmod.set_preserve_none_fns(["clone$spec_i32".to_string()]);
    let lf = llmod.define_function("clone$spec_i32", DOUBLE, vec![("i32", "%arg0".into())]);
    lf.linkage = "internal".to_string();
    lf.create_block("entry").ret(DOUBLE, "0.0");

    let f = llmod.function_mut(0).unwrap();
    assert_eq!(
        f.define_header(false),
        "define internal preserve_nonecc double @clone$spec_i32(i32 %arg0) {",
        "the in-module define carries linkage + convention"
    );
    assert_eq!(
        f.define_header(true),
        "define preserve_nonecc double @clone$spec_i32(i32 %arg0) {",
        "force_external drops the linkage keyword and NOTHING else"
    );
    assert_eq!(
        crate::module::declare_line_for(f),
        "declare preserve_nonecc double @clone$spec_i32(i32)",
        "the cross-unit declare binds with the callee's real convention"
    );
}

/// The end-to-end point of the convention: the clone's entry (the leaf path)
/// is shrink-wrapped frameless. Runs perry's own in-process pipeline —
/// RS4GC + `-O3` + the target machine — on the exact IR the compiler emits,
/// then reads the clone's prologue out of the assembly. This is the gate the
/// campaign asked for: with the default convention the first instruction is
/// the pinned CSR spill (`stp ... [sp, ...]!` / `sub sp, ...`); a change
/// that silently stops applying the convention re-grows that frame and goes
/// red here instead of quietly reverting the win.
#[cfg(feature = "llvm-inprocess")]
#[test]
fn the_clone_entry_is_shrink_wrapped_frameless() {
    let _pin = crate::codegen::helpers::NativeRootsPin::native();
    let ir = compile_ir(&recursive_module());
    assert!(
        ir.contains(CLONE_DEFINE),
        "subject not live — no preserve_nonecc clone in the IR:\n{ir}"
    );

    let triple = crate::codegen::default_target_triple();
    let asm_bytes = crate::inprocess::compile_ll_to_object_inprocess(
        &ir,
        &triple,
        &["-O3".to_string(), "-S".to_string()],
        "spec_preserve_none_asm",
        crate::codegen::helpers::native_stack_roots_enabled(),
    )
    .expect("in-process -O3 -S pipeline");
    let asm = String::from_utf8(asm_bytes).expect("assembly is UTF-8");

    // The label line for the clone (Mach-O prefixes `_`; ELF does not).
    let mut lines = asm.lines();
    let label = lines
        .by_ref()
        .find(|l| {
            let t = l.trim_end();
            t.ends_with(':') && t.contains(CLONE_SYMBOL) && !l.trim_start().starts_with('.')
        })
        .expect("clone symbol not found in assembly — subject not live");
    let first_inst = lines
        .find(|l| {
            let t = l.trim();
            !t.is_empty()
                && !t.starts_with('.')
                && !t.starts_with(';')
                && !t.starts_with("//")
                && !t.ends_with(':')
        })
        .unwrap_or_else(|| panic!("no instruction after {label}"));

    let t = first_inst.trim();
    let mnemonic = t.split_ascii_whitespace().next().unwrap_or("");
    let is_frame_store = (matches!(mnemonic, "stp" | "str" | "stur") && t.contains("[sp"))
        || (mnemonic == "sub" && t.contains("sp,"))
        || mnemonic.starts_with("push")
        || (mnemonic.starts_with("sub") && t.contains("%rsp"));
    assert!(
        !is_frame_store,
        "the clone's entry must be shrink-wrapped frameless; \
         its first instruction is a frame store:\n{label}\n{first_inst}"
    );
}
