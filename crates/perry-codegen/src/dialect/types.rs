//! Type and constant parsing for the in-process LLVM dialect reader.
//!
//! Split out of `dialect/mod.rs` for the 2000-line cap. These four functions
//! are the reader's grammar for LLVM *types* — every instruction parser funnels
//! its operands through `ty_and_val` and its type tokens through `basic_type`,
//! which is why #7982's `ptr addrspace(N)` support had to land in BOTH of them:
//! teaching only the type position moves the failure to the next shape.

use anyhow::{anyhow, bail, Result};
use inkwell::context::Context;
use inkwell::module::Module;
use inkwell::types::{BasicType, BasicTypeEnum, FunctionType, VectorType};
use inkwell::values::BasicValueEnum;
use inkwell::AddressSpace;

use super::{rmatch_paren, split_top_level, unquote};

/// `"i64 %r5"` -> `("i64", "%r5")`, honoring types with spaces (`[4 x i8]`,
/// `<4 x i32>`) and bracketed values (`<4 x i32> <i32 0, i32 1, ...>`): the
/// TYPE is parsed greedily from the left (balanced brackets, then trailing
/// `*`s), the remainder is the value.
pub(super) fn ty_and_val(s: &str) -> Result<(&str, &str)> {
    let s = s.trim();
    let bytes = s.as_bytes();
    let ty_end = if bytes[0] == b'[' || bytes[0] == b'<' || bytes[0] == b'{' {
        let (open, close) = match bytes[0] {
            b'[' => (b'[', b']'),
            b'<' => (b'<', b'>'),
            _ => (b'{', b'}'),
        };
        let mut depth = 0usize;
        let mut end = 0usize;
        for (i, &c) in bytes.iter().enumerate() {
            if c == open {
                depth += 1;
            } else if c == close {
                depth -= 1;
                if depth == 0 {
                    end = i + 1;
                    break;
                }
            }
        }
        if end == 0 {
            bail!("unbalanced type brackets in `{s}`");
        }
        // Trailing pointer stars (pre-opaque spellings).
        while end < bytes.len() && bytes[end] == b'*' {
            end += 1;
        }
        end
    } else {
        s.find(' ')
            .ok_or_else(|| anyhow!("expected `type value`, got `{s}`"))?
    };
    // `ptr addrspace(N) V` — the qualifier belongs to the TYPE, but it sits
    // after a space, so the naive split hands back type `ptr` and value
    // `addrspace(1) null` (#7982). Reach past the closing paren.
    let ty_end = match s[ty_end..].trim_start().starts_with("addrspace(") {
        true => {
            ty_end
                + s[ty_end..]
                    .find(')')
                    .ok_or_else(|| anyhow!("unterminated addrspace in `{s}`"))?
                + 1
        }
        false => ty_end,
    };
    Ok((s[..ty_end].trim(), s[ty_end..].trim()))
}

pub(super) fn strip_label(s: &str) -> Result<&str> {
    s.trim()
        .strip_prefix("label %")
        .map(str::trim)
        .ok_or_else(|| anyhow!("expected `label %...`, got `{s}`"))
}

