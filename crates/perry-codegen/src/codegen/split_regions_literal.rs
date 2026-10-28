//! Literal chunks: a giant literal-shape birth no longer lives in one body.
//!
//! A plain object literal lowers to `new __AnonShape_*(v0, v1, …)`, one
//! statement however many properties it has, so the run splitter keeps it in
//! place (a single giant statement) and its parent holds every value, every
//! value's root and the birth. Here the values of such a birth from slot
//! [`HEAD_SLOTS`] on move, in source order, into chunk functions: ordinary
//! regions (`try_extract`) whose body is one [`Expr::LiteralSlotInit`] per
//! slot. The birth itself stays where it was, with the same class and so the
//! same complete key vector and shape: its chunked arguments become
//! [`Expr::LiteralChunks`], the first holding the chunk calls, and codegen
//! births the object exactly as before (the chunked slots hold the birth's
//! `undefined` fill), keeps it rooted across the calls and yields it only
//! after the last one returns. The binding is therefore published after every
//! chunk ran, and a throw in a chunk abandons the unpublished object.
//!
//! Only what the existing HIR proves equivalent moves. A literal-shape class
//! has no accessors, methods, computed members or spreads (those literals are
//! other HIR forms); a duplicate key or a `__proto__` key keeps today's
//! lowering. A function value moves like any other: a ConstFn lane is a fact
//! of a head slot only (the birth representation word), and a closure that
//! captures `this` (the static birth's `this` patch) is frame-bound. A value
//! that is frame-bound, leaves other than by completing, or needs more than an
//! ordinary by-value live-in makes `try_extract` refuse its chunk, and then
//! nothing of the literal moves.

use std::collections::{HashMap, HashSet};

use perry_hir::types::Type;
use perry_hir::{Expr, Module as HirModule, Stmt};

use super::facts::{closure_captured_ids, let_counts, ref_counts, ref_set};
use super::{names_of, report_enabled, sanitize_tsv, try_extract, BodyCtx, ModuleFacts, Shared};

/// Slots that never move: the birth representation word (`BirthShape::rep`,
/// two bits per slot in a `u64`) describes slots `0..32`, and their lanes
/// follow the values (a numeric literal's raw lane, a ConstFn lane). Those
/// values stay in the birth, so its representation is the one it had.
pub(super) const HEAD_SLOTS: usize = 64 / 2;

/// One wide literal-shape class: its keys in slot order, and whether a slot
/// past [`HEAD_SLOTS`] is a raw-f64 lane of its typed layout
/// (`typed_shape::class_typed_layout`): such a slot needs its value at the
/// birth, so the class's births keep today's lowering.
pub(super) struct LiteralClass {
    keys: Vec<String>,
    raw_tail: bool,
}

/// The module's literal-shape classes wide enough to have chunkable slots.
pub(super) fn literal_classes(hir: &HirModule) -> HashMap<String, LiteralClass> {
    hir.classes
        .iter()
        .filter(|c| c.is_literal_shape() && c.fields.len() > HEAD_SLOTS)
        .map(|c| {
            (
                c.name.clone(),
                LiteralClass {
                    keys: c.fields.iter().map(|f| f.name.clone()).collect(),
                    raw_tail: c.fields[HEAD_SLOTS..]
                        .iter()
                        .any(|f| crate::typed_shape::type_is_raw_f64_candidate(&f.ty)),
                },
            )
        })
        .collect()
}

/// The expression position of `s` a birth may sit in: a declaration's
/// initializer, a returned value or an assigned value.
fn birth_position(s: &mut Stmt) -> Option<&mut Expr> {
    match s {
        Stmt::Let { init: Some(e), .. } | Stmt::Return(Some(e)) => Some(e),
        Stmt::Expr(Expr::LocalSet(_, e)) => Some(e.as_mut()),
        _ => None,
    }
}

/// Why a literal-shape birth keeps today's lowering, if it does.
fn refusal(keys: &[String], tail: &[Expr]) -> Option<&'static str> {
    let mut seen = HashSet::new();
    if keys.iter().any(|k| !seen.insert(k.as_str())) {
        return Some("duplicate-key");
    }
    if keys.iter().any(|k| k == "__proto__") {
        return Some("proto-key");
    }
    if tail.iter().any(|v| matches!(v, Expr::LiteralChunks(_))) {
        return Some("already-chunked");
    }
    None
}

/// Give back the local ids handed out since `saved` (and their spans).
fn return_locals(sh: &mut Shared<'_>, saved: u32) {
    for id in saved + 1..=sh.next_local {
        sh.spans.remove(&id);
    }
    sh.next_local = saved;
}

