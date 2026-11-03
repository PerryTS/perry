//! Private `(owner, offset_words, op, sent, out_kind)` resume ABI.
//!
//! `owner` is always the allocation start and the only GC root. The offset
//! describes ordinary JSValue slots scanned by its existing closure layout.
//! No frame address escapes an access, safepoint, call or queue operation.
use super::*;
use crate::types::{DOUBLE, I32, I64, PTR};

// Each private entry is emitted only for this fixed birth layout. Its sealed
// callers supply offset one; the callable async adapter supplies the same
// allocation owner. Specialize address arithmetic to that construction fact,
// while retaining the explicit owner/offset ABI and relocation rereads.
const FRAME_WORD_OFFSET: u32 = 1;

pub(super) fn slot_index(ctx: &mut FnCtx<'_>, index: u32) -> String {
    (index
        + if ctx.private_resume_frame {
            FRAME_WORD_OFFSET
        } else {
            0
        })
    .to_string()
}
pub(super) fn slot_offset(ctx: &mut FnCtx<'_>, index: &str) -> String {
    let header = crate::target_layout::closure_header_size_bytes(ctx.target_triple);
    (header + index.parse::<u64>().expect("constant frame capture") * 8).to_string()
}
pub(crate) fn write_out_kind(ctx: &mut FnCtx<'_>) -> Result<()> {
    if !ctx.private_resume_frame {
        return Ok(());
    }
    if ctx.private_async_resume_frame {
        // Async resumes return the existing driver's Promise, not a sync
        // yield/done pair. Their statically async adapter consumes no kind.
        return Ok(());
    }
    let owner = current_closure_ptr_value(ctx, "private resume output")?;
    let kind = if let Some(kind) = super::resume_locals::load_kind(ctx) {
        kind
    } else {
        let index = slot_index(ctx, super::resume_locals::KIND_INDEX);
        let offset = slot_offset(ctx, &index);
        let address = ctx.block().add(I64, &owner, &offset);
        let ptr = ctx.block().inttoptr(I64, &address);
        let kind = ctx.block().load(DOUBLE, &ptr);
        ctx.block().fptosi(DOUBLE, &kind, I32)
    };
    // GC_STORE_AUDIT(POINTER_FREE): output kind is a compiler-owned i32.
    ctx.block().store(I32, &kind, "%frame_out_kind");
    let header = crate::target_layout::closure_header_size_bytes(ctx.target_triple).to_string();
    let parent = ctx.block().add(I64, &owner, &header);
    let parent = ctx.block().inttoptr(I64, &parent);
    let boxed = ctx.block().sitofp(I32, &kind, DOUBLE);
    // GC_STORE_AUDIT(POINTER_FREE): the parent's own completion kind.
    ctx.block().store(DOUBLE, &boxed, &parent);
    Ok(())
}

