//! A numeric indexed-operation region. Declarations select element widths;
//! live owner headers, scalar guards and interval guards prove semantics.
//! The fast region has only numeric operations/intrinsics, no user calls or
//! allocations/polls, hence cannot detach,
//! resize, rebind or relocate its storage. No validity state survives it.
//! Misses enter the original loop before any body effect. Aliases are allowed:
//! operations retain source order. Distinct receivers must have disjoint live
//! byte spans before the region may attach its own isolated alias scopes.
use crate::expr::FnCtx;
use crate::types::{DOUBLE, F32, I1, I16, I32, I64, I8};
use anyhow::Result;
use perry_hir::{BinaryOp, CompareOp, Expr, Stmt, UnaryOp, UpdateOp};
use std::collections::{BTreeMap, BTreeSet};

#[path = "indexed_owner_string.rs"]
mod string;
const STRING_KIND: u8 = 12;

#[derive(Default)]
struct Plan {
    counter: u32,
    arrays: BTreeMap<u32, u8>,
    strings: BTreeSet<u32>,
    aliases: BTreeMap<u32, u32>,
    defs: BTreeMap<u32, Expr>,
    inputs: BTreeSet<u32>,
    writes: BTreeSet<u32>,
    stores: BTreeSet<u32>,
    i32_writes: BTreeMap<u32, bool>,
    nested: BTreeMap<u32, i64>,
    carried: BTreeSet<u32>,
    depth: u32,
    indices: Vec<(u32, Expr)>,
    locals: BTreeSet<u32>,
    body_bindings: BTreeSet<u32>,
}

fn plain_binding(ctx: &FnCtx<'_>, id: u32) -> bool {
    !ctx.boxed_vars.contains(&id)
        && !ctx.closure_captures.contains_key(&id)
        && !ctx.scalar_replaced.contains_key(&id)
        && !ctx.pod_records.contains_key(&id)
        && (ctx.locals.contains_key(&id)
            || ctx.module_globals.contains_key(&id)
            || ctx.local_slot_reps.contains_key(&id))
}

impl Plan {
    fn receiver(&mut self, ctx: &FnCtx<'_>, e: &Expr) -> Option<u32> {
        let Expr::LocalGet(id) = e else { return None };
        let root = *self.aliases.get(id).unwrap_or(id);
        if self.body_bindings.contains(&root) || !plain_binding(ctx, root) {
            return None;
        }
        // A declaration or descriptor selects a width only. The preheader
        // still proves the actual live owner; tracked views/arenas miss it.
        let kind = ctx
            .local_type_hint(&root)
            .and_then(crate::expr::typed_element::type_kind)
            .or_else(|| {
                ctx.module_global_proven_types
                    .get(&root)
                    .and_then(crate::expr::typed_element::type_kind)
            })
            .or_else(|| crate::expr::ta_element_read::receiver_kind(ctx, &Expr::LocalGet(root)))?;
        if matches!(kind, 9 | 10) || self.strings.contains(&root) {
            return None;
        }
        self.arrays.insert(root, kind);
        Some(root)
    }

