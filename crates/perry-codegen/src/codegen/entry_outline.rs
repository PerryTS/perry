//! Structured module-entry outlining (#8595).
//!
//! The module top level is lowered into a single LLVM function (`@main` /
//! `perry_module_init`). For a large minified bundle that one function is
//! enormous (the Claude Code `cli.js` entry is ~68 MB of IR, ~13,170 GC-root
//! slots), which is simultaneously pathological for `rewrite-statepoints-for-gc`
//! (relocation fan-out, #8583), instruction selection (#4880), and register
//! allocation. The fix is to outline the entry body into many small functions.
//!
//! Oversized entry bodies are split at top-level statement boundaries into
//! ordinary HIR functions. The original statements move unchanged, and calls
//! to the chunks remain in the original order. Codegen's module-global pass
//! recognises declarations in these compiler-owned chunks as module bindings,
//! so a declaration still executes at its source position while references
//! from another chunk share the same rooted storage.
//!
//! Outlining is automatic only for very large bodies. `PERRY_OUTLINE_ENTRY=1`
//! forces it for testing and measurement; `=0` disables it. Top-level await
//! and a module-level TDZ preallocation remain fail-safe exclusions because a
//! raw module-global load cannot yet perform the checked TDZ-box read.

use std::collections::HashSet;

use perry_hir::Module as HirModule;

use crate::collectors::{collect_let_ids, collect_ref_ids_in_stmts};

/// Default target number of top-level statements per outlined chunk. Chosen so
/// a chunk's live-root × safepoint product stays well under the RS4GC fan-out
/// regime (#8583). The independent safepoint budget below can flush sooner.
/// Overridable with `PERRY_OUTLINE_ENTRY_CHUNK_STMTS`.
const DEFAULT_CHUNK_STMTS: usize = 200;

/// Ordinary modules are deliberately left byte-for-byte unchanged. The
/// production pathology has tens of thousands of top-level HIR statements;
/// 1,000 is low enough to catch it while keeping normal source modules out.
const DEFAULT_AUTO_MIN_STMTS: usize = 1_000;

/// Call-like expressions are the dominant source of pointer temporaries and
/// statepoints. A generated entry with fewer top-level statements can still be
/// pathological, so both automatic admission and chunk flushing have a
/// safepoint budget.
const DEFAULT_CHUNK_SAFEPOINTS: usize = 1_000;
const DEFAULT_AUTO_MIN_SAFEPOINTS: usize = 4_000;

/// Compiler-owned name prefix used to distinguish outlined entry functions
/// from source functions when reconstructing the logical top-level stream.
const ENTRY_CHUNK_PREFIX: &str = "__perry_entry_chunk_";

fn target_chunk_stmts() -> usize {
    std::env::var("PERRY_OUTLINE_ENTRY_CHUNK_STMTS")
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .filter(|&n| n > 0)
        .unwrap_or(DEFAULT_CHUNK_STMTS)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OutlineMode {
    Auto,
    Forced,
    Disabled,
}

fn outline_mode_from_env(value: Option<&str>) -> OutlineMode {
    match value {
        Some("1" | "on" | "true") => OutlineMode::Forced,
        Some("0" | "off" | "false") => OutlineMode::Disabled,
        _ => OutlineMode::Auto,
    }
}

fn outline_mode() -> OutlineMode {
    let value = std::env::var("PERRY_OUTLINE_ENTRY").ok();
    outline_mode_from_env(value.as_deref())
}

fn meets_automatic_size_threshold(stmt_count: usize, safepoint_count: usize) -> bool {
    stmt_count >= DEFAULT_AUTO_MIN_STMTS || safepoint_count >= DEFAULT_AUTO_MIN_SAFEPOINTS
}

fn report_requested() -> bool {
    matches!(
        std::env::var("PERRY_OUTLINE_ENTRY_REPORT").as_deref(),
        Ok("1") | Ok("on") | Ok("true")
    )
}

/// Result of analysing whether/how a module entry body would outline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EntryOutlineAnalysis {
    /// Number of top-level statements in `hir.init`.
    pub total_stmts: usize,
    /// Number of chunks the body would split into at the current target.
    pub chunk_count: usize,
    /// Top-level `let`s defined in one chunk and referenced from another —
    /// the bindings the transform must globalize so chunks share state. (The
    /// existing `emit_module_globals` escape rule already globalizes any let
    /// referenced from a separate function body, so once chunks are functions
    /// these are globalized for free; this counts them for reporting.)
    pub cross_chunk_lets: usize,
    /// `Some(reason)` if the transform would decline to outline this body even
    /// when enabled — the body is not a safe candidate.
    pub gated_out: Option<&'static str>,
}

impl EntryOutlineAnalysis {
    /// Whether this body is a candidate the transform would act on (large
    /// enough to be worth splitting and not gated out).
    pub fn is_candidate(&self) -> bool {
        self.gated_out.is_none() && self.chunk_count > 1
    }
}

pub(crate) fn is_entry_chunk(function: &perry_hir::Function) -> bool {
    function.name.starts_with(ENTRY_CHUNK_PREFIX)
        && function.params.is_empty()
        && matches!(function.return_type, perry_hir::types::Type::Void)
        && !function.is_async
        && !function.is_generator
        && !function.is_exported
}