fn birth(ctx: &mut FnCtx<'_>, link: &Expr) -> Result<String> {
    let Expr::LinkGeneratorPrototype {
        obj,
        fixed_state: true,
        is_async: false,
        ..
    } = link
    else {
        anyhow::bail!("private frame requires a proven sync resume layout");
    };
    let Expr::Closure {
        func_id,
        params,
        body,
        captures,
        captures_this: false,
        captures_new_target: false,
        mutable_captures,
        ..
    } = obj.as_ref()
    else {
        anyhow::bail!("private frame cannot carry an observable closure receiver");
    };
    let captures = crate::type_analysis::compute_auto_captures(ctx, params, body, captures);
    anyhow::ensure!(
        mutable_captures.is_empty() && captures.iter().all(|id| !ctx.boxed_vars.contains(id)),
        "private frame admission retained a shared cell"
    );
    let name = symbol(ctx.strings.module_prefix(), *func_id);
    ctx.block().note_fn_info(&name, |facts| {
        facts.fixed_generator_state = true;
        facts.private_resume_frame = true;
    });
    let info = ctx.block().fn_info_ref(&name);
    // Parent word zero holds the consumer's completion kind. The child's frame
    // starts at word one: exercising a real nonzero offset prevents accidental
    // reinterpretation of a child address as an independent ClosureHeader.
    let mut birth = crate::stmt::binding_cell::BirthWords::new(ctx, &captures);
    for id in &captures {
        super::closure::push_capture_word(ctx, *id, &mut birth)?;
    }
    let words = birth.read(ctx);
    let count = words.len() + FRAME_WORD_OFFSET as usize;
    let buffer = ctx.func.alloca_entry_array(I64, count);
    let prefix = ctx.block().gep(I64, &buffer, &[(I64, "0")]);
    ctx.block().store(I64, "0", &prefix);
    for (i, word) in words.iter().enumerate() {
        let ptr = ctx.block().gep(
            I64,
            &buffer,
            &[(I64, &(i + FRAME_WORD_OFFSET as usize).to_string())],
        );
        ctx.block().store(I64, word, &ptr);
    }
    let header = (u64::from(crate::runtime_abi::FUNCTION_LAYOUT_WORD) << 32)
        | u64::from(crate::runtime_abi::checked_closure_capture_count(count));
    // Existing fixed allocation initializes all slots and uses the generic
    // JSValue capture scanner; the parent owns every edge from birth onward.
    let owner = ctx.block().call(
        I64,
        "js_closure_alloc_fixed",
        &[(PTR, &info), (I64, &header.to_string()), (PTR, &buffer)],
    );
    birth.release(ctx);
    Ok(super::nanbox_pointer_inline(ctx.block(), &owner))
}
fn resume(
    ctx: &mut FnCtx<'_>,
    owner: &Expr,
    fid: u32,
    op: &str,
    sent: &str,
) -> Result<(String, String)> {
    let value = lower_expr(ctx, owner)?;
    let bits = ctx.block().bitcast_double_to_i64(&value);
    let owner = ctx.block().and(I64, &bits, crate::nanbox::POINTER_MASK_I64);
    let out = ctx.func.alloca_entry(I32);
    let name = symbol(ctx.strings.module_prefix(), fid);
    let result = ctx.block().call(
        DOUBLE,
        &name,
        &[
            (I64, &owner),
            (I32, &FRAME_WORD_OFFSET.to_string()),
            (I32, op),
            (DOUBLE, sent),
            (PTR, &out),
        ],
    );
    let kind = ctx.block().load(I32, &out);
    Ok((result, kind))
}
pub(crate) fn lower(ctx: &mut FnCtx<'_>, method: &str, args: &[Expr]) -> Result<Option<String>> {
    if method == "privateAsyncResumeFrame" {
        return Ok(Some(async_birth(ctx, &args[0])?));
    }
    if method == "privateResumeFrame" {
        return Ok(Some(birth(ctx, &args[0])?));
    }
    if !matches!(
        method,
        "privateResumeFrameStep" | "privateResumeFrameClose" | "privateResumeFrameCloseOnThrow"
    ) {
        return Ok(None);
    }
    let Expr::Integer(fid) = args[1] else {
        anyhow::bail!("private resume needs a static entry");
    };
    let undef = crate::nanbox::double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
    if method == "privateResumeFrameStep" {
        let Expr::LocalSet(id, output) = &args[2] else {
            anyhow::bail!("private resume needs its output binding");
        };
        let (value, kind) = resume(ctx, &args[0], fid as u32, "0", &undef)?;
        crate::expr::invalidate_local_write_facts(ctx, *id);
        crate::expr::bind_lowered_value_to_local(ctx, *id, &value, output)?;
        let done = ctx.block().icmp_ne(I32, &kind, "0");
        let done = ctx.block().zext(crate::types::I1, &done, I32);
        return Ok(Some(crate::expr::i32_bool_to_nanbox(ctx.block(), &done)));
    }
    // These compiler-owned close sites carry literal false for not-done.
    // Keep refusal explicit instead of guessing at an unknown completion.
    anyhow::ensure!(
        matches!(args.get(2), Some(Expr::Bool(false))),
        "private close needs its completion proof"
    );
    if method == "privateResumeFrameCloseOnThrow" {
        // IteratorClose during a throw preserves the incoming exception even
        // if return throws. Emit the ordinary HIR catch boundary around the
        // private return and hand the original rooted exception back.
        return crate::rooting::with_rooted_group(ctx, 1, |ctx, group| {
            let incoming = group.lower(ctx, &args[3], true)?;
            let close = Expr::NativeMethodCall {
                module: "__perry_runtime".into(),
                class_name: None,
                object: None,
                method: "privateResumeFrameClose".into(),
                args: args[..3].to_vec(),
            };
            crate::stmt::lower_stmt(
                ctx,
                &perry_hir::Stmt::Try {
                    body: vec![perry_hir::Stmt::Expr(close)],
                    catch: Some(perry_hir::CatchClause {
                        param: None,
                        body: vec![],
                    }),
                    finally: None,
                },
            )?;
            Ok(Some(group.reread(ctx, incoming)?))
        });
    }
    // The consumer's completion lives in its own parent word, outside the
    // child's subframe. A completed canonical generator needs no further
    // private return; no source method/Get can be observed on this route.
    let owner = lower_expr(ctx, &args[0])?;
    let bits = ctx.block().bitcast_double_to_i64(&owner);
    let owner = ctx.block().and(I64, &bits, crate::nanbox::POINTER_MASK_I64);
    let header = crate::target_layout::closure_header_size_bytes(ctx.target_triple).to_string();
    let address = ctx.block().add(I64, &owner, &header);
    let ptr = ctx.block().inttoptr(I64, &address);
    let kind = ctx.block().load(DOUBLE, &ptr);
    let done = ctx
        .block()
        .fcmp_ty(DOUBLE, "oeq", &kind, &crate::nanbox::double_literal(1.0));
    let close = ctx.new_block("private_frame.close");
    let after = ctx.new_block("private_frame.closed");
    let close_label = ctx.block_label(close);
    let after_label = ctx.block_label(after);
    ctx.block().cond_br(&done, &after_label, &close_label);
    ctx.current_block = close;
    let _ = resume(ctx, &args[0], fid as u32, "1", &undef)?;
    ctx.block().br(&after_label);
    ctx.current_block = after;
    Ok(Some(undef))
}

