//! `Stmt::Try` lowering — LLVM `invoke`/`landingpad` exception handling
//! (#7302), one shape on every target.
//!
//! The CFG pattern:
//!   1. Branch into the try body. No handler is registered anywhere on
//!      entry or exit (ehfree slice 1). While the body lowers, its
//!      landing-pad label is the active EH scope: every potentially-throwing
//!      call the body emits becomes an `invoke` unwinding there
//!      (`LlBlock::eh_invoke_suffix`).
//!   2. Entering a handler scope reads the runtime-handle stack's height
//!      (`js_eh_handle_height`, one call). The landing pad's first call is
//!      `js_eh_pad_enter(llvm.stacksave(), handle height)`: the runtime pops
//!      the managed-stack entries of every frame the throw abandoned (frame
//!      marks below this frame's stack pointer; see perry-runtime
//!      `exception/marks.rs`) and truncates the runtime-handle stack to the
//!      height the scope read (handles are pushed far more often than a
//!      `try` is entered, so they carry no mark). Brackets this frame opened
//!      inside the region are closed by action pads (`stmt::eh_actions`). On
//!      shadow-stack targets the function's own shadow frame is made the top
//!      frame again (`LlFunction` rewrites the pad, as it rewrites `ret`).
//!   3. The landing pad funnels into the catch entry, which runs
//!      `js_catch_enter` (`js_get_exception` + `js_clear_exception`) and
//!      binds the catch parameter.
//!   4. Catch/finally bodies lower under the *enclosing* scope, so a throw
//!      escaping them wires to the outer handler — or leaves the function
//!      when there is none. Re-raise sites (`js_throw` after a finally copy)
//!      go through the same call chokepoint and pick up the correct edge
//!      automatically.
//!
//! WASI uses scoped wasm exception pads and a frame-owned depth checkpoint.
//! Its handlers restore the checkpoint before catchret enters ordinary JS CFG;
//! native action pads and stack-mark restoration remain unchanged.
//!
//! History: until #7302 this was setjmp/longjmp-based, which required
//! `returns_twice` + `noinline` on every try-containing function plus a
//! volatile-promotion pass over try-mutated allocas (#6385), and made
//! precise moving-GC roots unsound in try functions (a longjmp could skip a
//! statepoint relocation write-back — the motivating defect, #7174).

use super::*;
use crate::block::EhScope;
use crate::types::{I64, PTR};

