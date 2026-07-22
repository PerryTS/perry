//! ArkUI/ArkTS code generation for Perry --target harmonyos.
//!
//! HarmonyOS NEXT renders UI declaratively from `.ets` files annotated with
//! `@Entry @Component struct ... { build() { ... } }`. Perry's `perry/ui`
//! surface (`App({body: VStack([Text("hi"), Button("OK", () => {})])})`) is
//! normally lowered to native FFI calls (perry_ui_*_create / set_*) on
//! iOS / macOS / Android / Linux / Windows — backed by perry-ui-* crates that
//! call into UIKit / AppKit / GTK4 / Win32 imperatively.
//!
//! HarmonyOS doesn't fit that imperative model: ArkTS owns the UI tree, not
//! native code. So instead of routing perry/ui calls through FFI, this crate
//! walks the HIR pre-codegen, harvests the perry/ui widget tree, and emits
//! it as a real ArkUI `pages/Index.ets` file. The compiled `.so` then has
//! no UI calls at all — Perry's `main()` runs once at NAPI startup for any
//! non-UI logic, and ArkUI declaratively renders the harvested tree.
//!
//! Phase 2 v1.5 scope (visual surface):
//! - `App({body: <expr>})` extraction
//! - `Text(literal)` → `Text('lit').fontSize(20)`
//! - `VStack([...], spacing?)` → `Column({space: <spacing>}) { ... }`
//! - `HStack([...], spacing?)` → `Row({space: <spacing>}) { ... }`
//! - `Button(label, onPress)` → `Button('label')`
//! - `TextField(placeholder, onChange)` → `TextInput({placeholder: 'hint'})`
//! - `Toggle(label, onChange)` → label rendered as Text + ArkUI Toggle in a Row
//! - `Slider(min, max, onChange)` → `Slider({min, max, value: min})`
//! - `Spacer()` → `Blank()`
//! - `Divider()` → `Divider()`
//! - LocalGet escape: `let x = Text("hi"); App({body: x})` follows the
//!   binding back to its init expression for any read-only top-level local.
//!
//! Phase 2 v2 scope (callback bridge):
//! - `Button(label, onPress)` captures `onPress` as a closure, assigns it
//!   a slot id, and emits ArkUI `.onClick(() => perryEntry.invokeCallback(<id>))`.
//!   The closure is then registered into a runtime slot table by an
//!   injected `perry_arkts_register_callback(<id>, <closure>)` call (the
//!   compile harvest pass plants this in `module.init`). On tap, NAPI's
//!   `invokeCallback` looks the slot up and calls the closure via
//!   `js_closure_call0` — running the original Perry TS body.
//! - Toggle/TextField/Slider callbacks are still dropped because their
//!   event payloads (boolean / string / number) need NaN-box marshaling
//!   on the ArkTS → Rust boundary; that's v2.5.
//!
//! State-binding caveat: ArkUI's `@State` / `@Link` reactivity is handled
//! natively in the ArkTS runtime, but Perry's `State<T>` lives in the .so
//! heap and doesn't share memory with the ArkTS heap. Reactive UI updates
//! after a callback (e.g. `count++` re-rendering a `Text(count)`) need a
//! push channel from the .so back to ArkUI; that's a future phase.

use anyhow::Result;
use perry_hir::ir::{Class, Expr, Module, Stmt};
use std::collections::HashMap;

// ---- submodule declarations (issue #1100 mechanical split) ----
// Every helper retains its original flat-namespace visibility via
// `pub(crate) use <mod>::*;` so cross-module calls and the inline
// test module keep working unchanged.
mod app_extract;
mod bindings;
mod conditions;
mod emit_widget;
mod inline;
mod media;
mod mutations;
mod set_hidden;
mod state_rewrite;
#[cfg(test)]
mod tests;
mod types;
mod util;
mod view_builders;
mod widget_walks;
mod widgets;

pub(crate) use app_extract::*;
pub(crate) use bindings::*;
pub(crate) use conditions::*;
pub(crate) use emit_widget::*;
pub(crate) use inline::*;
pub(crate) use media::*;
pub(crate) use mutations::*;
pub(crate) use set_hidden::*;
pub(crate) use state_rewrite::*;
pub(crate) use types::*;
pub(crate) use util::*;
pub(crate) use view_builders::*;
pub(crate) use widget_walks::*;
pub(crate) use widgets::*;

// Public crate surface (consumed by perry::commands::compile and
// the phase2_full_app_smoke integration test).
pub use types::HarvestResult;