pub(crate) fn symbol(prefix: &str, fid: u32) -> String {
    format!("perry_resume_frame__{prefix}__{fid}")
}
/// Record the private body ABI before closure compilation (module init is
/// emitted later). Birth and body consume the same compiler-owned descriptor.
pub(crate) fn record_entries(
    llmod: &crate::module::LlModule,
    hir: &perry_hir::Module,
    prefix: &str,
) {
    fn visit(llmod: &crate::module::LlModule, e: &Expr, prefix: &str) {
        if let Expr::NativeMethodCall {
            module,
            method,
            args,
            ..
        } = e
        {
            if module == "__perry_runtime" && method == "privateAsyncResumeFrame" {
                if let Some(Expr::Closure { func_id, .. }) = args.first() {
                    for name in [
                        crate::fn_info::closure_body_symbol(prefix, *func_id),
                        symbol(prefix, *func_id),
                    ] {
                        llmod.note_fn_info(&name, |facts| {
                            facts.fixed_generator_state = true;
                            facts.private_resume_frame = true;
                            facts.private_async_resume_frame = true;
                            facts.set_strict();
                        });
                    }
                }
            }
            if module == "__perry_runtime" && method == "privateResumeFrame" {
                if let Some(Expr::LinkGeneratorPrototype { obj, .. }) = args.first() {
                    if let Expr::Closure { func_id, .. } = obj.as_ref() {
                        for name in [
                            crate::fn_info::closure_body_symbol(prefix, *func_id),
                            symbol(prefix, *func_id),
                        ] {
                            llmod.note_fn_info(&name, |facts| {
                                facts.fixed_generator_state = true;
                                facts.private_resume_frame = true;
                            });
                        }
                    }
                }
            }
        }
        if let Expr::Closure { body, .. } = e {
            for s in body {
                perry_hir::walker::stmt_any_expr(s, &mut |e| {
                    visit(llmod, e, prefix);
                    false
                });
            }
        }
        perry_hir::walker::walk_expr_children(e, &mut |e| visit(llmod, e, prefix));
    }
    for s in &hir.init {
        perry_hir::walker::stmt_any_expr(s, &mut |e| {
            visit(llmod, e, prefix);
            false
        });
    }
    for f in &hir.functions {
        for s in &f.body {
            perry_hir::walker::stmt_any_expr(s, &mut |e| {
                visit(llmod, e, prefix);
                false
            });
        }
    }
}

