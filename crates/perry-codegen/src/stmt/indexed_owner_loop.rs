//! A numeric indexed-operation region. Declarations select element widths;
//! live owner headers, scalar guards and interval guards prove semantics.
//! The fast region has no calls, allocations or polls, hence cannot detach,
//! resize, rebind or relocate its storage. No validity state survives it.
//! Misses enter the original loop before any body effect. Aliases are allowed:
//! operations retain source order. Distinct receivers must have disjoint live
//! byte spans before the region may attach its own isolated alias scopes.
use crate::expr::FnCtx;
use crate::types::{DOUBLE, F32, I1, I16, I32, I64, I8};
use anyhow::Result;
use perry_hir::{BinaryOp, CompareOp, Expr, Stmt, UnaryOp, UpdateOp};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Default)]
struct Plan {
    counter: u32,
    arrays: BTreeMap<u32, u8>,
    aliases: BTreeMap<u32, u32>,
    defs: BTreeMap<u32, Expr>,
    inputs: BTreeSet<u32>,
    writes: BTreeSet<u32>,
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
        if matches!(kind, 9 | 10) {
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

    fn numeric(&mut self, ctx: &FnCtx<'_>, e: &Expr) -> bool {
        if let Some((root, index)) = self.read(ctx, e) {
            if !self.numeric(ctx, index) {
                return false;
            }
            self.indices.push((root, index.clone()));
            return true;
        }
        match e {
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
            Expr::LocalSet(id, value)
                if *id != self.counter
                    && !self.aliases.contains_key(id)
                    && !self.defs.contains_key(id)
                    && !ctx.numeric_accumulator_f64_slots.contains_key(id)
                    && (self.locals.contains(id)
                        || (plain_binding(ctx, *id)
                            && (ctx.locals.contains_key(id)
                                || ctx.local_slot_reps.contains_key(id)))) =>
            {
                if !self.numeric(ctx, value) {
                    return false;
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
                Stmt::Expr(e) if self.expression(ctx, e) => {}
                _ => return false,
            }
        }
        !self.indices.is_empty()
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
            Expr::LocalGet(id) => *id == self.counter,
            Expr::Binary {
                op: BinaryOp::Add | BinaryOp::Sub,
                left,
                right,
            } => self.window(left) && self.window(right),
            _ => false,
        }
    }
}

#[derive(Clone)]
enum Number {
    F(String),
    I(String),
    U(String),
    Index { wide: String, boxed: Box<Number> },
}
impl Number {
    fn f64(&self, ctx: &mut FnCtx<'_>) -> String {
        match self {
            Self::F(v) => v.clone(),
            Self::Index { boxed, .. } => boxed.f64(ctx),
            Self::I(v) => ctx.block().sitofp(I32, v, DOUBLE),
            Self::U(v) => ctx.block().uitofp(I32, v, DOUBLE),
        }
    }
    fn i32(&self, ctx: &mut FnCtx<'_>) -> String {
        match self {
            Self::F(v) => ctx.toint32_wrap(v),
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
    let (raw, ok) = crate::expr::byte_cell::owner_guard(
        ctx,
        &boxed,
        crate::expr::byte_cell::brand_for_kind(kind),
    );
    branch_guard(ctx, &ok, slow);
    let word = crate::expr::byte_cell::header_word(ctx.block(), &raw);
    // Admit only an ordinary owner, never a view, resizable/shared owner or
    // native arena. Detachment also clears its length. Preserve frozen writes.
    let disallowed = ctx.block().and(
        I64,
        &word,
        &((1u64 << 16) | (1 << 24) | (1 << 30)).to_string(),
    );
    let plain = ctx.block().icmp_eq(I64, &disallowed, "0");
    branch_guard(ctx, &plain, slow);
    let ptr = ctx.block().inttoptr(I64, &raw);
    let len = ctx.block().load(I32, &ptr);
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
            0 | 1 | 8 => 0,
            2 | 3 | 11 => 1,
            4..=6 => 2,
            _ => 3,
        };
        let bytes = ctx.block().shl(I64, &wide, &shift.to_string());
        let end = ctx.block().add(I64, &s.data, &bytes);
        spans.push((*id, s.data.clone(), end));
    }
    for (i, (_, start, end)) in spans.iter().enumerate() {
        for (_, other_start, other_end) in &spans[..i] {
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
        Expr::LocalGet(_) => {
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
            Number::F(v) => {
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
                Number::F(v) => ctx.block().fptosi(DOUBLE, &v, I64),
            }
        }
    }
}
fn address(ctx: &mut FnCtx<'_>, storage: &Storage, idx: &str) -> String {
    let shift = match storage.kind {
        0 | 1 | 8 => 0,
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
        Expr::Integer(n) if i32::try_from(*n).is_ok() => Number::I(n.to_string()),
        Expr::Integer(n) => Number::F(crate::nanbox::double_literal(*n as f64)),
        Expr::Number(n) => Number::F(crate::nanbox::double_literal(*n)),
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
            let l = l.f64(ctx);
            let r = r.f64(ctx);
            Number::F(match op {
                BinaryOp::Add => ctx.block().fadd(&l, &r),
                BinaryOp::Sub => ctx.block().fsub(&l, &r),
                BinaryOp::Mul => ctx.block().fmul(&l, &r),
                BinaryOp::Div => ctx.block().fdiv(&l, &r),
                BinaryOp::Mod => ctx.block().frem(&l, &r),
                _ => unreachable!(),
            })
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

/// First slice: linear numeric bodies, any numeric typed-array width, exact
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
    let counter = match init {
        Some(Stmt::Let {
            id,
            init: Some(Expr::Integer(0)),
            ..
        }) => *id,
        Some(Stmt::Let {
            id,
            init: Some(Expr::Number(n)),
            ..
        }) if n.to_bits() == 0 => *id,
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
        body_bindings: body
            .iter()
            .filter_map(|s| match s {
                Stmt::Let { id, .. } => Some(*id),
                _ => None,
            })
            .collect(),
        ..Default::default()
    };
    if !p.body(ctx, body) {
        return Ok(false);
    }
    let length_receiver = match bound_expr {
        Expr::PropertyGet {
            object, property, ..
        } if property == "length" => p.receiver(ctx, object),
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
    if p.arrays.keys().all(|id| {
        (ctx.known_noalias_buffer_locals.contains(id) && ctx.sealed_buffer_locals.contains(id))
            || crate::expr::proven_view_receiver(ctx, &Expr::LocalGet(*id)).is_some()
    }) {
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
    let slow_idx = ctx.new_block("indexed.owner.slow");
    let slow_l = ctx.block_label(slow_idx);
    let done_idx = ctx.new_block("indexed.owner.done");
    let done_l = ctx.block_label(done_idx);
    let mut values = BTreeMap::new();
    for id in &p.inputs {
        let v = boxed_binding(ctx, *id);
        let bits = ctx.block().bitcast_double_to_i64(&v);
        let number = crate::codegen::emit_plain_number_bits_test(ctx.block(), &bits);
        branch_guard(ctx, &number, &slow_l);
        values.insert(*id, Number::F(v));
    }
    let mut storage = BTreeMap::new();
    for (id, kind) in &p.arrays {
        storage.insert(*id, admit_owner(ctx, *id, *kind, &slow_l));
    }
    disjoint_storage(ctx, &mut storage, &slow_l);
    if let Some(id) = length_receiver {
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
    let bound_int = integer_extent(ctx, &bound, &slow_l);
    // Empty loops still evaluate the original condition once. The admitted
    // length/numeric condition is effect-free, so they may exit immediately.
    let nonempty_idx = ctx.new_block("indexed.owner.nonempty");
    let nonempty_l = ctx.block_label(nonempty_idx);
    let nonempty = ctx.block().icmp_ne(I32, &bound_int, "0");
    ctx.block().cond_br(&nonempty, &nonempty_l, &done_l);
    ctx.current_block = nonempty_idx;
    for (id, index) in &indices {
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
        let integer = body.iter().any(|s| matches!(s,
            Stmt::Expr(Expr::LocalSet(w, e)) if w == id && matches!(e.as_ref(),
                Expr::Binary { op: BinaryOp::BitAnd | BinaryOp::BitOr | BinaryOp::BitXor | BinaryOp::Shl | BinaryOp::Shr, .. })));
        // Admit integer slot only when EVERY write is an i32 operation.
        let integer = integer && body.iter().all(|s| !matches!(s,
            Stmt::Expr(Expr::LocalSet(w, e)) if w == id && !matches!(e.as_ref(),
                Expr::Binary { op: BinaryOp::BitAnd | BinaryOp::BitOr | BinaryOp::BitXor | BinaryOp::Shl | BinaryOp::Shr, .. }))) ;
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
    // Integer slots require that the INITIAL value is an exact i32 too:
    // ToInt32 on entry alone would change an addition/read before first write.
    for (id, (_, integer)) in &slots {
        if *integer {
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
    ctx.block().store(I32, "0", &ctr_slot);
    let admission_idx = ctx.current_block;
    let scan_start = ctx.func.num_blocks();
    ctx.current_block = fast_idx;
    let ctr = ctx.block().load(I32, &ctr_slot);
    values.insert(counter, Number::I(ctr.clone()));
    for (id, (slot, int)) in &slots {
        let v = ctx.block().load(if *int { I32 } else { DOUBLE }, slot);
        values.insert(*id, if *int { Number::I(v) } else { Number::F(v) });
    }
    let mut native_indices = BTreeSet::new();
    for (_, e) in &p.indices {
        index_locals(e, &mut native_indices);
    }
    for stmt in body {
        match stmt {
            Stmt::Let {
                id, init: Some(e), ..
            } if !p.aliases.contains_key(id) => {
                let boxed = numeric(ctx, &p, e, &mut values, &storage);
                let value = if native_indices.contains(id) {
                    let wide = index_value(ctx, &p, e, &mut values, &storage);
                    Number::Index {
                        wide,
                        boxed: Box::new(boxed),
                    }
                } else {
                    boxed
                };
                values.insert(*id, value);
            }
            Stmt::Let { .. } => {}
            Stmt::Expr(e) => {
                expression(ctx, &p, e, &mut values, &storage);
            }
            _ => unreachable!(),
        }
    }
    for (id, (slot, int)) in &slots {
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
                format!("owners={:?}", p.arrays),
                format!("index_windows={}", indices.len()),
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
