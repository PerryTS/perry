//! Exception-handling instruction construction for the in-process LLVM
//! reader (#7302): `invoke` edges and `landingpad`.
//!
//! Split out of `dialect/mod.rs` to keep that file under the 2000-line
//! cap; these are the forms Perry's try/catch lowering emits, and they
//! share the parent reader's state (block map, value map, builder).

use anyhow::{anyhow, bail, Result};
use inkwell::values::BasicValueEnum;
use inkwell::AddressSpace;

use super::{
    basic_type, be, indirect_fn_type, rmatch_paren, split_top_level, ty_and_val, unquote, FnReader,
};

impl<'ctx, 'm> FnReader<'ctx, 'm> {
    /// `[%r =] invoke TY @callee(ARGS) to label %CONT unwind label %PAD`
    /// (#7302). Shares the callsite-typed, call-through-pointer semantics
    /// of [`call`]; the only additions are the two edges.
    pub(super) fn invoke(
        &mut self,
        dst: Option<&str>,
        rest: &str,
    ) -> Result<Option<BasicValueEnum<'ctx>>> {
        let to_pos = rest
            .rfind(" to label %")
            .ok_or_else(|| anyhow!("invoke without `to label`"))?;
        let head = &rest[..to_pos];
        let edges = &rest[to_pos + " to label %".len()..];
        let (cont_label, unwind_label) = edges
            .split_once(" unwind label %")
            .ok_or_else(|| anyhow!("invoke without `unwind label`"))?;
        let cont = self.block(cont_label.trim());
        let pad = self.block(unwind_label.trim());

        let callee_pos = head
            .find(['@', '%'])
            .ok_or_else(|| anyhow!("invoke without callee"))?;
        let sig_str = head[..callee_pos].trim().trim_end_matches('*').trim();
        let after = &head[callee_pos..];
        let paren = after
            .find('(')
            .ok_or_else(|| anyhow!("invoke missing arg list"))?;
        let callee = &after[..paren];
        let close = rmatch_paren(after, paren)?;
        let args_str = &after[paren + 1..close];

        // `build_indirect_invoke` takes basic values (not metadata enums
        // like the call path), so collect both shapes once.
        let mut args: Vec<BasicValueEnum> = Vec::new();
        let mut arg_types: Vec<inkwell::types::BasicMetadataTypeEnum> = Vec::new();
        for a in split_top_level(args_str) {
            let (aty, atok) = ty_and_val(&a)?;
            let ty = basic_type(self.ctx, aty)?;
            args.push(self.val(ty, atok)?);
            arg_types.push(ty.into());
        }

        let name = dst.map(|d| d.trim_start_matches('%')).unwrap_or("");
        let fn_ty = indirect_fn_type(self.ctx, sig_str, &arg_types)?;
        let callee_ptr = if let Some(fname) = callee.strip_prefix('@') {
            let n = unquote(fname);
            let f = match self.module.get_function(&n) {
                Some(f) => f,
                None if n.starts_with("llvm.") => self.module.add_function(&n, fn_ty, None),
                None => bail!("invoke of undeclared @{n}"),
            };
            f.as_global_value().as_pointer_value()
        } else {
            self.val(self.ctx.ptr_type(AddressSpace::default()).into(), callee)?
                .into_pointer_value()
        };
        let site = self
            .builder
            .build_indirect_invoke(fn_ty, callee_ptr, &args, cont, pad, name)
            .map_err(be)?;
        // An invoke terminates its block; the emitted text continues in
        // the inline continuation label, which arrives as the next line.
        match site.try_as_basic_value() {
            inkwell::values::ValueKind::Basic(v) => {
                if let Some(d) = dst {
                    self.vals.insert(d.trim().to_string(), v);
                }
                Ok(Some(v))
            }
            _ => Ok(None),
        }
    }

    /// `%r = landingpad { ptr, i32 } catch ptr null` (#7302). Perry emits
    /// exactly one shape — a catch-all pad whose `{ptr, i32}` result is
    /// unused (the thrown value comes from the runtime's rooted TLS slot),
    /// so anything else is a dialect drift and bails loudly.
    pub(super) fn landingpad(&mut self, dst: &str, rest: &str) -> Result<()> {
        let body = rest.trim();
        let clause_pos = body
            .find("catch")
            .ok_or_else(|| anyhow!("landingpad without a catch clause: {body}"))?;
        let ty_tok = body[..clause_pos].trim();
        if ty_tok.replace(' ', "") != "{ptr,i32}" {
            bail!("unexpected landingpad type `{ty_tok}`");
        }
        if body[clause_pos..].replace(' ', "") != "catchptrnull" {
            bail!("unexpected landingpad clauses `{}`", &body[clause_pos..]);
        }
        let pf = self
            .func
            .get_personality_function()
            .ok_or_else(|| anyhow!("landingpad in a function with no personality"))?;
        let ptr_ty = self.ctx.ptr_type(AddressSpace::default());
        let exc_ty = self
            .ctx
            .struct_type(&[ptr_ty.into(), self.ctx.i32_type().into()], false);
        let null = ptr_ty.const_null();
        let v = self
            .builder
            .build_landing_pad(
                exc_ty,
                pf,
                &[null.into()],
                false,
                dst.trim_start_matches('%'),
            )
            .map_err(be)?;
        self.vals.insert(dst.trim().to_string(), v);
        Ok(())
    }
}