    fn read<'a>(&mut self, ctx: &FnCtx<'_>, e: &'a Expr) -> Option<(u32, &'a Expr)> {
        let (object, index) = match e {
            Expr::IndexGet { object, index } => (object.as_ref(), index.as_ref()),
            Expr::Uint8ArrayGet { array, index } => (array.as_ref(), index.as_ref()),
            Expr::BufferIndexGet { buffer, index } => (buffer.as_ref(), index.as_ref()),
            _ => return None,
        };
        Some((self.receiver(ctx, object)?, index))
    }

    fn string_read<'a>(&mut self, ctx: &FnCtx<'_>, e: &'a Expr) -> Option<(u32, &'a Expr)> {
        let (id, index) = string::read(e)?;
        if self.body_bindings.contains(&id)
            || !plain_binding(ctx, id)
            || self.arrays.contains_key(&id)
        {
            return None;
        }
        self.strings.insert(id);
        Some((id, index))
    }

    fn condition(&mut self, ctx: &FnCtx<'_>, e: &Expr) -> bool {
        match e {
            Expr::Bool(_) => true,
            Expr::Compare { left, right, .. } => {
                self.numeric(ctx, left) && self.numeric(ctx, right)
            }
            _ => false,
        }
    }

    fn numeric(&mut self, ctx: &FnCtx<'_>, e: &Expr) -> bool {
        if let Some((root, index)) = self.string_read(ctx, e) {
            if !self.numeric(ctx, index) {
                return false;
            }
            self.indices.push((root, index.clone()));
            return true;
        }
        if let Some((root, index)) = self.read(ctx, e) {
            if !self.numeric(ctx, index) {
                return false;
            }
            self.indices.push((root, index.clone()));
            return true;
        }
        match e {
            Expr::MathSqrt(v) => self.numeric(ctx, v),
            Expr::Call { callee, args, .. } => {
                numeric_function(ctx, callee).is_some_and(|f| args.len() == f.params.len())
                    && args.iter().all(|a| self.numeric(ctx, a))
            }
            Expr::Integer(_) | Expr::Number(_) => true,
            Expr::LocalGet(id) if *id == self.counter || self.locals.contains(id) => {
                !self.aliases.contains_key(id)
            }
            Expr::LocalGet(id) if !self.body_bindings.contains(id) && plain_binding(ctx, *id) => {
                self.inputs.insert(*id);
                true
            }
            Expr::Binary { op, left, right } if !matches!(op, BinaryOp::Pow) => {
                self.numeric(ctx, left) && self.numeric(ctx, right)
            }
            Expr::Unary {
                op: UnaryOp::Neg | UnaryOp::Pos | UnaryOp::BitNot,
                operand,
            } => self.numeric(ctx, operand),
            _ => false,
        }
    }

    fn expression(&mut self, ctx: &FnCtx<'_>, e: &Expr) -> bool {
        match e {
            Expr::Update { id, op, .. } => self.expression(
                ctx,
                &Expr::LocalSet(
                    *id,
                    Box::new(Expr::Binary {
                        op: if matches!(op, UpdateOp::Increment) {
                            BinaryOp::Add
                        } else {
                            BinaryOp::Sub
                        },
                        left: Box::new(Expr::LocalGet(*id)),
                        right: Box::new(Expr::Integer(1)),
                    }),
                ),
            ),
            Expr::LocalSet(id, value)
                if *id != self.counter
                    && !self.aliases.contains_key(id)
                    && !self.strings.contains(id)
                    && (self.locals.contains(id) || !self.body_bindings.contains(id))
                    && !self.defs.contains_key(id)
                    && (self.locals.contains(id)
                        || (plain_binding(ctx, *id)
                            && (ctx.locals.contains_key(id)
                                || ctx.local_slot_reps.contains_key(id)))) =>
            {
                if self.nested.contains_key(id) || !self.numeric(ctx, value) {
                    return false;
                }
                let signed = matches!(
                    value.as_ref(),
                    Expr::Binary {
                        op: BinaryOp::BitAnd
                            | BinaryOp::BitOr
                            | BinaryOp::BitXor
                            | BinaryOp::Shl
                            | BinaryOp::Shr,
                        ..
                    }
                );
                self.i32_writes
                    .entry(*id)
                    .and_modify(|v| *v &= signed)
                    .or_insert(signed);
                if self.depth != 0 {
                    self.carried.insert(*id);
                }
                if !self.locals.contains(id) {
                    self.inputs.insert(*id);
                    self.writes.insert(*id);
                }
                true
            }
            Expr::PutValueSet {
                target,
                key,
                value,
                receiver,
                ..
            } => {
                if !matches!((target.as_ref(), receiver.as_ref()),
                    (Expr::LocalGet(a), Expr::LocalGet(b)) if a == b)
                {
                    return false;
                }
                self.expression(
                    ctx,
                    &Expr::IndexSet {
                        object: target.clone(),
                        index: key.clone(),
                        value: value.clone(),
                    },
                )
            }
            Expr::Uint8ArraySet {
                array,
                index,
                value,
            } => self.expression(
                ctx,
                &Expr::IndexSet {
                    object: array.clone(),
                    index: index.clone(),
                    value: value.clone(),
                },
            ),
            Expr::IndexSet {
                object,
                index,
                value,
            } => {
                let Some(root) = self.receiver(ctx, object) else {
                    return false;
                };
                // Clamping and half stores keep their existing semantic path.
                if matches!(self.arrays[&root], 8 | 11) {
                    return false;
                }
                if !self.numeric(ctx, index) || !self.numeric(ctx, value) {
                    return false;
                }
                self.stores.insert(root);
                self.indices.push((root, *index.clone()));
                true
            }
            Expr::Sequence(_) => false,
            _ => self.numeric(ctx, e),
        }
    }

    fn body(&mut self, ctx: &FnCtx<'_>, body: &[Stmt]) -> bool {
        for stmt in body {
            match stmt {
                Stmt::Let {
                    id,
                    init: Some(init),
                    mutable,
                    ..
                } => {
                    if self.locals.contains(id)
                        || *id == self.counter
                        || ctx.boxed_vars.contains(id)
                        || ctx.closure_captures.contains_key(id)
                    {
                        return false;
                    }
                    if let Some(root) = self.receiver(ctx, init) {
                        if *mutable {
                            return false;
                        }
                        self.aliases.insert(*id, root);
                    } else {
                        if !self.numeric(ctx, init) {
                            return false;
                        }
                        if !*mutable {
                            self.defs.insert(*id, init.clone());
                        }
                    }
                    self.locals.insert(*id);
                }
                Stmt::If {
                    condition: Expr::Bool(true),
                    then_branch,
                    else_branch: None,
                } => {
                    if !self.body(ctx, then_branch) {
                        return false;
                    }
                }
                Stmt::If {
                    condition,
                    then_branch,
                    else_branch,
                } => {
                    let has_bindings = |branch: &[Stmt]| {
                        let mut found = false;
                        perry_hir::tdz_check::for_each_stmt_deep(branch, &mut |s| {
                            found |= matches!(s, Stmt::Let { .. });
                        });
                        found
                    };
                    if has_bindings(then_branch)
                        || else_branch.as_ref().is_some_and(|b| has_bindings(b))
                        || !self.condition(ctx, condition)
                    {
                        return false;
                    }
                    self.depth += 1;
                    let accepted = self.body(ctx, then_branch)
                        && else_branch.as_ref().is_none_or(|b| self.body(ctx, b));
                    self.depth -= 1;
                    if !accepted {
                        return false;
                    }
                }
                Stmt::For {
                    init: Some(init),
                    condition:
                        Some(Expr::Compare {
                            op: CompareOp::Lt,
                            left,
                            right,
                        }),
                    update:
                        Some(Expr::Update {
                            id: updated,
                            op: UpdateOp::Increment,
                            ..
                        }),
                    body,
                } => {
                    let Stmt::Let {
                        id,
                        init: Some(Expr::Integer(0)),
                        ..
                    } = init.as_ref()
                    else {
                        return false;
                    };
                    if id != updated
                        || !matches!(left.as_ref(), Expr::LocalGet(i) if i == id)
                        || self.locals.contains(id)
                        || ctx.boxed_vars.contains(id)
                        || ctx.closure_captures.contains_key(id)
                    {
                        return false;
                    }
                    let Some((lo, hi)) = static_interval(ctx, self, right, 0) else {
                        return false;
                    };
                    if lo != hi || lo < 0 || hi > i32::MAX as i64 {
                        return false;
                    }
                    self.nested.insert(*id, hi);
                    self.locals.insert(*id);
                    self.depth += 1;
                    let accepted = self.body(ctx, body);
                    self.depth -= 1;
                    if !accepted {
                        return false;
                    }
                }
                Stmt::Expr(e) if self.expression(ctx, e) => {}
                _ => return false,
            }
        }
        true
    }

    fn expand(&self, e: &Expr) -> Expr {
        match e {
            Expr::LocalGet(id) if self.defs.contains_key(id) => self.expand(&self.defs[id]),
            Expr::Binary { op, left, right } => Expr::Binary {
                op: *op,
                left: Box::new(self.expand(left)),
                right: Box::new(self.expand(right)),
            },
            _ => e.clone(),
        }
    }

    fn invariant(&self, e: &Expr) -> bool {
        match e {
            Expr::Integer(_) | Expr::Number(_) => true,
            Expr::LocalGet(id) => {
                *id != self.counter
                    && !self.writes.contains(id)
                    && !self.locals.contains(id)
                    && self.inputs.contains(id)
            }
            Expr::Binary {
                op: BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul,
                left,
                right,
            } => self.invariant(left) && self.invariant(right),
            _ => false,
        }
    }

    fn window(&self, e: &Expr) -> bool {
        if crate::collectors::static_index_window(e).is_some() {
            return true;
        }
        if self.invariant(e) {
            return true;
        }
        match e {
            Expr::LocalGet(id) => *id == self.counter || self.nested.contains_key(id),
            Expr::Binary {
                op: BinaryOp::Add | BinaryOp::Sub,
                left,
                right,
            } => self.window(left) && self.window(right),
            _ => false,
        }
    }
}

fn numeric_function<'a>(
    ctx: &'a FnCtx<'_>,
    callee: &Expr,
) -> Option<&'a crate::codegen::FactsNumericFunction> {
    if ctx.wpfacts_off || ctx.wpfacts.interp_guard {
        return None;
    }
    let id = match callee {
        Expr::FuncRef(id) => *id,
        Expr::LocalGet(id) => {
            let bodies = ctx.wpfacts.local_bodies.get(id)?;
            if bodies.len() != 1 || bodies[0].closure {
                return None;
            }
            bodies[0].func_id
        }
        _ => return None,
    };
    ctx.wpfacts.numeric_functions.get(&id)
}

fn owner_proof<'a>(
    ctx: &'a FnCtx<'_>,
    id: u32,
    kind: u8,
) -> Option<&'a crate::codegen::FactsIndexedOwner> {
    if ctx.wpfacts_off || ctx.wpfacts.interp_guard {
        return None;
    }
    ctx.wpfacts
        .indexed_owners
        .get(&id)
        .filter(|p| p.kind == kind)
}

fn static_interval(ctx: &FnCtx<'_>, p: &Plan, e: &Expr, bound: i64) -> Option<(i64, i64)> {
    if let Some(w) = crate::collectors::static_index_window(e) {
        return Some(w);
    }
    if let Expr::LocalGet(id) = e {
        if *id == p.counter {
            return Some((0, bound.saturating_sub(1)));
        }
        if let Some(n) = p.nested.get(id) {
            return Some((0, n.saturating_sub(1)));
        }
        if let Some(n) = ctx
            .compile_time_constants
            .get(id)
            .or_else(|| ctx.const_number_locals.get(id))
        {
            if *n >= 0.0 && *n <= i32::MAX as f64 && n.fract() == 0.0 {
                return Some((*n as i64, *n as i64));
            }
        }
    }
    if let Expr::Binary { op, left, right } = e {
        let (a, b) = static_interval(ctx, p, left, bound)?;
        let (c, d) = static_interval(ctx, p, right, bound)?;
        let r = match op {
            BinaryOp::Add => (a.checked_add(c)?, b.checked_add(d)?),
            BinaryOp::Sub => (a.checked_sub(d)?, b.checked_sub(c)?),
            BinaryOp::Mul if a >= 0 && c >= 0 => (a.checked_mul(c)?, b.checked_mul(d)?),
            _ => return None,
        };
        return (r.0 >= 0 && r.1 <= i32::MAX as i64).then_some(r);
    }
    let r = crate::expr::int_range_expr(ctx, e)?;
    Some((r.min, r.max))
}

