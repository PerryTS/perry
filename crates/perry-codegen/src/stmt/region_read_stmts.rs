//! Statement reads share the existing keyed holder proof and compact Get fallback.
//! Proofs run before effects; a miss executes the original initializers in order.

use anyhow::Result;
use perry_hir::{types::Type, BinaryOp, Expr, LogicalOp, Stmt, UnaryOp};
use std::collections::HashMap;

use crate::expr::region_guard;
use crate::expr::{lower_expr, FnCtx};
use crate::types::{DOUBLE, I1, I32, I64, PTR};
const MAX_KEYS: usize = 4; // The existing keyed holder cache has four ways.

// Compiler-only scope: the original Gets publish into the same keyed sites
// that the region consumes. No second runtime priming path or state word.
thread_local! { static KEYED_GETS: std::cell::RefCell<Vec<(u32, String, Vec<String>)>> = const { std::cell::RefCell::new(Vec::new()) }; }
fn keyed_get_slot(object: &Expr, property: &str) -> Option<(String, usize)> {
    let Expr::LocalGet(id) = object else {
        return None;
    };
    KEYED_GETS.with(|v| {
        v.borrow()
            .iter()
            .find(|(r, _, _)| r == id)
            .and_then(|(_, slot, keys)| {
                keys.iter()
                    .position(|k| k == property)
                    .map(|i| (slot.clone(), i))
            })
    })
}
pub(crate) fn lower_keyed_get(ctx: &mut FnCtx<'_>, expr: &Expr) -> Result<Option<String>> {
    let Expr::PropertyGet {
        object, property, ..
    } = expr
    else {
        return Ok(None);
    };
    let Some((slots, i)) = keyed_get_slot(object, property) else {
        return Ok(None);
    };
    let slot = ctx.block().gep(PTR, &slots, &[(I64, &i.to_string())]);
    let recv = lower_expr(ctx, object)?;
    let index = ctx.strings.intern(property);
    let handle = format!("@{}", ctx.strings.entry(index).handle_global);
    let key = ctx.block().load(DOUBLE, &handle);
    Ok(Some(ctx.block().call(
        DOUBLE,
        "js_dyn_index_get_site",
        &[(PTR, &slot), (DOUBLE, &recv), (DOUBLE, &key)],
    )))
}
struct KeyedGets;
impl KeyedGets {
    fn enter(sites: Vec<(u32, String, Vec<String>)>) -> Self {
        KEYED_GETS.with(|v| *v.borrow_mut() = sites);
        Self
    }
}
impl Drop for KeyedGets {
    fn drop(&mut self) {
        KEYED_GETS.with(|v| v.borrow_mut().clear());
    }
}

#[derive(Clone)]
struct Receiver<'a> {
    id: u32,
    keys: Vec<&'a str>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Number,
    Boolean,
}

pub(crate) struct StmtRun<'a> {
    receivers: Vec<Receiver<'a>>,
    binds: Vec<u32>,
    external: Vec<u32>,
    numeric: bool,
    reads: usize,
    pub(crate) len: usize,
}

fn binding_is_plain_slot(ctx: &FnCtx<'_>, id: u32, ty: &Type, init: &Expr) -> bool {
    (!matches!(init, Expr::PropertyGet { .. })
        || (matches!(ty, Type::Any)
            && crate::type_analysis::refine_type_from_init(ctx, init).is_none()))
        && matches!(ty, Type::Any | Type::Number | Type::Boolean)
        && crate::type_analysis::refine_type_from_init(ctx, init)
            .is_none_or(|t| matches!(t, Type::Any | Type::Number | Type::Boolean))
        && !ctx.boxed_vars.contains(&id)
        && !ctx.prealloc_boxes.contains(&id)
        && !ctx.tdz_boxes.contains(&id)
        && !ctx.module_globals.contains_key(&id)
        && !ctx.pod_records.contains_key(&id)
        && !ctx.spec_ta_bindings.contains_key(&id)
        && !ctx.integer_locals.contains(&id)
        && !ctx.local_slot_reps.contains_key(&id)
        && !ctx.i32_counter_slots.contains_key(&id)
}

