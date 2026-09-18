//! Structured module-entry outlining (#8595, extended by #10575).
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
//!
//! ## #10575: CommonJS module bodies live outside `hir.init`
//!
//! For a CommonJS source file, `cjs_wrap::wrap_commonjs_for_target` wraps the
//! whole body as *text* — `const _cjs = (function() { function
//! __perry_cjs_factory() { <the actual module body> } return
//! __perry_cjs_factory(); })();` — before the normal parse/lower pipeline
//! ever sees it. Ordinary lowering represents the named, lexically-nested
//! `function __perry_cjs_factory() {...}` declaration the same way it would
//! an arrow/function EXPRESSION (it is not a top-level declaration, so it is
//! not hoisted into `hir.functions`): a `Stmt::Let` inside the wrapper's
//! outer IIFE, naming an `Expr::Closure`. `hir.init` itself holds only the
//! handful of statements the wrapper adds at module scope (the `_cjs`
//! binding, `export default`, …). Admission above therefore never fires for
//! CJS: the tens of thousands of real statements are not top-level HIR
//! statements anywhere, let alone in `hir.init`.
//!
//! [`outline_cjs_factory_module`] closes that gap: [`find_cjs_factory_closure_mut`]
//! walks `hir.init`'s statement/expression tree to locate that nested
//! closure, and the identical chunking transform
//! ([`chunk_statements`]/[`analyze_stmts_outlining`], same admission
//! thresholds, same fail-safe gates, same `__perry_entry_chunk_*` naming)
//! runs against ITS body instead of `hir.init`'s. New chunks are still
//! ordinary `hir.functions` entries, so `is_entry_chunk` and
//! `emit_module_globals`'s existing "referenced from a separate function
//! body" escape analysis pick them up for free. The one addition is
//! [`logical_outlined_function_stmts`]/the generalised
//! [`outlined_entry_global_let_ids`] residual scan, which teach the global-
//! promotion pass to also look for cross-chunk `var`s inside an outlined
//! factory body, not just inside `hir.init`. A module is only ever outlined
//! from ONE origin per compile (`hir.init` OR the factory, never both), so
//! there is no cross-origin interaction to reason about.
//!
//! The factory is only treated as a virtual module entry when it has the
//! exact wrap-generated shape: no params, not async/generator, and every id
//! it captures from its enclosing scope (the wrapper's outer IIFE) resolves
//! to a `Let` directly in that scope — see [`is_cjs_factory_shape`]. In
//! practice the factory always captures exactly one such id, its own name:
//! the wrapper's preamble does `__cjs_module.__perry_cjs_factory =
//! __perry_cjs_factory;`, a load-bearing self-reference `perry-runtime`'s
//! `module_require.rs` calls through on a circular-require recovery path.
//! Since a chunk is a plain, non-capturing `hir.functions` entry, it cannot
//! read a captured id the way the original (unsplit) closure could — so
//! [`classify_for_chunking`] keeps every statement that references one of
//! the factory's captured ids inline in the residual body (never relocated
//! into a chunk), preserving the exact closure-capture read codegen already
//! provides. This is deliberately NOT solved by promoting the captured id to
//! a module global the way an ordinary cross-chunk `hir.init` let is: a
//! global is one program-wide instance, but a captured id is fresh per
//! closure invocation — promoting it would silently break the recovery path
//! above if it ever re-invokes the factory closure. `wrap_commonjs_for_target`
//! never produces a factory outside this shape, so failing the shape check
//! is a defensive exit, not an expected one.

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

/// Every entry-chunk function in `hir.functions`, indexed by id. Chunks are
/// origin-agnostic — this map does not distinguish a chunk split from
/// `hir.init` from one split from an outlined function body (#10575).
fn chunk_map(hir: &HirModule) -> std::collections::HashMap<u32, &perry_hir::Function> {
    hir.functions
        .iter()
        .filter(|function| is_entry_chunk(function))
        .map(|function| (function.id, function))
        .collect()
}

/// Is `stmt` a bare, no-argument call to one of `chunks`? The shape
/// [`chunk_statements`] emits for every chunk call site.
fn as_chunk_call<'a>(
    stmt: &perry_hir::Stmt,
    chunks: &std::collections::HashMap<u32, &'a perry_hir::Function>,
) -> Option<&'a perry_hir::Function> {
    match stmt {
        perry_hir::Stmt::Expr(perry_hir::Expr::Call { callee, args, .. }) if args.is_empty() => {
            match callee.as_ref() {
                perry_hir::Expr::FuncRef(id) => chunks.get(id).copied(),
                _ => None,
            }
        }
        _ => None,
    }
}

/// Reconstruct the source-order statement stream of `stmts` after outlining:
/// every chunk-call site is replaced by that chunk's body, in place.
fn logical_stmts_of<'a>(
    stmts: &'a [perry_hir::Stmt],
    chunks: &std::collections::HashMap<u32, &'a perry_hir::Function>,
) -> Vec<&'a perry_hir::Stmt> {
    let mut logical = Vec::new();
    for stmt in stmts {
        if let Some(chunk) = as_chunk_call(stmt, chunks) {
            logical.extend(chunk.body.iter());
        } else {
            logical.push(stmt);
        }
    }
    logical
}