#[derive(Clone)]
enum Number {
    F(String),
    /// Finite Number interval entirely representable by signed i64 truncation.
    Ranged {
        value: String,
        min: f64,
        max: f64,
    },
    I(String),
    U(String),
    Index {
        wide: String,
        boxed: Box<Number>,
    },
}
impl Number {
    fn range(&self) -> Option<(f64, f64)> {
        match self {
            Self::Ranged { min, max, .. } => Some((*min, *max)),
            Self::I(_) => Some((i32::MIN as f64, i32::MAX as f64)),
            Self::U(_) => Some((0.0, u32::MAX as f64)),
            Self::Index { boxed, .. } => boxed.range(),
            Self::F(_) => None,
        }
    }
    fn bounded(value: String, range: Option<(f64, f64)>) -> Self {
        if let Some((min, max)) = range {
            // i64::MAX rounds UP to 2^63 as f64: the upper bound is strict.
            if min.is_finite()
                && max.is_finite()
                && min >= i64::MIN as f64
                && max < -(i64::MIN as f64)
            {
                return Self::Ranged { value, min, max };
            }
        }
        Self::F(value)
    }

    fn f64(&self, ctx: &mut FnCtx<'_>) -> String {
        match self {
            Self::F(v) | Self::Ranged { value: v, .. } => v.clone(),
            Self::Index { boxed, .. } => boxed.f64(ctx),
            Self::I(v) => ctx.block().sitofp(I32, v, DOUBLE),
            Self::U(v) => ctx.block().uitofp(I32, v, DOUBLE),
        }
    }
    fn i32(&self, ctx: &mut FnCtx<'_>) -> String {
        match self {
            Self::F(v) => ctx.toint32_wrap(v),
            Self::Ranged { value, .. } => {
                let wide = ctx.block().fptosi(DOUBLE, value, I64);
                ctx.block().trunc(I64, &wide, I32)
            }
            Self::I(v) | Self::U(v) => v.clone(),
            Self::Index { wide, .. } => ctx.block().trunc(I64, wide, I32),
        }
    }
}

fn boxed_binding(ctx: &mut FnCtx<'_>, id: u32) -> String {
    if let Some(slot) = ctx.numeric_accumulator_f64_slots.get(&id).cloned() {
        return ctx.block().load(DOUBLE, &slot);
    }
    if let Some(slot) = ctx
        .receiver_descriptors
        .rooted_box_slot(id)
        .map(str::to_owned)
    {
        return ctx.block().load(DOUBLE, &slot);
    }

    if let Some(v) = crate::expr::load_canonical_local_boxed(ctx, id) {
        return v;
    }
    let slot = ctx
        .locals
        .get(&id)
        .cloned()
        .unwrap_or_else(|| format!("@{}", ctx.module_globals[&id]));
    ctx.block().load(DOUBLE, &slot)
}

fn branch_guard(ctx: &mut FnCtx<'_>, pass: &str, slow: &str) {
    let next = ctx.new_block("indexed.owner.admitted");
    let next_l = ctx.block_label(next);
    ctx.block().cond_br(pass, &next_l, slow);
    ctx.current_block = next;
}

fn integer_extent(ctx: &mut FnCtx<'_>, n: &str, slow: &str) -> String {
    // Freeze before the round trip: invalid conversion must not poison the
    // guard itself. An extent is an exact integer in 0..=INT32_MAX.
    let converted = ctx.block().fptosi(DOUBLE, n, I32);
    let int = ctx.block().freeze(I32, &converted);
    let back = ctx.block().sitofp(I32, &int, DOUBLE);
    let exact = ctx.block().fcmp("oeq", n, &back);
    let nonnegative = ctx.block().icmp_sge(I32, &int, "0");
    let ok = ctx.block().and(I1, &exact, &nonnegative);
    branch_guard(ctx, &ok, slow);
    int
}

struct Storage {
    raw: String,
    data: String,
    len: String,
    kind: u8,
    metadata: String,
}

fn admit_owner(ctx: &mut FnCtx<'_>, id: u32, kind: u8, slow: &str) -> Storage {
    let boxed = boxed_binding(ctx, id);
    let proven = owner_proof(ctx, id, kind).is_some();
    let (raw, ok) = if proven {
        (
            crate::expr::unbox_to_i64(ctx.block(), &boxed),
            "true".to_owned(),
        )
    } else {
        crate::expr::byte_cell::owner_guard(
            ctx,
            &boxed,
            crate::expr::byte_cell::brand_for_kind(kind),
        )
    };
    if !proven {
        branch_guard(ctx, &ok, slow);
    }
    let word = crate::expr::byte_cell::header_word(ctx.block(), &raw);
    // Admit only an ordinary owner, never a view, resizable/shared owner or
    // native arena. Detachment also clears its length. Preserve frozen writes.
    let disallowed = ctx.block().and(
        I64,
        &word,
        &((1u64 << 16) | (1 << 24) | (1 << 30)).to_string(),
    );
    let plain = ctx.block().icmp_eq(I64, &disallowed, "0");
    if !proven {
        branch_guard(ctx, &plain, slow);
    }
    let ptr = ctx.block().inttoptr(I64, &raw);
    let len = owner_proof(ctx, id, kind)
        .and_then(|o| o.exact_len)
        .map(|n| n.to_string())
        .unwrap_or_else(|| ctx.block().load(I32, &ptr));
    let data = crate::expr::byte_cell::owner_data(ctx, &raw, &word, "indexed.owner.data");
    Storage {
        raw,
        data,
        len,
        kind,
        metadata: String::new(),
    }
}