pub(crate) fn try_match<'a>(ctx: &FnCtx<'_>, stmts: &'a [Stmt]) -> Option<StmtRun<'a>> {
    if !region_guard::emission_allowed() || !ctx.region_loop_facts.is_empty() {
        return None;
    }
    let run = scan(stmts, |id, ty, init| {
        binding_is_plain_slot(ctx, id, ty, init)
    })?;
    // Entry may read these locals before the original first GetValue. Refuse
    // environment/TDZ accesses that can throw before an earlier getter runs.
    let inert = |id| {
        ctx.locals.contains_key(&id)
            && !ctx.boxed_vars.contains(&id)
            && !ctx.prealloc_boxes.contains(&id)
            && !ctx.tdz_boxes.contains(&id)
            && !ctx.closure_captures.contains_key(&id)
            && !ctx.module_globals.contains_key(&id)
    };
    (run.receivers.iter().all(|r| inert(r.id)) && run.external.iter().all(|id| inert(*id)))
        .then_some(run)
}

/// Collect only inert leaves and operators whose numeric lowering cannot call
/// JavaScript. Calls, nested receivers, coercible objects and a third receiver
/// end the run. Candidate state is committed only after the whole init passes.
fn collect<'a>(
    expr: &'a Expr,
    receivers: &mut Vec<Receiver<'a>>,
    external: &mut Vec<u32>,
    kinds: &HashMap<u32, Kind>,
    reads: &mut usize,
) -> Option<Kind> {
    match expr {
        Expr::PropertyGet {
            object, property, ..
        } => {
            if property.starts_with('#') {
                return None;
            }
            let Expr::LocalGet(id) = object.as_ref() else {
                return None;
            };
            if kinds.contains_key(id) {
                return None;
            }
            let index = match receivers.iter().position(|r| r.id == *id) {
                Some(i) => i,
                None if receivers.len() < 2 => {
                    receivers.push(Receiver {
                        id: *id,
                        keys: Vec::new(),
                    });
                    receivers.len() - 1
                }
                None => return None,
            };
            let keys = &mut receivers[index].keys;
            if !keys.contains(&property.as_str()) {
                if keys.len() == MAX_KEYS {
                    return None;
                }
                keys.push(property);
            }
            *reads += 1;
            Some(Kind::Number)
        }
        Expr::LocalGet(id) => {
            if let Some(kind) = kinds.get(id) {
                return Some(*kind);
            }
            if !external.contains(id) {
                external.push(*id);
            }
            Some(Kind::Number)
        }
        Expr::Number(n) if n.to_bits() & 0x7fff_ffff_ffff_ffff < 0x7ff9_0000_0000_0000 => {
            Some(Kind::Number)
        }
        Expr::Integer(_) => Some(Kind::Number),
        Expr::Bool(_) => Some(Kind::Boolean),
        Expr::Binary { op, left, right }
            if matches!(
                op,
                BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Mod
            ) =>
        {
            let l = collect(left, receivers, external, kinds, reads)?;
            let r = collect(right, receivers, external, kinds, reads)?;
            (l == Kind::Number && r == Kind::Number).then_some(Kind::Number)
        }
        Expr::Compare { left, right, .. } => {
            let l = collect(left, receivers, external, kinds, reads)?;
            let r = collect(right, receivers, external, kinds, reads)?;
            (l == Kind::Number && r == Kind::Number).then_some(Kind::Boolean)
        }
        Expr::Logical { op, left, right } if matches!(op, LogicalOp::Or | LogicalOp::And) => {
            let l = collect(left, receivers, external, kinds, reads)?;
            let r = collect(right, receivers, external, kinds, reads)?;
            (l == r).then_some(l)
        }
        Expr::Unary { op, operand } if matches!(op, UnaryOp::Not | UnaryOp::Neg | UnaryOp::Pos) => {
            let k = collect(operand, receivers, external, kinds, reads)?;
            match op {
                UnaryOp::Not => Some(Kind::Boolean),
                _ => (k == Kind::Number).then_some(Kind::Number),
            }
        }
        _ => None,
    }
}