/// Walk `module.init` for the first `App({...})` call from `perry/ui`,
/// emit the corresponding ArkUI `pages/Index.ets`, capture every
/// closure-bearing arg into `HarvestResult.callbacks` so the compile
/// harvest pass can inject runtime registrations, AND **destructively
/// strip the App call from the HIR** so the LLVM backend doesn't emit
/// `perry_ui_*` FFI calls that would be unresolved on the OHOS target
/// (no `perry-ui-harmonyos` crate exists — UI is rendered declaratively
/// from the emitted `.ets`, not imperatively from native code).
///
/// Returns `Ok(None)` if the module doesn't use `perry/ui App` (the caller
/// should fall through to the blank EntryAbility-only stub; HIR is
/// untouched). Returns `Ok(Some(HarvestResult))` for static-UI programs.
pub fn emit_index_ets(module: &mut Module) -> Result<Option<HarvestResult>> {
    // Snapshot the class table BEFORE the &mut borrow on init so we can
    // look up __AnonShape_* classes (Perry's closed-shape object-literal
    // optimization, v0.5.337+) without aliasing &mut module.
    let classes = module.classes.clone();
    // Phase 2 v6 — pre-walk for `state<T>(initial)` declarations + rewrite
    // `state.set(v)` calls inside the entire module to `setText(synth_id, v)`.
    // This needs to run BEFORE find_and_strip_app + bindings collection so
    // the rewrites land before any harvest detection sees the closures.
    let state_registry = collect_state_bindings(&module.init);
    if !state_registry.is_empty() {
        rewrite_state_calls_in_stmts(&mut module.init, &state_registry);
    }
    // Phase 2 v3.5 — leaf-mutator state binding for `widgetSetHidden`.
    // Pre-walk the entire module (init + functions + closures) for any
    // `widgetSetHidden(LocalGet(target), _)` call. Targets touched outside
    // module.init earn a `VisibilityBinding`; their widget gets a bound
    // `.visibility(this.hidden_<id> ? ...)` modifier and closure-time
    // calls route through the NAPI drain queue at runtime. See
    // `VisibilityBinding` doc for the full design.
    let visibility_bindings = collect_visibility_bindings(module);
    if !visibility_bindings.is_empty() {
        // HIR rewrite: walk every `module.functions[*].body` and every
        // closure body. `widgetSetHidden(LocalGet(target), value)` calls
        // for a target with a binding get rewritten to
        // `setVisibility(synth_id, value)`. Module.init is intentionally
        // skipped — its `widgetSetHidden` calls are static-analyzed for
        // the `@State` initial value via `collect_visibility_bindings`'
        // pass 2 and don't need a runtime push at main()-time.
        for f in module.functions.iter_mut() {
            rewrite_set_hidden_calls_in_stmts(&mut f.body, &visibility_bindings);
        }
        rewrite_set_hidden_in_closures_in_stmts(&mut module.init, &visibility_bindings);
    }
    // Phase 2 v3.6 — view-builder lifting for tree-mutator functions.
    // See `ViewBuilder` doc for the full design. Pre-walk identifies
    // functions that are called from closures and build widget trees on
    // a module-level container; their body's mutations get lifted as
    // conditional branches keyed on `@State contentView_<target>`, and
    // closure call sites get a `setContentView(target, view_id)` call
    // prepended that pushes through the NAPI drain queue.
    let mut view_builder_group_counter: u32 = 1_000_000; // start high to avoid collision with v0.5.480 collect_mutations group counter
    let view_builders = collect_view_builders(module, &mut view_builder_group_counter);
    if !view_builders.is_empty() {
        // Inject `setContentView(target, view_id)` calls into every
        // closure body that calls a view-builder function. This rewrite
        // walks module.init's closures + every function's closures.
        rewrite_view_builder_calls_in_stmts(&mut module.init, &view_builders);
        for f in module.functions.iter_mut() {
            rewrite_view_builder_calls_in_stmts(&mut f.body, &view_builders);
        }
    }
    // Issue #369 — detect `perry/media` usage (createPlayer / play / etc.)
    // anywhere in the module's init stmts or function bodies. When seen,
    // wrap_index_page injects a `@ohos.multimedia.media` import + a
    // `setInterval(100ms)` drain pump that pulls AVPlayer ops out of the
    // runtime's media queues and pushes state observations back in.
    let uses_media = module_uses_media(module);
    // Build an analysis-only `init` that has top-level user-function calls
    // expanded inline. The harvest's collectors then see widgetAddChild /
    // setPadding / etc. that happen inside the called function's body.
    // Mango's pattern:
    //
    //     const connListContainer = VStack(10, []);
    //     function refreshConnectionList() {
    //         widgetClearChildren(connListContainer);
    //         if (connectionNames.length === 0) {
    //             const welcomeCard = VStack(16, []);
    //             widgetAddChild(connListContainer, welcomeCard);
    //         }
    //     }
    //     refreshConnectionList();
    //
    // We CANNOT mutate `module.init` directly — the same module then goes
    // through LLVM codegen and inlining a `return` from a void function
    // becomes a top-level `return` from `main()`, which fails the LLVM type
    // checker. So we work on a clone for analysis only. `find_and_strip_app`
    // still mutates module.init below to remove the App() call before LLVM
    // codegen sees it; that's the only intentional mutation.
    let analysis_init = inlined_analysis_init(module);
    // Build a const-binding lookup for top-level `let x = <perry/ui call>;`
    // so the Body can reference a local: `App({body: x})` finds x's init.
    let bindings = collect_const_bindings(&analysis_init);
    // Issue #410 — pre-walk for `declare const __platform__: number` style
    // compile-time constants. Used by serialize_condition to inline
    // `__platform__ === N` comparisons that would otherwise emit an
    // undeclared identifier into the ArkTS source. This codegen path is
    // only invoked for `--target harmonyos[-simulator]`, so __platform__
    // is always 9 here (matches the table in
    // `crates/perry-codegen/src/codegen.rs::platform_number`).
    let compile_time_consts = collect_compile_time_constants(&analysis_init);
    // Issue #408 — pre-walk for procedurally-built UI mutators
    // (widgetAddChild / scrollviewSetChild / setPadding / setCornerRadius /
    // widgetSetBackgroundColor / etc.). Recorded against their target
    // widget local so emit_widget can fold them into the ArkUI body.
    // Walks pre-strip so mutators that live alongside `App({...})` are
    // captured; the strip itself doesn't touch the mutator stmts.
    // Walks the inlined `analysis_init` so mutators inside user-function
    // bodies are seen too (e.g. Mango's `refreshConnectionList()` →
    // `widgetAddChild(connListContainer, welcomeCard)`).
    let mut mutations = collect_mutations(&analysis_init, &bindings, &compile_time_consts);
    // Phase 2 v3.5 — for any widget targeted by `widgetSetHidden` outside
    // module init (closures, function bodies), upgrade its mutation list
    // by (a) prepending a `Mutation::VisibilityBinding(synth_id)` entry
    // (consumed by `emit_modifier_mutations` to emit the bound modifier),
    // and (b) dropping any static `.visibility(Visibility.X)` entries that
    // collect_mutations recorded from module-init `widgetSetHidden` calls
    // (the @State init value handles those). The VisibilityBinding goes
    // FIRST in the vec so the modifier-chain ordering remains stable.
    if !visibility_bindings.is_empty() {
        for (target_id, binding) in &visibility_bindings {
            let entries = mutations.entry(*target_id).or_default();
            entries.retain(|e| {
                !matches!(&e.mutation,
                    Mutation::Modifier(s) if s.starts_with(".visibility(Visibility."))
            });
            entries.insert(
                0,
                MutationEntry {
                    mutation: Mutation::VisibilityBinding(binding.synth_id.clone()),
                    condition: None,
                },
            );
        }
    }
    // Phase 2 v3.6 — for each view-builder function, run collect_mutations
    // on its body with a synthetic condition that gates emission on
    // `this.contentView_<target_synth> === '<view_id>'`. The resulting
    // conditional mutations get merged into the main mutations map so
    // the target container's emit produces:
    //
    //     Column() {
    //         <default content>            // unconditional from module init
    //         if (this.contentView_X === 'Y') {
    //             <lifted view body>       // from showConnectionForm
    //         }
    //     }
    //
    // Each ViewBuilder gets its own group_id so multiple views (e.g.
    // showConnectionForm, showSettings) targeting the same container
    // emit as separate `if` blocks rather than colliding.
    //
    // The function body's local `let X = VStack(...)` etc. need to be
    // visible during emit so child references like `widgetAddChild(parent,
    // X)` can resolve. We MERGE the function-body bindings into the
    // main `bindings` map. Function-local LocalIds are unique per the
    // perry-hir lowering pass, so collisions are not expected; if any
    // arise, the function body's binding wins (consistent with
    // collect_const_bindings' last-write-wins semantics).
    let mut bindings = bindings;
    if !view_builders.is_empty() {
        // Build the function map needed by `expr_level_inline_pass` so
        // helper calls like `makeLabel(...)` / `makeSecondary(...)`
        // inside the view-builder body get inlined and their result
        // expression substitutes the call. Without this, emit_widget
        // hits `[unrecognized body]` for every helper-wrapped Text /
        // Stack child.
        let function_map_inline: HashMap<perry_hir::types::FuncId, perry_hir::ir::Function> =
            module.functions.iter().map(|f| (f.id, f.clone())).collect();
        let function_lookup: HashMap<perry_hir::types::FuncId, &perry_hir::ir::Function> =
            module.functions.iter().map(|f| (f.id, f)).collect();
        // Start view-builder Phase B remap counter ABOVE the highest
        // LocalId already used by `analysis_init` (Phase A + B inlining
        // for module.init). Without this, my view-builder body's
        // remapped lets collide with analysis_init's remapped lets and
        // bindings get clobbered (Mango: helper-call-from-init's
        // inlined `let X = Text('Databases & Collections')` overwritten
        // by view-builder's inlined `let X = Text('Explorer')` because
        // both ended up at the same X).
        let mut analysis_init_locals: Vec<u32> = Vec::new();
        collect_local_ids_in_stmts(&analysis_init, &mut analysis_init_locals);
        let analysis_init_max = analysis_init_locals.into_iter().max().unwrap_or(0);
        let module_max = max_local_id_in_module(module);
        let mut next_local: u32 = module_max.max(analysis_init_max).saturating_add(1);
        for builder in &view_builders {
            let Some(func) = function_lookup.get(&builder.func_id) else {
                continue;
            };
            // Phase B inline pass on a CLONE of the view-builder's body
            // so helper-function calls (`makeLabel(...)`) become
            // resolvable LocalGet references with their let-init hoisted
            // before the parent stmt. Same machinery as v0.5.491's
            // module-init inlining, just applied per-function.
            let mut inline_budget: usize = 256;
            let body_clone: Vec<Stmt> = func.body.clone();
            let body_bindings_pre = collect_const_bindings(&body_clone);
            let inlined_body = expr_level_inline_pass(
                body_clone,
                &function_map_inline,
                &body_bindings_pre,
                &mut next_local,
                &mut inline_budget,
            );
            // Build a synthetic enclosing condition. Re-use
            // collect_mutations' if/else-walking machinery to splice
            // the mutations into the right group + branch.
            let cond_str = format!(
                "this.contentView_{} === '{}'",
                builder.target_synth, builder.view_id
            );
            let synthetic_cond = MutationCondition {
                cond_str,
                branch: Branch::Then,
                group: builder.group_id,
            };
            let mut local_group_counter = builder.group_id + 1;
            let view_bindings = collect_const_bindings(&inlined_body);
            // Merge view-body bindings into the global map so emit_widget
            // can resolve the conditional addChild's child references.
            for (k, v) in &view_bindings {
                bindings.entry(*k).or_insert_with(|| v.clone());
            }
            for stmt in &inlined_body {
                collect_mutations_in_stmt(
                    stmt,
                    Some(synthetic_cond.clone()),
                    &mut mutations,
                    &mut local_group_counter,
                    &view_bindings,
                    &compile_time_consts,
                );
            }
        }
    }
    let Some(body_expr) = find_and_strip_app(&mut module.init, &classes) else {
        return Ok(None);
    };
    let mut callbacks: Vec<Expr> = Vec::new();
    let mut text_slots: Vec<TextSlot> = Vec::new();
    let mut lazy_sources: Vec<LazyDataSource> = Vec::new();
    let mut extras = HarvestExtras::default();
    let arkts_locals: HashMap<LocalId, String> = HashMap::new();
    let widget_arkui = emit_widget(
        &body_expr,
        &bindings,
        0,
        &mut callbacks,
        &mut text_slots,
        &arkts_locals,
        &classes,
        &state_registry,
        &mut lazy_sources,
        &mut extras,
        &mutations,
        None,
    );
    Ok(Some(HarvestResult {
        ets_source: wrap_index_page(
            &widget_arkui,
            &text_slots,
            &lazy_sources,
            uses_media,
            &visibility_bindings,
            &view_builders,
            &extras,
        ),
        callbacks,
    }))
}