/// Reconstruct the source-order module-entry statement stream after outlining.
///
/// Several codegen analyses intentionally inspect module declarations rather
/// than ordinary function bodies (exported closure signatures, const folding,
/// static-field deduplication, and early `process.env` assignments). Replacing
/// a range with a chunk call must not hide those original statements from the
/// analyses. Non-chunk calls and all inline statements are returned unchanged.
///
/// This only ever looks at `hir.init` — the module's own top level. A
/// CommonJS factory body outlined by [`outline_cjs_factory_module`] is a
/// different logical scope (see [`logical_outlined_function_stmts`]) and is
/// intentionally NOT included here: callers of this function want "the
/// module's own top-level declarations", which for a CJS-wrapped unit
/// genuinely is just the wrapper's handful of statements.
pub fn logical_entry_stmts(hir: &HirModule) -> Vec<&perry_hir::Stmt> {
    logical_stmts_of(&hir.init, &chunk_map(hir))
}

/// Literal name `cjs_wrap::wrap_commonjs_for_target` gives the CJS module
/// factory closure it generates (`crates/perry/src/commands/compile/cjs_wrap/
/// wrap.rs`: `function __perry_cjs_factory() { ... }`).
const CJS_FACTORY_NAME: &str = "__perry_cjs_factory";

/// If `expr` is itself an `Expr::Closure`, or a same-expression invocation of
/// one (`Expr::Call { callee: Box<Expr::Closure>, .. }` — the
/// `(function(){...})()` IIFE shape `wrap_commonjs_for_target` always uses to
/// wrap a CJS body), return that closure. `None` for anything else — this is
/// intentionally narrow rather than a fully general expression search; the
/// wrapper's shape is fixed and known.
fn as_inline_closure(expr: &perry_hir::Expr) -> Option<&perry_hir::Expr> {
    match expr {
        perry_hir::Expr::Closure { .. } => Some(expr),
        perry_hir::Expr::Call { callee, .. } => as_inline_closure(callee.as_ref()),
        _ => None,
    }
}

/// Mutable twin of [`as_inline_closure`].
fn as_inline_closure_mut(expr: &mut perry_hir::Expr) -> Option<&mut perry_hir::Expr> {
    match expr {
        perry_hir::Expr::Closure { .. } => Some(expr),
        perry_hir::Expr::Call { callee, .. } => as_inline_closure_mut(callee.as_mut()),
        _ => None,
    }
}

/// Is `closure` the exact shape `wrap_commonjs_for_target` generates for
/// `__perry_cjs_factory`: no params, an ordinary (non-async, non-generator)
/// function? `outer_body` is its enclosing scope (the wrapper's outer IIFE
/// body) — every id `closure` captures must be defined by a `Stmt::Let`
/// directly in `outer_body`, so [`outline_cjs_factory_module`] can always
/// find where a captured id comes from. (In practice `outer_body` has
/// exactly one such `Let` — the factory's own name — because the wrapper's
/// preamble does `__cjs_module.__perry_cjs_factory = __perry_cjs_factory;`,
/// a load-bearing self-reference `perry-runtime`'s `module_require.rs` calls
/// through for a circular-require recovery path; that is why the factory is
/// NOT required to capture nothing, unlike an ordinary outlining candidate.)
/// `wrap_commonjs_for_target` never produces anything outside this shape, so
/// failing the check is a defensive exit, not an expected one.
fn is_cjs_factory_shape(
    params: &[perry_hir::Param],
    captures: &[u32],
    is_async: bool,
    is_generator: bool,
    outer_body_let_ids: &HashSet<u32>,
) -> bool {
    if !params.is_empty() || is_async || is_generator {
        return false;
    }
    captures.iter().all(|id| outer_body_let_ids.contains(id))
}

/// The ids directly `Stmt::Let`-defined in `body` (one level, not recursive
/// — exactly what a closure's own `captures` list can name from this scope).
fn top_level_let_ids(body: &[perry_hir::Stmt]) -> HashSet<u32> {
    body.iter()
        .filter_map(|stmt| match stmt {
            perry_hir::Stmt::Let { id, .. } => Some(*id),
            _ => None,
        })
        .collect()
}

/// Find the `__perry_cjs_factory` closure inside `init` (`hir.init`), if
/// `init` has the exact shape `wrap_commonjs_for_target` generates: a
/// top-level statement whose value is an immediately-invoked closure (the
/// wrapper's outer anonymous IIFE), one of whose OWN direct statements is a
/// `let __perry_cjs_factory = function() { ... }`-shaped binding (JS lowers
/// the wrapper's named `function __perry_cjs_factory() {...}` declaration to
/// exactly this: a `Stmt::Let` naming an `Expr::Closure`, not a top-level
/// `hir.functions` entry, because it is lexically nested inside the IIFE).
fn find_cjs_factory_closure(init: &[perry_hir::Stmt]) -> Option<&perry_hir::Expr> {
    for stmt in init {
        let outer_init = match stmt {
            perry_hir::Stmt::Let {
                init: Some(init), ..
            } => init,
            perry_hir::Stmt::Expr(expr) => expr,
            _ => continue,
        };
        let Some(perry_hir::Expr::Closure {
            body: outer_body, ..
        }) = as_inline_closure(outer_init)
        else {
            continue;
        };
        let outer_body_let_ids = top_level_let_ids(outer_body);
        for inner in outer_body {
            let perry_hir::Stmt::Let {
                name,
                init: Some(init),
                ..
            } = inner
            else {
                continue;
            };
            if name != CJS_FACTORY_NAME {
                continue;
            }
            if let perry_hir::Expr::Closure {
                params,
                captures,
                is_async,
                is_generator,
                ..
            } = init
            {
                if is_cjs_factory_shape(
                    params,
                    captures,
                    *is_async,
                    *is_generator,
                    &outer_body_let_ids,
                ) {
                    return Some(init);
                }
            }
        }
    }
    None
}