fn scan<'a>(stmts: &'a [Stmt], plain: impl Fn(u32, &Type, &Expr) -> bool) -> Option<StmtRun<'a>> {
    let mut receivers = Vec::new();
    let mut binds = Vec::new();
    let mut external = Vec::new();
    let mut kinds = HashMap::new();
    let mut reads = 0;
    let mut numeric = false;
    for stmt in stmts.iter().take(16) {
        let Stmt::Let {
            id,
            ty,
            init: Some(init),
            ..
        } = stmt
        else {
            break;
        };
        if binds.contains(id) || external.contains(id) || !plain(*id, ty, init) {
            break;
        }
        let mut next_receivers = receivers.clone();
        let mut next_external = external.clone();
        let mut next_reads = reads;
        let Some(kind) = collect(
            init,
            &mut next_receivers,
            &mut next_external,
            &kinds,
            &mut next_reads,
        ) else {
            break;
        };
        if next_receivers.iter().any(|r| r.id == *id) || next_external.contains(id) {
            break;
        }
        receivers = next_receivers;
        external = next_external;
        reads = next_reads;
        numeric |= !matches!(init, Expr::PropertyGet { .. });
        kinds.insert(*id, kind);
        binds.push(*id);
    }
    if reads < 2 || binds.len() < 2 {
        return None;
    }
    Some(StmtRun {
        receivers,
        len: binds.len(),
        binds,
        external,
        numeric,
        reads,
    })
}

#[derive(Clone)]
struct Value {
    kind: Kind,
    value: String,
    nullable: bool,
}

impl Value {
    fn undefined(&self, ctx: &mut FnCtx<'_>) -> String {
        if !self.nullable {
            return "false".into();
        }
        let bits = ctx.block().bitcast_double_to_i64(&self.value);
        ctx.block()
            .icmp_eq(I64, &bits, crate::nanbox::TAG_UNDEFINED_I64)
    }
    fn numeric(&self, ctx: &mut FnCtx<'_>) -> String {
        if !self.nullable {
            return self.value.clone();
        }
        let absent = self.undefined(ctx);
        ctx.block().select(
            I1,
            &absent,
            DOUBLE,
            &crate::nanbox::double_literal(f64::NAN),
            &self.value,
        )
    }
    fn truth(&self, ctx: &mut FnCtx<'_>) -> String {
        match self.kind {
            Kind::Number => ctx.block().fcmp("one", &self.value, "0.0"),
            Kind::Boolean => self.value.clone(),
        }
    }
    fn boxed(&self, ctx: &mut FnCtx<'_>) -> String {
        match self.kind {
            Kind::Number => self.value.clone(),
            Kind::Boolean => crate::stmt::region_loop::box_numeric_predicate(ctx, &self.value),
        }
    }
}