/// Reconstruct the source-order module-entry statement stream after outlining.
///
/// Several codegen analyses intentionally inspect module declarations rather
/// than ordinary function bodies (exported closure signatures, const folding,
/// static-field deduplication, and early `process.env` assignments). Replacing
/// a range with a chunk call must not hide those original statements from the
/// analyses. Non-chunk calls and all inline statements are returned unchanged.
pub fn logical_entry_stmts(hir: &HirModule) -> Vec<&perry_hir::Stmt> {
    let chunks: std::collections::HashMap<u32, &perry_hir::Function> = hir
        .functions
        .iter()
        .filter(|function| is_entry_chunk(function))
        .map(|function| (function.id, function))
        .collect();
    let mut logical = Vec::new();
    for stmt in &hir.init {
        let chunk = match stmt {
            perry_hir::Stmt::Expr(perry_hir::Expr::Call { callee, args, .. })
                if args.is_empty() =>
            {
                match callee.as_ref() {
                    perry_hir::Expr::FuncRef(id) => chunks.get(id).copied(),
                    _ => None,
                }
            }
            _ => None,
        };
        if let Some(chunk) = chunk {
            logical.extend(chunk.body.iter());
        } else {
            logical.push(stmt);
        }
    }
    logical
}

/// Moved declarations whose storage crosses a generated-function boundary.
///
/// A declaration used only inside its defining chunk remains a cheap local.
/// References from another chunk or an inline entry statement require a rooted
/// module global. Re-declarations split across chunks share storage too.
/// Module-level preallocated boxes are also promoted: the prealloc statement
/// remains in `hir.init`, so a function-local box would otherwise be a
/// different cell from the declaration moved into the chunk.
pub(crate) fn outlined_entry_global_let_ids(hir: &HirModule) -> HashSet<u32> {
    let chunks: Vec<&perry_hir::Function> = hir
        .functions
        .iter()
        .filter(|function| is_entry_chunk(function))
        .collect();
    let mut definer: std::collections::HashMap<u32, u32> = std::collections::HashMap::new();
    let mut globals = HashSet::new();

    // Keep this in lock-step with `module_globals_emit::collect_init_lets`:
    // destructuring declarations can be wrapped in iterator-cleanup `Try`
    // scaffolding while still representing module bindings.
    fn record_definers(
        stmts: &[perry_hir::Stmt],
        function_id: u32,
        definer: &mut std::collections::HashMap<u32, u32>,
        globals: &mut HashSet<u32>,
    ) {
        for stmt in stmts {
            match stmt {
                perry_hir::Stmt::Let { id, .. } => {
                    if definer
                        .insert(*id, function_id)
                        .is_some_and(|prior| prior != function_id)
                    {
                        globals.insert(*id);
                    }
                }
                perry_hir::Stmt::Try {
                    body,
                    catch,
                    finally,
                } => {
                    record_definers(body, function_id, definer, globals);
                    if let Some(catch) = catch {
                        record_definers(&catch.body, function_id, definer, globals);
                    }
                    if let Some(finally) = finally {
                        record_definers(finally, function_id, definer, globals);
                    }
                }
                _ => {}
            }
        }
    }

    for function in &chunks {
        record_definers(&function.body, function.id, &mut definer, &mut globals);
    }

    for function in &chunks {
        let mut refs = HashSet::new();
        collect_ref_ids_in_stmts(&function.body, &mut refs);
        for id in refs {
            if definer
                .get(&id)
                .is_some_and(|defining_function| *defining_function != function.id)
            {
                globals.insert(id);
            }
        }
    }

    let chunk_ids: HashSet<u32> = chunks.iter().map(|function| function.id).collect();
    for stmt in &hir.init {
        match stmt {
            perry_hir::Stmt::PreallocateBoxes(ids) => {
                globals.extend(ids.iter().filter(|id| definer.contains_key(id)).copied());
            }
            perry_hir::Stmt::Expr(perry_hir::Expr::Call { callee, args, .. })
                if args.is_empty()
                    && matches!(callee.as_ref(), perry_hir::Expr::FuncRef(id) if chunk_ids.contains(id)) =>
            {
                // The compiler-owned call itself carries no module-local use.
            }
            _ => {
                let mut refs = HashSet::new();
                collect_ref_ids_in_stmts(std::slice::from_ref(stmt), &mut refs);
                globals.extend(refs.into_iter().filter(|id| definer.contains_key(id)));
            }
        }
    }
    globals
}

/// Chunk the top-level statement list into contiguous ranges of
/// `target`-ish statements. Boundaries fall ONLY between top-level statements,
/// never inside a compound statement, so a top-level `if`/`for`/`try` (and all
/// its control flow) stays wholly within one chunk. Returns the half-open
/// `[start, end)` index ranges.
fn chunk_ranges(total: usize, target: usize) -> Vec<(usize, usize)> {
    if total == 0 {
        return Vec::new();
    }
    let target = target.max(1);
    let mut ranges = Vec::new();
    let mut start = 0;
    while start < total {
        let end = start.saturating_add(target).min(total);
        ranges.push((start, end));
        start = end;
    }
    ranges
}