/// Lexical wasm handlers have no registration to pop; native pads likewise.
pub(crate) fn emit_try_end(_ctx: &mut FnCtx<'_>) {}

/// The first instructions of every Perry landing pad after its
/// `landingpad`: `js_eh_pad_enter(llvm.stacksave(), handle_height)` restores
/// the runtime's managed stacks to this frame. The unwinder installed this
/// frame's stack pointer, so `stacksave` names exactly the catching frame;
/// `handle_height` is the scope's [`EhScope::handle_height`].
pub(crate) fn emit_pad_enter(blk: &mut crate::block::LlBlock, handle_height: &str) {
    let sp = blk.call(PTR, "llvm.stacksave.p0", &[]);
    blk.call_void("js_eh_pad_enter", &[(PTR, &sp), (I64, handle_height)]);
}

/// The runtime-handle stack's height for a handler scope about to be
/// entered at the current block: one `js_eh_handle_height` call.
///
/// Everything above it when a pad of the scope runs was pushed after the
/// scope was entered: by frames the throw abandoned, or by this frame's own
/// `new.target` bracket around a call or an inlined constructor body inside
/// the scope, which the throw left as well. Every pad is reached only
/// through a call inside its scope, which the read dominates. Generator and
/// async resumes are separate activations, and a `try` never spans a resume
/// point (the generator linearizer turns a `try` that contains a `yield`
/// into a dispatch-loop `try` that every resume enters). An enclosing
/// scope's value is not reused: an inlined constructor body can open a
/// bracket between the two entries.
fn scope_handle_height(ctx: &mut FnCtx<'_>) -> String {
    ctx.block().call(I64, "js_eh_handle_height", &[])
}

/// Materialize the unwind-target block that funnels the exception into
/// `exc_label`. Returns the handler's EH scope (its pad, and `exc_label` as the
/// block an inner action scope continues at); the caller pushes it around the
/// protected body.
///
/// One shape on every native target: a single landing-pad block —
/// `landingpad {ptr,i32} catch ptr null` → `js_eh_pad_enter(sp)` →
/// `br %exc_label`. The pair is ignored; the thrown value is read back from
/// the runtime's rooted TLS slot via `js_get_exception`.
///
/// windows-msvc used to get a second, SEH shape here (`catchswitch` →
/// `catchpad [ptr @perry_seh_filter]` → `catchret`). It was removed because it
/// is fundamentally incompatible with precise moving-GC roots: LLVM's
/// `rewrite-statepoints-for-gc` does not support funclet EH and crashes
/// outright on `catchswitch`/`catchpad` (#7354, still reproducible on LLVM
/// 22.1.8), so every Windows module containing a `try` had to choose between
/// statepoints and compiling at all. Keeping the Itanium shape on windows-msvc
/// is what buys Windows the same precise roots every other target gets.
///
/// This is sound on COFF because the personality is Perry's own, not a
/// recognized MSVC one: LLVM therefore classifies the function as non-funclet
/// EH and emits `.seh_handler perry_eh_personality, @unwind, @except` plus an
/// Itanium-format `GCC_except_table` in `.xdata` — the same LSDA the
/// ELF/Mach-O path already parses. The x64 unwinder calls whatever handler
/// `.xdata` names, so a custom personality is a first-class citizen there.
///
/// The restore of the runtime's managed state runs at the pad
/// (`js_eh_pad_enter`), which is sound because no code runs between the
/// raise and the first pad: the unwinder skips Rust cleanups (the runtime is
/// built panic=abort with forced unwind tables; see `perry-runtime/src/eh.rs`).
///
/// Also used by the async rejection boundary in `stmt/mod.rs`
/// (`lower_async_rejecting_stmts_inner`) — same dispatch, different
/// exception continuation.
pub(super) fn emit_eh_dispatch(
    ctx: &mut FnCtx<'_>,
    exc_label: &str,
    normal_label: &str,
) -> EhScope {
    if ctx.target_triple.starts_with("wasm32") {
        return super::wasm_eh::emit_dispatch(ctx, exc_label, normal_label);
    }
    // One personality for every pad; runtime traps are landing pads too
    // (perry-runtime `exception::trap`), so the unwinder stops at whichever
    // handler is innermost.
    ctx.func.personality = Some("perry_eh_personality");

    let lpad_idx = ctx.new_block("eh.lpad");
    let lpad_label = ctx.block_label(lpad_idx);

    let handle_height = scope_handle_height(ctx);
    ctx.block().br(normal_label);

    let saved = ctx.current_block;
    ctx.current_block = lpad_idx;
    let lp = ctx.block().next_reg();
    ctx.block()
        .emit_raw(format!("{} = landingpad {{ ptr, i32 }} catch ptr null", lp));
    emit_pad_enter(ctx.block(), &handle_height);
    ctx.block().br(exc_label);
    ctx.current_block = saved;
    EhScope {
        handle_height: Some(handle_height),
        ..EhScope::handler(lpad_label, Some(exc_label.to_string()))
    }
}

fn iterator_cleanup_expr(expr: &perry_hir::Expr) -> bool {
    use perry_hir::Expr;
    match expr {
        Expr::Call { callee, .. } => {
            matches!(&**callee, Expr::ExternFuncRef { name, .. } if name == "js_iterator_close_on_throw")
        }
        Expr::NativeMethodCall {
            module,
            class_name: None,
            object: None,
            method,
            args,
        } => {
            module == "__perry_runtime"
                && (method == "iteratorCloseOnThrow"
                    || (method == "arrayRecordClose"
                        && matches!(args.last(), Some(Expr::Bool(true)))))
        }
        Expr::Conditional {
            then_expr,
            else_expr,
            ..
        } => iterator_cleanup_expr(then_expr) && iterator_cleanup_expr(else_expr),
        _ => false,
    }
}

fn iterator_cleanup_body(body: &[perry_hir::Stmt]) -> bool {
    body.iter().any(|s| match s {
        perry_hir::Stmt::Throw(expr) | perry_hir::Stmt::Expr(expr) => iterator_cleanup_expr(expr),
        perry_hir::Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            iterator_cleanup_body(then_branch)
                || else_branch
                    .as_ref()
                    .is_some_and(|s| iterator_cleanup_body(s))
        }
        _ => false,
    })
}