fn lane_load(ctx: &mut FnCtx<'_>, ty: crate::types::LlvmType, ptr: &str, metadata: &str) -> String {
    if metadata.is_empty() {
        return ctx.block().load(ty, ptr);
    }
    let value = ctx.block().fresh_reg();
    ctx.block()
        .emit_raw(format!("{value} = load {ty}, ptr {ptr}{metadata}"));
    value
}
fn lane_store(
    ctx: &mut FnCtx<'_>,
    ty: crate::types::LlvmType,
    value: &str,
    ptr: &str,
    metadata: &str,
) {
    if metadata.is_empty() {
        ctx.block().store(ty, value, ptr);
    } else {
        // GC_STORE_AUDIT(POINTER_FREE): typed-array numeric lane, never a managed pointer.
        ctx.block()
            .emit_raw(format!("store {ty} {value}, ptr {ptr}{metadata}"));
    }
}
fn disjoint_storage(ctx: &mut FnCtx<'_>, storage: &mut BTreeMap<u32, Storage>, slow: &str) {
    if storage.len() < 2 {
        return;
    }
    let mut spans = Vec::new();
    for (id, s) in storage.iter() {
        let wide = ctx.block().zext(I32, &s.len, I64);
        let shift = match s.kind {
            0 | 1 | 8 | STRING_KIND => 0,
            2 | 3 | 11 => 1,
            4..=6 => 2,
            _ => 3,
        };
        let bytes = ctx.block().shl(I64, &wide, &shift.to_string());
        let end = ctx.block().add(I64, &s.data, &bytes);
        spans.push((*id, s.data.clone(), end));
    }
    for (i, (id, start, end)) in spans.iter().enumerate() {
        for (other, other_start, other_end) in &spans[..i] {
            let separate = owner_proof(ctx, *id, storage[id].kind)
                .zip(owner_proof(ctx, *other, storage[other].kind))
                .is_some_and(|(a, b)| {
                    !a.allocations.is_empty()
                        && !b.allocations.is_empty()
                        && a.allocations.iter().all(|x| !b.allocations.contains(x))
                });
            if separate {
                continue;
            }
            let before = ctx.block().icmp_ule(I64, end, other_start);
            let after = ctx.block().icmp_ule(I64, other_end, start);
            let disjoint = ctx.block().or(I1, &before, &after);
            branch_guard(ctx, &disjoint, slow);
        }
    }
    // A region-local domain: never join the sealed-allocation alias domain.
    // The same owner can occur in another region or under a sealed proof.
    let domain = format!("indexed.owner.{}.{}", ctx.func.name, ctx.func.num_blocks())
        .replace('\\', "\\5C")
        .replace('"', "\\22");
    let scope = |id: u32| format!("!{{!\"owner.{id}\", !{{!\"{domain}\"}}}}");
    let ids: Vec<_> = storage.keys().copied().collect();
    for id in &ids {
        let declaration = format!("!{{{}}}", scope(*id));
        ctx.block().call_void(
            "llvm.experimental.noalias.scope.decl",
            &[("metadata", &declaration)],
        );
    }
    for (id, s) in storage.iter_mut() {
        let others = ids
            .iter()
            .filter(|other| *other != id)
            .map(|id| scope(*id))
            .collect::<Vec<_>>()
            .join(", ");
        s.metadata = format!(", !alias.scope !{{{}}}, !noalias !{{{others}}}", scope(*id));
    }
}

fn invariant_value(ctx: &mut FnCtx<'_>, e: &Expr, values: &BTreeMap<u32, Number>) -> String {
    match e {
        Expr::Integer(n) => crate::nanbox::double_literal(*n as f64),
        Expr::Number(n) => crate::nanbox::double_literal(*n),
        Expr::LocalGet(id) => values[id].f64(ctx),
        Expr::Binary { op, left, right } => {
            let l = invariant_value(ctx, left, values);
            let r = invariant_value(ctx, right, values);
            match op {
                BinaryOp::Add => ctx.block().fadd(&l, &r),
                BinaryOp::Sub => ctx.block().fsub(&l, &r),
                BinaryOp::Mul => ctx.block().fmul(&l, &r),
                _ => unreachable!(),
            }
        }
        _ => unreachable!(),
    }
}

fn interval(
    ctx: &mut FnCtx<'_>,
    p: &Plan,
    e: &Expr,
    bound: &str,
    values: &BTreeMap<u32, Number>,
    slow: &str,
) -> (String, String) {
    if let Some((lo, hi)) = crate::collectors::static_index_window(e) {
        return (
            crate::nanbox::double_literal(lo as f64),
            crate::nanbox::double_literal(hi as f64),
        );
    }
    if p.invariant(e) {
        let n = invariant_value(ctx, e, values);
        integer_extent(ctx, &n, slow);
        return (n.clone(), n);
    }
    match e {
        Expr::LocalGet(id) => {
            if let Some(n) = p.nested.get(id) {
                return (
                    "0.0".into(),
                    crate::nanbox::double_literal(n.saturating_sub(1) as f64),
                );
            }
            let max = ctx.block().fsub(bound, "1.0");
            ("0.0".into(), max)
        }
        Expr::Binary { op, left, right } => {
            let (ll, lh) = interval(ctx, p, left, bound, values, slow);
            let (rl, rh) = interval(ctx, p, right, bound, values, slow);
            let (lo, hi) = match op {
                BinaryOp::Add => (ctx.block().fadd(&ll, &rl), ctx.block().fadd(&lh, &rh)),
                BinaryOp::Sub => (ctx.block().fsub(&ll, &rh), ctx.block().fsub(&lh, &rl)),
                _ => unreachable!(),
            };
            // Every intermediate must remain exactly representable: endpoint
            // checks also prevent cancellation of a huge wrapped subexpression.
            integer_extent(ctx, &lo, slow);
            integer_extent(ctx, &hi, slow);
            (lo, hi)
        }
        _ => unreachable!(),
    }
}

fn index_value(
    ctx: &mut FnCtx<'_>,
    p: &Plan,
    e: &Expr,
    values: &mut BTreeMap<u32, Number>,
    storage: &BTreeMap<u32, Storage>,
) -> String {
    match e {
        Expr::LocalGet(id) => match &values[id] {
            Number::Index { wide, .. } => wide.clone(),
            Number::I(v) | Number::U(v) => {
                let v = v.clone();
                ctx.block().zext(I32, &v, I64)
            }
            Number::F(v) | Number::Ranged { value: v, .. } => {
                let v = v.clone();
                ctx.block().fptosi(DOUBLE, &v, I64)
            }
        },
        Expr::Integer(n) => n.to_string(),
        Expr::Number(n) => (*n as i64).to_string(),
        Expr::Binary {
            op: BinaryOp::Add | BinaryOp::Sub,
            left,
            right,
        } => {
            let l = index_value(ctx, p, left, values, storage);
            let r = index_value(ctx, p, right, values, storage);
            if matches!(
                e,
                Expr::Binary {
                    op: BinaryOp::Add,
                    ..
                }
            ) {
                ctx.block().add(I64, &l, &r)
            } else {
                ctx.block().sub(I64, &l, &r)
            }
        }
        _ => {
            let value = numeric(ctx, p, e, values, storage);
            match value {
                Number::I(v) | Number::U(v) => ctx.block().zext(I32, &v, I64),
                Number::Index { wide, .. } => wide,
                Number::F(v) | Number::Ranged { value: v, .. } => {
                    ctx.block().fptosi(DOUBLE, &v, I64)
                }
            }
        }
    }
}
fn address(ctx: &mut FnCtx<'_>, storage: &Storage, idx: &str) -> String {
    let shift = match storage.kind {
        0 | 1 | 8 | STRING_KIND => 0,
        2 | 3 | 11 => 1,
        4..=6 => 2,
        _ => 3,
    };
    let off = ctx.block().shl(I64, idx, &shift.to_string());
    let base = ctx.block().inttoptr(I64, &storage.data);
    ctx.block().gep(I8, &base, &[(I64, &off)])
}
// Follow only the integer affine spine. A mask proves its own result, not
// that each of its scalar operands is integral (NaN & 255 is still zero).
fn index_locals(e: &Expr, ids: &mut BTreeSet<u32>) {
    match e {
        Expr::LocalGet(id) => {
            ids.insert(*id);
        }
        Expr::Binary {
            op: BinaryOp::Add | BinaryOp::Sub,
            left,
            right,
        } => {
            index_locals(left, ids);
            index_locals(right, ids);
        }
        _ => {}
    }
}