pub(super) fn basic_type<'ctx>(ctx: &'ctx Context, tok: &str) -> Result<BasicTypeEnum<'ctx>> {
    let tok = tok.trim();
    Ok(match tok {
        "double" => ctx.f64_type().into(),
        "float" => ctx.f32_type().into(),
        "i64" => ctx.i64_type().into(),
        "i32" => ctx.i32_type().into(),
        "i16" => ctx.i16_type().into(),
        "i8" => ctx.i8_type().into(),
        "i1" => ctx.bool_type().into(),
        "i128" => ctx.i128_type().into(),
        "ptr" => ctx.ptr_type(AddressSpace::default()).into(),
        _ => {
            // `ptr addrspace(N)` — RS4GC names GC-managed pointers in address
            // space 1, so this is not an exotic corner: today's codegen emits
            // it 497 times in the spike module alone. It arrives in BOTH type
            // position (`alloca ptr addrspace(1)`, `... to ptr addrspace(1)`)
            // and operand position (`store ptr addrspace(1) null, ptr %r`),
            // which is why `ty_and_val` had to learn it too — teaching only
            // this function moves the failure to the next shape rather than
            // fixing it (#7982).
            if let Some(rest) = tok.strip_prefix("ptr") {
                if rest.starts_with(char::is_whitespace) {
                    if let Some(n) = rest
                        .trim_start()
                        .strip_prefix("addrspace(")
                        .and_then(|r| r.strip_suffix(')'))
                    {
                        let n: u16 = n
                            .trim()
                            .parse()
                            .map_err(|_| anyhow!("bad address space in `{tok}`"))?;
                        return Ok(ctx.ptr_type(AddressSpace::from(n)).into());
                    }
                }
            }
            // `[N x T]`
            if let Some(body) = tok.strip_prefix('[').and_then(|t| t.strip_suffix(']')) {
                let (n, elem) = body
                    .split_once(" x ")
                    .ok_or_else(|| anyhow!("bad array type `{tok}`"))?;
                let n: u32 = n.trim().parse()?;
                let elem_ty = basic_type(ctx, elem)?;
                return Ok(elem_ty.array_type(n).into());
            }
            // `<N x T>` (SIMD in expr/channel.rs)
            if let Some(body) = tok.strip_prefix('<').and_then(|t| t.strip_suffix('>')) {
                let (n, elem) = body
                    .split_once(" x ")
                    .ok_or_else(|| anyhow!("bad vector type `{tok}`"))?;
                let n: u32 = n.trim().parse()?;
                return Ok(match basic_type(ctx, elem)? {
                    BasicTypeEnum::IntType(t) => t.vec_type(n).into(),
                    BasicTypeEnum::FloatType(t) => t.vec_type(n).into(),
                    BasicTypeEnum::PointerType(t) => t.vec_type(n).into(),
                    other => bail!("unsupported vector element {other:?}"),
                });
            }
            // Pre-opaque pointer spellings (`i8*`, `double**`, `(sig)*`) all
            // collapse to `ptr` under LLVM 15+ semantics.
            if tok.ends_with('*') {
                return Ok(ctx.ptr_type(AddressSpace::default()).into());
            }
            bail!("unknown type `{tok}`")
        }
    })
}

pub(super) fn fn_type_of<'ctx>(
    ctx: &'ctx Context,
    ret_tok: &str,
    params: &[inkwell::types::BasicMetadataTypeEnum<'ctx>],
) -> Result<FunctionType<'ctx>> {
    Ok(match ret_tok {
        "void" => ctx.void_type().fn_type(params, false),
        _ => basic_type(ctx, ret_tok)?.fn_type(params, false),
    })
}

/// Function type for an indirect call site. `sig_str` is either just the
/// return type (`double`) or `RET (T1, T2, ...)`; when only the return type
/// is present the parameter types are taken from the argument list.
pub(super) fn indirect_fn_type<'ctx>(
    ctx: &'ctx Context,
    sig_str: &str,
    arg_types: &[inkwell::types::BasicMetadataTypeEnum<'ctx>],
) -> Result<FunctionType<'ctx>> {
    let sig_str = sig_str.trim();
    if let Some(open) = sig_str.find('(') {
        let ret_tok = sig_str[..open].trim();
        let close = rmatch_paren(sig_str, open)?;
        let params_str = &sig_str[open + 1..close];
        let mut params: Vec<inkwell::types::BasicMetadataTypeEnum> = Vec::new();
        let mut varargs = false;
        for p in split_top_level(params_str) {
            if p.trim() == "..." {
                varargs = true;
                continue;
            }
            params.push(basic_type(ctx, p.trim())?.into());
        }
        Ok(match ret_tok {
            "void" => ctx.void_type().fn_type(&params, varargs),
            _ => basic_type(ctx, ret_tok)?.fn_type(&params, varargs),
        })
    } else {
        fn_type_of(ctx, sig_str, arg_types)
    }
}

