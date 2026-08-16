//! Vector-instruction construction for the in-process LLVM reader (#8228):
//! `insertelement`, `extractelement`, `shufflevector`, and vector-typed
//! arithmetic.
//!
//! Split out of `dialect/mod.rs` to keep that file under the 2000-line cap,
//! following `eh.rs`; these share the parent reader's state (value map,
//! builder) exactly as the EH forms do.
//!
//! **The closed set this file accepts is the set perry-codegen emits.** Two
//! emitters produce vectors, and both reach this reader as `Raw` text:
//!
//! * the #8122/#8204 object-header image — `insertelement <2 x i64>
//!   <i64 PACKED, i64 0>, i64 %header_word, i32 1` from
//!   `function.rs::entry_init_object_header_image` and
//!   `codegen/string_pool.rs`, whose result is stored as one `<2 x i64>`
//!   per inline `new`;
//! * `expr/channel.rs`'s byte-channel reduction — `insertelement` /
//!   `shufflevector` / `extractelement` over `<4 x i32>`, plus vector `mul`
//!   and `add`.
//!
//! Nothing else emits a vector or aggregate instruction: `insertvalue` never
//! appears, and the tree's only `extractvalue` is an *input* fixture to a
//! landing-pad text transform, not emitted IR. Implementing those anyway
//! would add arms no test can reach, which is the opposite of this reader's
//! contract — it accepts a closed set precisely so a NEW emission form fails
//! loudly here instead of diverging silently.

use anyhow::{bail, Result};
use inkwell::types::BasicTypeEnum;
use inkwell::values::{BasicValueEnum, IntValue, VectorValue};

use super::{basic_type, be, split_top_level, ty_and_val, FnReader};

impl<'ctx, 'm> FnReader<'ctx, 'm> {
    /// `insertelement <N x T> VEC, T ELEM, i32 IDX`
    ///
    /// `VEC` is a register, a constant vector literal (`<i64 W, i64 0>`), or
    /// `poison` — `channel.rs` seeds its splat from the last of those and the
    /// header image from the middle one, so all three arrive here.
    pub(super) fn insert_element(&mut self, dst: &str, rest: &str) -> Result<BasicValueEnum<'ctx>> {
        let parts = split_top_level(rest);
        if parts.len() != 3 {
            bail!("bad insertelement operands: {rest}");
        }
        let vector = self.vector_operand("insertelement", &parts[0])?;
        let (elem_ty, elem_tok) = ty_and_val(&parts[1])?;
        let elem = self.val(basic_type(self.ctx, elem_ty)?, elem_tok)?;
        let index = self.vector_index("insertelement", &parts[2])?;
        Ok(self
            .builder
            .build_insert_element(vector, elem, index, dst.trim_start_matches('%'))
            .map_err(be)?
            .into())
    }

    /// `extractelement <N x T> VEC, i32 IDX`
    pub(super) fn extract_element(
        &mut self,
        dst: &str,
        rest: &str,
    ) -> Result<BasicValueEnum<'ctx>> {
        let parts = split_top_level(rest);
        if parts.len() != 2 {
            bail!("bad extractelement operands: {rest}");
        }
        let vector = self.vector_operand("extractelement", &parts[0])?;
        let index = self.vector_index("extractelement", &parts[1])?;
        self.builder
            .build_extract_element(vector, index, dst.trim_start_matches('%'))
            .map_err(be)
    }

    /// `shufflevector <N x T> A, <N x T> B, <M x i32> MASK`
    ///
    /// The mask is an ordinary constant vector operand in the text form, so
    /// it parses like the other two; `zeroinitializer` (the lane-0 splat
    /// `channel.rs` emits) is handled by the constant reader.
    pub(super) fn shuffle_vector(&mut self, dst: &str, rest: &str) -> Result<BasicValueEnum<'ctx>> {
        let parts = split_top_level(rest);
        if parts.len() != 3 {
            bail!("bad shufflevector operands: {rest}");
        }
        let a = self.vector_operand("shufflevector", &parts[0])?;
        let b = self.vector_operand("shufflevector", &parts[1])?;
        let mask = self.vector_operand("shufflevector", &parts[2])?;
        Ok(self
            .builder
            .build_shuffle_vector(a, b, mask, dst.trim_start_matches('%'))
            .map_err(be)?
            .into())
    }

    /// Vector-typed arithmetic, split off the scalar binary-op arm.
    ///
    /// inkwell's integer builders are generic over `IntMathValue`, which
    /// `VectorValue` implements — but the scalar arm reaches its operands via
    /// `into_int_value()`, which *panics* on a vector rather than returning an
    /// error. Only the two ops `channel.rs` emits are accepted.
    pub(super) fn vector_binary(
        &mut self,
        op: &str,
        name: &str,
        a: VectorValue<'ctx>,
        b: VectorValue<'ctx>,
    ) -> Result<BasicValueEnum<'ctx>> {
        Ok(match op {
            "add" => self.builder.build_int_add(a, b, name).map_err(be)?.into(),
            "mul" => self.builder.build_int_mul(a, b, name).map_err(be)?.into(),
            other => bail!(
                "vector binary op `{other}` is not emitted by perry-codegen — \
                 add it here and to the emitter's test if that changed"
            ),
        })
    }

    /// Parse one `<N x T> VALUE` operand and require it to be a vector.
    fn vector_operand(&mut self, op: &str, part: &str) -> Result<VectorValue<'ctx>> {
        let (ty_tok, tok) = ty_and_val(part)?;
        let ty = basic_type(self.ctx, ty_tok)?;
        if !matches!(ty, BasicTypeEnum::VectorType(_)) {
            bail!("{op} operand `{part}` is not a vector type");
        }
        let v = self.val(ty, tok)?;
        match v {
            BasicValueEnum::VectorValue(v) => Ok(v),
            other => bail!("{op} operand `{part}` resolved to a non-vector {other:?}"),
        }
    }

    /// Parse a lane index operand (`i32 1`).
    fn vector_index(&mut self, op: &str, part: &str) -> Result<IntValue<'ctx>> {
        let (ty_tok, tok) = ty_and_val(part)?;
        let ty = basic_type(self.ctx, ty_tok)?;
        if !matches!(ty, BasicTypeEnum::IntType(_)) {
            bail!("{op} index `{part}` is not an integer");
        }
        match self.val(ty, tok)? {
            BasicValueEnum::IntValue(v) => Ok(v),
            other => bail!("{op} index `{part}` resolved to a non-integer {other:?}"),
        }
    }
}
