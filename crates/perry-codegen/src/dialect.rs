//! Reader for Perry's LLVM-IR dialect: consumes one finalized function's
//! text (`LlFunction::to_ir()` output) and constructs it natively in an
//! inkwell module. See `native_emit.rs` for where this sits in Phase 2.
//!
//! This is NOT a general LLVM parser — it accepts the closed set of
//! instruction, type, and operand forms perry-codegen emits (bounded by the
//! corpus census in the experiment doc) and errors on anything else, so a
//! new emission form fails loudly at construction instead of silently
//! diverging. The LLVM verifier and `PERRY_LLVM_INPROCESS=diff` are the
//! nets behind it.
//!
//! Instructions keep their textual `%names`, so a natively-built function
//! prints comparably to the text-parsed arm in diff mode.

use std::collections::HashMap;

use anyhow::{anyhow, bail, Context as AnyhowContext, Result};
use inkwell::basic_block::BasicBlock;
use inkwell::builder::Builder;
use inkwell::context::Context;
use inkwell::module::{Linkage, Module};
use inkwell::types::{BasicType, BasicTypeEnum, FunctionType};
use inkwell::values::{
    AsValueRef, BasicMetadataValueEnum, BasicValue, BasicValueEnum, FunctionValue,
    InstructionValue, PhiValue,
};
use inkwell::{AddressSpace, FloatPredicate, IntPredicate};

/// Create (only) the function declaration for `fn_text`'s define header, so
/// later-defined functions are callable while earlier bodies are read. The
/// native path pre-declares every define before reading any body — calls to
/// module-internal functions are forward references at module scope, exactly
/// like registers are at function scope.
pub(crate) fn predeclare_function_from_text<'ctx>(
    context: &'ctx Context,
    module: &Module<'ctx>,
    fn_text: &str,
) -> Result<()> {
    let header = fn_text
        .lines()
        .next()
        .ok_or_else(|| anyhow!("empty function text"))?;
    FnReader::declare_from_header(context, module, header)?;
    Ok(())
}

/// Streaming construction: begin from a define header, feed finalized body
/// lines one at a time (the `LlFunction::for_each_final_line` seam — no
/// per-function text materialization), then `finish`.
pub(crate) struct FnStream<'ctx, 'm> {
    inner: FnReader<'ctx, 'm>,
}

impl<'ctx, 'm> FnStream<'ctx, 'm> {
    pub(crate) fn begin(
        context: &'ctx Context,
        module: &'m Module<'ctx>,
        header: &str,
    ) -> Result<Self> {
        Ok(Self {
            inner: FnReader::begin(context, module, header)?,
        })
    }

    pub(crate) fn line(&mut self, line: &str) -> Result<()> {
        let t = line.trim();
        if t.is_empty() || t.starts_with(';') || t == "}" {
            return Ok(());
        }
        if let Some(label) = t.strip_suffix(':') {
            // Block label lines are never indented instructions.
            if !line.starts_with(' ') {
                return self.inner.enter_block(label);
            }
        }
        self.inner
            .instruction(t)
            .with_context(|| format!("in line: {t}"))
    }

    /// Consume one finalized item — the native fast path. Typed
    /// instructions construct directly; only labels, splices, and `Raw`
    /// payloads go through the line reader.
    pub(crate) fn item(&mut self, item: &crate::function::FinalItem<'_>) -> Result<()> {
        use crate::function::FinalItem as FI;
        match item {
            FI::Label(l) => self.inner.enter_block(l),
            FI::Blank => Ok(()),
            FI::Text(t) => self.line(t),
            FI::Inst(crate::inst::LlInst::Raw(s)) => self.line(s),
            FI::Inst(i) => self.inner.typed(i),
        }
    }

    /// Returns `(typed, raw)` instruction counts — the migration ratchet.
    pub(crate) fn finish(self) -> Result<(usize, usize)> {
        self.inner.finish()
    }
}

/// Parse `fn_text` (a complete `define ... { ... }`) and build it into
/// `module`. Returns the number of instructions constructed.
pub(crate) fn add_function_from_text<'ctx>(
    context: &'ctx Context,
    module: &Module<'ctx>,
    fn_text: &str,
) -> Result<usize> {
    let mut lines = fn_text.lines();
    let header = lines.next().ok_or_else(|| anyhow!("empty function text"))?;
    let mut stream = FnStream::begin(context, module, header)?;
    for line in lines {
        stream.line(line)?;
    }
    stream.finish().map(|(t, r)| t + r)
}

struct FnReader<'ctx, 'm> {
    ctx: &'ctx Context,
    module: &'m Module<'ctx>,
    builder: Builder<'ctx>,
    func: FunctionValue<'ctx>,
    /// label -> basic block (created lazily on first reference).
    blocks: HashMap<String, BasicBlock<'ctx>>,
    /// `%name` -> value.
    vals: HashMap<String, BasicValueEnum<'ctx>>,
    /// Phi incomings deferred until every register is defined
    /// (loop-carried values are forward references at phi-build time).
    pending_phis: Vec<(PhiValue<'ctx>, BasicTypeEnum<'ctx>, Vec<(String, String)>)>,
    /// Non-phi forward references (measured in the corpus: the megamorphic
    /// `idispatch.tower` shape uses a register before its defining block
    /// appears in the text). Each unresolved `%reg` use gets a placeholder
    /// that is RAUW'd and erased when the real definition arrives — the
    /// same strategy LLVM's own `.ll` parser uses.
    ///
    /// The placeholder is a `load` from a scratch `alloca` — real
    /// instructions the builder can never constant-fold. The first version
    /// used `select true, undef, undef`, which the C-API builder folds to
    /// the uniqued constant `undef`: un-RAUW-able, verifier-clean, and it
    /// silently turned every non-phi forward reference into `undef` (caught
    /// as the class-family miscompile: `(number).set is not a function`).
    placeholders: HashMap<String, (BasicValueEnum<'ctx>, inkwell::values::PointerValue<'ctx>)>,
    /// Most recently *entered* (label line) block, used to keep the
    /// function's block order equal to textual order even when a forward
    /// branch created the block object earlier.
    last_entered: Option<BasicBlock<'ctx>>,
    positioned: bool,
    raw_count: usize,
    typed_count: usize,
}

/// Pieces of a `define [linkage] RET @NAME(T %p, ...) [attrs] {` line.
struct ParsedHeader {
    linkage: Option<Linkage>,
    ret_tok: String,
    name: String,
    /// (type token, `%name`) per parameter.
    params: Vec<(String, String)>,
    attr_str: String,
    /// `personality ptr @NAME` clause, if the define carries one (#7302:
    /// every function containing try/catch does).
    personality: Option<String>,
}

fn parse_header(header: &str) -> Result<ParsedHeader> {
    let rest = header
        .strip_prefix("define ")
        .ok_or_else(|| anyhow!("not a define line: {header}"))?;
    let rest = rest
        .strip_suffix('{')
        .ok_or_else(|| anyhow!("define line missing open brace"))?
        .trim_end();

    let mut toks = rest.split_whitespace().peekable();
    let mut linkage = None;
    if let Some(&t) = toks.peek() {
        match t {
            "internal" => {
                linkage = Some(Linkage::Internal);
                toks.next();
            }
            "private" => {
                linkage = Some(Linkage::Private);
                toks.next();
            }
            // Explicit default linkage (init_body functions carry it).
            "external" => {
                toks.next();
            }
            _ => {}
        }
    }
    let ret_tok = toks
        .next()
        .ok_or_else(|| anyhow!("define line missing return type"))?
        .to_string();

    let at = rest
        .find('@')
        .ok_or_else(|| anyhow!("define line missing @name"))?;
    let after = &rest[at + 1..];
    let paren = after
        .find('(')
        .ok_or_else(|| anyhow!("define line missing param list"))?;
    let name = unquote(&after[..paren]);
    let close = rmatch_paren(after, paren)?;
    let params_str = &after[paren + 1..close];
    let mut attr_str = after[close + 1..].trim().to_string();
    // `personality ptr @NAME` sits with the fn attributes on the define
    // line; lift it out so the attribute loop only sees real attributes.
    let mut personality = None;
    if let Some(pos) = attr_str.find("personality ") {
        let tail = attr_str[pos..].to_string();
        attr_str = attr_str[..pos].trim_end().to_string();
        let pname = tail
            .trim_start_matches("personality ")
            .trim()
            .trim_start_matches("ptr ")
            .trim()
            .trim_start_matches('@')
            .trim();
        if pname.is_empty() {
            bail!("malformed personality clause: {tail}");
        }
        personality = Some(unquote(pname));
    }

    let mut params = Vec::new();
    for p in split_top_level(params_str) {
        let p = p.trim();
        if p.is_empty() {
            continue;
        }
        let (ty_tok, name_tok) = p
            .rsplit_once(' ')
            .ok_or_else(|| anyhow!("bad param: {p}"))?;
        params.push((ty_tok.trim().to_string(), name_tok.trim().to_string()));
    }
    Ok(ParsedHeader {
        linkage,
        ret_tok,
        name,
        params,
        attr_str,
        personality,
    })
}

