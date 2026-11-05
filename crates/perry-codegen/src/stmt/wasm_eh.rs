//! Standard wasm EH. JS handlers execute after catchret, outside the funclet.
use super::*;
use crate::block::EhScope;
use crate::types::PTR;

pub(super) fn emit_dispatch(ctx: &mut FnCtx<'_>, exc: &str, normal: &str) -> EhScope {
    assert!(!crate::codegen::helpers::native_stack_roots_enabled());
    ctx.func.personality = Some("__gxx_wasm_personality_v0");
    let outer = ctx.func.innermost_eh_scope().map(|s| s.pad.clone());
    let checkpoint = ctx.func.alloca_entry_bytes_aligned(
        std::mem::size_of::<perry_abi::WasmEhCheckpoint>() as u32,
        std::mem::align_of::<perry_abi::WasmEhCheckpoint>() as u32,
    );
    ctx.block()
        .call_void("js_wasm_eh_checkpoint_save", &[(PTR, &checkpoint)]);
    ctx.block().br(normal);
    let saved = ctx.current_block;
    let dispatch = ctx.new_block("wasm.eh.dispatch");
    let pad = ctx.new_block("wasm.eh.pad");
    let accepted = ctx.new_block("wasm.eh.accepted");
    let foreign = ctx.new_block("wasm.eh.foreign");
    let dispatch_label = ctx.block_label(dispatch);
    let pad_label = ctx.block_label(pad);
    let accepted_label = ctx.block_label(accepted);
    let foreign_label = ctx.block_label(foreign);
    ctx.current_block = dispatch;
    let cs = ctx.block().next_reg();
    let unwind = outer
        .as_ref()
        .map_or("to caller".into(), |s| format!("label %{s}"));
    ctx.block().emit_raw(format!(
        "{cs} = catchswitch within none [label %{pad_label}] unwind {unwind}"
    ));
    ctx.block().mark_terminated();
    ctx.current_block = pad;
    let cp = ctx.block().next_reg();
    let object = ctx.block().next_reg();
    let ours = ctx.block().next_reg();
    ctx.block()
        .emit_raw(format!("{cp} = catchpad within {cs} [ptr null]"));
    ctx.block().emit_raw(format!(
        "{object} = call ptr @llvm.wasm.get.exception(token {cp})"
    ));
    ctx.block().emit_raw(format!(
        "{ours} = call i1 @js_wasm_eh_is_perry(ptr {object}) [ \"funclet\"(token {cp}) ]"
    ));
    ctx.block().cond_br(&ours, &accepted_label, &foreign_label);
    ctx.current_block = accepted;
    ctx.block().emit_raw(format!(
        "call void @js_wasm_eh_pad_enter(ptr {checkpoint}) [ \"funclet\"(token {cp}) ]"
    ));
    ctx.block()
        .emit_raw(format!("catchret from {cp} to label %{exc}"));
    ctx.block().mark_terminated();
    ctx.current_block = foreign;
    if let Some(outer) = outer {
        let dead = ctx.new_block("wasm.eh.dead");
        let dead_label = ctx.block_label(dead);
        ctx.block().emit_raw(format!("invoke void @llvm.wasm.rethrow() [ \"funclet\"(token {cp}) ] to label %{dead_label} unwind label %{outer}"));
        ctx.block().mark_terminated();
        ctx.current_block = dead;
    } else {
        ctx.block().emit_raw(format!(
            "call void @llvm.wasm.rethrow() [ \"funclet\"(token {cp}) ]"
        ));
    }
    ctx.block().unreachable();
    ctx.current_block = saved;
    // Checkpoint restore already closes every bracket opened in this region.
    EhScope::handler(dispatch_label, None)
}

/// Preserve a pending throw through finally, whose nested catches replace TLS
/// and whose allocations can relocate it. Native IR remains exactly unchanged.
pub(super) fn root_pending(ctx: &mut FnCtx<'_>, value: &str) -> Option<String> {
    if !ctx.target_triple.starts_with("wasm32") {
        return None;
    }
    let index = ctx
        .func
        .reserve_shadow_slot()
        .expect("wasm uses shadow roots");
    let slot = ctx.func.alloca_entry(DOUBLE);
    ctx.block().store(DOUBLE, value, &slot);
    crate::expr::shadow_slot::emit_shadow_slot_bind_ptr(ctx, index, &slot);
    Some(slot)
}
pub(super) fn reload_pending(ctx: &mut FnCtx<'_>, slot: Option<String>, value: String) -> String {
    let Some(slot) = slot else {
        return value;
    };
    let value = ctx.block().load(DOUBLE, &slot);
    ctx.block().store(DOUBLE, "0.0", &slot);
    value
}

#[cfg(all(test, feature = "target-wasi"))]
mod tests {
    #[test]
    fn wasm_eh_nested_regions_use_funclets_and_shadow_roots() {
        use perry_hir::{CatchClause, Expr, Stmt};
        let mut module = perry_hir::Module::new("wasm_eh_nested");
        let catch = || {
            Some(CatchClause {
                param: Some((1, "e".into())),
                body: vec![],
            })
        };
        module.init.push(Stmt::Try {
            body: vec![Stmt::Try {
                body: vec![Stmt::Throw(Expr::Number(42.0))],
                catch: catch(),
                finally: None,
            }],
            catch: catch(),
            finally: Some(vec![]),
        });
        let opts = crate::CompileOptions {
            emit_ir_only: true,
            is_entry_module: true,
            target: Some("wasm32-wasip2".into()),
            ..Default::default()
        };
        let ir = String::from_utf8(crate::compile_module(&module, opts).unwrap()).unwrap();
        assert!(ir.contains("personality ptr @__gxx_wasm_personality_v0"));
        assert!(ir.contains("catchswitch within none"));
        assert!(ir.contains("catchpad within"));
        assert!(ir.contains("catchret from"));
        assert!(ir.contains("invoke void @llvm.wasm.rethrow()"));
        assert!(ir.contains("@js_wasm_eh_checkpoint_save(ptr"));
        assert!(ir.contains("@js_wasm_eh_pad_enter(ptr"));
        assert!(ir.contains("@js_wasm_eh_is_perry(ptr"));
        assert!(ir.contains("@__perry_wasi_command_body("));
        assert!(ir.contains("@js_wasm_eh_exit_uncaught()"));
        assert!(!ir.contains("@setjmp"));
        assert!(!ir.contains("@js_try_push"));
        assert!(!ir.contains("returns_twice"));
        assert!(!ir.contains("statepoint"));
        assert!(!ir.contains("gc \"statepoint-example\""));
        assert!(ir.contains("js_shadow_frame"));
        // Exercise the real compile plan and in-process verifier/emitter, not
        // only the textual lowering. In particular the plan's wasm EH flags
        // must be understood by the in-process argument interpreter.
        let object = crate::linker::compile_ll_to_object(&ir, Some("wasm32-wasip2")).unwrap();
        assert!(object.starts_with(b"\0asm"));
    }
}