/// Analyse the entry body of `hir` for outlining, using the env-configured
/// chunk target.
pub(crate) fn analyze_entry_outlining(hir: &HirModule) -> EntryOutlineAnalysis {
    analyze_entry_outlining_with_target(hir, target_chunk_stmts())
}

/// Pure analysis for an explicit chunk target — the testable core (no env).
fn analyze_entry_outlining_with_target(hir: &HirModule, target: usize) -> EntryOutlineAnalysis {
    let stmts = &hir.init;
    let total_stmts = stmts.len();
    let ranges = chunk_ranges(total_stmts, target);
    let chunk_count = count_prospective_chunks(stmts, target);

    // A top-level await splits init across an async suspension. A module-level
    // TDZ preallocation needs checked global loads, which module globals do not
    // provide yet. Both cases stay on the original lowering rather than
    // accepting a semantic approximation.
    let gated_out = if hir.has_top_level_await {
        Some("top-level await")
    } else if stmts
        .iter()
        .any(|stmt| matches!(stmt, perry_hir::Stmt::PreallocateTdzBoxes(_)))
    {
        Some("module-level TDZ preallocation")
    } else {
        None
    };

    // Cross-chunk lets: for each chunk collect the `let`s it DEFINES and the
    // ids it REFERENCES (same collectors `emit_module_globals` uses). A let is
    // cross-chunk if any chunk other than its definer references it. Only
    // counted when there is more than one chunk — with one chunk nothing
    // crosses.
    let cross_chunk_lets = if chunk_count > 1 {
        let mut defs: Vec<HashSet<u32>> = Vec::with_capacity(chunk_count);
        let mut refs: Vec<HashSet<u32>> = Vec::with_capacity(chunk_count);
        for &(start, end) in &ranges {
            let slice = &stmts[start..end];
            let mut d = HashSet::new();
            collect_let_ids(slice, &mut d);
            defs.push(d);
            let mut r = HashSet::new();
            collect_ref_ids_in_stmts(slice, &mut r);
            refs.push(r);
        }
        let mut crossing: HashSet<u32> = HashSet::new();
        for (ci, d) in defs.iter().enumerate() {
            for &id in d {
                let referenced_elsewhere = refs
                    .iter()
                    .enumerate()
                    .any(|(ri, r)| ri != ci && r.contains(&id));
                if referenced_elsewhere {
                    crossing.insert(id);
                }
            }
        }
        crossing.len()
    } else {
        0
    };

    EntryOutlineAnalysis {
        total_stmts,
        chunk_count,
        cross_chunk_lets,
        gated_out,
    }
}

/// Print the analysis when `PERRY_OUTLINE_ENTRY_REPORT` is set. No effect on
/// codegen. Called once per module from `compile_module`.
pub(crate) fn report_entry_outlining(hir: &HirModule) {
    if !report_requested() {
        return;
    }
    let a = analyze_entry_outlining(hir);
    // The transform runs in the HIR pipeline before codegen. Report clearly
    // when these figures describe the compact call stream rather than source
    // top-level statements.
    let transform = if hir.functions.iter().any(is_entry_chunk) {
        " (already outlined; figures describe the chunk-call stream)"
    } else {
        ""
    };
    match a.gated_out {
        Some(reason) => eprintln!(
            "[perry] entry-outline: {}: {} top-level stmts; NOT a candidate ({}){}",
            hir.name, a.total_stmts, reason, transform
        ),
        None => eprintln!(
            "[perry] entry-outline: {}: {} top-level stmts → {} chunk(s) of ~{}, {} cross-chunk let(s) to globalize; candidate={}{}",
            hir.name,
            a.total_stmts,
            a.chunk_count,
            target_chunk_stmts(),
            a.cross_chunk_lets,
            a.is_candidate(),
            transform
        ),
    }
}

/// Outcome of attempting to outline a module entry body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutlineOutcome {
    /// Outlined into `chunks` chunk functions.
    Outlined { chunks: usize },
    /// Left unchanged; `&'static str` says why (fail-safe fallback).
    Skipped(&'static str),
}