impl<'ctx, 'm> FnReader<'ctx, 'm> {
    /// Find-or-create the function for a define header, with its real
    /// signature and linkage. Idempotent: pre-declaration and body reading
    /// both land here.
    fn declare_from_header(
        ctx: &'ctx Context,
        module: &Module<'ctx>,
        header: &str,
    ) -> Result<FunctionValue<'ctx>> {
        let h = parse_header(header)?;
        let mut mtypes: Vec<inkwell::types::BasicMetadataTypeEnum> = Vec::new();
        for (ty_tok, _) in &h.params {
            mtypes.push(basic_type(ctx, ty_tok)?.into());
        }
        let fn_type = fn_type_of(ctx, &h.ret_tok, &mtypes)?;
        let func = match module.get_function(&h.name) {
            Some(f) => f,
            None => module.add_function(&h.name, fn_type, None),
        };
        if let Some(l) = h.linkage {
            func.set_linkage(l);
        }
        Ok(func)
    }

    fn begin(ctx: &'ctx Context, module: &'m Module<'ctx>, header: &str) -> Result<Self> {
        let h = parse_header(header)?;
        let func = Self::declare_from_header(ctx, module, header)?;
        // The skeleton never declares defined names (`skeleton_ir` filters
        // them), so a body on the function here means a duplicate define.
        if func.count_basic_blocks() > 0 {
            bail!("duplicate define of @{}", h.name);
        }
        let param_names: Vec<String> = h.params.iter().map(|(_, n)| n.clone()).collect();
        if let Some(pname) = &h.personality {
            let pf = module
                .get_function(pname)
                .ok_or_else(|| anyhow!("personality @{pname} not declared"))?;
            func.set_personality_function(pf);
        }
        let attr_str = h.attr_str;
        for a in attr_str.split_whitespace() {
            match a {
                // Attribute-group references: the only group perry stamps on
                // defines is #1 (noinline, the setjmp boundary). Group
                // contents live in the skeleton for *declares*; for defines
                // we apply the concrete attribute.
                "#1" => add_enum_attr(ctx, func, "noinline"),
                "alwaysinline" | "inlinehint" | "noinline" => add_enum_attr(ctx, func, a),
                other => bail!("unknown define attribute `{other}`"),
            }
        }
        for (i, pname) in param_names.iter().enumerate() {
            let pv = func
                .get_nth_param(i as u32)
                .ok_or_else(|| anyhow!("param {i} missing"))?;
            pv.set_name(pname.trim_start_matches('%'));
            // Params are registered under their `%name`.
        }

        let builder = ctx.create_builder();
        let mut vals = HashMap::new();
        for (i, pname) in param_names.iter().enumerate() {
            vals.insert(pname.clone(), func.get_nth_param(i as u32).unwrap());
        }
        Ok(Self {
            ctx,
            module,
            builder,
            func,
            blocks: HashMap::new(),
            vals,
            pending_phis: Vec::new(),
            placeholders: HashMap::new(),
            last_entered: None,
            positioned: false,
            raw_count: 0,
            typed_count: 0,
        })
    }

    fn block(&mut self, label: &str) -> BasicBlock<'ctx> {
        if let Some(b) = self.blocks.get(label) {
            return *b;
        }
        let b = self.ctx.append_basic_block(self.func, label);
        self.blocks.insert(label.to_string(), b);
        b
    }

    fn enter_block(&mut self, label: &str) -> Result<()> {
        let b = self.block(label);
        if let Some(prev) = self.last_entered {
            // A forward branch may have created this block before blocks
            // that textually precede it; re-anchor to textual order (block
            // order is semantically free but affects layout and makes the
            // diff harness exact).
            let _ = b.move_after(prev);
        }
        self.last_entered = Some(b);
        self.builder.position_at_end(b);
        self.positioned = true;
        Ok(())
    }

    fn def(&mut self, name: &str, v: BasicValueEnum<'ctx>) -> Result<()> {
        if let Some((ph, scratch)) = self.placeholders.remove(name) {
            rauw(name, ph, v)?;
            if let Some(inst) = ph.as_instruction_value() {
                let _ = inst.erase_from_basic_block();
            }
            if let Some(inst) = scratch.as_instruction_value() {
                let _ = inst.erase_from_basic_block();
            }
        }
        self.vals.insert(name.to_string(), v);
        Ok(())
    }

    /// Resolve `(type, token)` to a value. An undefined `%reg` becomes a
    /// typed placeholder resolved by a later `def` (see `placeholders`).
    fn val(&mut self, ty: BasicTypeEnum<'ctx>, tok: &str) -> Result<BasicValueEnum<'ctx>> {
        let tok = tok.trim();
        if let Some(v) = self.vals.get(tok) {
            return Ok(*v);
        }
        if tok.starts_with('%') {
            // Non-foldable placeholder: load from a scratch alloca. Both
            // instructions are erased when the definition arrives.
            let reg = tok.trim_start_matches('%');
            let scratch = self
                .builder
                .build_alloca(ty, &format!("fwdslot.{reg}"))
                .map_err(be)?;
            let ph = self
                .builder
                .build_load(ty, scratch, &format!("fwd.{reg}"))
                .map_err(be)?;
            debug_assert!(
                ph.as_instruction_value().is_some(),
                "placeholder must be a real instruction"
            );
            self.placeholders.insert(tok.to_string(), (ph, scratch));
            self.vals.insert(tok.to_string(), ph);
            return Ok(ph);
        }
        constant(self.ctx, self.module, ty, tok)
    }

    fn instruction(&mut self, t: &str) -> Result<()> {
        if !self.positioned {
            bail!("instruction before first block label");
        }
        self.raw_count += 1;
        // `%r = op ...` vs bare op.
        if let Some((lhs, rhs)) = t.split_once(" = ") {
            let (op, rest) = rhs
                .split_once(' ')
                .ok_or_else(|| anyhow!("malformed instruction"))?;
            self.value_inst(lhs.trim(), op, rest.trim())
        } else {
            let (op, rest) = t.split_once(' ').unwrap_or((t, ""));
            self.bare_inst(op, rest.trim())
        }
    }

    fn value_inst(&mut self, dst: &str, op: &str, rest: &str) -> Result<()> {
        let v: BasicValueEnum = match op {
            "alloca" => {
                // `alloca T[, align N]`
                let ty_tok = rest.split(',').next().unwrap_or(rest).trim();
                let ty = basic_type(self.ctx, ty_tok)?;
                let p = self
                    .builder
                    .build_alloca(ty, dst.trim_start_matches('%'))
                    .map_err(be)?;
                set_alignment_if_any(p.as_instruction_value(), rest);
                p.into()
            }
            "load" => {
                let mut rest2 = rest;
                let mut volatile = false;
                let mut atomic = false;
                loop {
                    if let Some(r) = rest2.strip_prefix("volatile ") {
                        volatile = true;
                        rest2 = r;
                    } else if let Some(r) = rest2.strip_prefix("atomic ") {
                        atomic = true;
                        rest2 = r;
                    } else {
                        break;
                    }
                }
                self.load(dst, volatile, atomic, rest2)?
            }
            "fneg" => {
                let (ty_tok, vtok) = ty_and_val(rest)?;
                let v = self.val(basic_type(self.ctx, ty_tok)?, vtok)?;
                self.builder
                    .build_float_neg(v.into_float_value(), dst.trim_start_matches('%'))
                    .map_err(be)?
                    .into()
            }
            "phi" => return self.phi(dst, rest),
            "call" | "tail call" => self
                .call(Some(dst), rest)?
                .ok_or_else(|| anyhow!("call with result had void type"))?,
            "invoke" => return self.invoke(Some(dst), rest).map(|_| ()),
            "landingpad" => return self.landingpad(dst, rest),
            "getelementptr" => self.gep(dst, rest)?,
            "select" => {
                // `select i1 C, T A, T B`
                let parts = split_top_level(rest);
                if parts.len() != 3 {
                    bail!("bad select");
                }
                let (cty, ctok) = ty_and_val(&parts[0])?;
                let cond = self.val(basic_type(self.ctx, cty)?, ctok)?;
                let (aty, atok) = ty_and_val(&parts[1])?;
                let ty = basic_type(self.ctx, aty)?;
                let a = self.val(ty, atok)?;
                let (_bty, btok) = ty_and_val(&parts[2])?;
                let bv = self.val(ty, btok)?;
                self.builder
                    .build_select(cond.into_int_value(), a, bv, dst.trim_start_matches('%'))
                    .map_err(be)?
            }
            _ => self.binary_or_cast(dst, op, rest)?,
        };
        self.def(dst, v)?;
        Ok(())
    }

    fn bare_inst(&mut self, op: &str, rest: &str) -> Result<()> {
        match op {
            "store" => {
                let volatile = rest.starts_with("volatile ");
                let rest = rest.strip_prefix("volatile ").unwrap_or(rest);
                // `store T V, ptr P[, align N]`
                let parts = split_top_level(rest);
                if parts.len() < 2 {
                    bail!("bad store");
                }
                let (vty, vtok) = ty_and_val(&parts[0])?;
                let ty = basic_type(self.ctx, vty)?;
                let v = self.val(ty, vtok)?;
                let (_pty, ptok) = ty_and_val(&parts[1])?;
                let p = self.val(self.ctx.ptr_type(AddressSpace::default()).into(), ptok)?;
                let inst = self
                    .builder
                    .build_store(p.into_pointer_value(), v)
                    .map_err(be)?;
                if volatile {
                    let _ = inst.set_volatile(true);
                }
                set_alignment_if_any(Some(inst), rest);
                Ok(())
            }
            "br" => {
                if let Some(lbl) = rest.strip_prefix("label %") {
                    let b = self.block(lbl.trim());
                    self.builder.build_unconditional_branch(b).map_err(be)?;
                } else {
                    // `br i1 C, label %A, label %B`
                    let parts = split_top_level(rest);
                    if parts.len() != 3 {
                        bail!("bad conditional br");
                    }
                    let (cty, ctok) = ty_and_val(&parts[0])?;
                    let c = self.val(basic_type(self.ctx, cty)?, ctok)?;
                    let a = self.block(strip_label(&parts[1])?);
                    let b = self.block(strip_label(&parts[2])?);
                    self.builder
                        .build_conditional_branch(c.into_int_value(), a, b)
                        .map_err(be)?;
                }
                Ok(())
            }
            "ret" => {
                if rest == "void" {
                    self.builder.build_return(None).map_err(be)?;
                } else {
                    let (ty_tok, vtok) = ty_and_val(rest)?;
                    let ty = basic_type(self.ctx, ty_tok)?;
                    let v = self.val(ty, vtok)?;
                    self.builder.build_return(Some(&v)).map_err(be)?;
                }
                Ok(())
            }
            "call" | "tail call" => self.call(None, rest).map(|_| ()),
            "invoke" => self.invoke(None, rest).map(|_| ()),
            "switch" => self.switch(rest),
            "unreachable" => {
                self.builder.build_unreachable().map_err(be)?;
                Ok(())
            }
            other => bail!("unknown instruction `{other}`"),
        }
    }

    fn load(
        &mut self,
        dst: &str,
        volatile: bool,
        atomic: bool,
        rest: &str,
    ) -> Result<BasicValueEnum<'ctx>> {
        // `T, ptr P[ <ordering>][, align N]`
        let parts = split_top_level(rest);
        if parts.len() < 2 {
            bail!("bad load");
        }
        let ty = basic_type(self.ctx, parts[0].trim())?;
        let (_pty, pval) = ty_and_val(&parts[1])?;
        // An atomic load's ordering keyword trails the pointer operand
        // (`ptr @g seq_cst`), measured in the corpus census.
        let mut ptoks = pval.split_whitespace();
        let ptok = ptoks.next().ok_or_else(|| anyhow!("bad load pointer"))?;
        let ordering = ptoks.next();
        let p = self.val(self.ctx.ptr_type(AddressSpace::default()).into(), ptok)?;
        let v = self
            .builder
            .build_load(ty, p.into_pointer_value(), dst.trim_start_matches('%'))
            .map_err(be)?;
        if volatile {
            let _ = v.as_instruction_value().map(|i| i.set_volatile(true));
        }
        if rest.contains("!invariant.load") {
            if let Some(i) = v.as_instruction_value() {
                set_invariant_load(self.ctx, i);
            }
        }
        if atomic {
            let ord = match ordering {
                Some("seq_cst") => {
                    llvm_sys::LLVMAtomicOrdering::LLVMAtomicOrderingSequentiallyConsistent
                }
                Some("acquire") => llvm_sys::LLVMAtomicOrdering::LLVMAtomicOrderingAcquire,
                Some("monotonic") => llvm_sys::LLVMAtomicOrdering::LLVMAtomicOrderingMonotonic,
                Some("unordered") => llvm_sys::LLVMAtomicOrdering::LLVMAtomicOrderingUnordered,
                other => bail!("unsupported atomic ordering {other:?}"),
            };
            if let Some(inst) = v.as_instruction_value() {
                unsafe { llvm_sys::core::LLVMSetOrdering(inst.as_value_ref(), ord) };
            }
        }
        set_alignment_if_any(v.as_instruction_value(), rest);
        Ok(v)
    }

    fn phi(&mut self, dst: &str, rest: &str) -> Result<()> {
        // `T [ V, %B ], [ V2, %B2 ], ...`
        let (ty_tok, incs) = rest.split_once('[').ok_or_else(|| anyhow!("bad phi"))?;
        let ty = basic_type(self.ctx, ty_tok.trim())?;
        let phi = self
            .builder
            .build_phi(ty, dst.trim_start_matches('%'))
            .map_err(be)?;
        let mut pending = Vec::new();
        for pair in format!("[{incs}").split(']') {
            let pair = pair.trim().trim_start_matches(',').trim();
            let Some(body) = pair.strip_prefix('[') else {
                continue;
            };
            let (v, b) = body
                .split_once(',')
                .ok_or_else(|| anyhow!("bad phi incoming: {body}"))?;
            pending.push((
                v.trim().to_string(),
                b.trim().trim_start_matches('%').to_string(),
            ));
        }
        self.def(dst, phi.as_basic_value())?;
        self.pending_phis.push((phi, ty, pending));
        Ok(())
    }

    fn gep(&mut self, dst: &str, rest: &str) -> Result<BasicValueEnum<'ctx>> {
        let (inbounds, rest) = match rest.strip_prefix("inbounds ") {
            Some(r) => (true, r),
            None => (false, rest),
        };
        let parts = split_top_level(rest);
        if parts.len() < 2 {
            bail!("bad getelementptr");
        }
        let pointee = basic_type(self.ctx, parts[0].trim())?;
        let (_pty, ptok) = ty_and_val(&parts[1])?;
        let p = self
            .val(self.ctx.ptr_type(AddressSpace::default()).into(), ptok)?
            .into_pointer_value();
        let mut idx = Vec::new();
        for part in &parts[2..] {
            let (ity, itok) = ty_and_val(part)?;
            idx.push(self.val(basic_type(self.ctx, ity)?, itok)?.into_int_value());
        }
        let name = dst.trim_start_matches('%');
        let v = unsafe {
            if inbounds {
                self.builder.build_in_bounds_gep(pointee, p, &idx, name)
            } else {
                self.builder.build_gep(pointee, p, &idx, name)
            }
        }
        .map_err(be)?;
        Ok(v.into())
    }

    fn switch(&mut self, rest: &str) -> Result<()> {
        // `switch T V, label %DEF [ T C1, label %B1 T C2, label %B2 ... ]`
        // (zero occurrences in the corpus census; kept because the semantic
        // surface can emit it and losing it would fail loudly anyway).
        let (head, cases) = rest.split_once('[').ok_or_else(|| anyhow!("bad switch"))?;
        let head_parts = split_top_level(head.trim());
        if head_parts.len() != 2 {
            bail!("bad switch head");
        }
        let (vty, vtok) = ty_and_val(&head_parts[0])?;
        let ty = basic_type(self.ctx, vty)?;
        let v = self.val(ty, vtok)?.into_int_value();
        let def = self.block(strip_label(&head_parts[1])?);
        // Case list: `T C, label %B` repeated, whitespace-separated.
        let mut pairs = Vec::new();
        let toks: Vec<&str> = cases
            .trim()
            .trim_end_matches(']')
            .split_whitespace()
            .collect();
        let mut i = 0;
        while i + 3 < toks.len() + 1 {
            if i + 4 > toks.len() {
                break;
            }
            let cty = toks[i];
            let ctok = toks[i + 1].trim_end_matches(',');
            if toks[i + 2] != "label" {
                bail!("bad switch case near `{}`", toks[i..].join(" "));
            }
            let bb = self.block(toks[i + 3].trim_start_matches('%'));
            let cv = self.val(basic_type(self.ctx, cty)?, ctok)?.into_int_value();
            pairs.push((cv, bb));
            i += 4;
        }
        self.builder.build_switch(v, def, &pairs).map_err(be)?;
        Ok(())
    }

    /// Calls: direct `@f(...)`, indirect `%fp(...)` (with or without a
    /// pre-opaque `(sig)*` type), and the inline-asm barrier.
    fn call(&mut self, dst: Option<&str>, rest: &str) -> Result<Option<BasicValueEnum<'ctx>>> {
        // Optional fast-math prefix tokens on float-returning calls.
        let rest = rest
            .trim_start_matches("reassoc ")
            .trim_start_matches("contract ")
            .trim_start();

        // Inline asm: `void asm sideeffect "ASM", "CONSTRAINTS"(ARGS)`
        if let Some(asm_rest) = rest.strip_prefix("void asm ") {
            return self.call_asm(asm_rest).map(|_| None);
        }

        // Return type is the first top-level token; everything from the
        // callee marker on is `CALLEE(ARGS)[ #attrs]`.
        let callee_pos = rest
            .find(['@', '%'])
            .ok_or_else(|| anyhow!("call without callee"))?;
        let sig_str = rest[..callee_pos].trim().trim_end_matches('*').trim();
        let after = &rest[callee_pos..];
        let paren = after
            .find('(')
            .ok_or_else(|| anyhow!("call missing arg list"))?;
        let callee = &after[..paren];
        let close = rmatch_paren(after, paren)?;
        let args_str = &after[paren + 1..close];
        // Trailing callsite attribute-group ref. Only `#0` (returns_twice,
        // on setjmp calls) exists in the dialect; the declare carries it too,
        // so this is fidelity, not correctness.
        let trailing_attr = after[close + 1..].trim();

        let mut args: Vec<BasicMetadataValueEnum> = Vec::new();
        let mut arg_types: Vec<inkwell::types::BasicMetadataTypeEnum> = Vec::new();
        for a in split_top_level(args_str) {
            let (aty, atok) = ty_and_val(&a)?;
            let ty = basic_type(self.ctx, aty)?;
            args.push(self.val(ty, atok)?.into());
            arg_types.push(ty.into());
        }

        let name = dst.map(|d| d.trim_start_matches('%')).unwrap_or("");
        // The call's function type is the CALLSITE's, not the callee's
        // declared signature — under opaque pointers LLVM accepts (and Perry
        // emits) direct calls whose argument types differ from the declare
        // (measured: `js_native_call_value` declared `(double, i64, i64)`,
        // called with `(double, ptr, i64)`). Deriving the type from the
        // callsite and calling through the callee *pointer* reproduces the
        // text parser's semantics exactly; LLVM still prints it as a direct
        // call.
        let fn_ty = indirect_fn_type(self.ctx, sig_str, &arg_types)?;
        let callee_ptr = if let Some(fname) = callee.strip_prefix('@') {
            let name = unquote(fname);
            let f = match self.module.get_function(&name) {
                Some(f) => f,
                // LLVM's own .ll parser auto-declares `@llvm.*` intrinsics at
                // their first use (perry's clamp lowering emits smax/smin
                // calls with no textual declare). Mirror that — for
                // intrinsics ONLY; anything else undeclared stays a loud
                // error.
                None if name.starts_with("llvm.") => self.module.add_function(&name, fn_ty, None),
                None => bail!("call to undeclared @{name}"),
            };
            f.as_global_value().as_pointer_value()
        } else {
            self.val(self.ctx.ptr_type(AddressSpace::default()).into(), callee)?
                .into_pointer_value()
        };
        let site = self
            .builder
            .build_indirect_call(fn_ty, callee_ptr, &args, name)
            .map_err(be)?;
        match trailing_attr {
            "" => {}
            "#0" => {
                let kind = inkwell::attributes::Attribute::get_named_enum_kind_id("returns_twice");
                if kind != 0 {
                    site.add_attribute(
                        inkwell::attributes::AttributeLoc::Function,
                        self.ctx.create_enum_attribute(kind, 0),
                    );
                }
            }
            other => bail!("unknown callsite attribute `{other}`"),
        }
        match site.try_as_basic_value() {
            inkwell::values::ValueKind::Basic(v) => Ok(Some(v)),
            _ => Ok(None),
        }
    }

    /// `[%r =] invoke TY @callee(ARGS) to label %CONT unwind label %PAD`
    /// (#7302). Shares the callsite-typed, call-through-pointer semantics
    /// of [`call`]; the only additions are the two edges.
    fn invoke(&mut self, dst: Option<&str>, rest: &str) -> Result<Option<BasicValueEnum<'ctx>>> {
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
    fn landingpad(&mut self, dst: &str, rest: &str) -> Result<()> {
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

    fn call_asm(&mut self, rest: &str) -> Result<()> {
        // `sideeffect "ASM", "CONSTR"()` — the only asm perry emits is the
        // empty barrier, but parse the strings properly anyway.
        let sideeffect = rest.trim_start().starts_with("sideeffect");
        let mut strings = rest.split('"');
        let asm = strings.nth(1).unwrap_or("").to_string();
        let constraints = strings.nth(1).unwrap_or("").to_string();
        let void_fn = self.ctx.void_type().fn_type(&[], false);
        let ptr =
            self.ctx
                .create_inline_asm(void_fn, asm, constraints, sideeffect, false, None, false);
        self.builder
            .build_indirect_call(void_fn, ptr, &[], "")
            .map_err(be)?;
        Ok(())
    }

    fn binary_or_cast(&mut self, dst: &str, op: &str, rest: &str) -> Result<BasicValueEnum<'ctx>> {
        let name = dst.trim_start_matches('%');
        // Casts: `OP T V to T2`
        if let Some((src, dst_ty_tok)) = rest.rsplit_once(" to ") {
            let (sty, stok) = ty_and_val(src)?;
            let sty_t = basic_type(self.ctx, sty)?;
            let v = self.val(sty_t, stok)?;
            let dty = basic_type(self.ctx, dst_ty_tok.trim())?;
            let out = match op {
                "bitcast" => self.builder.build_bit_cast(v, dty, name).map_err(be)?,
                "zext" => self
                    .builder
                    .build_int_z_extend(v.into_int_value(), dty.into_int_type(), name)
                    .map_err(be)?
                    .into(),
                "sext" => self
                    .builder
                    .build_int_s_extend(v.into_int_value(), dty.into_int_type(), name)
                    .map_err(be)?
                    .into(),
                "trunc" => self
                    .builder
                    .build_int_truncate(v.into_int_value(), dty.into_int_type(), name)
                    .map_err(be)?
                    .into(),
                "fptosi" => self
                    .builder
                    .build_float_to_signed_int(v.into_float_value(), dty.into_int_type(), name)
                    .map_err(be)?
                    .into(),
                "fptoui" => self
                    .builder
                    .build_float_to_unsigned_int(v.into_float_value(), dty.into_int_type(), name)
                    .map_err(be)?
                    .into(),
                "sitofp" => self
                    .builder
                    .build_signed_int_to_float(v.into_int_value(), dty.into_float_type(), name)
                    .map_err(be)?
                    .into(),
                "uitofp" => self
                    .builder
                    .build_unsigned_int_to_float(v.into_int_value(), dty.into_float_type(), name)
                    .map_err(be)?
                    .into(),
                "fpext" => self
                    .builder
                    .build_float_ext(v.into_float_value(), dty.into_float_type(), name)
                    .map_err(be)?
                    .into(),
                "fptrunc" => self
                    .builder
                    .build_float_trunc(v.into_float_value(), dty.into_float_type(), name)
                    .map_err(be)?
                    .into(),
                "ptrtoint" => self
                    .builder
                    .build_ptr_to_int(v.into_pointer_value(), dty.into_int_type(), name)
                    .map_err(be)?
                    .into(),
                "inttoptr" => self
                    .builder
                    .build_int_to_ptr(v.into_int_value(), dty.into_pointer_type(), name)
                    .map_err(be)?
                    .into(),
                other => bail!("unknown cast `{other}`"),
            };
            return Ok(out);
        }

        // icmp / fcmp
        if op == "icmp" || op == "fcmp" {
            let (pred, rest2) = rest.split_once(' ').ok_or_else(|| anyhow!("bad {op}"))?;
            let parts = split_top_level(rest2);
            if parts.len() != 2 {
                bail!("bad {op} operands");
            }
            let (ty_tok, atok) = ty_and_val(&parts[0])?;
            let ty = basic_type(self.ctx, ty_tok)?;
            let a = self.val(ty, atok)?;
            let b2 = self.val(ty, parts[1].trim())?;
            let out: BasicValueEnum = if op == "icmp" {
                // `icmp eq ptr %x, null` is common (null checks) — pointers
                // compare directly, same as the text form.
                if a.is_pointer_value() {
                    self.builder
                        .build_int_compare(
                            int_pred(pred)?,
                            a.into_pointer_value(),
                            b2.into_pointer_value(),
                            name,
                        )
                        .map_err(be)?
                        .into()
                } else {
                    self.builder
                        .build_int_compare(
                            int_pred(pred)?,
                            a.into_int_value(),
                            b2.into_int_value(),
                            name,
                        )
                        .map_err(be)?
                        .into()
                }
            } else {
                self.builder
                    .build_float_compare(
                        float_pred(pred)?,
                        a.into_float_value(),
                        b2.into_float_value(),
                        name,
                    )
                    .map_err(be)?
                    .into()
            };
            return Ok(out);
        }

        // Binary ops: `[flags] T A, B` where op may carry nsw/nuw/fmf tokens.
        let op_base = op;
        let mut rest2 = rest;
        let mut flag_tokens: Vec<&str> = Vec::new();
        loop {
            let (first, r) = rest2.split_once(' ').unwrap_or((rest2, ""));
            match first {
                "nsw" | "nuw" | "reassoc" | "contract" | "arcp" | "afn" | "fast" | "nnan"
                | "ninf" | "nsz" | "exact" | "disjoint" => {
                    flag_tokens.push(first);
                    rest2 = r;
                }
                _ => break,
            }
        }
        let parts = split_top_level(rest2);
        if parts.len() != 2 {
            bail!("bad binary op `{op}` operands: {rest}");
        }
        let (ty_tok, atok) = ty_and_val(&parts[0])?;
        let ty = basic_type(self.ctx, ty_tok)?;
        let a = self.val(ty, atok)?;
        let b2 = self.val(ty, parts[1].trim())?;
        let nm = name;
        let out: BasicValueEnum = match op_base {
            "add" => self
                .builder
                .build_int_add(a.into_int_value(), b2.into_int_value(), nm)
                .map_err(be)?
                .into(),
            "sub" => self
                .builder
                .build_int_sub(a.into_int_value(), b2.into_int_value(), nm)
                .map_err(be)?
                .into(),
            "mul" => self
                .builder
                .build_int_mul(a.into_int_value(), b2.into_int_value(), nm)
                .map_err(be)?
                .into(),
            "sdiv" => self
                .builder
                .build_int_signed_div(a.into_int_value(), b2.into_int_value(), nm)
                .map_err(be)?
                .into(),
            "udiv" => self
                .builder
                .build_int_unsigned_div(a.into_int_value(), b2.into_int_value(), nm)
                .map_err(be)?
                .into(),
            "srem" => self
                .builder
                .build_int_signed_rem(a.into_int_value(), b2.into_int_value(), nm)
                .map_err(be)?
                .into(),
            "urem" => self
                .builder
                .build_int_unsigned_rem(a.into_int_value(), b2.into_int_value(), nm)
                .map_err(be)?
                .into(),
            "and" => self
                .builder
                .build_and(a.into_int_value(), b2.into_int_value(), nm)
                .map_err(be)?
                .into(),
            "or" => self
                .builder
                .build_or(a.into_int_value(), b2.into_int_value(), nm)
                .map_err(be)?
                .into(),
            "xor" => self
                .builder
                .build_xor(a.into_int_value(), b2.into_int_value(), nm)
                .map_err(be)?
                .into(),
            "shl" => self
                .builder
                .build_left_shift(a.into_int_value(), b2.into_int_value(), nm)
                .map_err(be)?
                .into(),
            "lshr" => self
                .builder
                .build_right_shift(a.into_int_value(), b2.into_int_value(), false, nm)
                .map_err(be)?
                .into(),
            "ashr" => self
                .builder
                .build_right_shift(a.into_int_value(), b2.into_int_value(), true, nm)
                .map_err(be)?
                .into(),
            "fadd" => self
                .builder
                .build_float_add(a.into_float_value(), b2.into_float_value(), nm)
                .map_err(be)?
                .into(),
            "fsub" => self
                .builder
                .build_float_sub(a.into_float_value(), b2.into_float_value(), nm)
                .map_err(be)?
                .into(),
            "fmul" => self
                .builder
                .build_float_mul(a.into_float_value(), b2.into_float_value(), nm)
                .map_err(be)?
                .into(),
            "fdiv" => self
                .builder
                .build_float_div(a.into_float_value(), b2.into_float_value(), nm)
                .map_err(be)?
                .into(),
            "frem" => self
                .builder
                .build_float_rem(a.into_float_value(), b2.into_float_value(), nm)
                .map_err(be)?
                .into(),
            other => bail!("unknown instruction `{other}`"),
        };
        apply_flags(out.as_instruction_value(), &flag_tokens);
        Ok(out)
    }

    /// Construct one typed instruction directly — the no-text fast path.
    /// Each arm mirrors the corresponding string-parser arm exactly; the
    /// parser remains authoritative for `Raw` lines and the corpus tests.
    fn typed(&mut self, inst: &crate::inst::LlInst) -> Result<()> {
        use crate::inst::{LlInst as I, LoadFlavor as LF};
        if !self.positioned {
            bail!("instruction before first block label");
        }
        self.typed_count += 1;
        match inst {
            I::Raw(_) => unreachable!("Raw goes through the line reader"),
            I::Bin {
                dst,
                op,
                pre,
                ty,
                a,
                b,
            } => {
                let t = basic_type(self.ctx, ty)?;
                let av = self.val(t, a)?;
                let bv = self.val(t, b)?;
                let name = dst.trim_start_matches('%');
                let out: BasicValueEnum = match *op {
                    "add" => self
                        .builder
                        .build_int_add(av.into_int_value(), bv.into_int_value(), name)
                        .map_err(be)?
                        .into(),
                    "sub" => self
                        .builder
                        .build_int_sub(av.into_int_value(), bv.into_int_value(), name)
                        .map_err(be)?
                        .into(),
                    "mul" => self
                        .builder
                        .build_int_mul(av.into_int_value(), bv.into_int_value(), name)
                        .map_err(be)?
                        .into(),
                    "sdiv" => self
                        .builder
                        .build_int_signed_div(av.into_int_value(), bv.into_int_value(), name)
                        .map_err(be)?
                        .into(),
                    "srem" => self
                        .builder
                        .build_int_signed_rem(av.into_int_value(), bv.into_int_value(), name)
                        .map_err(be)?
                        .into(),
                    "and" => self
                        .builder
                        .build_and(av.into_int_value(), bv.into_int_value(), name)
                        .map_err(be)?
                        .into(),
                    "or" => self
                        .builder
                        .build_or(av.into_int_value(), bv.into_int_value(), name)
                        .map_err(be)?
                        .into(),
                    "xor" => self
                        .builder
                        .build_xor(av.into_int_value(), bv.into_int_value(), name)
                        .map_err(be)?
                        .into(),
                    "shl" => self
                        .builder
                        .build_left_shift(av.into_int_value(), bv.into_int_value(), name)
                        .map_err(be)?
                        .into(),
                    "lshr" => self
                        .builder
                        .build_right_shift(av.into_int_value(), bv.into_int_value(), false, name)
                        .map_err(be)?
                        .into(),
                    "ashr" => self
                        .builder
                        .build_right_shift(av.into_int_value(), bv.into_int_value(), true, name)
                        .map_err(be)?
                        .into(),
                    "fadd" => self
                        .builder
                        .build_float_add(av.into_float_value(), bv.into_float_value(), name)
                        .map_err(be)?
                        .into(),
                    "fsub" => self
                        .builder
                        .build_float_sub(av.into_float_value(), bv.into_float_value(), name)
                        .map_err(be)?
                        .into(),
                    "fmul" => self
                        .builder
                        .build_float_mul(av.into_float_value(), bv.into_float_value(), name)
                        .map_err(be)?
                        .into(),
                    "fdiv" => self
                        .builder
                        .build_float_div(av.into_float_value(), bv.into_float_value(), name)
                        .map_err(be)?
                        .into(),
                    "frem" => self
                        .builder
                        .build_float_rem(av.into_float_value(), bv.into_float_value(), name)
                        .map_err(be)?
                        .into(),
                    other => bail!("typed Bin: unknown op `{other}`"),
                };
                if !pre.is_empty() {
                    let toks: Vec<&str> = pre.split_whitespace().collect();
                    apply_flags(out.as_instruction_value(), &toks);
                }
                self.def(dst, out)
            }
            I::FNeg { dst, pre, a } => {
                let t = basic_type(self.ctx, "double")?;
                let av = self.val(t, a)?;
                let out: BasicValueEnum = self
                    .builder
                    .build_float_neg(av.into_float_value(), dst.trim_start_matches('%'))
                    .map_err(be)?
                    .into();
                if !pre.is_empty() {
                    let toks: Vec<&str> = pre.split_whitespace().collect();
                    apply_flags(out.as_instruction_value(), &toks);
                }
                self.def(dst, out)
            }
            I::FCmp { dst, pred, a, b } => {
                let t = basic_type(self.ctx, "double")?;
                let av = self.val(t, a)?;
                let bv = self.val(t, b)?;
                let out: BasicValueEnum = self
                    .builder
                    .build_float_compare(
                        float_pred(pred)?,
                        av.into_float_value(),
                        bv.into_float_value(),
                        dst.trim_start_matches('%'),
                    )
                    .map_err(be)?
                    .into();
                self.def(dst, out)
            }
            I::ICmp {
                dst,
                pred,
                ty,
                a,
                b,
            } => {
                let t = basic_type(self.ctx, ty)?;
                let av = self.val(t, a)?;
                let bv = self.val(t, b)?;
                let name = dst.trim_start_matches('%');
                let out: BasicValueEnum = if av.is_pointer_value() {
                    self.builder
                        .build_int_compare(
                            int_pred(pred)?,
                            av.into_pointer_value(),
                            bv.into_pointer_value(),
                            name,
                        )
                        .map_err(be)?
                        .into()
                } else {
                    self.builder
                        .build_int_compare(
                            int_pred(pred)?,
                            av.into_int_value(),
                            bv.into_int_value(),
                            name,
                        )
                        .map_err(be)?
                        .into()
                };
                self.def(dst, out)
            }
            I::Alloca { dst, ty } => {
                let t = basic_type(self.ctx, ty)?;
                let out = self
                    .builder
                    .build_alloca(t, dst.trim_start_matches('%'))
                    .map_err(be)?;
                self.def(dst, out.into())
            }
            I::Load {
                dst,
                ty,
                ptr,
                flavor,
            } => {
                let t = basic_type(self.ctx, ty)?;
                let p = self
                    .val(self.ctx.ptr_type(AddressSpace::default()).into(), ptr)?
                    .into_pointer_value();
                let v = self
                    .builder
                    .build_load(t, p, dst.trim_start_matches('%'))
                    .map_err(be)?;
                match flavor {
                    LF::Plain => {}
                    LF::Aligned(n) => {
                        let _ = v.as_instruction_value().map(|i| i.set_alignment(*n));
                    }
                    LF::Volatile => {
                        let _ = v.as_instruction_value().map(|i| i.set_volatile(true));
                    }
                    LF::AtomicSeqCst(n) => {
                        if let Some(i) = v.as_instruction_value() {
                            unsafe {
                                llvm_sys::core::LLVMSetOrdering(
                                    i.as_value_ref(),
                                    llvm_sys::LLVMAtomicOrdering::LLVMAtomicOrderingSequentiallyConsistent,
                                )
                            };
                            let _ = i.set_alignment(*n);
                        }
                    }
                    LF::Invariant => {
                        if let Some(i) = v.as_instruction_value() {
                            set_invariant_load(self.ctx, i);
                        }
                    }
                }
                self.def(dst, v)
            }
            I::Store {
                ty,
                val,
                ptr,
                volatile,
                align,
            } => {
                let t = basic_type(self.ctx, ty)?;
                let v = self.val(t, val)?;
                let p = self
                    .val(self.ctx.ptr_type(AddressSpace::default()).into(), ptr)?
                    .into_pointer_value();
                let inst = self.builder.build_store(p, v).map_err(be)?;
                if *volatile {
                    let _ = inst.set_volatile(true);
                }
                if let Some(n) = align {
                    let _ = inst.set_alignment(*n);
                }
                Ok(())
            }
            I::Cast {
                dst,
                op,
                from,
                v,
                to,
            } => {
                let sty = basic_type(self.ctx, from)?;
                let sv = self.val(sty, v)?;
                let dty = basic_type(self.ctx, to)?;
                let name = dst.trim_start_matches('%');
                let out: BasicValueEnum = match *op {
                    "bitcast" => self.builder.build_bit_cast(sv, dty, name).map_err(be)?,
                    "zext" => self
                        .builder
                        .build_int_z_extend(sv.into_int_value(), dty.into_int_type(), name)
                        .map_err(be)?
                        .into(),
                    "sext" => self
                        .builder
                        .build_int_s_extend(sv.into_int_value(), dty.into_int_type(), name)
                        .map_err(be)?
                        .into(),
                    "trunc" => self
                        .builder
                        .build_int_truncate(sv.into_int_value(), dty.into_int_type(), name)
                        .map_err(be)?
                        .into(),
                    "fptosi" => self
                        .builder
                        .build_float_to_signed_int(sv.into_float_value(), dty.into_int_type(), name)
                        .map_err(be)?
                        .into(),
                    "fptoui" => self
                        .builder
                        .build_float_to_unsigned_int(
                            sv.into_float_value(),
                            dty.into_int_type(),
                            name,
                        )
                        .map_err(be)?
                        .into(),
                    "sitofp" => self
                        .builder
                        .build_signed_int_to_float(sv.into_int_value(), dty.into_float_type(), name)
                        .map_err(be)?
                        .into(),
                    "uitofp" => self
                        .builder
                        .build_unsigned_int_to_float(
                            sv.into_int_value(),
                            dty.into_float_type(),
                            name,
                        )
                        .map_err(be)?
                        .into(),
                    "fpext" => self
                        .builder
                        .build_float_ext(sv.into_float_value(), dty.into_float_type(), name)
                        .map_err(be)?
                        .into(),
                    "fptrunc" => self
                        .builder
                        .build_float_trunc(sv.into_float_value(), dty.into_float_type(), name)
                        .map_err(be)?
                        .into(),
                    "ptrtoint" => self
                        .builder
                        .build_ptr_to_int(sv.into_pointer_value(), dty.into_int_type(), name)
                        .map_err(be)?
                        .into(),
                    "inttoptr" => self
                        .builder
                        .build_int_to_ptr(sv.into_int_value(), dty.into_pointer_type(), name)
                        .map_err(be)?
                        .into(),
                    other => bail!("typed Cast: unknown op `{other}`"),
                };
                self.def(dst, out)
            }
            I::Select {
                dst,
                cond_ty,
                cond,
                ty,
                a,
                b,
            } => {
                let ct = basic_type(self.ctx, cond_ty)?;
                let cv = self.val(ct, cond)?;
                let t = basic_type(self.ctx, ty)?;
                let av = self.val(t, a)?;
                let bv = self.val(t, b)?;
                let out = self
                    .builder
                    .build_select(cv.into_int_value(), av, bv, dst.trim_start_matches('%'))
                    .map_err(be)?;
                self.def(dst, out)
            }
            I::Call {
                dst,
                ret,
                callee,
                args,
            } => {
                let mut argv: Vec<BasicMetadataValueEnum> = Vec::with_capacity(args.len());
                let mut argtys: Vec<inkwell::types::BasicMetadataTypeEnum> =
                    Vec::with_capacity(args.len());
                for (t, v) in args {
                    let bt = basic_type(self.ctx, t)?;
                    argv.push(self.val(bt, v)?.into());
                    argtys.push(bt.into());
                }
                let fn_ty = fn_type_of(self.ctx, ret, &argtys)?;
                let f = match self.module.get_function(callee) {
                    Some(f) => f,
                    None if callee.starts_with("llvm.") => {
                        self.module.add_function(callee, fn_ty, None)
                    }
                    None => bail!("call to undeclared @{callee}"),
                };
                let site = self
                    .builder
                    .build_indirect_call(
                        fn_ty,
                        f.as_global_value().as_pointer_value(),
                        &argv,
                        dst.as_deref()
                            .map(|d| d.trim_start_matches('%'))
                            .unwrap_or(""),
                    )
                    .map_err(be)?;
                if let Some(d) = dst {
                    match site.try_as_basic_value() {
                        inkwell::values::ValueKind::Basic(v) => self.def(d, v)?,
                        other => bail!("typed call expected a value, got {other:?}"),
                    }
                }
                Ok(())
            }
            I::CallIndirect {
                dst,
                ret,
                fptr,
                args,
            } => {
                let mut argv: Vec<BasicMetadataValueEnum> = Vec::with_capacity(args.len());
                let mut argtys: Vec<inkwell::types::BasicMetadataTypeEnum> =
                    Vec::with_capacity(args.len());
                for (t, v) in args {
                    let bt = basic_type(self.ctx, t)?;
                    argv.push(self.val(bt, v)?.into());
                    argtys.push(bt.into());
                }
                let fn_ty = fn_type_of(self.ctx, ret, &argtys)?;
                let fp = self
                    .val(self.ctx.ptr_type(AddressSpace::default()).into(), fptr)?
                    .into_pointer_value();
                let site = self
                    .builder
                    .build_indirect_call(fn_ty, fp, &argv, dst.trim_start_matches('%'))
                    .map_err(be)?;
                match site.try_as_basic_value() {
                    inkwell::values::ValueKind::Basic(v) => self.def(dst, v),
                    other => bail!("typed indirect call expected a value, got {other:?}"),
                }
            }
            I::AsmBarrier => {
                let void_fn = self.ctx.void_type().fn_type(&[], false);
                let ptr = self.ctx.create_inline_asm(
                    void_fn,
                    String::new(),
                    String::new(),
                    true,
                    false,
                    None,
                    false,
                );
                self.builder
                    .build_indirect_call(void_fn, ptr, &[], "")
                    .map_err(be)?;
                Ok(())
            }
            I::Br { label } => {
                let b = self.block(label);
                self.builder.build_unconditional_branch(b).map_err(be)?;
                Ok(())
            }
            I::CondBr { cond, t, f } => {
                let cv = self.val(self.ctx.bool_type().into(), cond)?;
                let tb = self.block(t);
                let fb = self.block(f);
                self.builder
                    .build_conditional_branch(cv.into_int_value(), tb, fb)
                    .map_err(be)?;
                Ok(())
            }
            I::Ret { ty, val } => {
                let t = basic_type(self.ctx, ty)?;
                let v = self.val(t, val)?;
                self.builder.build_return(Some(&v)).map_err(be)?;
                Ok(())
            }
            I::RetVoid => {
                self.builder.build_return(None).map_err(be)?;
                Ok(())
            }
            I::Unreachable => {
                self.builder.build_unreachable().map_err(be)?;
                Ok(())
            }
            I::Gep {
                dst,
                inbounds,
                ty,
                ptr,
                idxs,
            } => {
                let pointee = basic_type(self.ctx, ty)?;
                let p = self
                    .val(self.ctx.ptr_type(AddressSpace::default()).into(), ptr)?
                    .into_pointer_value();
                let mut iv = Vec::with_capacity(idxs.len());
                for (t, v) in idxs {
                    iv.push(self.val(basic_type(self.ctx, t)?, v)?.into_int_value());
                }
                let name = dst.trim_start_matches('%');
                let out = unsafe {
                    if *inbounds {
                        self.builder.build_in_bounds_gep(pointee, p, &iv, name)
                    } else {
                        self.builder.build_gep(pointee, p, &iv, name)
                    }
                }
                .map_err(be)?;
                self.def(dst, out.into())
            }
            I::Phi { dst, ty, pairs } => {
                let t = basic_type(self.ctx, ty)?;
                let phi = self
                    .builder
                    .build_phi(t, dst.trim_start_matches('%'))
                    .map_err(be)?;
                self.def(dst, phi.as_basic_value())?;
                self.pending_phis.push((phi, t, pairs.clone()));
                Ok(())
            }
        }
    }

    fn finish(mut self) -> Result<(usize, usize)> {
        // Phi incomings resolve against the COMPLETE value map — no
        // placeholders here; an unknown register at this point is a bug.
        for (phi, ty, incs) in std::mem::take(&mut self.pending_phis) {
            for (vtok, btok) in incs {
                let block = *self
                    .blocks
                    .get(&btok)
                    .ok_or_else(|| anyhow!("phi references unknown block %{btok}"))?;
                let v = if vtok.starts_with('%') {
                    *self
                        .vals
                        .get(&vtok)
                        .ok_or_else(|| anyhow!("phi references undefined register {vtok}"))?
                } else {
                    constant(self.ctx, self.module, ty, &vtok)?
                };
                phi.add_incoming(&[(&v as &dyn BasicValue, block)]);
            }
        }
        if let Some(name) = self.placeholders.keys().next() {
            bail!("register {name} was used but never defined");
        }
        Ok((self.typed_count, self.raw_count))
    }
}