#[cfg(test)]
#[path = "private_resume_frame_tests.rs"]
mod tests;

pub(crate) fn is_entry(llmod: &crate::module::LlModule, prefix: &str, fid: u32) -> bool {
    let mut private = false;
    llmod.note_fn_info(&crate::fn_info::closure_body_symbol(prefix, fid), |facts| {
        private = facts.private_resume_frame
    });
    private
}
pub(crate) fn abi_params(
    params: &[perry_hir::Param],
    private: bool,
    asynchronous: bool,
) -> Vec<(crate::types::LlvmType, String)> {
    if !private {
        return super::body_call::js_body_params(params.iter().map(|p| format!("%arg{}", p.id)));
    }
    assert_eq!(params.len(), 2, "private resume has op and sent bindings");
    vec![
        (I64, "%this_closure".into()),
        (I32, "%frame_offset".into()),
        (I32, "%frame_op".into()),
        (
            DOUBLE,
            format!("%arg{}", params[usize::from(!asynchronous)].id),
        ),
        (PTR, "%frame_out_kind".into()),
    ]
}

pub(crate) fn is_async_entry(llmod: &crate::module::LlModule, prefix: &str, fid: u32) -> bool {
    let mut asynchronous = false;
    llmod.note_fn_info(&crate::fn_info::closure_body_symbol(prefix, fid), |facts| {
        asynchronous = facts.private_async_resume_frame;
    });
    asynchronous
}

fn async_birth(ctx: &mut FnCtx<'_>, closure: &Expr) -> Result<String> {
    let Expr::Closure {
        func_id,
        params,
        body,
        captures,
        mutable_captures,
        captures_this: false,
        captures_new_target: false,
        enclosing_class: None,
        ..
    } = closure
    else {
        anyhow::bail!("private async frame needs a closed step owner");
    };
    let captures = crate::type_analysis::compute_auto_captures(ctx, params, body, captures);
    anyhow::ensure!(
        mutable_captures.is_empty() && captures.iter().all(|id| !ctx.boxed_vars.contains(id)),
        "private async frame retained a shared cell"
    );
    let adapter = crate::fn_info::closure_body_symbol(ctx.strings.module_prefix(), *func_id);
    let info = ctx.block().fn_info_ref(&adapter);
    let mut birth = crate::stmt::binding_cell::BirthWords::new(ctx, &captures);
    for id in &captures {
        super::closure::push_capture_word(ctx, *id, &mut birth)?;
    }
    let words = birth.read(ctx);
    let count = words.len() + FRAME_WORD_OFFSET as usize;
    let buffer = ctx.func.alloca_entry_array(I64, count);
    let zero = ctx.block().gep(I64, &buffer, &[(I64, "0")]);
    ctx.block().store(I64, "0", &zero);
    for (i, word) in words.iter().enumerate() {
        let p = ctx.block().gep(
            I64,
            &buffer,
            &[(I64, &(i + FRAME_WORD_OFFSET as usize).to_string())],
        );
        ctx.block().store(I64, word, &p);
    }
    let header = (u64::from(crate::runtime_abi::FUNCTION_LAYOUT_WORD) << 32)
        | u64::from(crate::runtime_abi::checked_closure_capture_count(count));
    let owner = ctx.block().call(
        I64,
        "js_closure_alloc_fixed",
        &[(PTR, &info), (I64, &header.to_string()), (PTR, &buffer)],
    );
    birth.release(ctx);
    Ok(super::nanbox_pointer_inline(ctx.block(), &owner))
}