/// Largest `FuncId` used anywhere in `hir`. New chunk ids are minted strictly
/// above it so generated functions cannot collide with a class member, nested
/// closure, or an id retained only in module metadata.
fn max_func_id(hir: &HirModule) -> u32 {
    let mut max = 0u32;
    for (_, id) in &hir.script_global_functions {
        max = max.max(*id);
    }
    for (_, id) in &hir.exported_functions {
        max = max.max(*id);
    }
    for id in hir
        .async_step_closures
        .iter()
        .chain(hir.async_generator_funcs.iter())
    {
        max = max.max(*id);
    }
    for id in hir
        .closure_display_names
        .keys()
        .chain(hir.closure_source_text.keys())
        .chain(hir.gen_param_prologue_len.keys())
    {
        max = max.max(*id);
    }

    let collect_max_closure = |stmts: &[perry_hir::Stmt], max: &mut u32| {
        let mut seen = std::collections::HashSet::new();
        let mut out: Vec<(perry_hir::types::FuncId, perry_hir::Expr)> = Vec::new();
        crate::collectors::collect_closures_in_stmts(stmts, &mut seen, &mut out);
        for (id, _) in out {
            *max = (*max).max(id);
        }
    };
    let collect_max_expr = |expr: &perry_hir::Expr, max: &mut u32| {
        let mut seen = std::collections::HashSet::new();
        let mut out: Vec<(perry_hir::types::FuncId, perry_hir::Expr)> = Vec::new();
        crate::collectors::collect_closures_in_expr(expr, &mut seen, &mut out);
        for (id, _) in out {
            *max = (*max).max(id);
        }
    };
    let collect_function = |function: &perry_hir::Function, max: &mut u32| {
        *max = (*max).max(function.id);
        collect_max_closure(&function.body, max);
        for param in &function.params {
            if let Some(default) = &param.default {
                collect_max_expr(default, max);
            }
            for decorator in &param.decorators {
                for arg in &decorator.args {
                    collect_max_expr(arg, max);
                }
            }
        }
        for decorator in &function.decorators {
            for arg in &decorator.args {
                collect_max_expr(arg, max);
            }
        }
    };

    collect_max_closure(&hir.init, &mut max);
    for f in &hir.functions {
        collect_function(f, &mut max);
    }
    for class in &hir.classes {
        if let Some(constructor) = &class.constructor {
            collect_function(constructor, &mut max);
        }
        for function in class
            .methods
            .iter()
            .chain(class.static_methods.iter())
            .chain(class.getters.iter().map(|(_, function)| function))
            .chain(class.setters.iter().map(|(_, function)| function))
            .chain(class.computed_members.iter().map(|member| &member.function))
        {
            collect_function(function, &mut max);
        }
        for member in &class.computed_members {
            collect_max_expr(&member.key_expr, &mut max);
        }
        if let Some(expr) = &class.extends_expr {
            collect_max_expr(expr, &mut max);
        }
        for field in class.fields.iter().chain(class.static_fields.iter()) {
            if let Some(expr) = &field.key_expr {
                collect_max_expr(expr, &mut max);
            }
            if let Some(expr) = &field.init {
                collect_max_expr(expr, &mut max);
            }
            for decorator in &field.decorators {
                for arg in &decorator.args {
                    collect_max_expr(arg, &mut max);
                }
            }
        }
        for decorator in &class.decorators {
            for arg in &decorator.args {
                collect_max_expr(arg, &mut max);
            }
        }
    }
    for global in &hir.globals {
        if let Some(expr) = &global.init {
            collect_max_expr(expr, &mut max);
        }
    }
    max
}

/// A top-level statement the transform can safely relocate into a chunk
/// function without changing which function an abrupt `return` completes.
/// Structured control flow moves as one indivisible statement. A statement
/// containing `return` remains inline; `break`/`continue` stay within the same
/// compound statement and therefore retain their target.
fn classify_top_level(stmt: &perry_hir::Stmt) -> Option<TopLevelKind> {
    use perry_hir::Stmt;
    match stmt {
        Stmt::Let { .. } | Stmt::Expr(_) | Stmt::Throw(_) => Some(TopLevelKind::Relocatable),
        Stmt::If { .. }
        | Stmt::While { .. }
        | Stmt::DoWhile { .. }
        | Stmt::For { .. }
        | Stmt::Labeled { .. }
        | Stmt::Try { .. }
        | Stmt::Switch { .. }
            if !stmt_contains_return(stmt) =>
        {
            Some(TopLevelKind::Relocatable)
        }
        _ => None,
    }
}

enum TopLevelKind {
    Relocatable,
}

fn stmt_contains_return(stmt: &perry_hir::Stmt) -> bool {
    use perry_hir::Stmt;
    match stmt {
        Stmt::Return(_) => true,
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            then_branch.iter().any(stmt_contains_return)
                || else_branch
                    .as_ref()
                    .is_some_and(|body| body.iter().any(stmt_contains_return))
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
            body.iter().any(stmt_contains_return)
        }
        Stmt::For { init, body, .. } => {
            init.as_deref().is_some_and(stmt_contains_return)
                || body.iter().any(stmt_contains_return)
        }
        Stmt::Labeled { body, .. } => stmt_contains_return(body),
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            body.iter().any(stmt_contains_return)
                || catch
                    .as_ref()
                    .is_some_and(|clause| clause.body.iter().any(stmt_contains_return))
                || finally
                    .as_ref()
                    .is_some_and(|body| body.iter().any(stmt_contains_return))
        }
        Stmt::Switch { cases, .. } => cases
            .iter()
            .any(|case| case.body.iter().any(stmt_contains_return)),
        // A return inside an expression-owned closure completes that closure,
        // not module init, so expression walkers are intentionally not used.
        _ => false,
    }
}