/// Mutable twin of [`find_cjs_factory_closure`]. Takes `&mut hir.init`
/// specifically (not `&mut HirModule`) so callers can hold this borrow while
/// independently borrowing `hir.functions` to append new chunk functions —
/// a function taking the whole module would make the borrow checker treat
/// every field as borrowed for as long as the returned reference lives.
fn find_cjs_factory_closure_mut(init: &mut [perry_hir::Stmt]) -> Option<&mut perry_hir::Expr> {
    for stmt in init.iter_mut() {
        let outer_init = match stmt {
            perry_hir::Stmt::Let {
                init: Some(init), ..
            } => init,
            perry_hir::Stmt::Expr(expr) => expr,
            _ => continue,
        };
        let Some(perry_hir::Expr::Closure {
            body: outer_body, ..
        }) = as_inline_closure_mut(outer_init)
        else {
            continue;
        };
        // Computed once as an OWNED set (not borrowed from `outer_body`) so
        // the shape check below doesn't alias the `iter_mut()` that follows
        // it — `outer_body`'s own `Let` ids don't change during this scan.
        let outer_body_let_ids = top_level_let_ids(outer_body);
        for inner in outer_body.iter_mut() {
            let perry_hir::Stmt::Let {
                name,
                init: Some(init),
                ..
            } = inner
            else {
                continue;
            };
            if name != CJS_FACTORY_NAME {
                continue;
            }
            let shape_ok = matches!(
                init,
                perry_hir::Expr::Closure { params, captures, is_async, is_generator, .. }
                    if is_cjs_factory_shape(params, captures, *is_async, *is_generator, &outer_body_let_ids)
            );
            if shape_ok {
                return Some(init);
            }
        }
    }
    None
}

/// The CJS factory's residual body, if it was itself outlined this compile
/// (#10575) — i.e. it now contains at least one call to an entry chunk.
/// `None` for every module that isn't a large CJS bundle, and always `None`
/// when `hir.init` itself was the one outlined (a module is only ever
/// outlined from one origin).
fn outlined_factory_residual_body(hir: &HirModule) -> Option<&[perry_hir::Stmt]> {
    let perry_hir::Expr::Closure { body, .. } = find_cjs_factory_closure(&hir.init)? else {
        return None;
    };
    let chunks = chunk_map(hir);
    if body
        .iter()
        .any(|stmt| as_chunk_call(stmt, &chunks).is_some())
    {
        Some(body)
    } else {
        None
    }
}