fn fold(
    ctx: &mut FnCtx<'_>,
    expr: &Expr,
    fields: &HashMap<(u32, &str), Value>,
    locals: &HashMap<u32, Value>,
) -> Value {
    let number = |value| Value {
        kind: Kind::Number,
        value,
        nullable: false,
    };
    let boolean = |value| Value {
        kind: Kind::Boolean,
        value,
        nullable: false,
    };
    match expr {
        Expr::PropertyGet {
            object, property, ..
        } => {
            let Expr::LocalGet(id) = object.as_ref() else {
                unreachable!()
            };
            fields[&(*id, property.as_str())].clone()
        }
        Expr::LocalGet(id) => locals[id].clone(),
        Expr::Integer(n) => number(crate::nanbox::double_literal(*n as f64)),
        Expr::Number(n) => number(crate::nanbox::double_literal(*n)),
        Expr::Bool(b) => boolean(b.to_string()),
        Expr::Binary { op, left, right } => {
            let l = fold(ctx, left, fields, locals).numeric(ctx);
            let r = fold(ctx, right, fields, locals).numeric(ctx);
            number(match op {
                BinaryOp::Add => ctx.block().fadd(&l, &r),
                BinaryOp::Sub => ctx.block().fsub(&l, &r),
                BinaryOp::Mul => ctx.block().fmul(&l, &r),
                BinaryOp::Div => ctx.block().fdiv(&l, &r),
                BinaryOp::Mod => ctx.block().frem(&l, &r),
                _ => unreachable!(),
            })
        }
        Expr::Compare { op, left, right } => {
            use perry_hir::CompareOp;
            let l = fold(ctx, left, fields, locals);
            let r = fold(ctx, right, fields, locals);
            let ln = l.numeric(ctx);
            let rn = r.numeric(ctx);
            let pred = crate::stmt::region_loop::numeric_predicate(ctx, *op, &ln, &rn);
            if matches!(
                op,
                CompareOp::Eq | CompareOp::LooseEq | CompareOp::Ne | CompareOp::LooseNe
            ) {
                let lu = l.undefined(ctx);
                let ru = r.undefined(ctx);
                let both = ctx.block().and(I1, &lu, &ru);
                boolean(if matches!(op, CompareOp::Eq | CompareOp::LooseEq) {
                    ctx.block().or(I1, &both, &pred)
                } else {
                    let not_both = ctx.block().xor(I1, &both, "true");
                    ctx.block().and(I1, &not_both, &pred)
                })
            } else {
                boolean(pred)
            }
        }
        Expr::Unary { op, operand } => {
            let v = fold(ctx, operand, fields, locals);
            match op {
                UnaryOp::Not => {
                    let truth = v.truth(ctx);
                    boolean(ctx.block().xor(I1, &truth, "true"))
                }
                UnaryOp::Neg => {
                    let n = v.numeric(ctx);
                    number(ctx.block().fneg(&n))
                }
                UnaryOp::Pos => number(v.numeric(ctx)),
                _ => unreachable!(),
            }
        }
        Expr::Logical { op, left, right } => {
            let l = fold(ctx, left, fields, locals);
            let r = fold(ctx, right, fields, locals);
            let truth = l.truth(ctx);
            let (yes, no) = if *op == LogicalOp::Or {
                (&l, &r)
            } else {
                (&r, &l)
            };
            Value {
                kind: l.kind,
                nullable: l.nullable || r.nullable,
                value: ctx.block().select(
                    I1,
                    &truth,
                    if l.kind == Kind::Number { DOUBLE } else { I1 },
                    &yes.value,
                    &no.value,
                ),
            }
        }
        _ => unreachable!("collector admits only pure numeric trees"),
    }
}