/// How many chunk functions the interleaving would emit for `stmts` at
/// `target` — a run of relocatable statements becomes ceil(run/target) chunks,
/// and a must-stay statement (an unclassifiable shape) ends the current run.
/// Used as a pre-scan so eligibility is decided before any mutation.
fn count_prospective_chunks(stmts: &[perry_hir::Stmt], target: usize) -> usize {
    let mut chunks = 0usize;
    let mut run = 0usize;
    let mut run_safepoints = 0usize;
    let flush = |run: &mut usize, run_safepoints: &mut usize, chunks: &mut usize| {
        if *run > 0 {
            *chunks += 1;
            *run = 0;
            *run_safepoints = 0;
        }
    };
    for stmt in stmts {
        match classify_top_level(stmt) {
            Some(TopLevelKind::Relocatable) => {
                run += 1;
                run_safepoints = run_safepoints.saturating_add(
                    crate::collectors::count_safepoint_sites(std::slice::from_ref(stmt)),
                );
                if run >= target.max(1) || run_safepoints >= DEFAULT_CHUNK_SAFEPOINTS {
                    flush(&mut run, &mut run_safepoints, &mut chunks);
                }
            }
            None => flush(&mut run, &mut run_safepoints, &mut chunks),
        }
    }
    flush(&mut run, &mut run_safepoints, &mut chunks);
    chunks
}

/// Attempt to outline `hir`'s entry body (#8595). Fail-safe: returns
/// `Skipped(reason)` and leaves `hir` untouched unless the whole body is
/// provably safe to relocate; callers proceed with the ordinary single-function
/// entry lowering in that case.
pub fn outline_entry_module(hir: &mut HirModule) -> OutlineOutcome {
    let mode = outline_mode();
    if mode == OutlineMode::Disabled {
        return OutlineOutcome::Skipped("PERRY_OUTLINE_ENTRY disabled");
    }
    let safepoints = crate::collectors::count_safepoint_sites(&hir.init);
    if mode == OutlineMode::Auto && !meets_automatic_size_threshold(hir.init.len(), safepoints) {
        return OutlineOutcome::Skipped("below automatic outlining threshold");
    }
    outline_entry_module_with_target(hir, target_chunk_stmts())
}

/// Env-free core of [`outline_entry_module`] — the testable seam.
fn outline_entry_module_with_target(hir: &mut HirModule, target: usize) -> OutlineOutcome {
    let analysis = analyze_entry_outlining_with_target(hir, target);
    if let Some(reason) = analysis.gated_out {
        return OutlineOutcome::Skipped(reason);
    }
    if !analysis.is_candidate() {
        return OutlineOutcome::Skipped("not a candidate (too small)");
    }
    // Pre-scan: decide eligibility before mutating. Outlining is worthwhile
    // only if the interleaving would emit more than one chunk.
    let prospective_chunks = count_prospective_chunks(&hir.init, target);
    if prospective_chunks <= 1 {
        return OutlineOutcome::Skipped("would not split into multiple chunks");
    }

    let max_id = max_func_id(hir);
    if prospective_chunks > (u32::MAX - max_id) as usize {
        return OutlineOutcome::Skipped("function id space exhausted");
    }
    let mut next_id = max_id + 1;
    let module_name = hir.name.clone();
    // #9423: a chunk is module top-level code that merely moved into a
    // function, so it carries the module's strictness. Read before `hir.init`
    // is taken, for the same reason `module_name` is.
    let module_is_strict = hir.init_is_strict;
    let original = std::mem::take(&mut hir.init);

    // The rewritten body: chunk calls interleaved with any statement that had
    // to stay inline, in original execution order.
    let mut new_body: Vec<perry_hir::Stmt> = Vec::new();
    let mut chunk_fns: Vec<perry_hir::Function> = Vec::new();
    // The current run of relocatable statements accumulating into a chunk.
    let mut run: Vec<perry_hir::Stmt> = Vec::new();
    let mut run_safepoints = 0usize;

    // Emit the accumulated run as a chunk function and append its call, unless
    // empty. `flush` is a closure over the mutable state via explicit params to
    // keep the borrow checker happy.
    fn flush(
        run: &mut Vec<perry_hir::Stmt>,
        chunk_fns: &mut Vec<perry_hir::Function>,
        new_body: &mut Vec<perry_hir::Stmt>,
        next_id: &mut u32,
        module_name: &str,
        module_is_strict: bool,
    ) {
        if run.is_empty() {
            return;
        }
        let fn_id = *next_id;
        *next_id = (*next_id).saturating_add(1);
        let ci = chunk_fns.len();
        chunk_fns.push(perry_hir::Function {
            id: fn_id,
            name: format!("{ENTRY_CHUNK_PREFIX}{module_name}_{ci}"),
            type_params: Vec::new(),
            params: Vec::new(),
            return_type: perry_hir::types::Type::Void,
            body: std::mem::take(run),
            is_async: false,
            is_generator: false,
            // #9423: match the entry lowering, which now carries the module's
            // real strictness. A chunk holds statements that were module
            // top-level code a moment ago; relocating them into a function must
            // not relax the mode they execute in.
            is_strict: module_is_strict,
            is_exported: false,
            captures: Vec::new(),
            decorators: Vec::new(),
            was_plain_async: false,
            was_unrolled: false,
        });
        new_body.push(perry_hir::Stmt::Expr(perry_hir::Expr::Call {
            callee: Box::new(perry_hir::Expr::FuncRef(fn_id)),
            args: Vec::new(),
            type_args: Vec::new(),
            byte_offset: 0,
        }));
    }

    for stmt in original {
        match classify_top_level(&stmt) {
            Some(TopLevelKind::Relocatable) => run.push(stmt),
            None => {
                // A statement we cannot safely relocate (control flow, etc.):
                // end the current chunk run and keep this statement inline, at
                // its original position, so eval order and any `hir.init` scan
                // that reads it are preserved.
                flush(
                    &mut run,
                    &mut chunk_fns,
                    &mut new_body,
                    &mut next_id,
                    &module_name,
                    module_is_strict,
                );
                run_safepoints = 0;
                new_body.push(stmt);
            }
        }
        if let Some(last) = run.last() {
            run_safepoints = run_safepoints.saturating_add(
                crate::collectors::count_safepoint_sites(std::slice::from_ref(last)),
            );
        }
        if run.len() >= target.max(1) || run_safepoints >= DEFAULT_CHUNK_SAFEPOINTS {
            flush(
                &mut run,
                &mut chunk_fns,
                &mut new_body,
                &mut next_id,
                &module_name,
                module_is_strict,
            );
            run_safepoints = 0;
        }
    }
    flush(
        &mut run,
        &mut chunk_fns,
        &mut new_body,
        &mut next_id,
        &module_name,
        module_is_strict,
    );

    let chunks = chunk_fns.len();
    hir.functions.extend(chunk_fns);
    hir.init = new_body;
    OutlineOutcome::Outlined { chunks }
}