fn numeric(
    ctx: &mut FnCtx<'_>,
    p: &Plan,
    e: &Expr,
    values: &mut BTreeMap<u32, Number>,
    storage: &BTreeMap<u32, Storage>,
) -> Number {
    if let Some((id, index)) = string::read(e) {
        let s = &storage[&id];
        let index = index_value(ctx, p, index, values, storage);
        let ptr = address(ctx, s, &index);
        let byte = lane_load(ctx, I8, &ptr, &s.metadata);
        return Number::U(ctx.block().zext(I8, &byte, I32));
    }
    let read = match e {
        Expr::IndexGet { object, index } => Some((object.as_ref(), index.as_ref())),
        Expr::Uint8ArrayGet { array, index } => Some((array.as_ref(), index.as_ref())),
        Expr::BufferIndexGet { buffer, index } => Some((buffer.as_ref(), index.as_ref())),
        _ => None,
    };
    if let Some((Expr::LocalGet(id), index)) = read {
        let root = p.aliases.get(id).unwrap_or(id);
        let s = &storage[root];
        let index = index_value(ctx, p, index, values, storage);
        let ptr = address(ctx, s, &index);
        return match s.kind {
            0 | 1 | 2 | 3 | 4 | 5 | 8 => {
                let ty = match s.kind {
                    0 | 1 | 8 => I8,
                    2 | 3 => I16,
                    _ => I32,
                };
                let lane = lane_load(ctx, ty, &ptr, &s.metadata);
                let signed = matches!(s.kind, 0 | 2 | 4);
                let lane = if ty == I32 {
                    lane
                } else if signed {
                    ctx.block().sext(ty, &lane, I32)
                } else {
                    ctx.block().zext(ty, &lane, I32)
                };
                if signed {
                    Number::I(lane)
                } else {
                    Number::U(lane)
                }
            }
            // This region has only Number consumers: raw floating NaNs
            // never enter a NaN-box discriminator. Canonicalize when a
            // scalar leaves the region, not at every numeric lane read.
            7 => Number::F(lane_load(ctx, DOUBLE, &ptr, &s.metadata)),
            6 => {
                let lane = lane_load(ctx, F32, &ptr, &s.metadata);
                Number::F(ctx.block().fpext(F32, &lane, DOUBLE))
            }
            _ => Number::F(crate::expr::ta_element_read::emit_element(
                ctx.block(),
                &ptr,
                s.kind,
            )),
        };
    }
    match e {
        Expr::MathSqrt(v) => {
            let v = numeric(ctx, p, v, values, storage).f64(ctx);
            Number::F(ctx.block().call(DOUBLE, "llvm.sqrt.f64", &[(DOUBLE, &v)]))
        }
        Expr::Call { callee, args, .. } => {
            let f = numeric_function(ctx, callee)
                .expect("proved pure numeric leaf")
                .clone();
            // Evaluate each actual once, left to right, before binding any
            // formal. Repeated formal uses cannot repeat an indexed read.
            let actual: Vec<_> = args
                .iter()
                .map(|e| numeric(ctx, p, e, values, storage))
                .collect();
            let saved: Vec<_> = f
                .params
                .iter()
                .zip(actual)
                .map(|(id, v)| (*id, values.insert(*id, v)))
                .collect();
            let result = numeric(ctx, p, &f.expression.to_hir(), values, storage);
            for (id, old) in saved {
                if let Some(v) = old {
                    values.insert(id, v);
                } else {
                    values.remove(&id);
                }
            }
            result
        }
        Expr::Integer(n) if i32::try_from(*n).is_ok() => Number::I(n.to_string()),
        Expr::Integer(n) => Number::bounded(
            crate::nanbox::double_literal(*n as f64),
            Some((*n as f64, *n as f64)),
        ),
        Expr::Number(n) => Number::bounded(crate::nanbox::double_literal(*n), Some((*n, *n))),
        Expr::LocalGet(id) => values[id].clone(),
        Expr::Binary { op, left, right } => {
            let l = numeric(ctx, p, left, values, storage);
            let r = numeric(ctx, p, right, values, storage);
            if matches!(
                op,
                BinaryOp::BitAnd
                    | BinaryOp::BitOr
                    | BinaryOp::BitXor
                    | BinaryOp::Shl
                    | BinaryOp::Shr
                    | BinaryOp::UShr
            ) {
                let l = l.i32(ctx);
                let mut r = r.i32(ctx);
                if matches!(op, BinaryOp::Shl | BinaryOp::Shr | BinaryOp::UShr) {
                    r = ctx.block().and(I32, &r, "31");
                }
                let v = match op {
                    BinaryOp::BitAnd => ctx.block().and(I32, &l, &r),
                    BinaryOp::BitOr => ctx.block().or(I32, &l, &r),
                    BinaryOp::BitXor => ctx.block().xor(I32, &l, &r),
                    BinaryOp::Shl => ctx.block().shl(I32, &l, &r),
                    BinaryOp::Shr => ctx.block().ashr(I32, &l, &r),
                    BinaryOp::UShr => ctx.block().lshr(I32, &l, &r),
                    _ => unreachable!(),
                };
                return if matches!(op, BinaryOp::UShr) {
                    Number::U(v)
                } else {
                    Number::I(v)
                };
            }
            let range = l.range().zip(r.range()).and_then(|((a, b), (c, d))| {
                Some(match op {
                    BinaryOp::Add => (a + c, b + d),
                    BinaryOp::Sub => (a - d, b - c),
                    BinaryOp::Mul => {
                        let v = [a * c, a * d, b * c, b * d];
                        if v.iter().any(|v| !v.is_finite()) {
                            return None;
                        }
                        (
                            v.iter().copied().fold(f64::INFINITY, f64::min),
                            v.iter().copied().fold(f64::NEG_INFINITY, f64::max),
                        )
                    }
                    _ => return None,
                })
            });
            let l = l.f64(ctx);
            let r = r.f64(ctx);
            let value = match op {
                BinaryOp::Add => ctx.block().fadd(&l, &r),
                BinaryOp::Sub => ctx.block().fsub(&l, &r),
                BinaryOp::Mul => ctx.block().fmul(&l, &r),
                BinaryOp::Div => ctx.block().fdiv(&l, &r),
                BinaryOp::Mod => ctx.block().frem(&l, &r),
                _ => unreachable!(),
            };
            Number::bounded(value, range)
        }
        Expr::Unary { op, operand } => {
            let v = numeric(ctx, p, operand, values, storage);
            match op {
                UnaryOp::BitNot => {
                    let v = v.i32(ctx);
                    Number::I(ctx.block().xor(I32, &v, "-1"))
                }
                UnaryOp::Pos => Number::F(v.f64(ctx)),
                UnaryOp::Neg => {
                    let v = v.f64(ctx);
                    Number::F(ctx.block().fneg(&v))
                }
                _ => unreachable!(),
            }
        }
        _ => unreachable!("validated numeric expression"),
    }
}

fn expression(
    ctx: &mut FnCtx<'_>,
    p: &Plan,
    e: &Expr,
    values: &mut BTreeMap<u32, Number>,
    storage: &BTreeMap<u32, Storage>,
) -> Number {
    match e {
        Expr::Update { id, op, prefix } => {
            let old = values[id].clone();
            let new = expression(
                ctx,
                p,
                &Expr::LocalSet(
                    *id,
                    Box::new(Expr::Binary {
                        op: if matches!(op, UpdateOp::Increment) {
                            BinaryOp::Add
                        } else {
                            BinaryOp::Sub
                        },
                        left: Box::new(Expr::LocalGet(*id)),
                        right: Box::new(Expr::Integer(1)),
                    }),
                ),
                values,
                storage,
            );
            if *prefix {
                new
            } else {
                old
            }
        }
        Expr::LocalSet(id, value) => {
            let v = numeric(ctx, p, value, values, storage);
            values.insert(*id, v.clone());
            v
        }
        Expr::PutValueSet {
            target, key, value, ..
        } => expression(
            ctx,
            p,
            &Expr::IndexSet {
                object: target.clone(),
                index: key.clone(),
                value: value.clone(),
            },
            values,
            storage,
        ),
        Expr::Uint8ArraySet {
            array,
            index,
            value,
        } => expression(
            ctx,
            p,
            &Expr::IndexSet {
                object: array.clone(),
                index: index.clone(),
                value: value.clone(),
            },
            values,
            storage,
        ),
        Expr::IndexSet {
            object,
            index,
            value,
        } => {
            let Expr::LocalGet(id) = object.as_ref() else {
                unreachable!()
            };
            let s = &storage[p.aliases.get(id).unwrap_or(id)];
            let i = index_value(ctx, p, index, values, storage);
            let v = numeric(ctx, p, value, values, storage);
            let ptr = address(ctx, s, &i);
            match s.kind {
                6 => {
                    let v = v.f64(ctx);
                    let f = ctx.block().fptrunc(DOUBLE, &v, F32);
                    lane_store(ctx, F32, &f, &ptr, &s.metadata);
                }
                7 => {
                    let v = v.f64(ctx);
                    lane_store(ctx, DOUBLE, &v, &ptr, &s.metadata);
                }
                _ => {
                    let ty = match s.kind {
                        0 | 1 => I8,
                        2 | 3 => I16,
                        _ => I32,
                    };
                    let i = v.i32(ctx);
                    let i = if ty == I32 {
                        i
                    } else {
                        ctx.block().trunc(I32, &i, ty)
                    };
                    // GC_STORE_AUDIT(POINTER_FREE): admitted numeric owner lane.
                    lane_store(ctx, ty, &i, &ptr, &s.metadata);
                }
            }
            v
        }
        Expr::Sequence(es) => {
            let mut v = Number::I("0".into());
            for e in es {
                v = expression(ctx, p, e, values, storage);
            }
            v
        }
        _ => numeric(ctx, p, e, values, storage),
    }
}

