//! Free-standing token/operand helpers and instruction-flag setters shared by
//! the dialect reader (`FnReader`) and its `eh`/`types`/`vector` siblings.
//! Split out of `dialect/mod.rs` to keep it under the 2,000-line cap (#10750).

use super::*;
use inkwell::values::MetadataValue;

/// Tag a load with `!invariant.load !0` (issue #52 semantics). Kind id via
/// llvm-sys; the node is the empty MDNode, same as the textual `!0 = !{}`.
pub(super) fn set_invariant_load(ctx: &Context, inst: InstructionValue<'_>) {
    set_metadata_kind(ctx, inst, "invariant.load", ctx.metadata_node(&[]));
}

fn set_metadata_kind<'ctx>(
    ctx: &'ctx Context,
    inst: InstructionValue<'ctx>,
    kind: &str,
    node: MetadataValue<'ctx>,
) {
    use inkwell::context::AsContextRef;
    let kind = unsafe {
        llvm_sys::core::LLVMGetMDKindIDInContext(
            ctx.as_ctx_ref(),
            kind.as_ptr() as *const std::os::raw::c_char,
            kind.len() as u32,
        )
    };
    let _ = inst.set_metadata(node, kind);
}

/// An inline metadata operand as the emitters write it: a tree of `!{...}`
/// tuples whose leaves are `!"string"` (LLVM `\XX` byte escapes). Covers
/// `llvm.read_register`'s `!{!"rsp"}` and the indexed-owner region's
/// `llvm.experimental.noalias.scope.decl` scope lists, whose scopes are
/// `!{!"owner.N", !{!"DOMAIN"}}`. Built through the uniquing MDNode API, so
/// equal text yields the identical node, exactly as LLVM's own parser
/// uniques inline tuples. Numbered `!N` references name module metadata the
/// reader cannot see and stay a loud error here.
pub(super) fn inline_metadata<'ctx>(ctx: &'ctx Context, tok: &str) -> Result<MetadataValue<'ctx>> {
    let (node, rest) =
        metadata_tuple(ctx, tok.trim()).with_context(|| format!("metadata operand `{tok}`"))?;
    if !rest.trim().is_empty() {
        bail!("trailing text after metadata operand `{tok}`");
    }
    Ok(node)
}

fn metadata_tuple<'ctx, 's>(
    ctx: &'ctx Context,
    s: &'s str,
) -> Result<(MetadataValue<'ctx>, &'s str)> {
    let mut s = s
        .strip_prefix("!{")
        .ok_or_else(|| anyhow!("unsupported metadata form `{s}`"))?;
    let mut elems: Vec<BasicMetadataValueEnum<'ctx>> = Vec::new();
    loop {
        s = s.trim_start();
        if let Some(rest) = s.strip_prefix('}') {
            return Ok((ctx.metadata_node(&elems), rest));
        }
        let (elem, rest) = if let Some(body) = s.strip_prefix("!\"") {
            let end = body
                .find('"')
                .ok_or_else(|| anyhow!("unterminated metadata string"))?;
            let text = unescape_llvm_bytes(&body[..end])?;
            (ctx.metadata_string(&text), &body[end + 1..])
        } else {
            metadata_tuple(ctx, s)?
        };
        elems.push(elem.into());
        s = rest.trim_start();
        if let Some(rest) = s.strip_prefix(',') {
            s = rest;
        } else if !s.starts_with('}') {
            bail!("expected `,` or `}}` in metadata tuple at `{s}`");
        }
    }
}

/// Decode LLVM's `\XX` hex byte escapes (`\5C` is a backslash).
fn unescape_llvm_bytes(s: &str) -> Result<String> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\' {
            let hex = s
                .get(i + 1..i + 3)
                .and_then(|h| u8::from_str_radix(h, 16).ok())
                .ok_or_else(|| anyhow!("bad `\\XX` escape in metadata string `{s}`"))?;
            out.push(hex);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).map_err(|_| anyhow!("metadata string `{s}` is not UTF-8"))
}