#[cfg(test)]
mod tests {
    use super::*;
    use perry_hir::types::Type;
    use perry_hir::{Expr, Module, Stmt};

    fn let_stmt(id: u32, name: &str, init: Expr) -> Stmt {
        Stmt::Let {
            id,
            name: name.to_string(),
            ty: Type::Any,
            mutable: false,
            init: Some(init),
        }
    }

    fn empty_closure_with_id(func_id: u32, body: Vec<Stmt>) -> Expr {
        Expr::Closure {
            func_id,
            params: vec![],
            return_type: Type::Any,
            body,
            captures: vec![],
            mutable_captures: vec![],
            captures_this: false,
            captures_new_target: false,
            enclosing_class: None,
            is_arrow: false,
            is_async: false,
            is_generator: false,
            is_strict: false,
        }
    }

    fn module_with_init(init: Vec<Stmt>) -> Module {
        let mut m = Module::new("test_mod");
        m.init = init;
        m
    }

    #[test]
    fn chunk_ranges_split_contiguously() {
        assert_eq!(chunk_ranges(0, 3), Vec::<(usize, usize)>::new());
        assert_eq!(chunk_ranges(3, 3), vec![(0, 3)]);
        assert_eq!(chunk_ranges(7, 3), vec![(0, 3), (3, 6), (6, 7)]);
        assert_eq!(chunk_ranges(2, usize::MAX), vec![(0, 2)]);
    }

    #[test]
    fn safepoint_budget_can_split_before_the_statement_target() {
        let allocation_heavy_stmt = || {
            Stmt::Expr(Expr::Array(
                (0..DEFAULT_CHUNK_SAFEPOINTS)
                    .map(|_| Expr::Array(vec![]))
                    .collect(),
            ))
        };
        let mut m = module_with_init(vec![allocation_heavy_stmt(), allocation_heavy_stmt()]);
        assert_eq!(
            count_prospective_chunks(&m.init, usize::MAX),
            2,
            "each allocation-heavy statement should exhaust a chunk budget"
        );
        assert_eq!(
            outline_entry_module_with_target(&mut m, usize::MAX),
            OutlineOutcome::Outlined { chunks: 2 }
        );
    }

    #[test]
    fn environment_mode_defaults_to_auto_and_has_explicit_overrides() {
        assert_eq!(outline_mode_from_env(None), OutlineMode::Auto);
        assert_eq!(outline_mode_from_env(Some("unexpected")), OutlineMode::Auto);
        assert_eq!(outline_mode_from_env(Some("1")), OutlineMode::Forced);
        assert_eq!(outline_mode_from_env(Some("on")), OutlineMode::Forced);
        assert_eq!(outline_mode_from_env(Some("0")), OutlineMode::Disabled);
        assert_eq!(outline_mode_from_env(Some("false")), OutlineMode::Disabled);
        assert!(!meets_automatic_size_threshold(
            DEFAULT_AUTO_MIN_STMTS - 1,
            DEFAULT_AUTO_MIN_SAFEPOINTS - 1
        ));
        assert!(meets_automatic_size_threshold(DEFAULT_AUTO_MIN_STMTS, 0));
        assert!(meets_automatic_size_threshold(
            1,
            DEFAULT_AUTO_MIN_SAFEPOINTS
        ));
    }

    #[test]
    fn small_body_is_a_single_chunk_and_not_a_candidate() {
        let m = module_with_init(vec![
            Stmt::Expr(Expr::Number(1.0)),
            Stmt::Expr(Expr::Number(2.0)),
        ]);
        let a = analyze_entry_outlining_with_target(&m, 200);
        assert_eq!(a.chunk_count, 1);
        assert_eq!(a.cross_chunk_lets, 0);
        assert!(!a.is_candidate());
    }

    #[test]
    fn cross_chunk_let_is_counted() {
        // chunk size 1: `let x = 1` in chunk 0, `x` read in chunk 1 -> crosses.
        let m = module_with_init(vec![
            let_stmt(0, "x", Expr::Number(1.0)),
            Stmt::Expr(Expr::LocalGet(0)),
        ]);
        let a = analyze_entry_outlining_with_target(&m, 1);
        assert_eq!(a.chunk_count, 2);
        assert_eq!(
            a.cross_chunk_lets, 1,
            "x is defined in chunk 0 and read in chunk 1"
        );
        assert!(a.is_candidate());
    }