fn reject(sh: &mut Shared<'_>, ctx: &BodyCtx<'_>, class: &str, why: &str) {
    if report_enabled() {
        sh.report.push(format!(
            "perry-split\tliteral-reject\t{}\t{}\t{}",
            sanitize_tsv(&ctx.parent),
            class,
            why
        ));
    }
}

/// Move the chunkable slots of a giant literal-shape birth in `s` into chunk
/// functions. `dominating`: ids whose declaration has run before `s`.
/// `module_lets`: for the module init, the bindings a value may read only once
/// their declaration has run (declared before `s`, plain, never written).
pub(super) fn chunk_giant_literal(
    s: &mut Stmt,
    ctx: &mut BodyCtx<'_>,
    sh: &mut Shared<'_>,
    dominating: &HashSet<u32>,
    proofs: &HashMap<u32, Type>,
    module_lets: Option<&HashSet<u32>>,
) {
    let target = ctx.facts.limits.target;
    let Some(Expr::New {
        class_name,
        args,
        cap_args_appended: 0,
        ..
    }) = birth_position(s)
    else {
        return;
    };
    let Some(literal) = sh.literals.get(class_name.as_str()) else {
        return;
    };
    if literal.keys.len() != args.len() {
        return;
    }
    let keys = literal.keys.clone();
    let raw_tail = literal.raw_tail;
    let class = class_name.clone();
    if let Some(why) = raw_tail
        .then_some("raw-f64-lane")
        .or_else(|| refusal(&keys, &args[HEAD_SLOTS..]))
    {
        reject(sh, ctx, &class, why);
        return;
    }

    // The unpublished object: a parent local only for the length of the
    // extraction, so each chunk takes it as its one by-value live-in. A birth
    // that keeps today's lowering leaves no trace: its fresh ids are returned.
    let saved_local = sh.next_local;
    let obj = sh.fresh_local(u32::MAX);
    let values: Vec<Expr> = args.drain(HEAD_SLOTS..).collect();
    let stmts: Vec<Stmt> = values
        .into_iter()
        .enumerate()
        .map(|(k, value)| {
            let slot = HEAD_SLOTS + k;
            Stmt::Expr(Expr::LiteralSlotInit {
                object: Box::new(Expr::LocalGet(obj)),
                key: keys[slot].clone(),
                slot: slot as u32,
                value: Box::new(value),
            })
        })
        .collect();
    let restore = |args: &mut Vec<Expr>, stmts: Vec<Stmt>| {
        args.extend(stmts.into_iter().map(|s| match s {
            Stmt::Expr(Expr::LiteralSlotInit { value, .. }) => *value,
            _ => unreachable!("literal chunk statements are slot inits"),
        }));
    };

    let weights: Vec<usize> = stmts
        .iter()
        .map(|st| sh.cost.body(std::slice::from_ref(st)))
        .collect();
    // The run splitter's giant single-statement bound (`extract_or_halve`):
    // a smaller birth is an ordinary statement.
    if weights.iter().sum::<usize>() <= 2 * target {
        restore(args, stmts);
        return_locals(sh, saved_local);
        return;
    }

    // A module init's values reach module bindings directly (a chunk is a
    // top-level function like any other); only a binding that already holds
    // its one value may be read that way.
    if let Some(lets) = module_lets {
        let used = ref_set(&stmts);
        if used
            .iter()
            .any(|id| *id != obj && ctx.decl_counts.contains_key(id) && !lets.contains(id))
        {
            restore(args, stmts);
            return_locals(sh, saved_local);
            reject(sh, ctx, &class, "module-binding-not-settled");
            return;
        }
    }

    // Chunks of about one region's size, in source order.
    let mut ranges: Vec<(usize, usize)> = Vec::new();
    let (mut start, mut acc) = (0usize, 0usize);
    for (i, w) in weights.iter().enumerate() {
        acc += w;
        if acc >= target {
            ranges.push((start, i + 1));
            start = i + 1;
            acc = 0;
        }
    }
    if start < stmts.len() {
        match ranges.last_mut() {
            Some(last) if acc < target / 4 => last.1 = stmts.len(),
            _ => ranges.push((start, stmts.len())),
        }
    }

    // Everything a refused chunk would leave behind, to undo it.
    let out_len = sh.out.len();
    let closures_len = sh.closure_names.len();
    let report_len = sh.report.len();
    let (seq, next_func) = (sh.region_seq, sh.next_func);
    let saved_counts = ctx.counts.clone();
    let saved_decls = ctx.decl_counts.clone();
    ctx.declared.insert(obj);
    ctx.names.insert(obj, "literal".to_string());
    ctx.counts.insert(obj, stmts.len() as i64);
    let mut dom = dominating.clone();
    dom.insert(obj);

    let mut calls: Vec<Expr> = Vec::new();
    let mut refused = None;
    for &(a, b) in &ranges {
        let Some(mut x) = try_extract(&stmts[a..b], ctx, sh, &dom, proofs, "literal") else {
            refused = Some("chunk-refused");
            break;
        };
        // One plain call, nothing written back: the chunk only initializes
        // slots of the object it is handed.
        let call = match (x.calls.pop(), x.calls.is_empty()) {
            (Some(Stmt::Expr(call @ Expr::Call { .. })), true) => call,
            _ => {
                refused = Some("chunk-writes-back");
                break;
            }
        };
        let Expr::Call {
            callee,
            mut args,
            type_args,
            byte_offset,
            ..
        } = call
        else {
            unreachable!()
        };
        let Expr::FuncRef(fid) = *callee else {
            refused = Some("chunk-is-closure");
            break;
        };
        // The object is the chunk's first parameter (the codegen calls the
        // chunk with its rooted birth as argument 0).
        let Some(at) = args
            .iter()
            .position(|a| matches!(a, Expr::LocalGet(id) if *id == obj))
        else {
            refused = Some("chunk-without-object");
            break;
        };
        let Some(f) = sh.out.iter_mut().rev().find(|f| f.id == fid) else {
            refused = Some("chunk-without-object");
            break;
        };
        let p = f.params.remove(at);
        f.params.insert(0, p);
        args.remove(at);
        args.insert(0, Expr::LiteralChunkObject);
        calls.push(Expr::Call {
            byte_end: 0,
            callee: Box::new(Expr::FuncRef(fid)),
            args,
            type_args,
            byte_offset,
        });
    }
    ctx.declared.remove(&obj);
    ctx.names.remove(&obj);
    if let Some(why) = refused {
        sh.out.truncate(out_len);
        sh.closure_names.truncate(closures_len);
        sh.report.truncate(report_len);
        sh.region_seq = seq;
        sh.next_func = next_func;
        return_locals(sh, saved_local);
        ctx.counts = saved_counts;
        ctx.decl_counts = saved_decls;
        restore(args, stmts);
        reject(sh, ctx, &class, why);
        return;
    }
    ctx.counts.remove(&obj);
    let chunked = stmts.len();
    let chunks = calls.len();
    args.push(Expr::LiteralChunks(calls));
    args.extend((1..chunked).map(|_| Expr::LiteralChunks(Vec::new())));
    if report_enabled() {
        sh.report.push(format!(
            "perry-split\tliteral\t{}\t{}\tslots={}\tchunked={}\tchunks={}\tnodes={}",
            sanitize_tsv(&ctx.parent),
            class,
            keys.len(),
            chunked,
            chunks,
            weights.iter().sum::<usize>()
        ));
    }
}