/// Attach the trailing `!kind !{...}` metadata of a load/store line. Inline
/// tuples (the indexed-owner region's `!alias.scope`/`!noalias`) are built
/// like LLVM's own parse; `!invariant.load` keeps its empty-node meaning.
/// Numbered `!N` references to module metadata are not visible to the reader
/// and are left off, which only drops an optimization hint.
pub(super) fn attach_trailing_metadata<'ctx>(
    ctx: &'ctx Context,
    inst: Option<InstructionValue<'ctx>>,
    parts: &[String],
) -> Result<()> {
    let Some(inst) = inst else { return Ok(()) };
    for part in parts {
        let Some(attachment) = part.strip_prefix('!') else {
            continue;
        };
        let Some((kind, node)) = attachment.split_once(char::is_whitespace) else {
            continue;
        };
        let node = node.trim();
        if kind == "invariant.load" {
            set_invariant_load(ctx, inst);
        } else if node.starts_with("!{") {
            set_metadata_kind(ctx, inst, kind, inline_metadata(ctx, node)?);
        }
    }
    Ok(())
}

/// Replace every use of a placeholder with the real definition. A type
/// mismatch means the operand-context type at the USE disagreed with the
/// defining instruction — always a reader bug, and silently leaving the
/// placeholder in place is exactly how the select-undef version shipped a
/// miscompile, so it is a hard error.
pub(super) fn rauw<'ctx>(
    name: &str,
    ph: BasicValueEnum<'ctx>,
    real: BasicValueEnum<'ctx>,
) -> Result<()> {
    match (ph, real) {
        (BasicValueEnum::IntValue(a), BasicValueEnum::IntValue(b)) => a.replace_all_uses_with(b),
        (BasicValueEnum::FloatValue(a), BasicValueEnum::FloatValue(b)) => {
            a.replace_all_uses_with(b)
        }
        (BasicValueEnum::PointerValue(a), BasicValueEnum::PointerValue(b)) => {
            a.replace_all_uses_with(b)
        }
        (a, b) => bail!(
            "forward reference {name} used as {:?} but defined as {:?}",
            a.get_type(),
            b.get_type()
        ),
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Small helpers
// ---------------------------------------------------------------------------

pub(super) fn be(e: inkwell::builder::BuilderError) -> anyhow::Error {
    anyhow!("builder error: {e}")
}

pub(super) fn unquote(s: &str) -> String {
    s.trim_matches('"').to_string()
}

/// Consume one LLVM quoted string. Perry's emitted templates and constraint
/// strings use LLVM's `\XX` byte escapes rather than embedded quote
/// characters, so the next quote is the structural terminator and the bytes
/// between can be passed to `create_inline_asm` unchanged.
pub(super) fn take_quoted<'a>(s: &'a str, what: &str) -> Result<(String, &'a str)> {
    let s = s.trim_start();
    let body = s
        .strip_prefix('"')
        .ok_or_else(|| anyhow!("{what} is not quoted"))?;
    let end = body
        .find('"')
        .ok_or_else(|| anyhow!("unterminated {what}"))?;
    Ok((body[..end].to_string(), &body[end + 1..]))
}

/// Find the matching `)` for the `(` at `open` in `s`.
pub(super) fn rmatch_paren(s: &str, open: usize) -> Result<usize> {
    let mut depth = 0usize;
    let mut quoted = false;
    for (i, c) in s.char_indices().skip(open) {
        match c {
            // LLVM strings carry `\XX` byte escapes, never a bare quote, so
            // each `"` toggles; brackets inside a string are bytes.
            '"' => quoted = !quoted,
            _ if quoted => {}
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Ok(i);
                }
            }
            _ => {}
        }
    }
    bail!("unbalanced parens")
}

/// Split on top-level commas (not inside (), [], <>, {}, or a quoted
/// string: metadata strings carry arbitrary function names).
pub(super) fn split_top_level(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut quoted = false;
    let mut cur = String::new();
    for c in s.chars() {
        match c {
            '"' => {
                quoted = !quoted;
                cur.push(c);
            }
            _ if quoted => cur.push(c),
            '(' | '[' | '{' | '<' => {
                depth += 1;
                cur.push(c);
            }
            ')' | ']' | '}' | '>' => {
                depth -= 1;
                cur.push(c);
            }
            ',' if depth == 0 => {
                out.push(cur.trim().to_string());
                cur = String::new();
            }
            _ => cur.push(c),
        }
    }
    if !cur.trim().is_empty() {
        out.push(cur.trim().to_string());
    }
    out
}