    #[test]
    fn a_let_used_only_within_its_own_chunk_does_not_cross() {
        // Two lets, chunk size 2: both defined+used inside their chunk -> none cross.
        let m = module_with_init(vec![
            let_stmt(0, "x", Expr::Number(1.0)),
            Stmt::Expr(Expr::LocalGet(0)),
            let_stmt(1, "y", Expr::Number(2.0)),
            Stmt::Expr(Expr::LocalGet(1)),
        ]);
        let a = analyze_entry_outlining_with_target(&m, 2);
        assert_eq!(a.chunk_count, 2);
        assert_eq!(
            a.cross_chunk_lets, 0,
            "x and y are each confined to their own chunk"
        );
    }

    #[test]
    fn top_level_await_gates_the_body_out() {
        let mut m = module_with_init(vec![
            let_stmt(0, "x", Expr::Number(1.0)),
            Stmt::Expr(Expr::LocalGet(0)),
        ]);
        m.has_top_level_await = true;
        let a = analyze_entry_outlining_with_target(&m, 1);
        assert_eq!(a.gated_out, Some("top-level await"));
        assert!(!a.is_candidate(), "a gated-out body is never a candidate");
    }

    #[test]
    fn module_level_tdz_preallocation_gates_the_body_out() {
        let m = module_with_init(vec![
            Stmt::PreallocateTdzBoxes(vec![0]),
            Stmt::Expr(Expr::LocalGet(0)),
            let_stmt(0, "x", Expr::Number(1.0)),
        ]);
        let a = analyze_entry_outlining_with_target(&m, 1);
        assert_eq!(a.gated_out, Some("module-level TDZ preallocation"));
        assert!(!a.is_candidate());
    }

    #[test]
    fn transform_preserves_declarations_and_emits_ordered_chunk_calls() {
        // let x = 1 (chunk 0); read x + let y = 2 (chunk 1); read y (chunk 2)
        let mut m = module_with_init(vec![
            let_stmt(0, "x", Expr::Number(1.0)),
            Stmt::Expr(Expr::LocalGet(0)),
            let_stmt(1, "y", Expr::Number(2.0)),
            Stmt::Expr(Expr::LocalGet(1)),
        ]);
        let before_fns = m.functions.len();
        let outcome = outline_entry_module_with_target(&mut m, 2);
        assert_eq!(outcome, OutlineOutcome::Outlined { chunks: 2 });
        // two chunk functions added
        assert_eq!(m.functions.len(), before_fns + 2);
        // The physical init is just the two ordered calls. The logical view
        // reconstructs the unchanged declaration statements for codegen scans.
        assert_eq!(m.init.len(), 2);
        let calls: Vec<u32> = m
            .init
            .iter()
            .filter_map(|s| match s {
                Stmt::Expr(Expr::Call { callee, .. }) => match callee.as_ref() {
                    Expr::FuncRef(id) => Some(*id),
                    _ => None,
                },
                _ => None,
            })
            .collect();
        assert_eq!(calls.len(), 2, "two ordered chunk calls");
        assert_eq!(
            calls[0], m.functions[before_fns].id,
            "call 0 targets chunk 0"
        );
        assert_eq!(
            calls[1],
            m.functions[before_fns + 1].id,
            "call 1 targets chunk 1"
        );
        // Chunk 0 holds the original immutable declaration and initializer;
        // it was not degraded into a mutable LocalSet assignment.
        let chunk0 = &m.functions[before_fns].body;
        assert!(chunk0.iter().any(|s| matches!(
            s,
            Stmt::Let {
                id: 0,
                init: Some(Expr::Number(1.0)),
                mutable: false,
                ..
            }
        )));
        let logical = logical_entry_stmts(&m);
        assert_eq!(logical.len(), 4);
        assert!(matches!(logical[0], Stmt::Let { id: 0, .. }));
        assert!(matches!(logical[2], Stmt::Let { id: 1, .. }));
        assert!(
            outlined_entry_global_let_ids(&m).is_empty(),
            "bindings confined to one chunk stay function-local"
        );
    }

    #[test]
    fn only_boundary_crossing_or_preallocated_bindings_become_globals() {
        let mut crossing = module_with_init(vec![
            let_stmt(10, "shared", Expr::Number(1.0)),
            Stmt::Expr(Expr::Number(0.0)),
            Stmt::Expr(Expr::LocalGet(10)),
        ]);
        assert_eq!(
            outline_entry_module_with_target(&mut crossing, 1),
            OutlineOutcome::Outlined { chunks: 3 }
        );
        assert_eq!(
            outlined_entry_global_let_ids(&crossing),
            HashSet::from([10])
        );

        let mut preallocated = module_with_init(vec![
            Stmt::PreallocateBoxes(vec![20]),
            Stmt::Try {
                body: vec![let_stmt(20, "captured", Expr::Number(2.0))],
                catch: None,
                finally: None,
            },
            Stmt::Expr(Expr::Number(0.0)),
        ]);
        assert_eq!(
            outline_entry_module_with_target(&mut preallocated, 1),
            OutlineOutcome::Outlined { chunks: 2 }
        );
        assert_eq!(
            outlined_entry_global_let_ids(&preallocated),
            HashSet::from([20])
        );
    }