/// Tag a load with `!invariant.load !0` (issue #52 semantics). Kind id via
/// llvm-sys; the node is the empty MDNode, same as the textual `!0 = !{}`.
fn set_invariant_load(ctx: &Context, inst: InstructionValue<'_>) {
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
fn rauw<'ctx>(name: &str, ph: BasicValueEnum<'ctx>, real: BasicValueEnum<'ctx>) -> Result<()> {
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

fn be(e: inkwell::builder::BuilderError) -> anyhow::Error {
    anyhow!("builder error: {e}")
}

fn unquote(s: &str) -> String {
    s.trim_matches('"').to_string()
}

/// Find the matching `)` for the `(` at `open` in `s`.
fn rmatch_paren(s: &str, open: usize) -> Result<usize> {
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
fn split_top_level(s: &str) -> Vec<String> {
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

/// `"i64 %r5"` -> `("i64", "%r5")`, honoring types with spaces (`[4 x i8]`,
/// `<4 x i32>`) and bracketed values (`<4 x i32> <i32 0, i32 1, ...>`): the
/// TYPE is parsed greedily from the left (balanced brackets, then trailing
/// `*`s), the remainder is the value.
fn ty_and_val(s: &str) -> Result<(&str, &str)> {
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
    Ok((s[..ty_end].trim(), s[ty_end..].trim()))
}

fn strip_label(s: &str) -> Result<&str> {
    s.trim()
        .strip_prefix("label %")
        .map(str::trim)
        .ok_or_else(|| anyhow!("expected `label %...`, got `{s}`"))
}

fn basic_type<'ctx>(ctx: &'ctx Context, tok: &str) -> Result<BasicTypeEnum<'ctx>> {
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

fn fn_type_of<'ctx>(
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
fn indirect_fn_type<'ctx>(
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

fn constant<'ctx>(
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
    Ok(match tok {
        "null" => ctx.ptr_type(AddressSpace::default()).const_null().into(),
        "undef" => match ty {
            BasicTypeEnum::FloatType(t) => t.get_undef().into(),
            BasicTypeEnum::IntType(t) => t.get_undef().into(),
            BasicTypeEnum::PointerType(t) => t.get_undef().into(),
            other => bail!("undef of unsupported type {other:?}"),
        },
        "poison" => match ty {
            BasicTypeEnum::FloatType(t) => t.get_poison().into(),
            BasicTypeEnum::IntType(t) => t.get_poison().into(),
            BasicTypeEnum::PointerType(t) => t.get_poison().into(),
            other => bail!("poison of unsupported type {other:?}"),
        },
        "true" => ctx.bool_type().const_int(1, false).into(),
        "false" => ctx.bool_type().const_int(0, false).into(),
        "zeroinitializer" => match ty {
            BasicTypeEnum::FloatType(t) => t.const_zero().into(),
            BasicTypeEnum::IntType(t) => t.const_zero().into(),
            BasicTypeEnum::ArrayType(t) => t.const_zero().into(),
            BasicTypeEnum::PointerType(t) => t.const_null().into(),
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
            other => bail!("cannot materialize `{tok}` as {other:?}"),
        },
    })
}

fn int_pred(p: &str) -> Result<IntPredicate> {
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

fn float_pred(p: &str) -> Result<FloatPredicate> {
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

fn add_enum_attr(ctx: &Context, func: FunctionValue<'_>, name: &str) {
    let kind = inkwell::attributes::Attribute::get_named_enum_kind_id(name);
    if kind != 0 {
        let attr = ctx.create_enum_attribute(kind, 0);
        func.add_attribute(inkwell::attributes::AttributeLoc::Function, attr);
    }
}

fn set_alignment_if_any(inst: Option<InstructionValue<'_>>, rest: &str) {
    if let (Some(inst), Some(idx)) = (inst, rest.rfind(", align ")) {
        if let Ok(align) = rest[idx + ", align ".len()..].trim().parse::<u32>() {
            let _ = inst.set_alignment(align);
        }
    }
}

fn apply_flags(inst: Option<InstructionValue<'_>>, flags: &[&str]) {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn split_corpus(text: &str) -> (String, Vec<String>) {
        let mut skeleton = String::new();
        let mut fns = Vec::new();
        let mut cur: Option<String> = None;
        for line in text.lines() {
            if line.starts_with("define ") {
                cur = Some(String::new());
            }
            match cur.as_mut() {
                Some(f) => {
                    f.push_str(line);
                    f.push('\n');
                    if line == "}" {
                        fns.push(cur.take().unwrap());
                    }
                }
                None => {
                    skeleton.push_str(line);
                    skeleton.push('\n');
                }
            }
        }
        (skeleton, fns)
    }

    /// Every function in a real perry-emitted corpus file must construct
    /// natively and pass the LLVM verifier. This is the reader's primary
    /// gate: a form it cannot express fails here, not in a user build.
    fn corpus_roundtrip(path: &str) {
        // The corpora are tracked in-tree alongside this reader, so a missing
        // file is a broken checkout, not a branch without artifacts. Skipping
        // would make the reader's primary gate pass vacuously — precisely the
        // failure mode the Linux bring-up had to rule out by hand.
        let text = std::fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("corpus file {path} is not readable: {e}"));
        let (skeleton, fns) = split_corpus(&text);
        let ctx = Context::create();
        let module = crate::inprocess::parse_ir_text(&ctx, &skeleton, "corpus_skel")
            .expect("skeleton parses");
        for f in &fns {
            predeclare_function_from_text(&ctx, &module, f)
                .unwrap_or_else(|e| panic!("predeclare: {e:#}"));
        }
        let mut n = 0usize;
        for f in &fns {
            n += add_function_from_text(&ctx, &module, f).unwrap_or_else(|e| panic!("{e:#}"));
        }
        assert!(
            n > 1000,
            "expected a real corpus, built only {n} instructions"
        );
        module
            .verify()
            .unwrap_or_else(|e| panic!("verifier rejected native module:\n{}", e.to_string()));
    }

    #[test]
    fn corpus_spike() {
        corpus_roundtrip(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../experiments/llvm-inprocess-spike/spike_text.ll"
        ));
    }

    /// #7302: a try/catch/finally corpus — invoke edges, landing pads,
    /// the personality clause on the define, and the inline continuation
    /// labels an invoke split leaves behind. Without this the reader's EH
    /// support would be exercised only by the async spike, which has one
    /// shape (the async rejection boundary) and no nesting.
    #[test]
    fn corpus_exception_handling() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../experiments/llvm-inprocess-spike/eh_text.ll"
        );
        // Assert the subject is LIVE: a corpus that lost its EH forms would
        // still round-trip, and would silently stop testing invoke.
        let text = std::fs::read_to_string(path).expect("eh corpus readable");
        assert!(
            text.matches("invoke ").count() >= 20,
            "eh corpus has no invoke edges left"
        );
        assert!(text.contains("landingpad"), "eh corpus has no landing pad");
        assert!(
            text.contains("personality ptr @perry_eh_personality"),
            "eh corpus lost its personality clause"
        );
        corpus_roundtrip(path);
    }

    #[test]
    fn corpus_batch_kernel() {
        corpus_roundtrip(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../experiments/llvm-inprocess-spike/batch_kernel.ll"
        ));
    }
}