pub(super) fn int_pred(p: &str) -> Result<IntPredicate> {
    Ok(match p {
        "eq" => IntPredicate::EQ,
        "ne" => IntPredicate::NE,
        "slt" => IntPredicate::SLT,
        "sle" => IntPredicate::SLE,
        "sgt" => IntPredicate::SGT,
        "sge" => IntPredicate::SGE,
        "ult" => IntPredicate::ULT,
        "ule" => IntPredicate::ULE,
        "ugt" => IntPredicate::UGT,
        "uge" => IntPredicate::UGE,
        other => bail!("unknown icmp predicate `{other}`"),
    })
}

pub(super) fn float_pred(p: &str) -> Result<FloatPredicate> {
    Ok(match p {
        "oeq" => FloatPredicate::OEQ,
        "one" => FloatPredicate::ONE,
        "olt" => FloatPredicate::OLT,
        "ole" => FloatPredicate::OLE,
        "ogt" => FloatPredicate::OGT,
        "oge" => FloatPredicate::OGE,
        "ord" => FloatPredicate::ORD,
        "ueq" => FloatPredicate::UEQ,
        "une" => FloatPredicate::UNE,
        "ult" => FloatPredicate::ULT,
        "ule" => FloatPredicate::ULE,
        "ugt" => FloatPredicate::UGT,
        "uge" => FloatPredicate::UGE,
        "uno" => FloatPredicate::UNO,
        other => bail!("unknown fcmp predicate `{other}`"),
    })
}

pub(super) fn add_enum_attr(ctx: &Context, func: FunctionValue<'_>, name: &str) {
    let kind = inkwell::attributes::Attribute::get_named_enum_kind_id(name);
    if kind != 0 {
        let attr = ctx.create_enum_attribute(kind, 0);
        func.add_attribute(inkwell::attributes::AttributeLoc::Function, attr);
    }
}

pub(super) fn set_alignment_if_any(inst: Option<InstructionValue<'_>>, rest: &str) {
    if let (Some(inst), Some(idx)) = (inst, rest.rfind(", align ")) {
        if let Ok(align) = rest[idx + ", align ".len()..].trim().parse::<u32>() {
            let _ = inst.set_alignment(align);
        }
    }
}

pub(super) fn apply_flags(inst: Option<InstructionValue<'_>>, flags: &[&str]) {
    let Some(inst) = inst else { return };
    let mut fmf: u32 = 0;
    for f in flags {
        match *f {
            "reassoc" => fmf |= llvm_sys::LLVMFastMathAllowReassoc,
            "contract" => fmf |= llvm_sys::LLVMFastMathAllowContract,
            "arcp" => fmf |= llvm_sys::LLVMFastMathAllowReciprocal,
            "afn" => fmf |= llvm_sys::LLVMFastMathApproxFunc,
            "nnan" => fmf |= llvm_sys::LLVMFastMathNoNaNs,
            "ninf" => fmf |= llvm_sys::LLVMFastMathNoInfs,
            "nsz" => fmf |= llvm_sys::LLVMFastMathNoSignedZeros,
            "fast" => fmf |= llvm_sys::LLVMFastMathAll,
            "nsw" => unsafe {
                llvm_sys::core::LLVMSetNSW(inst.as_value_ref(), 1);
            },
            "nuw" => unsafe {
                llvm_sys::core::LLVMSetNUW(inst.as_value_ref(), 1);
            },
            "exact" => unsafe {
                llvm_sys::core::LLVMSetExact(inst.as_value_ref(), 1);
            },
            _ => {}
        }
    }
    if fmf != 0 {
        unsafe { llvm_sys::core::LLVMSetFastMathFlags(inst.as_value_ref(), fmf) };
    }
}