fn emit_body(
    ctx: &mut FnCtx<'_>,
    p: &Plan,
    body: &[Stmt],
    values: &mut BTreeMap<u32, Number>,
    storage: &BTreeMap<u32, Storage>,
    slots: &BTreeMap<u32, (String, bool)>,
    native_indices: &BTreeSet<u32>,
) {
    for stmt in body {
        match stmt {
            Stmt::Let {
                id, init: Some(e), ..
            } if !p.aliases.contains_key(id) => {
                let boxed = numeric(ctx, p, e, values, storage);
                let value = if native_indices.contains(id) {
                    let wide = index_value(ctx, p, e, values, storage);
                    Number::Index {
                        wide,
                        boxed: Box::new(boxed),
                    }
                } else {
                    boxed
                };
                values.insert(*id, value);
                if let Some((slot, int)) = slots.get(id) {
                    let v = if *int {
                        values[id].i32(ctx)
                    } else {
                        values[id].f64(ctx)
                    };
                    ctx.block().store(if *int { I32 } else { DOUBLE }, &v, slot);
                }
            }
            Stmt::Let { .. } => {}
            Stmt::Expr(e) => {
                expression(ctx, p, e, values, storage);
                if let Expr::LocalSet(id, _) | Expr::Update { id, .. } = e {
                    if let Some((slot, int)) = slots.get(id) {
                        let v = if *int {
                            values[id].i32(ctx)
                        } else {
                            values[id].f64(ctx)
                        };
                        ctx.block().store(if *int { I32 } else { DOUBLE }, &v, slot);
                    }
                }
            }
            Stmt::If {
                condition: Expr::Bool(true),
                then_branch,
                else_branch: None,
            } => {
                emit_body(ctx, p, then_branch, values, storage, slots, native_indices);
            }
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                let condition = match condition {
                    Expr::Bool(b) => b.to_string(),
                    Expr::Compare { op, left, right } => {
                        let l = numeric(ctx, p, left, values, storage).f64(ctx);
                        let r = numeric(ctx, p, right, values, storage).f64(ctx);
                        let predicate = match op {
                            CompareOp::Eq | CompareOp::LooseEq => "oeq",
                            CompareOp::Ne | CompareOp::LooseNe => "une",
                            CompareOp::Lt => "olt",
                            CompareOp::Le => "ole",
                            CompareOp::Gt => "ogt",
                            CompareOp::Ge => "oge",
                        };
                        ctx.block().fcmp(predicate, &l, &r)
                    }
                    _ => unreachable!("validated numeric comparison"),
                };
                let yes = ctx.new_block("indexed.owner.if");
                let no = ctx.new_block("indexed.owner.else");
                let merge = ctx.new_block("indexed.owner.if.end");
                let yes_l = ctx.block_label(yes);
                let no_l = ctx.block_label(no);
                let merge_l = ctx.block_label(merge);
                ctx.block().cond_br(&condition, &yes_l, &no_l);
                let before = values.clone();
                ctx.current_block = yes;
                emit_body(
                    ctx,
                    p,
                    then_branch,
                    &mut before.clone(),
                    storage,
                    slots,
                    native_indices,
                );
                ctx.block().br(&merge_l);
                ctx.current_block = no;
                if let Some(body) = else_branch {
                    emit_body(
                        ctx,
                        p,
                        body,
                        &mut before.clone(),
                        storage,
                        slots,
                        native_indices,
                    );
                }
                ctx.block().br(&merge_l);
                ctx.current_block = merge;
                // Only scalar writes use shared native slots. Branch-local
                // SSA values never escape their block, and promotion forms phis.
                for (id, (slot, int)) in slots {
                    if before.contains_key(id) {
                        let v = ctx.block().load(if *int { I32 } else { DOUBLE }, slot);
                        values.insert(*id, if *int { Number::I(v) } else { Number::F(v) });
                    }
                }
            }
            Stmt::For {
                init: Some(init),
                body,
                ..
            } => {
                let Stmt::Let { id, .. } = init.as_ref() else {
                    unreachable!()
                };
                let bound = p.nested[id];
                let ctr_slot = ctx.func.alloca_entry(I32);
                ctx.block().store(I32, "0", &ctr_slot);
                let head = ctx.new_block("indexed.owner.nested");
                let head_l = ctx.block_label(head);
                let exit = ctx.new_block("indexed.owner.nested.exit");
                let exit_l = ctx.block_label(exit);
                ctx.block().br(if bound == 0 { &exit_l } else { &head_l });
                let before = values.clone();
                ctx.current_block = head;
                let ctr = ctx.block().load(I32, &ctr_slot);
                let mut inner = before.clone();
                inner.insert(*id, Number::I(ctr.clone()));
                for (key, (slot, int)) in slots {
                    if inner.contains_key(key) {
                        let v = ctx.block().load(if *int { I32 } else { DOUBLE }, slot);
                        inner.insert(*key, if *int { Number::I(v) } else { Number::F(v) });
                    }
                }
                emit_body(ctx, p, body, &mut inner, storage, slots, native_indices);
                let next = ctx.block().add(I32, &ctr, "1");
                ctx.block().store(I32, &next, &ctr_slot);
                let more = ctx.block().icmp_slt(I32, &next, &bound.to_string());
                ctx.block().cond_br(&more, &head_l, &exit_l);
                ctx.current_block = exit;
                for (key, (slot, int)) in slots {
                    if before.contains_key(key) {
                        let v = ctx.block().load(if *int { I32 } else { DOUBLE }, slot);
                        values.insert(*key, if *int { Number::I(v) } else { Number::F(v) });
                    }
                }
            }
            _ => unreachable!("validated nested numeric region"),
        }
    }
}