/// The module init's top-level giant literal births. The init is not split
/// into regions itself (`entry_outline` owns that); only its literal births'
/// values move. Its declarations are module bindings, which a chunk (a
/// top-level function) reads directly, so nothing but the object crosses the
/// call.
pub(super) fn chunk_init_literals(
    init: &mut [Stmt],
    parent: &str,
    is_strict: bool,
    facts: &ModuleFacts,
    sh: &mut Shared<'_>,
) {
    if sh.literals.is_empty() {
        return;
    }
    let mut counts = HashMap::new();
    ref_counts(init, &mut counts, 1);
    let mut ctx = BodyCtx {
        facts,
        parent: parent.to_string(),
        is_strict,
        params: HashSet::new(),
        captures: HashSet::new(),
        declared: HashSet::new(),
        captured: closure_captured_ids(init),
        typed: HashMap::new(),
        names: names_of(&[], init),
        counts,
        decl_counts: let_counts(init),
        single_init: HashSet::new(),
    };
    // Bindings a value may read: declared once, with a value, earlier at the
    // top level, never written, no cell (captured or preallocated).
    let mut settled: HashSet<u32> = HashSet::new();
    for i in 0..init.len() {
        chunk_giant_literal(
            &mut init[i],
            &mut ctx,
            sh,
            &HashSet::new(),
            &HashMap::new(),
            Some(&settled),
        );
        if let Stmt::Let {
            id, init: Some(_), ..
        } = &init[i]
        {
            if ctx.decl_counts.get(id) == Some(&1)
                && !facts.writes.contains(id)
                && !facts.prealloc.contains(id)
                && !facts.prealloc_tdz.contains(id)
                && !ctx.captured.contains(id)
            {
                settled.insert(*id);
            }
        }
    }
}
