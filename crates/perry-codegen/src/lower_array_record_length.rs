//! Record entry supplies a real array proof to the existing indexed length
//! lowering. The private binding still has its actual Any hint.
use crate::expr::{lower_expr, FnCtx};
use anyhow::Result;
use perry_hir::Expr;

pub(crate) fn lower(ctx: &mut FnCtx<'_>, source: &Expr) -> Result<String> {
    let Expr::LocalGet(source_id) = source else {
        anyhow::bail!("arrayRecordLength requires its compiler-owned source binding");
    };
    let previous = ctx.array_record_length_local.replace(*source_id);
    let result = lower_expr(
        ctx,
        &Expr::PropertyGet {
            object: Box::new(source.clone()),
            property: "length".into(),
            byte_offset: 0,
        },
    );
    ctx.array_record_length_local = previous;
    result
}

#[cfg(test)]
mod tests {
    #[test]
    fn record_length_outside_a_stack_record_is_an_ordinary_length_site() {
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
            let mut second = m.functions[0].clone();
            second.id = 2;
            second.name = "length_again".into();
            m.functions.push(second);
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
            assert!(ir.contains("plen."));
            // Outside a stack record the binding is an ordinary length site:
            // its typed tier and cold IC tier stay inline, one IC per site.
            assert!(
                !ir.contains("@perry_length_cold_"),
                "an ordinary site does not cross a call boundary: {ir}"
            );
            assert_eq!(
                ir.lines()
                    .filter(
                        |line| line.contains("call double @js_value_length_property_key_ic_f64(")
                    )
                    .count(),
                2,
                "each site keeps ordinary override and getter semantics"
            );
            assert!(ir.contains("uitofp i32"));
            assert!(!ir.contains("call double @js_object_get_field_ic"));
            #[cfg(feature = "llvm-inprocess")]
            crate::testing::verify_ir(&ir, "record_live_length").unwrap();
        });
    }
}