pub(super) fn constant<'ctx>(
    ctx: &'ctx Context,
    module: &Module<'ctx>,
    ty: BasicTypeEnum<'ctx>,
    tok: &str,
) -> Result<BasicValueEnum<'ctx>> {
    if let Some(g) = tok.strip_prefix('@') {
        let name = unquote(g);
        if let Some(f) = module.get_function(&name) {
            return Ok(f.as_global_value().as_pointer_value().into());
        }
        if let Some(gv) = module.get_global(&name) {
            return Ok(gv.as_pointer_value().into());
        }
        bail!("reference to unknown global @{name}");
    }
    // LLVM constant expressions can appear anywhere an ordinary constant is
    // accepted. Perry emits this exact form when a non-entry module passes its
    // `__init_body` function pointer through the integer-valued runtime ABI:
    //
    //   call void @js_run_module_init_catching(
    //       i64 ptrtoint (ptr @module__init_body to i64))
    //
    // The text backend has always delegated this to LLVM's assembler. The
    // in-process reader used to feed the whole expression to `i128::parse`, so
    // only split native codegen units failed, late in a large build, with
    // `bad integer ptrtoint (...)`. Materialize the relocatable constant with
    // LLVM's constant API; using a runtime instruction would be invalid here
    // because this is an operand constant, not an SSA definition.
    if let Some(body) = tok
        .strip_prefix("ptrtoint (")
        .and_then(|body| body.strip_suffix(')'))
    {
        let (source, destination) = body
            .rsplit_once(" to ")
            .ok_or_else(|| anyhow!("bad ptrtoint constant `{tok}`"))?;
        let (source_ty, source_value) = ty_and_val(source)?;
        let source_ty = basic_type(ctx, source_ty)?;
        if !source_ty.is_pointer_type() {
            bail!("ptrtoint source is not a pointer in `{tok}`");
        }
        let destination_ty = basic_type(ctx, destination.trim())?;
        let BasicTypeEnum::IntType(destination_ty) = destination_ty else {
            bail!("ptrtoint destination is not an integer in `{tok}`");
        };
        let BasicTypeEnum::IntType(expected_ty) = ty else {
            bail!("ptrtoint constant used as non-integer {ty:?}");
        };
        if destination_ty != expected_ty {
            bail!(
                "ptrtoint destination type {} disagrees with operand type {}",
                destination_ty.print_to_string(),
                expected_ty.print_to_string()
            );
        }
        let source = constant(ctx, module, source_ty, source_value)?.into_pointer_value();
        return Ok(source.const_to_int(destination_ty).into());
    }
    Ok(match tok {
        // The null of the OPERAND's type, not of address space 0: RS4GC emits
        // `store ptr addrspace(1) null, ptr %slot`, and an addrspace(0) null
        // stored into an `alloca ptr addrspace(1)` is a verifier error, not a
        // silent coercion (#7982).
        "null" => match ty {
            BasicTypeEnum::PointerType(t) => t.const_null().into(),
            _ => ctx.ptr_type(AddressSpace::default()).const_null().into(),
        },
        "undef" => match ty {
            BasicTypeEnum::FloatType(t) => t.get_undef().into(),
            BasicTypeEnum::IntType(t) => t.get_undef().into(),
            BasicTypeEnum::PointerType(t) => t.get_undef().into(),
            BasicTypeEnum::VectorType(t) => t.get_undef().into(),
            other => bail!("undef of unsupported type {other:?}"),
        },
        // `insertelement <4 x i32> poison, ...` seeds `expr/channel.rs`'s
        // lane-0 splat, so a vector poison is an emitted form, not a corner.
        "poison" => match ty {
            BasicTypeEnum::FloatType(t) => t.get_poison().into(),
            BasicTypeEnum::IntType(t) => t.get_poison().into(),
            BasicTypeEnum::PointerType(t) => t.get_poison().into(),
            BasicTypeEnum::VectorType(t) => t.get_poison().into(),
            other => bail!("poison of unsupported type {other:?}"),
        },
        "true" => ctx.bool_type().const_int(1, false).into(),
        "false" => ctx.bool_type().const_int(0, false).into(),
        "zeroinitializer" => match ty {
            BasicTypeEnum::FloatType(t) => t.const_zero().into(),
            BasicTypeEnum::IntType(t) => t.const_zero().into(),
            BasicTypeEnum::ArrayType(t) => t.const_zero().into(),
            BasicTypeEnum::PointerType(t) => t.const_null().into(),
            // The all-zero `shufflevector` mask (`channel.rs`'s splat).
            BasicTypeEnum::VectorType(t) => t.const_zero().into(),
            other => bail!("zeroinitializer of unsupported type {other:?}"),
        },
        _ => match ty {
            BasicTypeEnum::FloatType(t) => {
                // LLVM hex-float form is raw IEEE-754 bits — exactly how
                // NaN-boxed constants must survive.
                if let Some(hex) = tok.strip_prefix("0x") {
                    let bits = u64::from_str_radix(hex, 16)
                        .map_err(|_| anyhow!("bad hex float `{tok}`"))?;
                    t.const_float(f64::from_bits(bits)).into()
                } else {
                    t.const_float(
                        tok.parse::<f64>()
                            .map_err(|_| anyhow!("bad float `{tok}`"))?,
                    )
                    .into()
                }
            }
            BasicTypeEnum::IntType(t) => {
                let v: i128 = tok.parse().map_err(|_| anyhow!("bad integer `{tok}`"))?;
                t.const_int(v as u64, v < 0).into()
            }
            BasicTypeEnum::VectorType(t) => return vector_constant(ctx, module, t, tok),
            other => bail!("cannot materialize `{tok}` as {other:?}"),
        },
    })
}