/// The existing scheduler calls this ordinary two-argument step closure. It
/// carries the allocation owner into the private child entry at offset one.
/// The queued pointer, reaction capture and TLS self all name the parent;
/// no interior pointer or native-stack lifetime crosses a suspension.
pub(crate) fn async_adapter(llmod: &mut crate::module::LlModule, prefix: &str, fid: u32) {
    let name = crate::fn_info::closure_body_symbol(prefix, fid);
    let frame = symbol(prefix, fid);
    let params = super::body_call::js_body_params(["%sent", "%error"]);
    let function = llmod.define_function(&name, DOUBLE, params);
    function.create_block("entry");
    let block = function.block_mut(0).unwrap();
    let bits = block.bitcast_double_to_i64("%error");
    let yes = block.icmp_eq(I64, &bits, crate::nanbox::TAG_TRUE_I64);
    let operation = block.zext(crate::types::I1, &yes, I32);
    let value = block.call(
        DOUBLE,
        &frame,
        &[
            (I64, "%this_closure"),
            (I32, &FRAME_WORD_OFFSET.to_string()),
            (I32, &operation),
            (DOUBLE, "%sent"),
            (PTR, "null"),
        ],
    );
    block.ret(DOUBLE, &value);
}

/// The private descriptor proves the exact owner and inline slot index. The
/// existing barrier emitter tests the stored value and current generation;
/// every managed edge still reaches the same exact-slot runtime barrier.
pub(super) fn store_slot(
    ctx: &mut FnCtx<'_>,
    index: u32,
    value: &str,
    pointer_free: bool,
    string_alias: bool,
) -> Result<()> {
    if string_alias && !pointer_free {
        super::helpers::emit_string_addref_if_heap_string(ctx, value);
    }
    let owner = current_closure_ptr_value(ctx, "private frame slot store")?;
    let index = slot_index(ctx, index);
    let offset = slot_offset(ctx, &index);
    let address = ctx.block().add(I64, &owner, &offset);
    let pointer = ctx.block().inttoptr(I64, &address);
    let bits = ctx.block().bitcast_double_to_i64(value);
    // GC_STORE_AUDIT(BARRIERED): the parent scanner owns every child word.
    ctx.block().store(I64, &bits, &pointer);
    if !pointer_free {
        super::write_barrier::emit_write_barrier_slot_value_and_generation_tested(
            ctx,
            &owner,
            &owner,
            &address,
            &bits,
            "frame_slot",
        );
    }
    Ok(())
}

/// Translate the private operation to the original step parameter without
/// changing the ordinary closure ABI or the sent JSValue representation.
pub(crate) fn parameter_value(
    block: &mut crate::block::LlBlock,
    params: &[perry_hir::Param],
    id: u32,
    private: bool,
    asynchronous: bool,
) -> String {
    if asynchronous && id == params[1].id {
        let error = block.icmp_ne(I32, "%frame_op", "0");
        let bits = block.select(
            crate::types::I1,
            &error,
            I64,
            crate::nanbox::TAG_TRUE_I64,
            crate::nanbox::TAG_FALSE_I64,
        );
        block.bitcast_i64_to_double(&bits)
    } else if private && !asynchronous && params.len() == 2 && id == params[0].id {
        block.sitofp(I32, "%frame_op", DOUBLE)
    } else if private && !asynchronous && params.len() == 2 && id == params[1].id {
        // The sealed private callers (resume, above) pass undefined as
        // the sent value on every step and close; no other caller reaches a
        // private entry. The body therefore starts from that constant.
        crate::nanbox::double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
    } else {
        format!("%arg{id}")
    }
}

/// These are transform-created LocalIds carried by the private descriptor,
/// not a scan of source variable names or TypeScript annotations. LocalGet
/// nodes let the normal HIR remapper keep descriptors tied to fresh scopes.
pub(crate) fn collect_async_controls(
    args: &[Expr],
    numbers: &mut std::collections::HashSet<u32>,
    booleans: &mut std::collections::HashSet<u32>,
) {
    for (index, out) in [(1, numbers), (2, booleans)] {
        if let Some(Expr::Sequence(ids)) = args.get(index) {
            for id in ids {
                if let Expr::LocalGet(id) = id {
                    out.insert(*id);
                }
            }
        }
    }
}