    #[test]
    fn minted_chunk_ids_clear_existing_function_and_closure_ids() {
        let mut m = module_with_init(vec![
            let_stmt(0, "x", Expr::Number(1.0)),
            Stmt::Expr(Expr::LocalGet(0)),
        ]);
        // an existing function with a high id, and a closure with an even higher id
        m.functions.push(perry_hir::Function {
            id: 500,
            name: "f".into(),
            type_params: vec![],
            params: vec![],
            return_type: Type::Void,
            body: vec![Stmt::Expr(empty_closure_with_id(9000, vec![]))],
            is_async: false,
            is_generator: false,
            is_strict: true,
            is_exported: false,
            captures: vec![],
            decorators: vec![],
            was_plain_async: false,
            was_unrolled: false,
        });
        m.classes.push(perry_hir::Class {
            id: 1,
            name: "C".into(),
            type_params: vec![],
            extends: None,
            extends_name: None,
            native_extends: None,
            extends_expr: None,
            heritage_lexically_shadowed: false,
            fields: vec![],
            constructor: None,
            methods: vec![perry_hir::Function {
                id: 12_000,
                name: "method".into(),
                type_params: vec![],
                params: vec![],
                return_type: Type::Void,
                body: vec![],
                is_async: false,
                is_generator: false,
                is_strict: true,
                is_exported: false,
                captures: vec![],
                decorators: vec![],
                was_plain_async: false,
                was_unrolled: false,
            }],
            getters: vec![],
            setters: vec![],
            static_accessor_names: vec![],
            static_accessor_fn_ids: vec![],
            static_fields: vec![],
            static_methods: vec![],
            computed_members: vec![],
            decorators: vec![],
            is_exported: false,
            aliases: vec![],
            is_nested: false,
            alloc_width_hint: 0,
            specialized_from: None,
        });
        let base = m.functions.len();
        let outcome = outline_entry_module_with_target(&mut m, 1);
        assert!(matches!(outcome, OutlineOutcome::Outlined { .. }));
        for f in &m.functions[base..] {
            assert!(
                f.id > 12_000,
                "chunk id {} must clear closure and class-member ids",
                f.id
            );
        }
    }

    #[test]
    fn transform_interleaves_chunks_around_a_must_stay_statement() {
        // A top-level return cannot move into a helper because it completes
        // module init. The transform outlines runs on either side and keeps the
        // return inline, in order. target=1 maximizes chunking.
        let mut m = module_with_init(vec![
            let_stmt(0, "x", Expr::Number(1.0)), // chunk
            Stmt::Return(None),                  // must-stay, inline
            let_stmt(1, "y", Expr::Number(2.0)), // chunk
            Stmt::Expr(Expr::LocalGet(1)),       // chunk
        ]);
        let fns_before = m.functions.len();
        let outcome = outline_entry_module_with_target(&mut m, 1);
        assert!(
            matches!(outcome, OutlineOutcome::Outlined { .. }),
            "runs around the if are outlined, not bailed: {outcome:?}"
        );
        let return_pos = m
            .init
            .iter()
            .position(|s| matches!(s, Stmt::Return(_)))
            .expect("the top-level return is kept inline");
        let call_positions: Vec<usize> = m
            .init
            .iter()
            .enumerate()
            .filter_map(|(i, s)| match s {
                Stmt::Expr(Expr::Call { callee, .. })
                    if matches!(callee.as_ref(), Expr::FuncRef(_)) =>
                {
                    Some(i)
                }
                _ => None,
            })
            .collect();
        assert!(
            call_positions.iter().any(|&i| i < return_pos),
            "a chunk call precedes the return (the `x` run)"
        );
        assert!(
            call_positions.iter().any(|&i| i > return_pos),
            "a chunk call follows the return (the `y` run)"
        );
        assert!(
            m.functions.len() > fns_before + 1,
            "more than one chunk function emitted"
        );
    }

    #[test]
    fn structured_control_flow_moves_as_one_indivisible_statement() {
        let structured = Stmt::If {
            condition: Expr::Bool(true),
            then_branch: vec![Stmt::Expr(Expr::Number(1.0))],
            else_branch: None,
        };
        let mut m = module_with_init(vec![structured, Stmt::Expr(Expr::Number(2.0))]);
        let outcome = outline_entry_module_with_target(&mut m, 1);
        assert_eq!(outcome, OutlineOutcome::Outlined { chunks: 2 });
        assert!(matches!(m.functions[0].body.as_slice(), [Stmt::If { .. }]));
    }

    #[test]
    fn exported_modules_are_eligible() {
        let mut m = module_with_init(vec![
            let_stmt(0, "x", Expr::Number(1.0)),
            Stmt::Expr(Expr::LocalGet(0)),
        ]);
        m.exported_functions.push(("g".into(), 42));
        let outcome = outline_entry_module_with_target(&mut m, 1);
        assert_eq!(outcome, OutlineOutcome::Outlined { chunks: 2 });
    }
}