fn unregistered_iterator_cleanup(target: &str, catch: Option<&perry_hir::CatchClause>) -> bool {
    target.starts_with("x86_64")
        && target.ends_with("-linux-gnu")
        && catch
            .and_then(|c| c.param.as_ref())
            .is_some_and(|(_, name)| name.starts_with("__forof_err_"))
        && catch.is_some_and(|c| iterator_cleanup_body(&c.body))
}

pub(crate) fn lower_try(
    ctx: &mut FnCtx<'_>,
    body: &[perry_hir::Stmt],
    catch: Option<&perry_hir::CatchClause>,
    finally: Option<&[perry_hir::Stmt]>,
) -> Result<()> {
    // An iterator-cleanup `try` (linux x86_64) catches only to close the
    // iterator and rethrow: its catch reads no frame state, so it does not
    // count as a handler of this function for the fast-tier gates
    // (`ctx.try_depth`), and its catch+finally pair may be outlined
    // (`arrayRecordAbrupt`).
    let registered = !unregistered_iterator_cleanup(ctx.target_triple, catch);
    let try_body_idx = ctx.new_block("try.body");
    let catch_idx = ctx.new_block("try.catch");
    let finally_idx = ctx.new_block("try.finally");

    let try_body_label = ctx.block_label(try_body_idx);
    let catch_label = ctx.block_label(catch_idx);
    let finally_label = ctx.block_label(finally_idx);

    // --- current block: enter body; landing pad → catch ---
    let scope = emit_eh_dispatch(ctx, &catch_label, &try_body_label);

    // --- try body (scope active) ---
    ctx.current_block = try_body_idx;
    // Count scopes whose normal exits run the existing completion cleanup.
    // Lexical wasm handlers require no dynamic registration or pop.
    let counted = registered;
    ctx.try_depth += usize::from(counted);
    ctx.func.push_eh_scope_entry(scope);
    lower_stmts(ctx, body)?;
    ctx.func.pop_eh_scope();
    ctx.try_depth -= usize::from(counted);
    if !ctx.block().is_terminated() {
        emit_try_end(ctx);
        ctx.block().br(&finally_label);
    }

    // --- catch (reached only through the landing pad) ---
    ctx.current_block = catch_idx;
    let outlined = catch
        .zip(finally)
        .filter(|_| !registered)
        .and_then(|(c, f)| crate::lower_conditional::array_record::abrupt_args(c, f));
    if let Some(args) = outlined {
        lower_expr(
            ctx,
            &perry_hir::Expr::NativeMethodCall {
                module: "__perry_runtime".into(),
                class_name: None,
                object: None,
                method: "arrayRecordAbrupt".into(),
                args,
            },
        )?;
    } else if let Some(clause) = catch {
        let exc = ctx.block().call(DOUBLE, "js_catch_enter", &[]);
        // Bind the catch param (if any) to the exception value.
        if let Some((id, _name)) = &clause.param {
            // Slot lives in the entry block — a closure inside the catch
            // body may capture the exception binding and get called from a
            // sibling branch that the catch block doesn't dominate.
            //
            // #7209: the shadow-slot BIND must follow the store — after
            // js_clear_exception this alloca is the only root keeping the
            // exception alive, and an entry-hoisted bind would hand the
            // root-word decoder uninitialized stack bytes on the
            // non-throwing path.
            let slot = ctx.func.alloca_entry(DOUBLE);
            ctx.locals.insert(*id, slot.clone());
            ctx.block().store(DOUBLE, &exc, &slot);
            crate::expr::emit_shadow_slot_bind_for_local(ctx, *id);
        }
        if let Some(f) = finally {
            // Per spec TryStatement : try Block Catch Finally — a throw
            // escaping the CATCH body must still run the finally, whose own
            // abrupt completion (throw) replaces the pending one. Protect
            // the catch body with its own handler; its landing pad runs a
            // dedicated copy of the finally body, then re-raises the
            // catch's exception (unless the finally itself terminated
            // abruptly — its terminator stands).
            // Refs test262 S12.14_A7_T2/T3, S12.14_A13_T3.
            let cbody_idx = ctx.new_block("try.catch.body");
            let cfail_idx = ctx.new_block("try.catch.fail");
            let cbody_label = ctx.block_label(cbody_idx);
            let cfail_label = ctx.block_label(cfail_idx);
            let cfail_scope = emit_eh_dispatch(ctx, &cfail_label, &cbody_label);

            ctx.current_block = cbody_idx;
            ctx.try_depth += 1;
            ctx.func.push_eh_scope_entry(cfail_scope);
            lower_stmts(ctx, &clause.body)?;
            ctx.func.pop_eh_scope();
            ctx.try_depth -= 1;
            if !ctx.block().is_terminated() {
                emit_try_end(ctx);
                ctx.block().br(&finally_label);
            }

            ctx.current_block = cfail_idx;
            emit_try_end(ctx);
            let exc2 = ctx.block().call(DOUBLE, "js_get_exception", &[]);
            let pending = super::wasm_eh::root_pending(ctx, &exc2);
            lower_stmts(ctx, f)?;
            if !ctx.block().is_terminated() {
                let exc2 = super::wasm_eh::reload_pending(ctx, pending, exc2);
                ctx.block().call_void("js_throw", &[(DOUBLE, &exc2)]);
                ctx.block().unreachable();
            }
        } else {
            lower_stmts(ctx, &clause.body)?;
            if !ctx.block().is_terminated() {
                ctx.block().br(&finally_label);
            }
        }
    } else {
        // No catch clause: `try { ... } finally { ... }`. ECMAScript
        // requires the finally to run and then the ORIGINAL exception to
        // RE-PROPAGATE — it must NOT be swallowed (issue #37: effect's
        // `internalCall` "forced" path). Capture the pending exception
        // BEFORE running finally (the finally body may touch exception
        // state), run a dedicated copy of the finally body on this
        // exception path, then re-raise via js_throw — unless the finally
        // itself completed abruptly (a `return`/`throw` inside finally
        // overrides the pending exception, per spec).
        emit_try_end(ctx);
        let exc = ctx.block().call(DOUBLE, "js_get_exception", &[]);
        let pending = super::wasm_eh::root_pending(ctx, &exc);
        if let Some(f) = finally {
            lower_stmts(ctx, f)?;
        }
        if !ctx.block().is_terminated() {
            let exc = super::wasm_eh::reload_pending(ctx, pending, exc);
            ctx.block().call_void("js_throw", &[(DOUBLE, &exc)]);
            ctx.block().unreachable();
        }
    }

    // --- finally / merge (normal-completion path) ---
    ctx.current_block = finally_idx;
    if let Some(f) = finally {
        lower_stmts(ctx, f)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn array_record_cleanup_has_no_normal_path_savepoint() {
        use perry_hir::{Expr, Stmt};
        let close = |method: &str, args| Expr::NativeMethodCall {
            module: "__perry_runtime".into(),
            class_name: None,
            object: None,
            method: method.into(),
            args,
        };
        let expr = Expr::Conditional {
            condition: Box::new(Expr::Bool(false)),
            then_expr: Box::new(close("iteratorCloseOnThrow", vec![])),
            else_expr: Box::new(close(
                "arrayRecordClose",
                vec![
                    Expr::Undefined,
                    Expr::Integer(0),
                    Expr::Bool(false),
                    Expr::Undefined,
                    Expr::Bool(true),
                ],
            )),
        };
        let catch = perry_hir::CatchClause {
            param: Some((1, "__forof_err_1".into())),
            body: vec![Stmt::Throw(expr)],
        };
        assert!(unregistered_iterator_cleanup(
            "x86_64-unknown-linux-gnu",
            Some(&catch)
        ));
        assert!(
            !iterator_cleanup_expr(&close("arrayRecordClose", vec![Expr::Bool(false)])),
            "normal close must not be mistaken for exception cleanup"
        );
    }

    #[test]
    fn iterator_cleanup_has_no_normal_path_savepoint() {
        let mut catch = perry_hir::CatchClause {
            param: Some((1, "__forof_err_1".into())),
            body: vec![perry_hir::Stmt::Throw(perry_hir::Expr::Call {
                callee: Box::new(perry_hir::Expr::ExternFuncRef {
                    name: "js_iterator_close_on_throw".into(),
                    param_types: vec![perry_hir::types::Type::Any; 3],
                    return_type: perry_hir::types::Type::Any,
                }),
                args: vec![],
                type_args: vec![],
                byte_offset: 0,
            })],
        };
        assert!(unregistered_iterator_cleanup(
            "x86_64-unknown-linux-gnu",
            Some(&catch)
        ));
        assert!(!unregistered_iterator_cleanup(
            "aarch64-apple-darwin",
            Some(&catch)
        ));
        catch.param.as_mut().unwrap().1 = "user_catch".into();
        assert!(!unregistered_iterator_cleanup(
            "x86_64-unknown-linux-gnu",
            Some(&catch)
        ));
        catch.param.as_mut().unwrap().1 = "__forof_err_1".into();
        catch.body.clear();
        assert!(!unregistered_iterator_cleanup(
            "x86_64-unknown-linux-gnu",
            Some(&catch)
        ));
    }
    #[test]
    fn iterator_cleanup_ir_keeps_unwind_edge_without_hot_push_or_pop() {
        let _pin = crate::codegen::helpers::NativeRootsPin::native();
        let mut module = perry_hir::Module::new("iterret_cleanup");
        module.init.push(perry_hir::Stmt::Try {
            body: vec![perry_hir::Stmt::Throw(perry_hir::Expr::Number(42.0))],
            catch: Some(perry_hir::CatchClause {
                param: Some((1, "__forof_err_1".into())),
                body: vec![perry_hir::Stmt::Throw(perry_hir::Expr::NativeMethodCall {
                    module: "__perry_runtime".into(),
                    class_name: None,
                    object: None,
                    method: "iteratorCloseOnThrow".into(),
                    args: vec![
                        perry_hir::Expr::Undefined,
                        perry_hir::Expr::Bool(false),
                        perry_hir::Expr::LocalGet(1),
                    ],
                })],
            }),
            finally: None,
        });
        let opts = crate::CompileOptions {
            emit_ir_only: true,
            is_entry_module: true,
            target: Some("x86_64-unknown-linux-gnu".into()),
            ..Default::default()
        };
        let ir = String::from_utf8(crate::compile_module(&module, opts).unwrap()).unwrap();
        let main = ir
            .split("define i32 @main()")
            .nth(1)
            .unwrap()
            .split(
                "
}
",
            )
            .next()
            .unwrap();
        assert!(main.contains("landingpad"));
        assert!(main.contains("invoke"));
        assert!(main.contains("@js_iterator_close_on_throw"));
        assert!(!main.contains("@js_eh_try_push"));
        assert!(!main.contains("@js_try_end"));
    }

    /// ehfree slice 1: an ordinary `try`/`catch` records nothing on entry or
    /// exit; its landing pad's first call restores the managed stacks to the
    /// catching frame (`js_eh_pad_enter(llvm.stacksave())`), and the catch
    /// entry only takes the exception.
    #[test]
    fn try_entry_is_free_and_the_pad_restores_by_stack_pointer() {
        let _pin = crate::codegen::helpers::NativeRootsPin::native();
        let mut module = perry_hir::Module::new("free_try");
        module.init.push(perry_hir::Stmt::Try {
            body: vec![perry_hir::Stmt::Throw(perry_hir::Expr::Number(42.0))],
            catch: Some(perry_hir::CatchClause {
                param: Some((1, "e".into())),
                body: vec![],
            }),
            finally: None,
        });
        for target in ["x86_64-unknown-linux-gnu", "aarch64-apple-darwin"] {
            let opts = crate::CompileOptions {
                emit_ir_only: true,
                is_entry_module: true,
                target: Some(target.into()),
                ..Default::default()
            };
            let ir = String::from_utf8(crate::compile_module(&module, opts).unwrap()).unwrap();
            let main = ir
                .split("define i32 @main()")
                .nth(1)
                .unwrap()
                .split("\n}\n")
                .next()
                .unwrap();
            assert!(!main.contains("@js_eh_try_push"), "{target}: {main}");
            assert!(!main.contains("@js_try_end"), "{target}: {main}");
            let pad = main
                .split("landingpad")
                .nth(1)
                .unwrap_or_else(|| panic!("{target}: no landing pad: {main}"));
            let save = pad.find("@llvm.stacksave.p0()").expect("pad reads its sp");
            let enter = pad
                .find("call void @js_eh_pad_enter(ptr")
                .expect("pad restores the managed stacks");
            assert!(save < enter, "{target}: {pad}");
            assert!(main.contains("@js_catch_enter"), "{target}");
        }
    }
}