/// Like [`logical_entry_stmts`], but for the outlined CJS factory body
/// instead of `hir.init` (#10575). Empty unless [`outline_cjs_factory_module`]
/// actually outlined something this compile.
pub(crate) fn logical_outlined_function_stmts(hir: &HirModule) -> Vec<&perry_hir::Stmt> {
    let chunks = chunk_map(hir);
    match outlined_factory_residual_body(hir) {
        Some(body) => logical_stmts_of(body, &chunks),
        None => Vec::new(),
    }
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
    // Residual bodies to scan for must-stay statements that reference a
    // chunk-defined let: `hir.init` (always) plus any function body that was
    // ITSELF outlined by `outline_cjs_factory_module` (#10575) — today at
    // most the CJS factory, never both origins in the same compile.
    let residual_bodies: Vec<&[perry_hir::Stmt]> = std::iter::once(hir.init.as_slice())
        .chain(outlined_factory_residual_body(hir))
        .collect();
    for body in residual_bodies {
        for stmt in body {
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
    analyze_stmts_outlining(&hir.init, hir.has_top_level_await, target, &HashSet::new())
}

/// Pure statement-list analysis shared by `hir.init` and (#10575) an outlined
/// function body such as the CJS factory — no `HirModule` needed beyond the
/// statements themselves and whether the surrounding scope can suspend across
/// a top-level `await` (only ever true for `hir.init`; a plain function body
/// passes `false`). `must_stay_ids` is the CJS-factory closure's own captured
/// ids (empty for `hir.init`) — see [`classify_for_chunking`].
fn analyze_stmts_outlining(
    stmts: &[perry_hir::Stmt],
    has_top_level_await: bool,
    target: usize,
    must_stay_ids: &HashSet<u32>,
) -> EntryOutlineAnalysis {
    let total_stmts = stmts.len();
    let ranges = chunk_ranges(total_stmts, target);
    let chunk_count = count_prospective_chunks(stmts, target, must_stay_ids);

    // A top-level await splits init across an async suspension. A module-level
    // TDZ preallocation needs checked global loads, which module globals do not
    // provide yet. Both cases stay on the original lowering rather than
    // accepting a semantic approximation.
    let gated_out = if has_top_level_await {
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
    let already_outlined = hir.functions.iter().any(is_entry_chunk);
    let transform = if already_outlined {
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
    // #10575: a CommonJS module's real body is not in `hir.init` at all — it
    // is the `__perry_cjs_factory` closure `cjs_wrap::wrap_commonjs_for_target`
    // generates. Report on it too, using the same analysis, so the report
    // reflects the body that will actually be outlined for a CJS module.
    if let Some(perry_hir::Expr::Closure { body, captures, .. }) =
        find_cjs_factory_closure(&hir.init)
    {
        let target = target_chunk_stmts();
        let must_stay_ids: HashSet<u32> = captures.iter().copied().collect();
        let fa = analyze_stmts_outlining(body, false, target, &must_stay_ids);
        match fa.gated_out {
            Some(reason) => eprintln!(
                "[perry] entry-outline: {}: cjs factory: {} stmts; NOT a candidate ({}){}",
                hir.name, fa.total_stmts, reason, transform
            ),
            None => eprintln!(
                "[perry] entry-outline: {}: cjs factory: {} stmts → {} chunk(s) of ~{}, {} cross-chunk let(s) to globalize; candidate={}{}",
                hir.name,
                fa.total_stmts,
                fa.chunk_count,
                target,
                fa.cross_chunk_lets,
                fa.is_candidate(),
                transform
            ),
        }
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

/// Does `stmt` reference any id in `must_stay_ids`? Always `false` (and O(1))
/// for the overwhelmingly common empty case — `hir.init` never has must-stay
/// ids; only an outlined CJS factory closure does, one for each id it
/// captures from its enclosing scope (see [`is_cjs_factory_shape`]).
fn stmt_references_any(stmt: &perry_hir::Stmt, must_stay_ids: &HashSet<u32>) -> bool {
    if must_stay_ids.is_empty() {
        return false;
    }
    let mut refs = HashSet::new();
    collect_ref_ids_in_stmts(std::slice::from_ref(stmt), &mut refs);
    refs.iter().any(|id| must_stay_ids.contains(id))
}

/// Like [`classify_top_level`], but a statement referencing one of
/// `must_stay_ids` is always treated as must-stay (`None`), regardless of
/// what `classify_top_level` would otherwise say.
///
/// This exists for #10575's CJS-factory path: the factory closure captures
/// its own name from its enclosing scope (the wrapper's outer IIFE) — a
/// load-bearing self-reference `perry-runtime`'s `module_require.rs` calls
/// through on a circular-require recovery path. Chunk functions are plain,
/// non-capturing `hir.functions` entries, so a captured id can only be read
/// correctly from wherever the ORIGINAL closure-capture read already was —
/// it must never be relocated into a chunk. Keeping that one statement (in
/// practice, the wrapper's `__cjs_module.__perry_cjs_factory =
/// __perry_cjs_factory;` preamble line) inline preserves the exact
/// per-invocation closure-capture semantics codegen already provides,
/// instead of promoting the captured id to a module global — which would
/// change its lifetime from "fresh per closure call" to "one program-wide
/// instance," silently breaking that recovery path if it ever re-invokes the
/// factory. `hir.init`'s own outlining always passes an empty set here, so
/// this is a no-op for every non-CJS module.
fn classify_for_chunking(
    stmt: &perry_hir::Stmt,
    must_stay_ids: &HashSet<u32>,
) -> Option<TopLevelKind> {
    if stmt_references_any(stmt, must_stay_ids) {
        return None;
    }
    classify_top_level(stmt)
}

/// How many chunk functions the interleaving would emit for `stmts` at
/// `target` — a run of relocatable statements becomes ceil(run/target) chunks,
/// and a must-stay statement (an unclassifiable shape) ends the current run.
/// Used as a pre-scan so eligibility is decided before any mutation.
fn count_prospective_chunks(
    stmts: &[perry_hir::Stmt],
    target: usize,
    must_stay_ids: &HashSet<u32>,
) -> usize {
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
        match classify_for_chunking(stmt, must_stay_ids) {
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

/// Attempt to outline `hir`'s entry body (#8595), and — if that finds nothing
/// to do — the CJS factory body instead (#10575). Fail-safe: returns
/// `Skipped(reason)` and leaves `hir` untouched unless a body is provably
/// safe to relocate; callers proceed with the ordinary single-function
/// lowering in that case.
///
/// A module is only ever outlined from one origin: `hir.init` for an
/// ordinary (ESM/script) module, or the CJS factory for a CommonJS-wrapped
/// one. `hir.init` is tried first because it is cheap to check (a CJS
/// module's own `hir.init` is a handful of wrapper statements, never a
/// candidate) and because it is the historical #8595 behavior.
pub fn outline_entry_module(hir: &mut HirModule) -> OutlineOutcome {
    outline_entry_module_core(hir, outline_mode(), target_chunk_stmts())
}

/// Env-free core of [`outline_entry_module`] — the testable seam for the
/// hir.init-vs-CJS-factory orchestration itself (mode and chunk target are
/// ordinary parameters here, not read from the process environment, so
/// tests can exercise both branches without touching global env-var state
/// shared across a parallel test run).
fn outline_entry_module_core(
    hir: &mut HirModule,
    mode: OutlineMode,
    target: usize,
) -> OutlineOutcome {
    if mode == OutlineMode::Disabled {
        return OutlineOutcome::Skipped("PERRY_OUTLINE_ENTRY disabled");
    }
    let safepoints = crate::collectors::count_safepoint_sites(&hir.init);
    let init_outcome = if mode == OutlineMode::Auto
        && !meets_automatic_size_threshold(hir.init.len(), safepoints)
    {
        OutlineOutcome::Skipped("below automatic outlining threshold")
    } else {
        outline_entry_module_with_target(hir, target)
    };
    if matches!(init_outcome, OutlineOutcome::Outlined { .. }) {
        return init_outcome;
    }
    // #10575: `hir.init` was not a candidate (the overwhelmingly common case
    // for a CJS-wrapped unit, whose whole body lives in `__perry_cjs_factory`
    // instead — see the module doc comment). Try that function's body with
    // the identical transform before giving up.
    match outline_cjs_factory_module(hir, mode, target) {
        outlined @ OutlineOutcome::Outlined { .. } => outlined,
        OutlineOutcome::Skipped(_) => init_outcome,
    }
}

/// Env-free core of [`outline_entry_module`]'s `hir.init` path — the testable
/// seam.
fn outline_entry_module_with_target(hir: &mut HirModule, target: usize) -> OutlineOutcome {
    let analysis = analyze_entry_outlining_with_target(hir, target);
    if let Some(reason) = analysis.gated_out {
        return OutlineOutcome::Skipped(reason);
    }
    if !analysis.is_candidate() {
        return OutlineOutcome::Skipped("not a candidate (too small)");
    }
    // Pre-scan: decide eligibility before mutating. Outlining is worthwhile
    // only if the interleaving would emit more than one chunk. `hir.init`
    // has no must-stay ids of its own (that concept exists only for the CJS
    // factory's captured self-reference, #10575).
    let no_must_stay_ids = HashSet::new();
    let prospective_chunks = count_prospective_chunks(&hir.init, target, &no_must_stay_ids);
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

    let (new_body, chunk_fns) = chunk_statements(
        original,
        target,
        &module_name,
        module_is_strict,
        &no_must_stay_ids,
        &mut next_id,
    );
    let chunks = chunk_fns.len();
    hir.functions.extend(chunk_fns);
    hir.init = new_body;
    OutlineOutcome::Outlined { chunks }
}

/// The CJS-factory half of [`outline_entry_module`] (#10575): apply the
/// identical chunking transform to the `__perry_cjs_factory` closure's body
/// instead of `hir.init`.
///
/// Unlike an ordinary named function declaration, this closure is NOT a
/// `hir.functions` entry — it is lexically nested inside the wrapper's outer
/// anonymous IIFE, so lowering represents it the same way as any other
/// function EXPRESSION: a `Stmt::Let` (naming it `__perry_cjs_factory`)
/// whose `init` is an `Expr::Closure`, reachable only by walking `hir.init`'s
/// statement/expression tree (see [`find_cjs_factory_closure_mut`]). New
/// chunk functions are still ordinary `hir.functions` entries — nothing
/// about where a *chunk* lives changes — only the body being split is found
/// differently.
fn outline_cjs_factory_module(
    hir: &mut HirModule,
    mode: OutlineMode,
    target: usize,
) -> OutlineOutcome {
    // Whole-module reads that must happen BEFORE taking a mutable borrow of
    // `hir.init` below: once `find_cjs_factory_closure_mut` hands back a
    // `&mut Expr` borrowed from `hir.init`, only `hir.init`-disjoint fields
    // (like `hir.functions`, appended after) remain independently
    // borrowable — a helper taking `&mut HirModule` as a whole would make
    // the borrow checker treat every field as borrowed for the reference's
    // lifetime, since it can't see the field-level split through the call.
    let max_id = max_func_id(hir);
    let module_name = hir.name.clone();

    let Some(closure) = find_cjs_factory_closure_mut(&mut hir.init) else {
        return OutlineOutcome::Skipped("no CommonJS factory function");
    };
    let perry_hir::Expr::Closure {
        body,
        captures,
        is_strict,
        ..
    } = closure
    else {
        unreachable!("find_cjs_factory_closure_mut only ever returns Expr::Closure");
    };
    // The factory's own captured ids (in practice, just its self-reference —
    // see the module doc comment) must never be relocated into a chunk: a
    // chunk is a plain, non-capturing function and cannot read them.
    // `is_cjs_factory_shape` already proved each one resolves to a `Let` in
    // the enclosing IIFE, so keeping their reference sites inline preserves
    // the exact closure-capture read codegen already emits for them.
    let must_stay_ids: HashSet<u32> = captures.iter().copied().collect();

    let safepoints = crate::collectors::count_safepoint_sites(body);
    if mode == OutlineMode::Auto && !meets_automatic_size_threshold(body.len(), safepoints) {
        return OutlineOutcome::Skipped("cjs factory below automatic outlining threshold");
    }
    let analysis = analyze_stmts_outlining(body, false, target, &must_stay_ids);
    if let Some(reason) = analysis.gated_out {
        return OutlineOutcome::Skipped(reason);
    }
    if !analysis.is_candidate() {
        return OutlineOutcome::Skipped("not a candidate (too small)");
    }
    let prospective_chunks = count_prospective_chunks(body, target, &must_stay_ids);
    if prospective_chunks <= 1 {
        return OutlineOutcome::Skipped("would not split into multiple chunks");
    }
    if prospective_chunks > (u32::MAX - max_id) as usize {
        return OutlineOutcome::Skipped("function id space exhausted");
    }
    let mut next_id = max_id + 1;
    // A chunk carries the factory's own strictness, exactly as an `hir.init`
    // chunk carries the module's (#9423) — these statements were the
    // factory's top-level body a moment ago.
    let is_strict = *is_strict;
    let original = std::mem::take(body);

    let (new_body, chunk_fns) = chunk_statements(
        original,
        target,
        &module_name,
        is_strict,
        &must_stay_ids,
        &mut next_id,
    );
    *body = new_body;
    let chunks = chunk_fns.len();
    // `closure`/`body` borrowed only `hir.init`, so `hir.functions` remains
    // independently borrowable here — see the comment above.
    hir.functions.extend(chunk_fns);
    OutlineOutcome::Outlined { chunks }
}

/// Split `original` into chunk functions of ~`target` relocatable statements
/// each, returning the rewritten residual body (chunk calls interleaved with
/// any must-stay statement, in original order) and the new chunk functions.
/// Shared by the `hir.init` path and the CJS-factory path (#10575) — the only
/// difference between them is WHERE `original` came from and where the
/// results get written back.
fn chunk_statements(
    original: Vec<perry_hir::Stmt>,
    target: usize,
    module_name: &str,
    is_strict: bool,
    must_stay_ids: &HashSet<u32>,
    next_id: &mut u32,
) -> (Vec<perry_hir::Stmt>, Vec<perry_hir::Function>) {
    let mut new_body: Vec<perry_hir::Stmt> = Vec::new();
    let mut chunk_fns: Vec<perry_hir::Function> = Vec::new();
    // The current run of relocatable statements accumulating into a chunk.
    let mut run: Vec<perry_hir::Stmt> = Vec::new();
    let mut run_safepoints = 0usize;

    // Emit the accumulated run as a chunk function and append its call, unless
    // empty. `flush` is a plain fn over explicit params to keep the borrow
    // checker happy.
    fn flush(
        run: &mut Vec<perry_hir::Stmt>,
        chunk_fns: &mut Vec<perry_hir::Function>,
        new_body: &mut Vec<perry_hir::Stmt>,
        next_id: &mut u32,
        module_name: &str,
        is_strict: bool,
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
            is_strict,
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
        match classify_for_chunking(&stmt, must_stay_ids) {
            Some(TopLevelKind::Relocatable) => run.push(stmt),
            None => {
                // A statement we cannot safely relocate (control flow, etc.):
                // end the current chunk run and keep this statement inline, at
                // its original position, so eval order and any residual-body
                // scan that reads it are preserved.
                flush(
                    &mut run,
                    &mut chunk_fns,
                    &mut new_body,
                    next_id,
                    module_name,
                    is_strict,
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
                next_id,
                module_name,
                is_strict,
            );
            run_safepoints = 0;
        }
    }
    flush(
        &mut run,
        &mut chunk_fns,
        &mut new_body,
        next_id,
        module_name,
        is_strict,
    );

    (new_body, chunk_fns)
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
            count_prospective_chunks(&m.init, usize::MAX, &HashSet::new()),
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

    // --- #10575: CJS factory outlining -------------------------------------

    /// A plain closure expression, reused for both the wrapper's outer
    /// anonymous IIFE and (by default) its inner `__perry_cjs_factory`.
    fn factory_closure(func_id: u32, body: Vec<Stmt>) -> Expr {
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

    fn call_no_args(callee: Expr) -> Expr {
        Expr::Call {
            callee: Box::new(callee),
            args: vec![],
            type_args: vec![],
            byte_offset: 0,
        }
    }

    /// Build the exact `hir.init` shape `wrap_commonjs_for_target` produces
    /// for a CJS module — `const _cjs = (function() { let
    /// __perry_cjs_factory = function() { ... }; return
    /// __perry_cjs_factory(); })();` — with an explicit `factory` closure
    /// expression, so the shape-rejection tests can hand in a deliberately
    /// wrong one. `leading` is prepended to the IIFE's own body (used to
    /// prove a preceding `PreallocateBoxes`, as a real compile emits, does
    /// not defeat the search).
    fn cjs_wrapped_init_with(leading: Vec<Stmt>, factory: Expr) -> Vec<Stmt> {
        let mut outer_body = leading;
        outer_body.push(let_stmt(101, CJS_FACTORY_NAME, factory));
        outer_body.push(Stmt::Return(Some(call_no_args(Expr::LocalGet(101)))));
        let outer_closure = factory_closure(100, outer_body);
        vec![let_stmt(102, "_cjs", call_no_args(outer_closure))]
    }

    fn cjs_wrapped_init(factory_body: Vec<Stmt>) -> Vec<Stmt> {
        cjs_wrapped_init_with(vec![], factory_closure(103, factory_body))
    }

    fn factory_closure_with_captures(func_id: u32, body: Vec<Stmt>, captures: Vec<u32>) -> Expr {
        let mut closure = factory_closure(func_id, body);
        if let Expr::Closure { captures: c, .. } = &mut closure {
            *c = captures;
        }
        closure
    }

    #[test]
    fn find_cjs_factory_closure_matches_only_the_wrap_generated_shape() {
        assert!(find_cjs_factory_closure(&cjs_wrapped_init(vec![])).is_some());

        // A preceding `PreallocateBoxes` (as a real compile emits for the
        // hoisted function declaration) does not defeat the search.
        let with_prealloc = cjs_wrapped_init_with(
            vec![Stmt::PreallocateBoxes(vec![101])],
            factory_closure(103, vec![]),
        );
        assert!(find_cjs_factory_closure(&with_prealloc).is_some());

        // A param disqualifies it — the wrapper's factory always takes none.
        let mut with_param = factory_closure(103, vec![]);
        if let Expr::Closure { params, .. } = &mut with_param {
            params.push(perry_hir::Param {
                id: 200,
                name: "x".into(),
                ty: Type::Any,
                default: None,
                decorators: vec![],
                is_rest: false,
                arguments_object: None,
            });
        }
        assert!(find_cjs_factory_closure(&cjs_wrapped_init_with(vec![], with_param)).is_none());

        // Async/generator/capturing disqualify it too.
        let mut async_factory = factory_closure(103, vec![]);
        if let Expr::Closure { is_async, .. } = &mut async_factory {
            *is_async = true;
        }
        assert!(find_cjs_factory_closure(&cjs_wrapped_init_with(vec![], async_factory)).is_none());

        let mut generator_factory = factory_closure(103, vec![]);
        if let Expr::Closure { is_generator, .. } = &mut generator_factory {
            *is_generator = true;
        }
        assert!(
            find_cjs_factory_closure(&cjs_wrapped_init_with(vec![], generator_factory)).is_none()
        );

        let mut capturing_factory = factory_closure(103, vec![]);
        if let Expr::Closure { captures, .. } = &mut capturing_factory {
            captures.push(7);
        }
        assert!(
            find_cjs_factory_closure(&cjs_wrapped_init_with(vec![], capturing_factory)).is_none()
        );

        // A differently-named binding is never mistaken for the factory.
        let mut unrelated_body = vec![let_stmt(101, "helper", factory_closure(103, vec![]))];
        unrelated_body.push(Stmt::Return(Some(call_no_args(Expr::LocalGet(101)))));
        let unrelated = vec![let_stmt(
            102,
            "_cjs",
            call_no_args(factory_closure(100, unrelated_body)),
        )];
        assert!(find_cjs_factory_closure(&unrelated).is_none());

        // No CJS wrapper at all (an ordinary ESM module).
        assert!(find_cjs_factory_closure(&[let_stmt(0, "x", Expr::Number(1.0))]).is_none());
    }

    #[test]
    fn cjs_factory_body_is_outlined_when_hir_init_is_not_a_candidate() {
        // hir.init is the wrap_commonjs shape: a single statement (the `_cjs`
        // binding), never a candidate on its own (chunk_count is always 1
        // for one statement, regardless of target) — the real body lives
        // inside the nested `__perry_cjs_factory` closure.
        //
        // Factory body: `let shared = 1` (chunk), a lone statement (chunk),
        // then `shared` read back (chunk) — the same cross-chunk-let shape as
        // `only_boundary_crossing_or_preallocated_bindings_become_globals`,
        // now living inside a closure instead of directly in `hir.init`.
        let mut m = module_with_init(cjs_wrapped_init(vec![
            let_stmt(10, "shared", Expr::Number(1.0)),
            Stmt::Expr(Expr::Number(0.0)),
            Stmt::Expr(Expr::LocalGet(10)),
        ]));

        let outcome = outline_entry_module_core(&mut m, OutlineMode::Forced, 1);
        assert_eq!(outcome, OutlineOutcome::Outlined { chunks: 3 });

        // hir.init's own top-level shape is untouched — one statement, the
        // `_cjs` binding — the transform outlined the FACTORY, not the
        // module top level.
        assert_eq!(m.init.len(), 1);
        assert!(matches!(&m.init[0], Stmt::Let { name, .. } if name == "_cjs"));

        let Some(Expr::Closure { body, .. }) = find_cjs_factory_closure(&m.init) else {
            panic!("factory closure still present and findable");
        };
        assert_eq!(body.len(), 3, "three ordered chunk calls");
        assert!(
            body.iter()
                .all(|s| matches!(s, Stmt::Expr(Expr::Call { .. }))),
            "every residual statement in the factory is a chunk call: {body:?}"
        );
        assert_eq!(
            m.functions.iter().filter(|f| is_entry_chunk(f)).count(),
            3,
            "three chunk functions were created"
        );

        // The cross-chunk let inside the factory is promoted exactly like a
        // cross-chunk `hir.init` let would be.
        assert_eq!(outlined_entry_global_let_ids(&m), HashSet::from([10]));

        // The factory's original statement order is recoverable for any
        // codegen scan that needs it (mirrors `logical_entry_stmts` for
        // `hir.init`).
        let logical = logical_outlined_function_stmts(&m);
        assert_eq!(logical.len(), 3);
        assert!(matches!(logical[0], Stmt::Let { id: 10, .. }));
    }

    #[test]
    fn cjs_factory_self_reference_capture_stays_inline_not_chunked() {
        // Mirrors the real `wrap_commonjs_for_target` shape: the wrapper's
        // preamble does `__cjs_module.__perry_cjs_factory =
        // __perry_cjs_factory;`, so the factory closure ALWAYS captures its
        // own name (id 101 here — the wrapper's own `Let`) from the
        // enclosing IIFE. `perry-runtime`'s `module_require.rs` calls
        // through that captured value on a circular-require recovery path,
        // so it must keep working after outlining. A chunk is a plain,
        // non-capturing `hir.functions` entry and cannot read a captured id
        // — the statement reading it (here, a bare `LocalGet(101)` standing
        // in for the real assignment) must stay in the factory's own
        // residual body, never relocated into a chunk, so it keeps reading
        // it via ordinary closure-capture codegen exactly as before
        // outlining (#10575) — NOT via a promoted module global, which
        // would change a per-invocation-fresh capture into a program-wide
        // single instance and silently break that recovery path if it ever
        // re-invokes the factory.
        let self_reference_read = Stmt::Expr(Expr::LocalGet(101));
        let factory = factory_closure_with_captures(
            103,
            vec![
                self_reference_read,
                let_stmt(10, "shared", Expr::Number(1.0)),
                Stmt::Expr(Expr::Number(0.0)),
                Stmt::Expr(Expr::LocalGet(10)),
            ],
            vec![101],
        );
        let mut m = module_with_init(cjs_wrapped_init_with(vec![], factory));

        let outcome = outline_entry_module_core(&mut m, OutlineMode::Forced, 1);
        assert_eq!(outcome, OutlineOutcome::Outlined { chunks: 3 });

        let Some(Expr::Closure { body, .. }) = find_cjs_factory_closure(&m.init) else {
            panic!("factory closure still present and findable");
        };
        assert!(
            matches!(&body[0], Stmt::Expr(Expr::LocalGet(101))),
            "the captured self-reference read stayed inline, in its \
             original (first) position: {body:?}"
        );
        assert!(
            body[1..]
                .iter()
                .all(|s| matches!(s, Stmt::Expr(Expr::Call { .. }))),
            "everything else still outlined into ordered chunk calls: {body:?}"
        );
    }

    #[test]
    fn outline_entry_module_core_prefers_hir_init_over_the_cjs_factory() {
        // hir.init has three top-level statements — two ordinary ones and
        // (as one item among them) the CJS wrapper statement — all
        // independently large enough to outline at target=1. hir.init must
        // win: #8595's original behavior is unchanged, and a module is only
        // ever outlined from one origin. #8595's transform has no CJS-aware
        // special case, so it relocates the wrapper statement whole (as an
        // opaque `Stmt::Let`) into its own chunk, untouched internally.
        let mut init = vec![
            let_stmt(0, "x", Expr::Number(1.0)),
            Stmt::Expr(Expr::LocalGet(0)),
        ];
        init.extend(cjs_wrapped_init(vec![
            let_stmt(10, "shared", Expr::Number(1.0)),
            Stmt::Expr(Expr::LocalGet(10)),
        ]));
        let mut m = module_with_init(init);

        let outcome = outline_entry_module_core(&mut m, OutlineMode::Forced, 1);
        assert_eq!(outcome, OutlineOutcome::Outlined { chunks: 3 });
        assert_eq!(m.init.len(), 3, "hir.init's own three chunk calls");

        // The factory body is untouched: reconstruct the logical hir.init
        // view (inlining hir.init's own chunks back) and dig into the
        // relocated CJS-wrapper statement's nested closure.
        let logical = logical_entry_stmts(&m);
        assert_eq!(logical.len(), 3);
        let cjs_stmt: &Stmt = *logical
            .iter()
            .find(|s| matches!(s, Stmt::Let { name, .. } if name == "_cjs"))
            .expect("the CJS wrapper statement survived, just relocated");
        let Some(Expr::Closure { body, .. }) =
            find_cjs_factory_closure(std::slice::from_ref(cjs_stmt))
        else {
            panic!("the factory closure is still findable inside it");
        };
        assert_eq!(body.len(), 2, "the factory body was never touched");
        assert!(matches!(&body[0], Stmt::Let { id: 10, .. }));

        // No factory-outlining occurred: only hir.init's own three chunks
        // exist.
        assert_eq!(m.functions.iter().filter(|f| is_entry_chunk(f)).count(), 3);
    }

    #[test]
    fn outline_entry_module_core_declines_with_no_candidate_on_either_side() {
        let mut m = module_with_init(vec![let_stmt(0, "_cjs", Expr::Number(0.0))]);
        // No `__perry_cjs_factory` function at all (an ordinary small ESM
        // module) — nothing to outline on either side.
        let outcome = outline_entry_module_core(&mut m, OutlineMode::Forced, 1);
        assert_eq!(
            outcome,
            OutlineOutcome::Skipped("not a candidate (too small)")
        );
        assert_eq!(m.init.len(), 1);
        assert!(m.functions.is_empty());
    }
}
