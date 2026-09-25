//! Free-standing token/operand helpers and instruction-flag setters shared by
//! the dialect reader (`FnReader`) and its `eh`/`types`/`vector` siblings.
//! Split out of `dialect/mod.rs` to keep it under the 2,000-line cap (#10750).

use super::*;

/// Tag a load with `!invariant.load !0` (issue #52 semantics). Kind id via
/// llvm-sys; the node is the empty MDNode, same as the textual `!0 = !{}`.
pub(super) fn set_invariant_load(ctx: &Context, inst: InstructionValue<'_>) {
    use inkwell::context::AsContextRef;
    let kind = unsafe {
        llvm_sys::core::LLVMGetMDKindIDInContext(
            ctx.as_ctx_ref(),
            b"invariant.load".as_ptr() as *const std::os::raw::c_char,
            14,
        )
    };
    let node = ctx.metadata_node(&[]);
    let _ = inst.set_metadata(node, kind);
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
    for (i, c) in s.char_indices().skip(open) {
        match c {
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

/// Split on top-level commas (not inside (), [], <>, {}).
pub(super) fn split_top_level(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut cur = String::new();
    for c in s.chars() {
        match c {
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
