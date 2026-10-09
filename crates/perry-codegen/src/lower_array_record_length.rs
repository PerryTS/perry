//! The entry proof owns the ordinary-array length fact. Keep its live
//! header read in line; forwarding and source-root repair share the runtime.
use crate::expr::{lower_expr, FnCtx};
use crate::types::{DOUBLE, I16, I32, I64};
use anyhow::Result;
use perry_hir::Expr;

pub(crate) fn lower(ctx: &mut FnCtx<'_>, source: &Expr) -> Result<String> {
    let Expr::LocalGet(source_id) = source else {
        anyhow::bail!("arrayRecordLength requires its compiler-owned source binding");
    };
    let boxed = lower_expr(ctx, source)?;
    let bits = ctx.block().bitcast_double_to_i64(&boxed);
    let handle = ctx.block().and(I64, &bits, crate::nanbox::POINTER_MASK_I64);
    // ArrayRecordEnter has proved an ordinary array. Its own nonconfigurable
    // length stays in the header, even with a changed prototype/descriptors.
    // Growth/evacuation may leave a forwarding stub since the entry proof.
    let header = ctx.block().sub(I64, &handle, "8");
    let header = ctx.block().inttoptr(I64, &header);
    let word = ctx.block().load(I16, &header);
    let type_and_forwarding = ctx.block().and(I16, &word, "33023");
    let live = ctx.block().icmp_eq(I16, &type_and_forwarding, "1");
    let range = ctx.new_block("array_record_length.fast");
    let cold = ctx.new_block("array_record_length.forward");
    let range_label = ctx.block_label(range);
    let cold_label = ctx.block_label(cold);
    let direct_end = ctx.block().label.clone();
    ctx.block().cond_br(&live, &range_label, &cold_label);
    ctx.current_block = cold;
    let fresh = ctx
        .block()
        .call(DOUBLE, "js_array_refresh_local_head", &[(DOUBLE, &boxed)]);
    crate::expr::invalidate_local_write_facts(ctx, *source_id);
    crate::expr::bind_lowered_value_to_local(ctx, *source_id, &fresh, source)?;
    let bits = ctx.block().bitcast_double_to_i64(&fresh);
    let fresh_handle = ctx.block().and(I64, &bits, crate::nanbox::POINTER_MASK_I64);
    let cold_end = ctx.block().label.clone();
    ctx.block().br(&range_label);
    ctx.current_block = range;
    let handle = ctx
        .block()
        .phi(I64, &[(&handle, &direct_end), (&fresh_handle, &cold_end)]);
    let ptr = ctx.block().inttoptr(I64, &handle);
    let length = ctx.block().load(I32, &ptr);
    Ok(ctx.block().uitofp(I32, &length, DOUBLE))
}

#[cfg(test)]
mod tests {
    #[test]
    fn record_length_reads_the_live_header_and_repairs_the_existing_source() {
        crate::temp_root_coverage::under_both_lowerings(|_mode| {
            let mut m = perry_hir::Module::new("record_length");
            m.functions.push(perry_hir::Function {
                id: 1,
                name: "length".into(),
                type_params: vec![],
                params: vec![perry_hir::Param {
                    id: 1,
                    name: "source".into(),
                    ty: perry_hir::types::Type::Any,
                    default: None,
                    decorators: vec![],
                    is_rest: false,
                    arguments_object: None,
                }],
                return_type: perry_hir::types::Type::Number,
                body: vec![perry_hir::Stmt::Return(Some(
                    perry_hir::Expr::NativeMethodCall {
                        module: "__perry_runtime".into(),
                        class_name: None,
                        object: None,
                        method: "arrayRecordLength".into(),
                        args: vec![perry_hir::Expr::LocalGet(1)],
                    },
                ))],
                is_async: false,
                is_generator: false,
                is_strict: true,
                is_exported: true,
                captures: vec![],
                decorators: vec![],
                was_plain_async: false,
                was_unrolled: false,
            });
            let ir = String::from_utf8(
                crate::compile_module(
                    &m,
                    crate::CompileOptions {
                        emit_ir_only: true,
                        ..Default::default()
                    },
                )
                .unwrap(),
            )
            .unwrap();
            assert!(ir.contains("array_record_length.fast"));
            assert!(ir.contains("array_record_length.forward"));
            assert!(ir.contains("call double @js_array_refresh_local_head"));
            assert!(!ir.contains("call double @js_object_get_field_ic"));
            let cold = ir.split_once("array_record_length.forward").unwrap().1;
            assert!(
                cold.contains("store "),
                "the live head must repair the existing source root"
            );
            #[cfg(feature = "llvm-inprocess")]
            crate::testing::verify_ir(&ir, "record_live_length").unwrap();
        });
    }
}