/// Numeric bodies and fixed-bound nested loops, numeric typed-array widths,
/// unit-stride counters, affine or mask-bounded indices. Unsupported effects
/// and index expressions keep the original lowering.
pub(super) fn lower(
    ctx: &mut FnCtx<'_>,
    init: Option<&Stmt>,
    condition: Option<&Expr>,
    update: Option<&Expr>,
    body: &[Stmt],
) -> Result<bool> {
    if ctx.is_async_fn || ctx.disable_buffer_fast_path || !ctx.pending_labels.is_empty() {
        return Ok(false);
    }
    let (counter, start_expr) = match init {
        Some(Stmt::Let {
            id, init: Some(e), ..
        }) => (*id, e),
        _ => return Ok(false),
    };
    if !plain_binding(ctx, counter)
        || (!ctx.locals.contains_key(&counter) && !ctx.local_slot_reps.contains_key(&counter))
        || !matches!(update, Some(Expr::Update { id, op: UpdateOp::Increment, .. }) if *id == counter)
    {
        return Ok(false);
    }
    let bound_expr = match condition {
        Some(Expr::Compare {
            op: CompareOp::Lt,
            left,
            right,
        }) if matches!(left.as_ref(), Expr::LocalGet(id) if *id == counter) => right.as_ref(),
        _ => return Ok(false),
    };
    let mut p = Plan {
        counter,
        body_bindings: {
            let mut ids = BTreeSet::new();
            perry_hir::tdz_check::for_each_stmt_deep(body, &mut |s| {
                if let Stmt::Let { id, .. } = s {
                    ids.insert(*id);
                }
            });
            ids
        },
        ..Default::default()
    };
    if !p.body(ctx, body) || p.indices.is_empty() {
        return Ok(false);
    }
    if !p.numeric(ctx, start_expr) || !p.invariant(start_expr) {
        return Ok(false);
    }
    let length_receiver = match bound_expr {
        Expr::PropertyGet {
            object, property, ..
        } if property == "length" => {
            if let Expr::LocalGet(id) = object.as_ref() {
                if p.strings.contains(id) {
                    Some(*id)
                } else {
                    p.receiver(ctx, object)
                }
            } else {
                None
            }
        }
        Expr::Uint8ArrayLength(array) => p.receiver(ctx, array),
        _ => None,
    };
    if length_receiver.is_none() && (!p.numeric(ctx, bound_expr) || !p.invariant(bound_expr)) {
        return Ok(false);
    }
    let indices: Vec<_> = p
        .indices
        .iter()
        .map(|(root, e)| (*root, p.expand(e)))
        .collect();
    if indices.iter().any(|(_, e)| !p.window(e)) {
        return Ok(false);
    }
    if !matches!(start_expr, Expr::Integer(0))
        && !matches!(start_expr, Expr::Number(n) if n.to_bits() == 0)
        && !crate::expr::int_range_expr(ctx, start_expr)
            .is_some_and(|r| r.min > 0 && r.max <= i32::MAX as i64)
    {
        return Ok(false);
    }
    // A mutable local cannot be used as a let-expansion proof.
    for stmt in body {
        if let Stmt::Expr(Expr::LocalSet(id, _)) = stmt {
            p.defs.remove(id);
        }
    }
    // Writes to scalar inputs are kept in private slots, never to globals or
    // captured cells. The raw binding and any i32 shadow are flushed on exit.
    if p.strings
        .iter()
        .any(|id| p.inputs.contains(id) || p.writes.contains(id))
    {
        return Ok(false);
    }
    if !p.strings.is_empty() {
        // The original condition/body can drain work deferred by a preceding
        // allocation even though this admitted region cannot allocate. Keep
        // that entry safepoint before loading any owner or raw string bytes;
        // removing it can retain the previous allocation cohort through the
        // next allocating operation. The ordinary effect/arming checks still
        // decide whether the poll exists and whether it calls the collector.
        let controls: Vec<_> = condition.into_iter().chain(update).collect();
        super::loops::emit_gc_loop_safepoint(ctx, body, &controls);
    }
    let slow_idx = ctx.new_block("indexed.owner.slow");
    let slow_l = ctx.block_label(slow_idx);
    let done_idx = ctx.new_block("indexed.owner.done");
    let done_l = ctx.block_label(done_idx);
    let entry_blocks_start = ctx.func.num_blocks();
    let mut values = BTreeMap::new();
    for id in &p.inputs {
        let v = boxed_binding(ctx, *id);
        let bits = ctx.block().bitcast_double_to_i64(&v);
        let number = crate::codegen::emit_plain_number_bits_test(ctx.block(), &bits);
        let native_number = matches!(
            ctx.local_slot_reps.get(id),
            Some(crate::expr::SlotRep::I32 | crate::expr::SlotRep::U32)
        ) || ctx.numeric_accumulator_f64_slots.contains_key(id);
        if !native_number && (ctx.wpfacts_off || !ctx.wpfacts.number_inputs.contains(id)) {
            branch_guard(ctx, &number, &slow_l);
        }
        values.insert(*id, Number::F(v));
    }
    if !p.strings.is_empty() {
        string::prove_pristine(ctx, &slow_l);
    }
    let mut storage = BTreeMap::new();
    for (id, kind) in &p.arrays {
        storage.insert(*id, admit_owner(ctx, *id, *kind, &slow_l));
    }
    for id in &p.strings {
        storage.insert(*id, string::admit(ctx, *id, &slow_l));
    }
    // Read-only regions require no alias separation. For writes, allocation
    // identities can prove separation before falling back to live spans.
    if !p.stores.is_empty() {
        disjoint_storage(ctx, &mut storage, &slow_l);
    }
    if let Some(id) = length_receiver.filter(|id| {
        storage[id].kind != STRING_KIND
            && !(ctx.wpfacts.indexed_metadata_pristine
                && owner_proof(ctx, *id, storage[id].kind).is_some())
    }) {
        // Reuse the ordinary typed metadata-accessor contract. An owner
        // brand alone does not prove an observable `.length` property read:
        // its bag may shadow it or install a custom prototype, and a builtin
        // prototype mutation withdraws the existing intrinsic contract.
        let raw = &storage[&id].raw;
        let link_addr = ctx
            .block()
            .add(I64, raw, &crate::runtime_abi::BYTES_LINK.to_string());
        let link_ptr = ctx.block().inttoptr(I64, &link_addr);
        let link = ctx.block().load(I64, &link_ptr);
        let no_bag = ctx.block().icmp_eq(I64, &link, "0");
        let withdrawn =
            ctx.block()
                .load_atomic_acquire(I8, "@PERRY_TYPED_NAMED_PROPS_INVALIDATED", 1);
        let builtin = ctx.block().icmp_eq(I8, &withdrawn, "0");
        let metadata_ok = ctx.block().and(I1, &no_bag, &builtin);
        branch_guard(ctx, &metadata_ok, &slow_l);
    }
    let bound = if let Some(id) = length_receiver {
        ctx.block().uitofp(I32, &storage[&id].len, DOUBLE)
    } else {
        invariant_value(ctx, bound_expr, &values)
    };
    let static_bound = length_receiver
        .and_then(|id| {
            owner_proof(ctx, id, storage[&id].kind)?
                .exact_len
                .map(i64::from)
        })
        .or_else(|| {
            static_interval(ctx, &p, bound_expr, 0)
                .filter(|(a, b)| a == b && *a >= 0 && *a <= i32::MAX as i64)
                .map(|r| r.0)
        });
    let bound_int = if let Some(n) = static_bound {
        n.to_string()
    } else {
        integer_extent(ctx, &bound, &slow_l)
    };
    let start = invariant_value(ctx, start_expr, &values);
    let start_int = if let Some((a, b)) = static_interval(ctx, &p, start_expr, 0)
        .filter(|(a, b)| a == b && *a >= 0 && *b <= i32::MAX as i64)
    {
        let _ = b;
        a.to_string()
    } else {
        integer_extent(ctx, &start, &slow_l)
    };
    // Empty loops still evaluate the original condition once. The admitted
    // length/numeric condition is effect-free, so they may exit immediately.
    let nonempty_idx = ctx.new_block("indexed.owner.nonempty");
    let nonempty_l = ctx.block_label(nonempty_idx);
    let nonempty = ctx.block().icmp_slt(I32, &start_int, &bound_int);
    ctx.block().cond_br(&nonempty, &nonempty_l, &done_l);
    ctx.current_block = nonempty_idx;
    let mut static_windows = 0;
    for (id, index) in &indices {
        let proved = static_bound
            .and_then(|n| static_interval(ctx, &p, index, n))
            .is_some_and(|(lo, hi)| {
                lo >= 0
                    && owner_proof(ctx, *id, storage[id].kind)
                        .is_some_and(|o| hi < i64::from(o.min_len))
            });
        if proved {
            static_windows += 1;
            continue;
        }
        let (lo, hi) = interval(ctx, &p, index, &bound, &values, &slow_l);
        let low = ctx.block().fcmp("oge", &lo, "0.0");
        let length = ctx.block().uitofp(I32, &storage[id].len, DOUBLE);
        let high = ctx.block().fcmp("olt", &hi, &length);
        let ok = ctx.block().and(I1, &low, &high);
        branch_guard(ctx, &ok, &slow_l);
    }
    let fast_idx = ctx.new_block("indexed.owner.fast");
    let fast_l = ctx.block_label(fast_idx);
    let exit_idx = ctx.new_block("indexed.owner.exit");
    let exit_l = ctx.block_label(exit_idx);
    // Give each loop-carried scalar one native slot; SSA promotion removes
    // it. Preserve integer bitwise results as i32 across iterations.
    let mut slots = BTreeMap::new();
    for id in &p.writes {
        let integer = p.i32_writes.get(id).copied().unwrap_or(false);
        let slot = ctx.func.alloca_entry(if integer { I32 } else { DOUBLE });
        let v = if integer {
            values[id].i32(ctx)
        } else {
            values[id].f64(ctx)
        };
        ctx.block()
            .store(if integer { I32 } else { DOUBLE }, &v, &slot);
        slots.insert(*id, (slot, integer));
    }
    for id in &p.carried {
        if !slots.contains_key(id) {
            slots.insert(*id, (ctx.func.alloca_entry(DOUBLE), false));
        }
    }
    // Integer slots require that the INITIAL value is an exact i32 too:
    // ToInt32 on entry alone would change an addition/read before first write.
    for (id, (_, integer)) in &slots {
        if *integer && !matches!(ctx.local_slot_reps.get(id), Some(crate::expr::SlotRep::I32)) {
            let f = values[id].f64(ctx);
            let i = values[id].i32(ctx);
            let back = ctx.block().sitofp(I32, &i, DOUBLE);
            let f_bits = ctx.block().bitcast_double_to_i64(&f);
            let back_bits = ctx.block().bitcast_double_to_i64(&back);
            let exact = ctx.block().icmp_eq(I64, &f_bits, &back_bits);
            branch_guard(ctx, &exact, &slow_l);
        }
    }
    let ctr_slot = ctx.func.alloca_entry(I32);
    ctx.block().store(I32, &start_int, &ctr_slot);
    let admission_idx = ctx.current_block;
    let dynamic_entry_guards = ctx.func.blocks()[entry_blocks_start..]
        .iter()
        .filter(|b| b.label.starts_with("indexed.owner.admitted"))
        .count();
    let scan_start = ctx.func.num_blocks();
    ctx.current_block = fast_idx;
    let ctr = ctx.block().load(I32, &ctr_slot);
    values.insert(counter, Number::I(ctr.clone()));
    for (id, (slot, int)) in &slots {
        if p.writes.contains(id) {
            let v = ctx.block().load(if *int { I32 } else { DOUBLE }, slot);
            values.insert(*id, if *int { Number::I(v) } else { Number::F(v) });
        }
    }
    let mut native_indices = BTreeSet::new();
    for (_, e) in &p.indices {
        index_locals(e, &mut native_indices);
    }
    emit_body(
        ctx,
        &p,
        body,
        &mut values,
        &storage,
        &slots,
        &native_indices,
    );
    for (id, (slot, int)) in &slots {
        if !values.contains_key(id) {
            continue;
        }
        let v = if *int {
            values[id].i32(ctx)
        } else {
            values[id].f64(ctx)
        };
        ctx.block().store(if *int { I32 } else { DOUBLE }, &v, slot);
    }
    let next = ctx.block().add(I32, &ctr, "1");
    ctx.block().store(I32, &next, &ctr_slot);
    let more = ctx.block().icmp_slt(I32, &next, &bound_int);
    ctx.block().cond_br(&more, &fast_l, &exit_l);
    ctx.current_block = exit_idx;
    for id in p.writes.iter().chain(std::iter::once(&counter)) {
        let v = if *id == counter {
            ctx.block().sitofp(I32, &bound_int, DOUBLE)
        } else {
            let (slot, int) = &slots[id];
            let v = ctx.block().load(if *int { I32 } else { DOUBLE }, slot);
            if *int {
                ctx.block().sitofp(I32, &v, DOUBLE)
            } else {
                v
            }
        };
        if let Some(slot) = ctx.numeric_accumulator_f64_slots.get(id).cloned() {
            ctx.block().store(DOUBLE, &v, &slot);
            continue;
        }
        let v = crate::expr::nanbox_inline::canonicalize_lane_f64(ctx.block(), &v);
        if !crate::expr::store_canonical_local_from_double(ctx, *id, &v, None) {
            let slot = ctx.locals[id].clone();
            ctx.block().store(DOUBLE, &v, &slot);
            if let Some(shadow) = ctx.i32_counter_slots.get(id).cloned() {
                let i = ctx.toint32_wrap(&v);
                ctx.block().store(I32, &i, &shadow);
            }
        }
    }
    ctx.block().br(&done_l);
    let scan_end = ctx.func.num_blocks();
    // Verify the emitted revocation proof as well as the source grammar.
    // A future emitter that adds a call cannot silently retain raw pointers.
    let safe = !ctx.func.blocks()[fast_idx].contains_gc_unsafe_call()
        && !ctx.func.blocks()[exit_idx].contains_gc_unsafe_call()
        && (scan_start..scan_end).all(|i| !ctx.func.blocks()[i].contains_gc_unsafe_call());
    ctx.current_block = admission_idx;
    ctx.block().br(if safe { &fast_l } else { &slow_l });
    if safe {
        ctx.func.has_indexed_owner_region = true;
        let marker = crate::native_value::LoweredValue::js_value("0.0");
        ctx.record_lowered_value(
            "IndexedOwnerLoop",
            Some(counter),
            "IndexedOwnerLoop.hoisted_numeric_region",
            &marker,
            None,
            None,
            None,
            false,
            false,
            vec![
                format!("owners={:?};ascii_strings={:?}", p.arrays, p.strings),
                format!("index_windows={}", indices.len()),
                format!(
                    "dynamic_entry_guards={dynamic_entry_guards};nested_loops={}",
                    p.nested.len()
                ),
                format!(
                    "static_index_windows={static_windows};static_owners={}",
                    p.arrays
                        .iter()
                        .filter(|(id, k)| owner_proof(ctx, **id, **k).is_some())
                        .count()
                ),
                "proof=live_owner_header+exact_integer_intervals+number_inputs".into(),
                "effects=emitted_call_free;aliases=disjoint_live_byte_spans;exit=original_loop"
                    .into(),
            ],
        );
    }

    ctx.current_block = slow_idx;
    super::loops::lower_for_after_init(
        ctx,
        init,
        condition,
        update,
        body,
        "for.indexed_owner_slow",
    )?;
    if !ctx.block().is_terminated() {
        ctx.block().br(&done_l);
    }
    ctx.current_block = done_idx;
    Ok(true)
}

#[cfg(test)]
#[path = "indexed_owner_loop_tests.rs"]
mod tests;
