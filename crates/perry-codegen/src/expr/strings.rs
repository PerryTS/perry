//! String-literal rodata emission (extracted from `expr.rs`, issue
//! #1098). Pure move — no logic changes.

use super::FnCtx;

/// Issue #841: materialize a NUL-terminated rodata constant carrying
/// `text`'s UTF-8 bytes and return the LLVM IR pointer expression that
/// names it. Used by the named-import + namespace-import value-form
/// lowerings to hand a stable `(ptr, len)` pair to the runtime helpers
/// `js_node_submodule_export_as_function` /
/// `js_node_submodule_namespace`. The label uses a per-invocation
/// counter so multiple call sites in the same function don't collide.
pub(crate) fn emit_string_literal_global(ctx: &mut FnCtx<'_>, text: &str) -> String {
    let idx = ctx.typed_parse_counter;
    ctx.typed_parse_counter += 1;
    let func_part: String = ctx
        .func
        .name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    let global_name = format!("perry_node_submod_str_{}_{}", func_part, idx);
    let bytes = text.as_bytes();
    let mut lit = String::with_capacity(bytes.len() + 4);
    lit.push('c');
    lit.push('"');
    for &b in bytes {
        if (32..127).contains(&b) && b != b'"' && b != b'\\' {
            lit.push(b as char);
        } else {
            lit.push('\\');
            lit.push_str(&format!("{:02X}", b));
        }
    }
    lit.push_str("\\00\"");
    ctx.typed_parse_rodata.push(format!(
        "@{} = private unnamed_addr constant [{} x i8] {}",
        global_name,
        bytes.len() + 1,
        lit
    ));
    format!("@{}", global_name)
}