/// `<i64 207232172546, i64 0>` — an LLVM constant vector literal.
///
/// Perry emits one as the seed operand of the #8122/#8204 header-image
/// `insertelement` (`function.rs`, `codegen/string_pool.rs`) and another as
/// `channel.rs`'s all-zero `<4 x i32>` accumulator seed. Every element of a
/// vector literal is itself a constant, so this recurses through `constant`
/// with the vector's element type and cross-checks the per-element type token
/// — a `<2 x i64>` whose elements claim `i32` is a codegen bug worth a loud
/// error, not a silent truncation.
fn vector_constant<'ctx>(
    ctx: &'ctx Context,
    module: &Module<'ctx>,
    ty: VectorType<'ctx>,
    tok: &str,
) -> Result<BasicValueEnum<'ctx>> {
    let body = tok
        .strip_prefix('<')
        .and_then(|t| t.strip_suffix('>'))
        .ok_or_else(|| anyhow!("cannot materialize `{tok}` as a vector constant"))?;
    let elem_ty = ty.get_element_type();
    let mut elems: Vec<BasicValueEnum<'ctx>> = Vec::new();
    for part in split_top_level(body) {
        let (part_ty_tok, val_tok) = ty_and_val(&part)?;
        let part_ty = basic_type(ctx, part_ty_tok)?;
        if part_ty != elem_ty {
            bail!("vector constant `{tok}` element type `{part_ty_tok}` is not the element type");
        }
        elems.push(constant(ctx, module, part_ty, val_tok)?);
    }
    if elems.len() as u32 != ty.get_size() {
        bail!(
            "vector constant `{tok}` has {} elements, type has {}",
            elems.len(),
            ty.get_size()
        );
    }
    Ok(VectorType::const_vector(&elems).into())
}
