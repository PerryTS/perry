//! Web Storage global method call lowering.

use anyhow::Result;
use perry_hir::Expr;

use crate::expr::FnCtx;
use crate::types::{DOUBLE, I64, PTR};

fn is_web_storage_global_expr(e: &Expr) -> bool {
    matches!(
        e,
        Expr::PropertyGet { object, property, .. }
            if matches!(property.as_str(), "localStorage" | "sessionStorage")
                && matches!(object.as_ref(), Expr::GlobalGet(_))
    )
}

pub(super) fn try_lower_web_storage_method_call(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    property: &str,
    args: &[Expr],
) -> Result<Option<String>> {
    if !is_web_storage_global_expr(object)
        || !matches!(
            property,
            "clear" | "getItem" | "key" | "removeItem" | "setItem"
        )
    {
        return Ok(None);
    }

    // #11789 sweep: `localStorage.setItem(String(k), f())` holds the key
    // across the value's evaluation.
    let (recv_box, lowered_args, group) = super::lower_operands_rooted(ctx, object, args)?;
    let (args_ptr, args_len) = if lowered_args.is_empty() {
        ("null".to_string(), "0".to_string())
    } else {
        let n = lowered_args.len();
        let buf_reg = ctx.func.alloca_entry_array(DOUBLE, n);
        for (i, arg_val) in lowered_args.iter().enumerate() {
            let slot = ctx
                .block()
                .gep(DOUBLE, &buf_reg, &[(I64, &format!("{}", i))]);
            ctx.block().store(DOUBLE, arg_val, &slot);
        }
        let ptr_reg = ctx.block().next_reg();
        ctx.block().emit_raw(format!(
            "{} = getelementptr [{} x double], ptr {}, i64 0, i64 0",
            ptr_reg, n, buf_reg
        ));
        (ptr_reg, n.to_string())
    };

    let key_idx = ctx.strings.intern(property);
    let entry = ctx.strings.entry(key_idx);
    let bytes_global = format!("@{}", entry.bytes_global);
    let name_len_str = entry.byte_len.to_string();
    let result = ctx.block().call(
        DOUBLE,
        "js_native_call_method",
        &[
            (DOUBLE, &recv_box),
            (PTR, &bytes_global),
            (I64, &name_len_str),
            (PTR, &args_ptr),
            (I64, &args_len),
        ],
    );
    group.release(ctx);
    Ok(Some(result))
}