pub(crate) fn lower(ctx: &mut FnCtx<'_>, stmts: &[Stmt], run: &StmtRun<'_>) -> Result<()> {
    region_guard::note_stmt_region(run.reads as u64);
    let start = ctx.current_block;
    let generic = ctx.new_block("region.stmt.generic");
    let fast = ctx.new_block("region.stmt.fast");
    let merge = ctx.new_block("region.stmt.merge");
    let generic_l = ctx.block_label(generic);
    let fast_l = ctx.block_label(fast);
    let merge_l = ctx.block_label(merge);

    let mut sites = Vec::new();
    for receiver in &run.receivers {
        let site = ctx.ic_site_counter;
        ctx.ic_site_counter += 1;
        let slot = format!(
            "@{}_region_holder",
            crate::expr::inline_cache_global_name(ctx, site)
        );
        ctx.typed_parse_rodata.push(format!(
            "{slot} = private global [4 x ptr] zeroinitializer, align 8"
        ));
        sites.push((
            receiver.id,
            slot,
            receiver.keys.iter().map(|k| k.to_string()).collect(),
        ));
    }
    // Declare the original bindings ONCE, with their actual initializer facts.
    ctx.current_block = generic;
    crate::expr::emit_versioned_loop_callback_deopt(ctx);
    {
        let _suppressed = region_guard::Suppressed::enter();
        let _keyed = KeyedGets::enter(sites.clone());
        for stmt in stmts.iter().take(run.len) {
            super::lower_stmt(ctx, stmt)?;
        }
    }
    ctx.block().br(&merge_l);

    ctx.current_block = start;
    let mut locals = HashMap::new();
    // External leaves may need environment/box access. Evaluate them before
    // retaining any receiver pointer; no effects occur on the fast path after R1.
    for id in &run.external {
        let value = lower_expr(ctx, &Expr::LocalGet(*id))?;
        let number = crate::stmt::emit_js_value_is_number(ctx, &value);
        let next = ctx.new_block("region.local.number");
        let next_l = ctx.block_label(next);
        ctx.block().cond_br(&number, &next_l, &generic_l);
        ctx.current_block = next;
        locals.insert(
            *id,
            Value {
                kind: Kind::Number,
                value,
                nullable: false,
            },
        );
    }
    // Materialize every receiver while values still have their boxed form.
    // An environment access can collect; none may occur after R1 has retained
    // a raw receiver pointer for the run.
    let receivers = run
        .receivers
        .iter()
        .map(|receiver| lower_expr(ctx, &Expr::LocalGet(receiver.id)))
        .collect::<Result<Vec<_>>>()?;
    let mut fields = HashMap::new();
    for ((receiver, recv), (_, slot, _)) in run.receivers.iter().zip(&receivers).zip(&sites) {
        let keys = ctx.func.alloca_entry("[4 x i64]");
        let out = ctx.func.alloca_entry("[4 x double]");
        for (i, name) in receiver.keys.iter().enumerate() {
            let idx = ctx.strings.intern(name);
            let handle = format!("@{}", ctx.strings.entry(idx).handle_global);
            let key = ctx.block().load(I64, &handle);
            let ptr = ctx.block().gep(I64, &keys, &[(I64, &i.to_string())]);
            ctx.block().store(I64, &key, &ptr);
        }
        let ok = ctx.block().call(
            I32,
            "js_region_holder_read",
            &[
                (PTR, &slot),
                (DOUBLE, recv),
                (PTR, &keys),
                (I32, &receiver.keys.len().to_string()),
                (PTR, &out),
                (I32, if run.numeric { "1" } else { "0" }),
            ],
        );
        let hit = ctx.block().icmp_ne(I32, &ok, "0");
        let next = ctx.new_block("region.stmt.receiver");
        let next_l = ctx.block_label(next);
        ctx.block().cond_br(&hit, &next_l, &generic_l);
        ctx.current_block = next;
        for (i, name) in receiver.keys.iter().enumerate() {
            let ptr = ctx.block().gep(DOUBLE, &out, &[(I64, &i.to_string())]);
            let value = ctx.block().load(DOUBLE, &ptr);
            fields.insert(
                (receiver.id, *name),
                Value {
                    kind: Kind::Number,
                    value,
                    nullable: run.numeric,
                },
            );
        }
    }
    ctx.block().br(&fast_l);
    ctx.current_block = fast;
    for (id, stmt) in run.binds.iter().zip(stmts) {
        let Stmt::Let {
            init: Some(init), ..
        } = stmt
        else {
            unreachable!()
        };
        let value = fold(ctx, init, &fields, &locals);
        let boxed = value.boxed(ctx);
        let slot = ctx.locals[id].clone();
        ctx.block().store(DOUBLE, &boxed, &slot);
        // Numeric entry facts permit clearing the shadow root without a tag
        // test; other values need the same binding as the original Let.
        crate::expr::emit_shadow_slot_update_for_expr(
            ctx,
            *id,
            &boxed,
            if run.numeric {
                &Expr::Number(0.0)
            } else {
                init
            },
        );
        if let Some(slot) = ctx.i1_local_slots.get(id).cloned() {
            let truth = value.truth(ctx);
            ctx.block().store(I1, &truth, &slot);
        }
        if let Some(slot) = ctx.i32_counter_slots.get(id).cloned() {
            let integer = ctx.block().fptosi(DOUBLE, &boxed, I32);
            ctx.block().store(I32, &integer, &slot);
        }
        locals.insert(*id, value);
    }
    ctx.block().br(&merge_l);
    ctx.current_block = merge;
    Ok(())
}

#[cfg(test)]
mod tests {
    use perry_hir::types::Type;
    use perry_hir::{Expr, Stmt};

    use super::scan;

    fn read_let(id: u32, recv: u32, key: &str) -> Stmt {
        Stmt::Let {
            id,
            name: format!("v{id}"),
            ty: Type::Any,
            mutable: false,
            init: Some(Expr::PropertyGet {
                object: Box::new(Expr::LocalGet(recv)),
                property: key.to_string(),
                byte_offset: 0,
            }),
        }
    }

    fn plain_always(_: u32, ty: &Type, _: &Expr) -> bool {
        matches!(ty, Type::Any)
    }

    #[test]
    fn consecutive_reads_of_one_receiver_form_one_run() {
        let stmts = vec![
            read_let(10, 1, "a"),
            read_let(11, 1, "b"),
            read_let(12, 1, "c"),
        ];
        let run = scan(&stmts, plain_always).expect("three reads of one receiver are a run");
        assert_eq!(run.len, 3, "the run consumes all three statements");
        assert_eq!(run.receivers[0].keys, vec!["a", "b", "c"]);
    }

    /// One read already pays one guard, so there is nothing to share.
    #[test]
    fn a_single_read_is_not_a_run() {
        assert!(scan(&[read_let(10, 1, "a")], plain_always).is_none());
    }

    /// A repeated key is one slot in the word, read twice.
    #[test]
    fn a_repeated_key_shares_its_slot() {
        let stmts = vec![
            read_let(10, 1, "a"),
            read_let(11, 1, "b"),
            read_let(12, 1, "a"),
        ];
        let run = scan(&stmts, plain_always).unwrap();
        assert_eq!(run.receivers[0].keys, vec!["a", "b"], "two distinct keys");
        assert_eq!(run.len, 3, "but three bindings");
    }

    /// A second receiver joins the run with its own guard.
    #[test]
    fn two_receivers_share_the_run() {
        let stmts = vec![
            read_let(10, 1, "a"),
            read_let(11, 1, "b"),
            read_let(12, 2, "a"),
        ];
        let run = scan(&stmts, plain_always).unwrap();
        assert_eq!(run.len, 3);
        assert_eq!(run.receivers.len(), 2);
    }

    /// Any statement that is not such a read is an R1-R4 event: a call, a store,
    /// an allocation or an unverified operator all arrive here as "not a read".
    #[test]
    fn a_non_read_statement_ends_the_run() {
        let call = Stmt::Expr(Expr::Call {
            callee: Box::new(Expr::LocalGet(7)),
            args: Vec::new(),
            type_args: Vec::new(),
            byte_offset: 0,
        });
        let stmts = vec![
            read_let(10, 1, "a"),
            read_let(11, 1, "b"),
            call,
            read_let(12, 1, "c"),
        ];
        let run = scan(&stmts, plain_always).unwrap();
        assert_eq!(run.len, 2, "the call ends the run before the third read");
    }

    /// Four keyed ways cover a receiver; the fifth distinct key ends the run rather
    /// than silently dropping a read out of it.
    #[test]
    fn the_fifth_distinct_key_ends_the_run() {
        let stmts: Vec<Stmt> = ["a", "b", "c", "d", "e", "f"]
            .iter()
            .enumerate()
            .map(|(i, k)| read_let(10 + i as u32, 1, k))
            .collect();
        let run = scan(&stmts, plain_always).unwrap();
        assert_eq!(run.len, 4);
        assert_eq!(run.receivers[0].keys.len(), 4);
    }

    /// Reading into the receiver's own binding would make one guard cover a
    /// receiver it did not prove.
    #[test]
    fn a_binding_that_is_the_receiver_ends_the_run() {
        let stmts = vec![
            read_let(10, 1, "a"),
            read_let(11, 1, "b"),
            read_let(1, 1, "c"),
        ];
        let run = scan(&stmts, plain_always).unwrap();
        assert_eq!(run.len, 2);
    }

    /// A binding whose storage is not a plain slot declines the whole run: the
    /// fast arm assigns it with a bare store, which only a plain slot accepts.
    #[test]
    fn a_binding_that_is_not_a_plain_slot_declines() {
        let stmts = vec![read_let(10, 1, "a"), read_let(11, 1, "b")];
        assert!(scan(&stmts, |_, _, _| false).is_none());
    }
}

#[cfg(test)]
#[path = "region_read_stmts/ir_tests.rs"]
mod ir_tests;
