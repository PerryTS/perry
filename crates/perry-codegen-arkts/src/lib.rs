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

// LocalId is `u32` upstream; re-import directly so we don't carry a
// transitive dep on perry-types just for the type alias.
type LocalId = u32;

/// Result of harvesting an `App({body: ...})` call: the emitted ArkUI
/// source plus the closures that need to be registered into the runtime
/// callback table. Each `callbacks[i]` is the original Perry HIR closure
/// expression at slot `i`; the emitted .ets references it as
/// `perryEntry.invokeCallback(i)`.
pub struct HarvestResult {
    pub ets_source: String,
    pub callbacks: Vec<Expr>,
}

/// Per-id reactive Text registration. `Text("Count: 0", "counter")`
/// registers `id="counter", initial="Count: 0"`. The harvest pass emits
/// `@State text_counter: string = 'Count: 0'` on the page struct and
/// `Text(this.text_counter)` at the widget site; user code calls
/// `setText("counter", newValue)` from inside a closure to rerender.
///
/// Two ids are tracked: `original_id` is the verbatim string the user
/// wrote (used in the switch case, since that's what the runtime drain
/// queue produces), and `field_id` is the ArkTS-safe field-name suffix.
struct TextSlot {
    original_id: String,
    field_id: String,
    initial: String,
}

/// Phase 2 v10 — Real LazyVStack registration. Each
/// `LazyVStack(items.map(item => widget))` allocates a
/// `PerryListDataSource`-backed `@State` field on the page struct. The
/// harvest collects these so `wrap_index_page` can emit the field decls +
/// the `PerryListDataSource` helper-class boilerplate once.
struct LazyDataSource {
    field_id: String,
    items_source: String,
}

/// Issue #669 — Chart widget HarmonyOS backend. Each `Chart(kind, w, h)`
/// allocates an inline CanvasRenderingContext2D on the @Component
/// (`private __chart_<n>_ctx: CanvasRenderingContext2D = new
/// CanvasRenderingContext2D(new RenderingContextSettings(true))`).
/// `chartAddDataPoint` / `chartClearData` / `chartSetTitle` calls in the
/// module-init harvest are folded into the static `data` / `title`
/// arrays baked into the Canvas's `onReady` draw closure. `chartReload`
/// is a no-op at codegen time — the draw closure is already wired to
/// the Canvas's draw cycle.
#[allow(dead_code)] // kind / width / height / points / title are
                    // baked into the draw closure at emit time, but
                    // we also stash them on the instance so a future
                    // diagnostic pass (e.g. "how many points does each
                    // chart have?") can read them off the harvest.
struct ChartInstance {
    /// Per-page sequential id used to name `__chart_<n>_ctx`.
    field_id: String,
    /// 0 = line, 1 = bar, 2 = pie. Falls back to bar for unknown values.
    kind: i64,
    /// Layout hints — flow through to `.width(N).height(N)`. 0 means
    /// "use intrinsic / leave unset".
    width: f64,
    height: f64,
    /// Folded `(label, value)` pairs. `chartClearData` resets the list;
    /// `chartAddDataPoint` appends; later wins.
    points: Vec<(String, f64)>,
    /// Folded title string (last `chartSetTitle` wins). Empty = no title.
    title: String,
}

/// Issue #670 — TreeView widget HarmonyOS backend. Each `TreeView(root,
/// onSelect)` builds a static node graph at codegen time (by following
/// the chain of `TreeNode(id, label)` constructors + `treeNodeAddChild`
/// mutators referenced by `root`) and emits the graph as a private
/// `__tree_<n>_nodes` constant + `@State __tree_<n>_expanded: Set<string>`
/// + `@State __tree_<n>_selectedId: string` on the @Component. A
/// `__tree_<n>_flatten()` method walks the graph using the expanded set
/// and produces the visible rows ArkUI's `ForEach` iterates over.
#[allow(dead_code)] // on_select_idx is used in the inline ForEach
                    // body (literal interpolation) rather than read
                    // back at emit time, but we record it on the
                    // instance for diagnostic clarity.
struct TreeViewInstance {
    /// Per-page sequential id used to name fields + helper method.
    field_id: String,
    /// Resolved tree built at codegen time. `Some` when the root walk
    /// succeeded; `None` is impossible here (we only push on success).
    root: TreeViewNode,
    /// Callback slot id for the user-supplied onSelect closure, if any.
    on_select_idx: Option<usize>,
}

/// Static tree node captured at codegen time. Mirrors `TreeNode(id, label)`
/// + its accumulated `treeNodeAddChild` children, recursively.
#[derive(Clone)]
struct TreeViewNode {
    id: String,
    label: String,
    children: Vec<TreeViewNode>,
}

/// Bundled holder for per-page state collectors that emit_widget grew
/// to need beyond text_slots / lazy_sources. Lives behind a single
/// `&mut HarvestExtras` param so adding a new widget that registers
/// @Component-level state doesn't require touching every emit_*
/// signature. Currently holds:
/// - chart_instances: see [`ChartInstance`] (issue #669)
/// - tree_view_instances: see [`TreeViewInstance`] (issue #670)
#[derive(Default)]
struct HarvestExtras {
    chart_instances: Vec<ChartInstance>,
    tree_view_instances: Vec<TreeViewInstance>,
}

/// Phase 2 v6 — `state<T>(initial)` registry. Each `let x = state(initial)`
/// declaration in `module.init` registers a synthetic id (`__state_<N>`)
/// + the initial value. Subsequent `x.text()` calls emit reactive Text
/// using the synth id; `x.set(v)` calls inside closures get rewritten to
/// `setText(synth_id, v)` calls (the runtime's `perry_arkts_set_text`
/// already coerces non-string args via `js_jsvalue_to_string`).
struct StateBinding {
    synth_id: String,
    initial_str: String,
}

/// Phase 2 v3.5 — leaf-mutator state binding for `widgetSetHidden`.
///
/// Mango's pattern (and any procedurally-built Perry UI):
/// ```text
/// const formContainer = VStack(12, []);
/// widgetSetHidden(formContainer, 1);              // module-init: initial = hidden
/// // ...
/// btn.onClick = () => { widgetSetHidden(formContainer, 0); };  // closure: flip
/// ```
///
/// HarmonyOS has no runtime widget tree to mutate (no `perry-ui-harmonyos`
/// crate by design — ArkUI renders declaratively from `@State`). Pre-fix,
/// the closure-time `widgetSetHidden(formContainer, 0)` call was a no-op
/// (auto-stubbed in `perry-runtime/build.rs`); the form never appeared.
///
/// The fix: any widget that's targeted by `widgetSetHidden` from a closure
/// or function body gets a synth-id, an `@State hidden_<id>: boolean`
/// field on the page struct, and `.visibility(this.hidden_<id> ? Hidden :
/// Visible)` modifier. Closure-time calls are HIR-rewritten to
/// `perry_arkts_set_visibility(synth_id, hidden)` which pushes to a NAPI
/// drain queue; ArkTS pumps the queue and updates the `@State` field;
/// ArkUI re-renders.
#[derive(Debug, Clone)]
struct VisibilityBinding {
    /// Synth identifier — `vis_0`, `vis_1`, … one per unique target LocalId.
    synth_id: String,
    /// Initial visibility from module init: `true` = hidden by default.
    /// Determined by the LAST literal `widgetSetHidden(target, V)` seen in
    /// `module.init` (latest wins). When no module-init call is found,
    /// defaults to `false` (visible) — matches the fact that widgets are
    /// born visible in Perry.
    initial_hidden: bool,
}

/// Phase 2 v3.6 — tree-mutator state binding for view-builder functions.
///
/// Mango's pattern: a function called from a closure that builds a widget
/// tree and attaches it to a module-level container via `widgetAddChild(target,
/// root)`. On HarmonyOS those addChild + clearChildren calls are no-op stubs,
/// so the runtime construction is dead — but the *resulting widget tree* is
/// statically determinable. We lift it.
///
/// ```text
/// function showConnectionForm(): void {
///     widgetClearChildren(formContainer);
///     const formCard = VStack(12, []);
///     widgetAddChild(formCard, title);
///     ...
///     widgetAddChild(formContainer, formCard);  // ← terminal
/// }
/// // Caller:
/// const ctaBtn = Button('+', () => { showConnectionForm(); });
/// ```
///
/// The lift:
/// - allocates `@State contentView_<target_synth>: string = 'default'` on
///   the page struct
/// - runs the function body's mutations through `collect_mutations` with
///   a synthetic condition `this.contentView_<target_synth> === '<view_id>'`
/// - merges those into the main mutations map so the target's
///   `emit_widget` produces a conditional `if (cond) { … }` branch
/// - rewrites the closure call site to PREPEND a
///   `perry/arkts.setContentView(target_synth, view_id)` call (the
///   original call still runs to drive non-UI side effects)
/// - on click: closure pushes `(target_synth, view_id)` to the runtime
///   drain queue; ArkTS pumps via `drainContentViewUpdate` and assigns to
///   `@State contentView_<target_synth>`; ArkUI re-renders, picking up
///   the new branch.
#[derive(Debug, Clone)]
struct ViewBuilder {
    /// Function id (matches `Function::id` in the HIR).
    func_id: perry_types::FuncId,
    /// Function name (used for the synth view id when sanitized).
    func_name: String,
    /// Module-level container LocalId that this function adds children to
    /// via the terminal `widgetAddChild(LocalGet(target_id), X)` call.
    target_id: LocalId,
    /// Synth identifier for the target — `cv_<n>`. Stable across re-runs.
    target_synth: String,
    /// Synth view id for THIS function — sanitized function name. Used as
    /// both the `@State contentView_<target_synth>` field's expected value
    /// and the case-arm key in `applyContentViewUpdate`.
    view_id: String,
    /// Mutation group id used to keep this view's lifted addChild +
    /// modifier mutations grouped together in `fold_child_mutations`.
    group_id: u32,
}

/// Issue #408 — mutation tracking for procedurally-built UIs.
///
/// Many Perry apps build their widget tree imperatively after construction:
///
/// ```text
/// const toolbar = HStack(0, []);
/// widgetAddChild(toolbar, button1);
/// widgetAddChild(toolbar, button2);
/// setPadding(toolbar, 8, 12, 8, 12);
/// ```
///
/// The harvest model needs to fold these post-construction mutations into
/// the ArkUI emission so the resulting page actually renders the children
/// + applies the modifiers. The pre-walk records each mutator call against
/// its target widget local; `emit_widget` then merges them into the
/// emitted widget body / modifier chain.
///
/// Conditional mutations (mutators called inside `if`/`else` branches)
/// carry the enclosing condition so the emitted ArkUI can produce
/// `if (cond) { ChildA() } else { ChildB() }` blocks. Loop-conditional
/// mutations and unresolved-condition shapes degrade to a comment + skip.
#[derive(Debug, Clone)]
enum Mutation {
    /// `widgetAddChild(parent, child)` → child becomes a body child of parent.
    AddChild(Expr),
    /// `widgetClearChildren(parent)` → drop all earlier `AddChild` mutations
    /// recorded against this parent (preserves the chronological semantics).
    ClearChildren,
    /// `scrollviewSetChild(scroll, content)` → content becomes the Scroll's
    /// single child (replaces any previously set child or AddChild mutations).
    SetScrollChild(Expr),
    /// Pre-formatted ArkUI modifier chain entry, e.g. `.padding(8)`,
    /// `.backgroundColor('red')`, `.borderRadius(8)`. Concatenated to the
    /// widget core after construction.
    Modifier(String),
    /// An untraceable / unsupported mutator shape — emit a comment when this
    /// fires so the user can see the gap.
    Comment(String),
    /// Phase 2 v3.5 — leaf-mutator state binding for `widgetSetHidden`. When
    /// pre-walk detects a `widgetSetHidden(target, _)` call inside ANY
    /// function or closure body (i.e. the call fires at runtime, post-mount,
    /// not during the static module init harvest), the target widget gets
    /// a synth-id and the modifier `.visibility(this.hidden_<id> ? Hidden :
    /// Visible)` is emitted instead of the static `.visibility(Visibility.X)`
    /// the v0.5.480 module-init path produces. Closure-time calls then route
    /// through a NAPI drain queue (`perry_arkts_set_visibility`) which
    /// ArkTS pumps into the bound `@State hidden_<id>: boolean` field.
    VisibilityBinding(String),
    /// Issue #669 — `chartAddDataPoint(chart, label, value)` recorded
    /// against the chart's local. Folded into the static `data` array
    /// the chart's draw closure iterates over. Label resolves through
    /// bindings; value through `numeric_arg_resolved`.
    ChartAddDataPoint(String, f64),
    /// Issue #669 — `chartClearData(chart)` recorded against the chart.
    /// Wipes any earlier `ChartAddDataPoint` entries in the fold pass.
    ChartClearData,
    /// Issue #669 — `chartSetTitle(chart, title)`. Last-wins fold.
    ChartSetTitle(String),
    /// Issue #669 — `chartReload(chart)`. No-op at codegen time
    /// (ArkUI's Canvas redraws automatically when the build() runs);
    /// recorded so we don't flag it as an unrecognized mutator.
    ChartReload,
    /// Issue #670 — `treeNodeAddChild(parent, child)` recorded against
    /// the parent tree-node's local. Walked recursively by
    /// `build_tree_node_from_local` when emitting the TreeView.
    TreeAddChild(Expr),
}

/// A recorded mutation plus its enclosing condition, if any.
///
/// `condition` is `None` for unconditional mutations. When `Some((cond_key,
/// branch))`, the mutation belongs to the corresponding `if (...) { ... }`
/// branch where `cond_key` is a string-serialized condition expression
/// (used to group mutations from the same if statement) and `branch` is
/// `Then` for the then-branch or `Else` for the else-branch.
///
/// String-keying the condition lets us group related mutations even when
/// the condition expression is repeated in an HIR walk, without needing
/// expression-equality comparisons. The string is also used directly as
/// the emitted ArkUI `if (...)` predicate.
#[derive(Debug, Clone)]
struct MutationEntry {
    mutation: Mutation,
    condition: Option<MutationCondition>,
}

#[derive(Debug, Clone)]
struct MutationCondition {
    /// String-serialized condition expression; reused as the ArkUI
    /// predicate. e.g. `"this.text___state_0 === 'mobile'"`.
    cond_str: String,
    /// Which branch this mutation lives in.
    branch: Branch,
    /// Group key — same source-statement id, so all mutations from one
    /// `if` statement share a key and can be grouped at emit time.
    group: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Branch {
    Then,
    Else,
}

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
        let function_map_inline: HashMap<perry_types::FuncId, perry_hir::ir::Function> =
            module.functions.iter().map(|f| (f.id, f.clone())).collect();
        let function_lookup: HashMap<perry_types::FuncId, &perry_hir::ir::Function> =
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

/// Issue #369 — does this module use `perry/media`? Walks every statement
/// (init + every function body's statements) looking for any HIR
/// `Expr::NativeMethodCall { module: "perry/media", ... }`. Returns true
/// on first hit so the caller can opt the harvested .ets into the
/// `@ohos.multimedia.media` AVPlayer drain bridge.
fn module_uses_media(module: &Module) -> bool {
    fn stmts_use(stmts: &[Stmt]) -> bool {
        stmts.iter().any(stmt_uses)
    }
    fn stmt_uses(stmt: &Stmt) -> bool {
        match stmt {
            Stmt::Expr(e) | Stmt::Return(Some(e)) => expr_uses(e),
            Stmt::Let { init: Some(e), .. } => expr_uses(e),
            Stmt::If {
                condition,
                then_branch,
                else_branch,
                ..
            } => {
                expr_uses(condition)
                    || stmts_use(then_branch)
                    || else_branch.as_ref().map(|b| stmts_use(b)).unwrap_or(false)
            }
            Stmt::While {
                condition, body, ..
            }
            | Stmt::DoWhile {
                body, condition, ..
            } => expr_uses(condition) || stmts_use(body),
            Stmt::For {
                init,
                condition,
                update,
                body,
                ..
            } => {
                init.as_ref().map(|i| stmt_uses(i)).unwrap_or(false)
                    || condition.as_ref().map(expr_uses).unwrap_or(false)
                    || update.as_ref().map(expr_uses).unwrap_or(false)
                    || stmts_use(body)
            }
            _ => false,
        }
    }
    fn expr_uses(e: &Expr) -> bool {
        match e {
            Expr::NativeMethodCall {
                module: m, args, ..
            } => m == "perry/media" || args.iter().any(expr_uses),
            Expr::Call { callee, args, .. } => expr_uses(callee) || args.iter().any(expr_uses),
            Expr::Closure { body, .. } => stmts_use(body),
            Expr::Array(items) => items.iter().any(expr_uses),
            Expr::Object(fields) => fields.iter().any(|(_, v)| expr_uses(v)),
            _ => false,
        }
    }
    if stmts_use(&module.init) {
        return true;
    }
    for f in &module.functions {
        if stmts_use(&f.body) {
            return true;
        }
    }
    false
}

/// Phase 2 v6 — discover top-level `let x = state(initial)` declarations
/// and assign each a synthetic id `__state_<N>`. The initial value is
/// stringified for the v3.2 reactive-Text initial state.
/// Build an analysis-only copy of `module.init` with calls to user-defined
/// module-level functions expanded inline. The harvest's collectors
/// (collect_const_bindings, collect_mutations, collect_compile_time_consts)
/// run against this expanded view so widget mutations inside function
/// bodies are seen — but `module.init` itself stays untouched so the
/// downstream LLVM codegen sees the original program semantics.
///
/// Bounds: skip async/generator, ≤16 inlines per harvest call, skip
/// recursive calls. Param substitution lands as synthesized `Stmt::Let`
/// BEFORE the cloned body so collect_const_bindings picks them up the
/// same way as real top-level lets.
fn inlined_analysis_init(module: &Module) -> Vec<Stmt> {
    use perry_hir::analysis::remap_local_ids_in_stmts;
    use perry_types::FuncId;
    use std::collections::HashSet;

    let mut function_map: HashMap<FuncId, perry_hir::ir::Function> = HashMap::new();
    for f in &module.functions {
        if f.is_async || f.is_generator {
            continue;
        }
        function_map.insert(f.id, f.clone());
    }
    if function_map.is_empty() {
        return module.init.clone();
    }

    let mut next_local: u32 = max_local_id_in_module(module).saturating_add(1);
    // Inline budget — bumped from 32 → 256 to handle Mango's full
    // refreshConnectionList (which transitively expands to dozens of
    // makePill / makeLabel / makeMuted / makeCard / makeDangerBtn /
    // makePrimaryBtn calls). Each inline operation is bounded; the
    // overall HIR size is a hard upper bound on how many calls can
    // possibly land. 256 is comfortably above the worst-case Mango
    // shape (verified at v0.5.491: ~25K bytes Index.ets, ~40 inlines).
    let mut budget: usize = 256;
    let mut visited: HashSet<FuncId> = HashSet::new();

    // Phase A: top-level `Stmt::Expr(Call(FuncRef))` inlining (the
    // original v0.5.489 behavior). For each top-level user-function
    // call, splice the body in place of the call statement.
    let mut new_init: Vec<Stmt> = Vec::with_capacity(module.init.len());
    for stmt in &module.init {
        if budget == 0 {
            new_init.push(stmt.clone());
            continue;
        }
        match stmt {
            Stmt::Expr(Expr::Call { callee, args, .. }) => {
                let func_id = match callee.as_ref() {
                    Expr::FuncRef(id) => Some(*id),
                    _ => None,
                };
                let Some(id) = func_id else {
                    new_init.push(stmt.clone());
                    continue;
                };
                if visited.contains(&id) {
                    new_init.push(stmt.clone());
                    continue;
                }
                let Some(func) = function_map.get(&id) else {
                    new_init.push(stmt.clone());
                    continue;
                };
                if func.params.len() != args.len() {
                    new_init.push(stmt.clone());
                    continue;
                }
                visited.insert(id);
                let inlined =
                    inline_one_call(func, args, &mut next_local, &remap_local_ids_in_stmts);
                visited.remove(&id);
                new_init.extend(inlined);
                budget -= 1;
            }
            _ => new_init.push(stmt.clone()),
        }
    }

    // Phase B: expression-level inlining inside the inlined bodies.
    // Walks every Stmt's expressions and substitutes:
    //   - `Expr::Call { callee: FuncRef(id) }` (top-level fn call)
    //   - `Expr::Call { callee: LocalGet(id) }` where bindings[id]
    //     is `Expr::Closure { ... }` (Mango's `function makePill`
    //     nested inside refreshConnectionList lowers to this shape:
    //     `Stmt::Let { id: 297, init: Closure { ... } }`)
    // with the function's return value, hoisting the body's let-and-
    // mutator statements BEFORE the enclosing Stmt.
    // Mango's pattern: `const pillRow = HStack(8, [makePill('A'),
    // makePill('B')])` — each makePill call's body gets hoisted, and
    // the call expression is replaced with `LocalGet(remapped_pill)`.
    // Both calls run before pillRow is constructed, so the array
    // literal's items resolve cleanly.
    let local_bindings_for_inline = collect_const_bindings(&new_init);
    new_init = expr_level_inline_pass(
        new_init,
        &function_map,
        &local_bindings_for_inline,
        &mut next_local,
        &mut budget,
    );

    new_init
}

/// Phase B of inlining: walk every Stmt's expressions and substitute
/// `Expr::Call { callee: FuncRef(id) }` with the function's return
/// value, hoisting the function body's statements BEFORE the enclosing
/// Stmt. Each found Call gets its body inlined (with locals remapped
/// to fresh ids) and the call expression is replaced with `LocalGet(
/// remapped_return_id)`.
///
/// The function's return value is detected by walking its body
/// backwards looking for `Stmt::Return(Some(expr))`. If the expr is
/// `Expr::LocalGet(id)`, that local id is the return target (after
/// remapping). If the body has no `Return` or returns a non-LocalGet
/// expression, the call is left as-is — the simple-shape constraint
/// covers Mango's makePill but punts on more complex returning fns.
fn expr_level_inline_pass(
    stmts: Vec<Stmt>,
    function_map: &HashMap<perry_types::FuncId, perry_hir::ir::Function>,
    bindings: &HashMap<LocalId, Expr>,
    next_local: &mut u32,
    budget: &mut usize,
) -> Vec<Stmt> {
    let mut out: Vec<Stmt> = Vec::with_capacity(stmts.len());
    for mut stmt in stmts {
        if *budget == 0 {
            out.push(stmt);
            continue;
        }
        let mut hoists: Vec<Stmt> = Vec::new();
        inline_calls_in_stmt(
            &mut stmt,
            function_map,
            bindings,
            next_local,
            budget,
            &mut hoists,
        );
        out.extend(hoists);
        out.push(stmt);
    }
    out
}

fn inline_calls_in_stmt(
    stmt: &mut Stmt,
    function_map: &HashMap<perry_types::FuncId, perry_hir::ir::Function>,
    bindings: &HashMap<LocalId, Expr>,
    next_local: &mut u32,
    budget: &mut usize,
    hoists: &mut Vec<Stmt>,
) {
    match stmt {
        Stmt::Let { init: Some(e), .. } => {
            inline_calls_in_expr(e, function_map, bindings, next_local, budget, hoists);
        }
        Stmt::Expr(e) => {
            inline_calls_in_expr(e, function_map, bindings, next_local, budget, hoists)
        }
        Stmt::Return(Some(e)) => {
            inline_calls_in_expr(e, function_map, bindings, next_local, budget, hoists)
        }
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            inline_calls_in_expr(
                condition,
                function_map,
                bindings,
                next_local,
                budget,
                hoists,
            );
            // Recurse into branches: their own hoists land within the
            // branch, not above the if.
            *then_branch = expr_level_inline_pass(
                std::mem::take(then_branch),
                function_map,
                bindings,
                next_local,
                budget,
            );
            if let Some(eb) = else_branch {
                *eb = expr_level_inline_pass(
                    std::mem::take(eb),
                    function_map,
                    bindings,
                    next_local,
                    budget,
                );
            }
        }
        _ => {}
    }
}

fn inline_calls_in_expr(
    expr: &mut Expr,
    function_map: &HashMap<perry_types::FuncId, perry_hir::ir::Function>,
    bindings: &HashMap<LocalId, Expr>,
    next_local: &mut u32,
    budget: &mut usize,
    hoists: &mut Vec<Stmt>,
) {
    use perry_hir::analysis::remap_local_ids_in_stmts;
    // First descend into sub-expressions (post-order: children inlined
    // first so a call's args might themselves be inlined calls).
    match expr {
        Expr::Call { callee, args, .. } => {
            for a in args.iter_mut() {
                inline_calls_in_expr(a, function_map, bindings, next_local, budget, hoists);
            }
            inline_calls_in_expr(callee, function_map, bindings, next_local, budget, hoists);
        }
        Expr::NativeMethodCall { args, object, .. } => {
            for a in args.iter_mut() {
                inline_calls_in_expr(a, function_map, bindings, next_local, budget, hoists);
            }
            if let Some(obj) = object {
                inline_calls_in_expr(obj, function_map, bindings, next_local, budget, hoists);
            }
        }
        Expr::Array(items) => {
            for item in items.iter_mut() {
                inline_calls_in_expr(item, function_map, bindings, next_local, budget, hoists);
            }
        }
        Expr::Object(props) => {
            for (_, v) in props.iter_mut() {
                inline_calls_in_expr(v, function_map, bindings, next_local, budget, hoists);
            }
        }
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            inline_calls_in_expr(
                condition,
                function_map,
                bindings,
                next_local,
                budget,
                hoists,
            );
            inline_calls_in_expr(
                then_expr,
                function_map,
                bindings,
                next_local,
                budget,
                hoists,
            );
            inline_calls_in_expr(
                else_expr,
                function_map,
                bindings,
                next_local,
                budget,
                hoists,
            );
        }
        _ => {}
    }
    // Now check if THIS expression is a Call we can inline. Resolve
    // the callee through:
    //   - Expr::FuncRef(id): module-level user function in function_map
    //   - Expr::LocalGet(id) → bindings[id] = Expr::Closure: nested
    //     function declared via `function name() { ... }` inside
    //     another function (Mango's `makePill` shape — lowered to
    //     `Stmt::Let { id, init: Closure { params, body, ... } }`).
    if *budget == 0 {
        return;
    }
    let (params, body, args) = match expr {
        Expr::Call { callee, args, .. } => match callee.as_ref() {
            Expr::FuncRef(id) => match function_map.get(id) {
                Some(func) if func.params.len() == args.len() => {
                    (func.params.clone(), func.body.clone(), args.clone())
                }
                _ => return,
            },
            Expr::LocalGet(local_id) => match bindings.get(local_id) {
                Some(Expr::Closure {
                    params,
                    body,
                    is_async: false,
                    ..
                }) if params.len() == args.len() => (params.clone(), body.clone(), args.clone()),
                _ => return,
            },
            _ => return,
        },
        _ => return,
    };
    // Detect simple-return shape: last `Stmt::Return(Some(LocalGet(id)))`.
    // Anything else (no return, return non-local, multiple returns) is
    // out of scope — leaves the call as-is.
    if !matches!(body.last(), Some(Stmt::Return(Some(Expr::LocalGet(_))))) {
        return;
    }
    // Build a synthetic Function so `inline_one_call` can do its
    // standard remapping work. Re-uses the existing helper rather
    // than duplicating the local-id offset / param-substitution logic.
    let synth_func = perry_hir::ir::Function {
        id: 0,
        name: String::new(),
        type_params: Vec::new(),
        params,
        return_type: perry_types::Type::Any,
        body,
        is_async: false,
        is_generator: false,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
        was_plain_async: false,
        was_unrolled: false,
    };
    let mut inlined = inline_one_call(&synth_func, &args, next_local, &remap_local_ids_in_stmts);
    let return_remapped = match inlined.last() {
        Some(Stmt::Return(Some(Expr::LocalGet(id)))) => *id,
        _ => return,
    };
    inlined.pop(); // drop the trailing Return
    hoists.extend(inlined);
    *expr = Expr::LocalGet(return_remapped);
    *budget -= 1;
}

fn inline_one_call(
    func: &perry_hir::ir::Function,
    call_args: &[Expr],
    next_local: &mut u32,
    remap_fn: &dyn Fn(&mut Vec<Stmt>, &HashMap<u32, u32>),
) -> Vec<Stmt> {
    let mut local_ids: Vec<u32> = Vec::new();
    for param in &func.params {
        local_ids.push(param.id);
    }
    collect_local_ids_in_stmts(&func.body, &mut local_ids);
    let mut seen: std::collections::HashSet<u32> = std::collections::HashSet::new();
    local_ids.retain(|id| seen.insert(*id));

    let mut remap: HashMap<u32, u32> = HashMap::new();
    for &id in &local_ids {
        remap.insert(id, *next_local);
        *next_local += 1;
    }

    // Rewrite early-return patterns to if/else before remapping. Mango's
    // `refreshConnectionList`:
    //
    //     if (connectionNames.length === 0) {
    //         /* welcome card */
    //         widgetAddChild(connListContainer, welcomeCard);
    //         return;
    //     }
    //     const sectionTitle = ...;
    //     widgetAddChild(connListContainer, sectionTitle);
    //     /* connection-list build */
    //     widgetAddChild(connListContainer, addMoreBtn);
    //
    // After rewrite the rest of the body becomes the else branch:
    //
    //     if (connectionNames.length === 0) {
    //         /* welcome card */
    //         widgetAddChild(connListContainer, welcomeCard);
    //     } else {
    //         const sectionTitle = ...;
    //         /* ... */
    //         widgetAddChild(connListContainer, addMoreBtn);
    //     }
    //
    // collect_mutations's dead-branch elim then picks ONE branch (the
    // then-branch heuristic when the condition is unfoldable / unclean
    // serializable). Without this rewrite both the welcomeCard's CTA
    // button AND the addMoreBtn would render unconditionally because
    // the addMoreBtn lives at the function-body sibling level rather
    // than inside an else block.
    let mut body = rewrite_early_returns(func.body.clone());
    remap_fn(&mut body, &remap);
    // perry-hir's `remap_local_ids_in_stmts` only walks Stmt::Let's init,
    // not the Let's `id` field itself (its #212 design constraint:
    // outer-scope captured ids shouldn't get rewritten when remapping
    // inner-scope refs). We need both — the Let creates a new binding
    // and the LocalGets that reference it must agree. Walk the inlined
    // body once more and rewrite any Stmt::Let / catch-param / for-init
    // ids that match the remap.
    remap_let_ids_in_stmts(&mut body, &remap);

    let mut out: Vec<Stmt> = Vec::with_capacity(func.params.len() + body.len());
    for (i, param) in func.params.iter().enumerate() {
        let new_id = remap[&param.id];
        out.push(Stmt::Let {
            id: new_id,
            name: param.name.clone(),
            ty: param.ty.clone(),
            mutable: false,
            init: Some(call_args[i].clone()),
        });
    }
    out.extend(body);
    out
}

fn max_local_id_in_module(module: &Module) -> u32 {
    let mut buf: Vec<u32> = Vec::new();
    collect_local_ids_in_stmts(&module.init, &mut buf);
    for f in &module.functions {
        for p in &f.params {
            buf.push(p.id);
        }
        collect_local_ids_in_stmts(&f.body, &mut buf);
    }
    for c in &module.classes {
        if let Some(ctor) = &c.constructor {
            for p in &ctor.params {
                buf.push(p.id);
            }
            collect_local_ids_in_stmts(&ctor.body, &mut buf);
        }
        for m in &c.methods {
            for p in &m.params {
                buf.push(p.id);
            }
            collect_local_ids_in_stmts(&m.body, &mut buf);
        }
    }
    buf.into_iter().max().unwrap_or(0)
}

/// Rewrite `if (cond) { ...; return; } <rest>` → `if (cond) { ... }
/// else { <rest> }` so dead-branch elim can correctly drop one or the
/// other. Stops walking after the first such pattern (the rest moved
/// into the else). Recurses into nested if/else / for / while bodies.
fn rewrite_early_returns(stmts: Vec<Stmt>) -> Vec<Stmt> {
    let mut out: Vec<Stmt> = Vec::with_capacity(stmts.len());
    let mut iter = stmts.into_iter();
    while let Some(stmt) = iter.next() {
        match stmt {
            Stmt::If {
                condition,
                then_branch,
                else_branch: None,
            } if matches!(then_branch.last(), Some(Stmt::Return(_))) => {
                // Found the early-return pattern. Pull the trailing
                // return out of the then-branch (it's redundant once
                // the rest is in the else), and gather all remaining
                // siblings into a new else-branch. Recurse into both
                // branches so nested patterns are handled too.
                let mut new_then = rewrite_early_returns(then_branch);
                new_then.pop(); // drop the trailing Return
                let rest: Vec<Stmt> = iter.collect();
                let new_else = rewrite_early_returns(rest);
                out.push(Stmt::If {
                    condition,
                    then_branch: new_then,
                    else_branch: Some(new_else),
                });
                return out;
            }
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                let new_then = rewrite_early_returns(then_branch);
                let new_else = else_branch.map(rewrite_early_returns);
                out.push(Stmt::If {
                    condition,
                    then_branch: new_then,
                    else_branch: new_else,
                });
            }
            Stmt::While { condition, body } => {
                out.push(Stmt::While {
                    condition,
                    body: rewrite_early_returns(body),
                });
            }
            Stmt::DoWhile { body, condition } => {
                out.push(Stmt::DoWhile {
                    body: rewrite_early_returns(body),
                    condition,
                });
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                out.push(Stmt::For {
                    init,
                    condition,
                    update,
                    body: rewrite_early_returns(body),
                });
            }
            other => out.push(other),
        }
    }
    out
}

/// Walk Stmt::Let / catch-param / Stmt::For-init looking for declared
/// local ids that match the remap; rewrite them in place. Sibling to
/// `perry_hir::analysis::remap_local_ids_in_stmts` which only remaps
/// LocalGet / LocalSet / Update references — not the declarations.
/// The inliner needs both: the cloned body's let creates a new binding
/// and the references to it must agree.
fn remap_let_ids_in_stmts(stmts: &mut Vec<Stmt>, remap: &HashMap<u32, u32>) {
    for s in stmts.iter_mut() {
        remap_let_ids_in_stmt(s, remap);
    }
}

fn remap_let_ids_in_stmt(stmt: &mut Stmt, remap: &HashMap<u32, u32>) {
    match stmt {
        Stmt::Let { id, .. } => {
            if let Some(&new_id) = remap.get(id) {
                *id = new_id;
            }
        }
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            remap_let_ids_in_stmts(then_branch, remap);
            if let Some(eb) = else_branch {
                remap_let_ids_in_stmts(eb, remap);
            }
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
            remap_let_ids_in_stmts(body, remap);
        }
        Stmt::For { init, body, .. } => {
            if let Some(init_stmt) = init {
                remap_let_ids_in_stmt(init_stmt.as_mut(), remap);
            }
            remap_let_ids_in_stmts(body, remap);
        }
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            remap_let_ids_in_stmts(body, remap);
            if let Some(c) = catch {
                if let Some((id, _)) = &mut c.param {
                    if let Some(&new_id) = remap.get(id) {
                        *id = new_id;
                    }
                }
                remap_let_ids_in_stmts(&mut c.body, remap);
            }
            if let Some(f) = finally {
                remap_let_ids_in_stmts(f, remap);
            }
        }
        _ => {}
    }
}

fn collect_local_ids_in_stmts(stmts: &[Stmt], out: &mut Vec<u32>) {
    for s in stmts {
        match s {
            Stmt::Let { id, .. } => out.push(*id),
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                collect_local_ids_in_stmts(then_branch, out);
                if let Some(eb) = else_branch {
                    collect_local_ids_in_stmts(eb, out);
                }
            }
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
                collect_local_ids_in_stmts(body, out);
            }
            Stmt::For { init, body, .. } => {
                if let Some(init_stmt) = init {
                    if let Stmt::Let { id, .. } = init_stmt.as_ref() {
                        out.push(*id);
                    }
                }
                collect_local_ids_in_stmts(body, out);
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                collect_local_ids_in_stmts(body, out);
                if let Some(c) = catch {
                    if let Some((id, _)) = &c.param {
                        out.push(*id);
                    }
                    collect_local_ids_in_stmts(&c.body, out);
                }
                if let Some(f) = finally {
                    collect_local_ids_in_stmts(f, out);
                }
            }
            _ => {}
        }
    }
}

fn collect_state_bindings(init: &[Stmt]) -> HashMap<LocalId, StateBinding> {
    let mut map = HashMap::new();
    let mut counter: usize = 0;
    for stmt in init {
        if let Stmt::Let {
            id,
            init: Some(call_expr),
            ..
        } = stmt
        {
            let initial = match call_expr {
                // Match either `Expr::NativeMethodCall { module: "perry/ui", method: "state", args: [v] }`
                // OR `Expr::Call { callee: Ident("state"), args: [v] }` (whichever
                // shape the perry-hir lowerer produces for the import).
                Expr::NativeMethodCall {
                    module,
                    method,
                    object: None,
                    args,
                    ..
                } if module == "perry/ui" && method == "state" && args.len() == 1 => {
                    Some(args[0].clone())
                }
                _ => None,
            };
            if let Some(initial_expr) = initial {
                let synth_id = format!("__state_{}", counter);
                counter += 1;
                let initial_str = match &initial_expr {
                    Expr::String(s) => s.clone(),
                    Expr::Number(n) => fmt_num(*n),
                    Expr::Integer(n) => format!("{}", n),
                    Expr::Bool(b) => format!("{}", b),
                    _ => "".to_string(),
                };
                map.insert(
                    *id,
                    StateBinding {
                        synth_id,
                        initial_str,
                    },
                );
            }
        }
    }
    map
}

/// Phase 2 v3.5 — pre-walk for `widgetSetHidden(LocalGet(target), _)` calls
/// across the ENTIRE module (init + every function body + every closure body
/// recursively). Targets that get touched in any non-init scope earn a
/// `VisibilityBinding` with a synth-id; the harvest then emits a bound
/// `.visibility(this.hidden_<id> ? Hidden : Visible)` modifier on the
/// widget instead of the static `.visibility(Visibility.X)` it would
/// otherwise produce, AND the closure-body call sites are HIR-rewritten to
/// route through the NAPI drain queue.
///
/// Initial value: walks `module.init` only, picking the LAST literal
/// `widgetSetHidden(target, V)` it finds. Latest-wins matches Mango's
/// pattern where the file might call `widgetSetHidden(formContainer, 0)`
/// then `widgetSetHidden(formContainer, 1)` at module-init top-level (the
/// second is the actual initial state). Non-literal init values fall
/// through to `false` (visible) — same default as widgets in general.
fn collect_visibility_bindings(module: &Module) -> HashMap<LocalId, VisibilityBinding> {
    let mut map: HashMap<LocalId, VisibilityBinding> = HashMap::new();
    let mut counter: usize = 0;

    // Pass 1 — discover all targets reached from runtime call paths only.
    // Module-init TOP-LEVEL `widgetSetHidden(target, V)` calls stay static
    // (the v0.5.480 collect_mutations path emits `.visibility(Visibility.X)`
    // directly). A target earns a binding only when there's a call site
    // that fires AT RUNTIME — i.e. inside a function body that's invoked
    // post-mount, or inside a closure (anywhere). Module-init init-time
    // calls are out of scope by design.
    let mut targets: std::collections::BTreeSet<LocalId> = std::collections::BTreeSet::new();
    walk_init_for_closure_targets(&module.init, &mut targets);
    for f in &module.functions {
        walk_for_set_hidden_targets_in_stmts(&f.body, &mut targets);
    }

    // Stable synth-id assignment by sorted LocalId (BTreeSet iteration is
    // ordered) so re-running the harvest produces the same .ets bytes.
    for target_id in &targets {
        let synth_id = format!("vis_{}", counter);
        counter += 1;
        map.insert(
            *target_id,
            VisibilityBinding {
                synth_id,
                initial_hidden: false, // overwritten below if a literal init call is found
            },
        );
    }

    // Pass 2 — walk module.init only for initial value detection.
    // Only top-level Stmt::Expr is considered; nested if/loop init values
    // intentionally skip (the runtime branch only fires after main()
    // returns control to ArkUI in the harvest model anyway).
    for stmt in &module.init {
        if let Stmt::Expr(e) = stmt {
            if let Some((target_id, hide)) = extract_widget_set_hidden_literal(e) {
                if let Some(binding) = map.get_mut(&target_id) {
                    binding.initial_hidden = hide;
                }
            }
        }
    }

    map
}

fn walk_for_set_hidden_targets_in_stmts(
    stmts: &[Stmt],
    out: &mut std::collections::BTreeSet<LocalId>,
) {
    for stmt in stmts {
        walk_for_set_hidden_targets_in_stmt(stmt, out);
    }
}

/// Variant for walking module.init: descends through nested control flow
/// + sub-exprs WITHOUT recording any widgetSetHidden targets it sees at
/// the outer scope, but when it encounters an `Expr::Closure`, switches
/// to the unrestricted target-recording walker for the closure body. This
/// is what makes "module-init top-level widgetSetHidden stays static" work
/// while "widgetSetHidden inside an onClick closure earns a binding"
/// also work — same module, different scope.
fn walk_init_for_closure_targets(stmts: &[Stmt], out: &mut std::collections::BTreeSet<LocalId>) {
    for stmt in stmts {
        walk_init_for_closure_targets_in_stmt(stmt, out);
    }
}

fn walk_init_for_closure_targets_in_stmt(
    stmt: &Stmt,
    out: &mut std::collections::BTreeSet<LocalId>,
) {
    match stmt {
        Stmt::Expr(e) | Stmt::Let { init: Some(e), .. } | Stmt::Return(Some(e)) => {
            walk_init_for_closure_targets_in_expr(e, out);
        }
        Stmt::If {
            condition,
            then_branch,
            else_branch,
            ..
        } => {
            walk_init_for_closure_targets_in_expr(condition, out);
            walk_init_for_closure_targets(then_branch, out);
            if let Some(eb) = else_branch {
                walk_init_for_closure_targets(eb, out);
            }
        }
        Stmt::While {
            condition, body, ..
        }
        | Stmt::DoWhile {
            body, condition, ..
        } => {
            walk_init_for_closure_targets_in_expr(condition, out);
            walk_init_for_closure_targets(body, out);
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
            ..
        } => {
            if let Some(i) = init {
                walk_init_for_closure_targets_in_stmt(i.as_ref(), out);
            }
            if let Some(c) = condition {
                walk_init_for_closure_targets_in_expr(c, out);
            }
            if let Some(u) = update {
                walk_init_for_closure_targets_in_expr(u, out);
            }
            walk_init_for_closure_targets(body, out);
        }
        _ => {}
    }
}

fn walk_init_for_closure_targets_in_expr(e: &Expr, out: &mut std::collections::BTreeSet<LocalId>) {
    // The whole point: a Closure body switches to the unrestricted walker.
    if let Expr::Closure { body, .. } = e {
        walk_for_set_hidden_targets_in_stmts(body, out);
        return;
    }
    // Otherwise descend without recording.
    match e {
        Expr::Call { callee, args, .. } => {
            walk_init_for_closure_targets_in_expr(callee, out);
            for a in args {
                walk_init_for_closure_targets_in_expr(a, out);
            }
        }
        Expr::NativeMethodCall { object, args, .. } => {
            if let Some(o) = object {
                walk_init_for_closure_targets_in_expr(o, out);
            }
            for a in args {
                walk_init_for_closure_targets_in_expr(a, out);
            }
        }
        Expr::PropertyGet { object, .. } => {
            walk_init_for_closure_targets_in_expr(object, out);
        }
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            walk_init_for_closure_targets_in_expr(condition, out);
            walk_init_for_closure_targets_in_expr(then_expr, out);
            walk_init_for_closure_targets_in_expr(else_expr, out);
        }
        Expr::Binary { left, right, .. } | Expr::Logical { left, right, .. } => {
            walk_init_for_closure_targets_in_expr(left, out);
            walk_init_for_closure_targets_in_expr(right, out);
        }
        Expr::Unary { operand, .. } => {
            walk_init_for_closure_targets_in_expr(operand, out);
        }
        Expr::Array(items) => {
            for i in items {
                walk_init_for_closure_targets_in_expr(i, out);
            }
        }
        Expr::Object(props) => {
            for (_, v) in props {
                walk_init_for_closure_targets_in_expr(v, out);
            }
        }
        Expr::New { args, .. } => {
            for a in args {
                walk_init_for_closure_targets_in_expr(a, out);
            }
        }
        _ => {}
    }
}

fn walk_for_set_hidden_targets_in_stmt(stmt: &Stmt, out: &mut std::collections::BTreeSet<LocalId>) {
    match stmt {
        Stmt::Expr(e) | Stmt::Let { init: Some(e), .. } | Stmt::Return(Some(e)) => {
            walk_for_set_hidden_targets_in_expr(e, out);
        }
        Stmt::If {
            condition,
            then_branch,
            else_branch,
            ..
        } => {
            walk_for_set_hidden_targets_in_expr(condition, out);
            walk_for_set_hidden_targets_in_stmts(then_branch, out);
            if let Some(eb) = else_branch {
                walk_for_set_hidden_targets_in_stmts(eb, out);
            }
        }
        Stmt::While {
            condition, body, ..
        }
        | Stmt::DoWhile {
            body, condition, ..
        } => {
            walk_for_set_hidden_targets_in_expr(condition, out);
            walk_for_set_hidden_targets_in_stmts(body, out);
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
            ..
        } => {
            if let Some(i) = init {
                walk_for_set_hidden_targets_in_stmt(i.as_ref(), out);
            }
            if let Some(c) = condition {
                walk_for_set_hidden_targets_in_expr(c, out);
            }
            if let Some(u) = update {
                walk_for_set_hidden_targets_in_expr(u, out);
            }
            walk_for_set_hidden_targets_in_stmts(body, out);
        }
        _ => {}
    }
}

fn walk_for_set_hidden_targets_in_expr(e: &Expr, out: &mut std::collections::BTreeSet<LocalId>) {
    // Detect `widgetSetHidden(LocalGet(target), _)` shape first.
    if let Some((target, _)) =
        extract_widget_set_hidden_literal(e).or_else(|| extract_widget_set_hidden_target(e))
    {
        out.insert(target);
    }
    // Recurse into all sub-expressions so closures, nested calls, etc.
    // contribute their targets too.
    match e {
        Expr::Closure { body, .. } => {
            walk_for_set_hidden_targets_in_stmts(body, out);
        }
        Expr::Call { callee, args, .. } => {
            walk_for_set_hidden_targets_in_expr(callee, out);
            for a in args {
                walk_for_set_hidden_targets_in_expr(a, out);
            }
        }
        Expr::NativeMethodCall { object, args, .. } => {
            if let Some(o) = object {
                walk_for_set_hidden_targets_in_expr(o, out);
            }
            for a in args {
                walk_for_set_hidden_targets_in_expr(a, out);
            }
        }
        Expr::PropertyGet { object, .. } => {
            walk_for_set_hidden_targets_in_expr(object, out);
        }
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            walk_for_set_hidden_targets_in_expr(condition, out);
            walk_for_set_hidden_targets_in_expr(then_expr, out);
            walk_for_set_hidden_targets_in_expr(else_expr, out);
        }
        Expr::Binary { left, right, .. } | Expr::Logical { left, right, .. } => {
            walk_for_set_hidden_targets_in_expr(left, out);
            walk_for_set_hidden_targets_in_expr(right, out);
        }
        Expr::Unary { operand, .. } => {
            walk_for_set_hidden_targets_in_expr(operand, out);
        }
        Expr::Array(items) => {
            for i in items {
                walk_for_set_hidden_targets_in_expr(i, out);
            }
        }
        Expr::Object(props) => {
            for (_, v) in props {
                walk_for_set_hidden_targets_in_expr(v, out);
            }
        }
        Expr::New { args, .. } => {
            for a in args {
                walk_for_set_hidden_targets_in_expr(a, out);
            }
        }
        _ => {}
    }
}

/// Recognize `widgetSetHidden(LocalGet(target), V)` where V is a literal,
/// returning `(target_id, hide)`. Both the perry/ui native-method shape
/// AND the bare-call shape from non-typed-import paths are accepted.
fn extract_widget_set_hidden_literal(e: &Expr) -> Option<(LocalId, bool)> {
    let (target, val) = extract_widget_set_hidden_call(e)?;
    let hide = match val {
        Expr::Bool(true) => true,
        Expr::Bool(false) => false,
        Expr::Number(n) => *n != 0.0,
        Expr::Integer(n) => *n != 0,
        _ => return None,
    };
    Some((target, hide))
}

/// Recognize `widgetSetHidden(LocalGet(target), _)` — target only, no
/// requirement on the value being a literal. Used in target collection.
fn extract_widget_set_hidden_target(e: &Expr) -> Option<(LocalId, bool)> {
    let (target, _) = extract_widget_set_hidden_call(e)?;
    Some((target, false))
}

/// Phase 2 v3.6 — pre-walk that returns one `ViewBuilder` per function
/// matching the view-builder pattern: at least one
/// `widgetAddChild(LocalGet(target), X)` call where `target` is a
/// MODULE-LEVEL `let X = widget` declaration AND the function is invoked
/// from at least one `Expr::Closure` body anywhere in the module.
///
/// Functions called only from `module.init` (e.g. Mango's
/// `refreshConnectionList()` at the top-level) are EXCLUDED — they're
/// already inlined by `inlined_analysis_init`'s Phase A and the result
/// becomes module-init mutations directly. Lifting them as conditional
/// branches would emit duplicate content.
///
/// Functions called from BOTH module-init AND closures are also excluded
/// from this pass for now (the duplicate-emit hazard would require a
/// more involved merge); they fall back to v0.5.489 inlining (renders
/// the initial state but doesn't update on tap). Tracked as a follow-up.
fn collect_view_builders(module: &Module, next_group_id: &mut u32) -> Vec<ViewBuilder> {
    use std::collections::HashSet;

    // Pass 1 — collect every module-level `let X = ...` LocalId.
    let mut module_level_locals: HashSet<LocalId> = HashSet::new();
    for stmt in &module.init {
        if let Stmt::Let { id, .. } = stmt {
            module_level_locals.insert(*id);
        }
    }

    // Pass 2 — collect every function's `widgetAddChild(LocalGet(id), _)`
    // targets that are module-level. Pick the function's "primary target"
    // as the first matching one (Mango's pattern is one terminal target
    // per view-builder; multi-target view-builders aren't supported yet).
    let mut primary_target: HashMap<perry_types::FuncId, LocalId> = HashMap::new();
    for f in &module.functions {
        if f.is_async || f.is_generator {
            continue;
        }
        let mut found: Option<LocalId> = None;
        scan_module_level_addchild(&f.body, &module_level_locals, &mut found);
        if let Some(target) = found {
            primary_target.insert(f.id, target);
        }
    }
    if primary_target.is_empty() {
        return Vec::new();
    }

    // Pass 3 — find functions called from any `Expr::Closure` body
    // anywhere in the module (closures in module.init AND inside other
    // function bodies). Calls inside top-level Stmts of module.init or
    // function bodies that are NOT inside a closure don't count.
    let mut called_from_closure: HashSet<perry_types::FuncId> = HashSet::new();
    walk_for_funcref_calls_in_closures_in_stmts(&module.init, &mut called_from_closure);
    for f in &module.functions {
        walk_for_funcref_calls_in_closures_in_stmts(&f.body, &mut called_from_closure);
    }

    // Pass 4 — find functions called from module.init OR from a function
    // that's itself called from module.init. Used to EXCLUDE module-init
    // call paths from view-builder treatment (avoids duplicate emit).
    let mut called_from_module_init: HashSet<perry_types::FuncId> = HashSet::new();
    walk_for_funcref_calls_top_level_in_stmts(&module.init, &mut called_from_module_init);

    // Pass 5 — assemble ViewBuilders. Stable target_synth assignment by
    // sorted target LocalId so re-runs produce the same output.
    let mut target_synth_for: HashMap<LocalId, String> = HashMap::new();
    let mut next_target_synth: usize = 0;

    let mut builders: Vec<ViewBuilder> = Vec::new();
    let function_lookup: HashMap<perry_types::FuncId, &perry_hir::ir::Function> =
        module.functions.iter().map(|f| (f.id, f)).collect();
    let mut sorted_func_ids: Vec<perry_types::FuncId> = primary_target.keys().copied().collect();
    sorted_func_ids.sort();
    for func_id in sorted_func_ids {
        if !called_from_closure.contains(&func_id) {
            continue;
        }
        if called_from_module_init.contains(&func_id) {
            // Mixed call sites — defer.
            continue;
        }
        let target_id = primary_target[&func_id];
        let target_synth = target_synth_for
            .entry(target_id)
            .or_insert_with(|| {
                let synth = format!("cv_{}", next_target_synth);
                next_target_synth += 1;
                synth
            })
            .clone();
        let func_name = function_lookup
            .get(&func_id)
            .map(|f| f.name.clone())
            .unwrap_or_else(|| format!("fn_{}", func_id));
        let view_id = sanitize_view_id(&func_name);
        let group_id = *next_group_id;
        *next_group_id += 1;
        builders.push(ViewBuilder {
            func_id,
            func_name: func_name.clone(),
            target_id,
            target_synth,
            view_id,
            group_id,
        });
    }
    builders
}

fn sanitize_view_id(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if c.is_ascii_alphanumeric() || c == '_' {
            out.push(c);
        } else {
            out.push('_');
        }
    }
    if out.is_empty() {
        out.push_str("view");
    }
    out
}

fn scan_module_level_addchild(
    stmts: &[Stmt],
    module_locals: &std::collections::HashSet<LocalId>,
    found: &mut Option<LocalId>,
) {
    for stmt in stmts {
        scan_module_level_addchild_in_stmt(stmt, module_locals, found);
    }
}

fn scan_module_level_addchild_in_stmt(
    stmt: &Stmt,
    module_locals: &std::collections::HashSet<LocalId>,
    found: &mut Option<LocalId>,
) {
    if found.is_some() {
        return;
    }
    match stmt {
        Stmt::Expr(e) | Stmt::Let { init: Some(e), .. } | Stmt::Return(Some(e)) => {
            scan_module_level_addchild_in_expr(e, module_locals, found);
        }
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            scan_module_level_addchild(then_branch, module_locals, found);
            if let Some(eb) = else_branch {
                scan_module_level_addchild(eb, module_locals, found);
            }
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
            scan_module_level_addchild(body, module_locals, found);
        }
        Stmt::For { body, .. } => {
            scan_module_level_addchild(body, module_locals, found);
        }
        _ => {}
    }
}

fn scan_module_level_addchild_in_expr(
    e: &Expr,
    module_locals: &std::collections::HashSet<LocalId>,
    found: &mut Option<LocalId>,
) {
    if found.is_some() {
        return;
    }
    if let Expr::NativeMethodCall {
        module,
        method,
        args,
        object: None,
        ..
    } = e
    {
        if module == "perry/ui" && method == "widgetAddChild" && args.len() == 2 {
            if let Expr::LocalGet(target_id) = &args[0] {
                if module_locals.contains(target_id) {
                    *found = Some(*target_id);
                    return;
                }
            }
        }
    }
    // Don't recurse into closures — only the outer function body counts
    // for primary-target detection. (We will still handle the closure
    // body separately if it contains a view-builder pattern.)
    match e {
        Expr::Call { callee, args, .. } => {
            scan_module_level_addchild_in_expr(callee, module_locals, found);
            for a in args {
                scan_module_level_addchild_in_expr(a, module_locals, found);
            }
        }
        Expr::NativeMethodCall { object, args, .. } => {
            if let Some(o) = object {
                scan_module_level_addchild_in_expr(o, module_locals, found);
            }
            for a in args {
                scan_module_level_addchild_in_expr(a, module_locals, found);
            }
        }
        _ => {}
    }
}

fn walk_for_funcref_calls_in_closures_in_stmts(
    stmts: &[Stmt],
    out: &mut std::collections::HashSet<perry_types::FuncId>,
) {
    for stmt in stmts {
        walk_for_funcref_calls_in_closures_in_stmt(stmt, out);
    }
}

fn walk_for_funcref_calls_in_closures_in_stmt(
    stmt: &Stmt,
    out: &mut std::collections::HashSet<perry_types::FuncId>,
) {
    match stmt {
        Stmt::Expr(e) | Stmt::Let { init: Some(e), .. } | Stmt::Return(Some(e)) => {
            walk_for_funcref_calls_in_closures_in_expr(e, out);
        }
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            walk_for_funcref_calls_in_closures_in_stmts(then_branch, out);
            if let Some(eb) = else_branch {
                walk_for_funcref_calls_in_closures_in_stmts(eb, out);
            }
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
            walk_for_funcref_calls_in_closures_in_stmts(body, out);
        }
        Stmt::For { body, .. } => {
            walk_for_funcref_calls_in_closures_in_stmts(body, out);
        }
        _ => {}
    }
}

fn walk_for_funcref_calls_in_closures_in_expr(
    e: &Expr,
    out: &mut std::collections::HashSet<perry_types::FuncId>,
) {
    if let Expr::Closure { body, .. } = e {
        walk_for_funcref_calls_in_body(body, out);
        return;
    }
    match e {
        Expr::Call { callee, args, .. } => {
            walk_for_funcref_calls_in_closures_in_expr(callee, out);
            for a in args {
                walk_for_funcref_calls_in_closures_in_expr(a, out);
            }
        }
        Expr::NativeMethodCall { object, args, .. } => {
            if let Some(o) = object {
                walk_for_funcref_calls_in_closures_in_expr(o, out);
            }
            for a in args {
                walk_for_funcref_calls_in_closures_in_expr(a, out);
            }
        }
        _ => {}
    }
}

/// Walks the body recording every `Expr::Call { callee: Expr::FuncRef(id) }`
/// — the inverse of the outer "skip into closures" walker. Used to record
/// which functions are called transitively inside a closure body.
fn walk_for_funcref_calls_in_body(
    stmts: &[Stmt],
    out: &mut std::collections::HashSet<perry_types::FuncId>,
) {
    for stmt in stmts {
        walk_for_funcref_calls_in_body_stmt(stmt, out);
    }
}

fn walk_for_funcref_calls_in_body_stmt(
    stmt: &Stmt,
    out: &mut std::collections::HashSet<perry_types::FuncId>,
) {
    match stmt {
        Stmt::Expr(e) | Stmt::Let { init: Some(e), .. } | Stmt::Return(Some(e)) => {
            walk_for_funcref_calls_in_body_expr(e, out);
        }
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            walk_for_funcref_calls_in_body(then_branch, out);
            if let Some(eb) = else_branch {
                walk_for_funcref_calls_in_body(eb, out);
            }
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
            walk_for_funcref_calls_in_body(body, out);
        }
        Stmt::For { body, .. } => {
            walk_for_funcref_calls_in_body(body, out);
        }
        _ => {}
    }
}

fn walk_for_funcref_calls_in_body_expr(
    e: &Expr,
    out: &mut std::collections::HashSet<perry_types::FuncId>,
) {
    if let Expr::Call { callee, args, .. } = e {
        if let Expr::FuncRef(id) = callee.as_ref() {
            out.insert(*id);
        }
        walk_for_funcref_calls_in_body_expr(callee, out);
        for a in args {
            walk_for_funcref_calls_in_body_expr(a, out);
        }
        return;
    }
    match e {
        Expr::NativeMethodCall { object, args, .. } => {
            if let Some(o) = object {
                walk_for_funcref_calls_in_body_expr(o, out);
            }
            for a in args {
                walk_for_funcref_calls_in_body_expr(a, out);
            }
        }
        Expr::Closure { body, .. } => {
            walk_for_funcref_calls_in_body(body, out);
        }
        _ => {}
    }
}

/// Variant that walks ONLY top-level Stmts (skips into closures and
/// nested functions). Used to find functions called from module.init's
/// top-level (Phase A inlining sees these and inlines them).
fn walk_for_funcref_calls_top_level_in_stmts(
    stmts: &[Stmt],
    out: &mut std::collections::HashSet<perry_types::FuncId>,
) {
    for stmt in stmts {
        if let Stmt::Expr(e) = stmt {
            if let Expr::Call { callee, .. } = e {
                if let Expr::FuncRef(id) = callee.as_ref() {
                    out.insert(*id);
                }
            }
        }
    }
}

/// Phase 2 v3.6 — rewrite every closure body's call to a view-builder
/// function: prepend a `setContentView(target_synth, view_id)` call
/// before the existing function call. The function call itself is left
/// untouched so non-UI side effects (state assignments to module locals,
/// etc.) continue to fire. Widget construction inside the function is
/// no-op stubs on harmonyos so doesn't need stripping.
fn rewrite_view_builder_calls_in_stmts(stmts: &mut Vec<Stmt>, builders: &[ViewBuilder]) {
    if builders.is_empty() {
        return;
    }
    let lookup: HashMap<perry_types::FuncId, &ViewBuilder> =
        builders.iter().map(|b| (b.func_id, b)).collect();
    rewrite_view_builder_calls_in_stmts_with_lookup(stmts, &lookup);
}

fn rewrite_view_builder_calls_in_stmts_with_lookup(
    stmts: &mut Vec<Stmt>,
    lookup: &HashMap<perry_types::FuncId, &ViewBuilder>,
) {
    let mut i = 0;
    while i < stmts.len() {
        rewrite_view_builder_calls_in_stmt(&mut stmts[i], lookup);
        // After rewrite, the stmt's expr may have been wrapped — but we
        // don't insert siblings here; the prepend happens INSIDE closures,
        // not at the call's enclosing stmt level. (Top-level closure-call
        // shape is `Stmt::Expr(Closure { body: vec![Stmt::Expr(Call(...))] })`,
        // so the prepend lands inside the closure body.)
        i += 1;
    }
}

fn rewrite_view_builder_calls_in_stmt(
    stmt: &mut Stmt,
    lookup: &HashMap<perry_types::FuncId, &ViewBuilder>,
) {
    match stmt {
        Stmt::Expr(e) | Stmt::Return(Some(e)) => {
            rewrite_view_builder_calls_in_expr(e, lookup);
        }
        Stmt::Let { init: Some(e), .. } => {
            rewrite_view_builder_calls_in_expr(e, lookup);
        }
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            rewrite_view_builder_calls_in_expr(condition, lookup);
            rewrite_view_builder_calls_in_stmts_with_lookup(then_branch, lookup);
            if let Some(eb) = else_branch {
                rewrite_view_builder_calls_in_stmts_with_lookup(eb, lookup);
            }
        }
        Stmt::While {
            condition, body, ..
        }
        | Stmt::DoWhile {
            body, condition, ..
        } => {
            rewrite_view_builder_calls_in_expr(condition, lookup);
            rewrite_view_builder_calls_in_stmts_with_lookup(body, lookup);
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
            ..
        } => {
            if let Some(i) = init {
                rewrite_view_builder_calls_in_stmt(i.as_mut(), lookup);
            }
            if let Some(c) = condition {
                rewrite_view_builder_calls_in_expr(c, lookup);
            }
            if let Some(u) = update {
                rewrite_view_builder_calls_in_expr(u, lookup);
            }
            rewrite_view_builder_calls_in_stmts_with_lookup(body, lookup);
        }
        _ => {}
    }
}

fn rewrite_view_builder_calls_in_expr(
    e: &mut Expr,
    lookup: &HashMap<perry_types::FuncId, &ViewBuilder>,
) {
    // When we hit a closure: prepend a setContentView call for every
    // view-builder funcref called inside the closure's body, then recurse
    // into the body for any nested closures.
    if let Expr::Closure { body, .. } = e {
        // Collect all view-builder funcrefs called inside this closure.
        let mut called_builders: Vec<&ViewBuilder> = Vec::new();
        let mut seen: std::collections::HashSet<perry_types::FuncId> =
            std::collections::HashSet::new();
        scan_closure_body_for_view_builder_calls(body, lookup, &mut called_builders, &mut seen);
        if !called_builders.is_empty() {
            // Prepend one setContentView call per unique view-builder
            // (deduped by func_id). Order: stable by sorted target_synth
            // so re-runs produce the same .ets bytes.
            let mut sorted = called_builders.clone();
            sorted.sort_by_key(|b| b.func_id);
            let prepends: Vec<Stmt> = sorted
                .iter()
                .map(|b| {
                    Stmt::Expr(Expr::NativeMethodCall {
                        module: "perry/arkts".to_string(),
                        class_name: None,
                        object: None,
                        method: "setContentView".to_string(),
                        args: vec![
                            Expr::String(b.target_synth.clone()),
                            Expr::String(b.view_id.clone()),
                        ],
                    })
                })
                .collect();
            let mut new_body = prepends;
            new_body.extend(std::mem::take(body));
            *body = new_body;
        }
        // Recurse into nested closures regardless.
        rewrite_view_builder_calls_in_stmts_with_lookup(body, lookup);
        return;
    }
    match e {
        Expr::Call { callee, args, .. } => {
            rewrite_view_builder_calls_in_expr(callee, lookup);
            for a in args.iter_mut() {
                rewrite_view_builder_calls_in_expr(a, lookup);
            }
        }
        Expr::NativeMethodCall { object, args, .. } => {
            if let Some(o) = object {
                rewrite_view_builder_calls_in_expr(o, lookup);
            }
            for a in args.iter_mut() {
                rewrite_view_builder_calls_in_expr(a, lookup);
            }
        }
        Expr::PropertyGet { object, .. } => {
            rewrite_view_builder_calls_in_expr(object, lookup);
        }
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            rewrite_view_builder_calls_in_expr(condition, lookup);
            rewrite_view_builder_calls_in_expr(then_expr, lookup);
            rewrite_view_builder_calls_in_expr(else_expr, lookup);
        }
        Expr::Binary { left, right, .. } | Expr::Logical { left, right, .. } => {
            rewrite_view_builder_calls_in_expr(left, lookup);
            rewrite_view_builder_calls_in_expr(right, lookup);
        }
        Expr::Unary { operand, .. } => {
            rewrite_view_builder_calls_in_expr(operand, lookup);
        }
        Expr::Array(items) => {
            for i in items.iter_mut() {
                rewrite_view_builder_calls_in_expr(i, lookup);
            }
        }
        Expr::Object(props) => {
            for (_, v) in props.iter_mut() {
                rewrite_view_builder_calls_in_expr(v, lookup);
            }
        }
        Expr::New { args, .. } => {
            for a in args.iter_mut() {
                rewrite_view_builder_calls_in_expr(a, lookup);
            }
        }
        _ => {}
    }
}

fn scan_closure_body_for_view_builder_calls<'a>(
    stmts: &[Stmt],
    lookup: &HashMap<perry_types::FuncId, &'a ViewBuilder>,
    out: &mut Vec<&'a ViewBuilder>,
    seen: &mut std::collections::HashSet<perry_types::FuncId>,
) {
    for stmt in stmts {
        scan_closure_body_for_view_builder_calls_in_stmt(stmt, lookup, out, seen);
    }
}

fn scan_closure_body_for_view_builder_calls_in_stmt<'a>(
    stmt: &Stmt,
    lookup: &HashMap<perry_types::FuncId, &'a ViewBuilder>,
    out: &mut Vec<&'a ViewBuilder>,
    seen: &mut std::collections::HashSet<perry_types::FuncId>,
) {
    match stmt {
        Stmt::Expr(e) | Stmt::Return(Some(e)) => {
            scan_closure_body_for_view_builder_calls_in_expr(e, lookup, out, seen);
        }
        Stmt::Let { init: Some(e), .. } => {
            scan_closure_body_for_view_builder_calls_in_expr(e, lookup, out, seen);
        }
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            scan_closure_body_for_view_builder_calls_in_expr(condition, lookup, out, seen);
            scan_closure_body_for_view_builder_calls(then_branch, lookup, out, seen);
            if let Some(eb) = else_branch {
                scan_closure_body_for_view_builder_calls(eb, lookup, out, seen);
            }
        }
        _ => {}
    }
}

fn scan_closure_body_for_view_builder_calls_in_expr<'a>(
    e: &Expr,
    lookup: &HashMap<perry_types::FuncId, &'a ViewBuilder>,
    out: &mut Vec<&'a ViewBuilder>,
    seen: &mut std::collections::HashSet<perry_types::FuncId>,
) {
    // Don't recurse into nested closures — their setContentView prepend
    // happens at their own level via `rewrite_view_builder_calls_in_expr`.
    if matches!(e, Expr::Closure { .. }) {
        return;
    }
    if let Expr::Call { callee, .. } = e {
        if let Expr::FuncRef(id) = callee.as_ref() {
            if let Some(b) = lookup.get(id) {
                if seen.insert(*id) {
                    out.push(*b);
                }
            }
        }
    }
    match e {
        Expr::Call { callee, args, .. } => {
            scan_closure_body_for_view_builder_calls_in_expr(callee, lookup, out, seen);
            for a in args {
                scan_closure_body_for_view_builder_calls_in_expr(a, lookup, out, seen);
            }
        }
        Expr::NativeMethodCall { object, args, .. } => {
            if let Some(o) = object {
                scan_closure_body_for_view_builder_calls_in_expr(o, lookup, out, seen);
            }
            for a in args {
                scan_closure_body_for_view_builder_calls_in_expr(a, lookup, out, seen);
            }
        }
        _ => {}
    }
}

fn extract_widget_set_hidden_call(e: &Expr) -> Option<(LocalId, &Expr)> {
    match e {
        Expr::NativeMethodCall {
            module,
            method,
            args,
            object: None,
            ..
        } if module == "perry/ui" && method == "widgetSetHidden" && args.len() == 2 => {
            if let Expr::LocalGet(id) = &args[0] {
                return Some((*id, &args[1]));
            }
            None
        }
        _ => None,
    }
}

/// Phase 2 v3.5 — rewrite every `widgetSetHidden(LocalGet(target), value)`
/// call in `stmts` to a NAPI bridge call when target has a binding. The
/// bridge call is shaped as `Expr::NativeMethodCall { module: "perry/arkts",
/// method: "setVisibility", args: [String(synth_id), value] }`. The codegen
/// dispatcher (`crates/perry-codegen/src/lower_call/native.rs`) recognizes
/// this shape and lowers to the runtime FFI `perry_arkts_set_visibility`,
/// which pushes the (id, hidden) tuple to a NAPI drain queue.
fn rewrite_set_hidden_calls_in_stmts(
    stmts: &mut Vec<Stmt>,
    bindings: &HashMap<LocalId, VisibilityBinding>,
) {
    for stmt in stmts.iter_mut() {
        rewrite_set_hidden_in_stmt(stmt, bindings);
    }
}

fn rewrite_set_hidden_in_stmt(stmt: &mut Stmt, bindings: &HashMap<LocalId, VisibilityBinding>) {
    match stmt {
        Stmt::Expr(e) => rewrite_set_hidden_in_expr(e, bindings),
        Stmt::Let { init: Some(e), .. } => rewrite_set_hidden_in_expr(e, bindings),
        Stmt::Return(Some(e)) => rewrite_set_hidden_in_expr(e, bindings),
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            rewrite_set_hidden_in_expr(condition, bindings);
            rewrite_set_hidden_calls_in_stmts(then_branch, bindings);
            if let Some(eb) = else_branch {
                rewrite_set_hidden_calls_in_stmts(eb, bindings);
            }
        }
        Stmt::While {
            condition, body, ..
        }
        | Stmt::DoWhile {
            body, condition, ..
        } => {
            rewrite_set_hidden_in_expr(condition, bindings);
            rewrite_set_hidden_calls_in_stmts(body, bindings);
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
            ..
        } => {
            if let Some(i) = init {
                rewrite_set_hidden_in_stmt(i.as_mut(), bindings);
            }
            if let Some(c) = condition {
                rewrite_set_hidden_in_expr(c, bindings);
            }
            if let Some(u) = update {
                rewrite_set_hidden_in_expr(u, bindings);
            }
            rewrite_set_hidden_calls_in_stmts(body, bindings);
        }
        _ => {}
    }
}

fn rewrite_set_hidden_in_expr(e: &mut Expr, bindings: &HashMap<LocalId, VisibilityBinding>) {
    // Detect the rewrite target FIRST (most specific shape), before
    // recursing into children — otherwise children of the call to
    // rewrite would be visited as if they were in a regular call.
    if let Expr::NativeMethodCall {
        module,
        method,
        args,
        object: None,
        ..
    } = e
    {
        if module == "perry/ui" && method == "widgetSetHidden" && args.len() == 2 {
            if let Expr::LocalGet(target_id) = &args[0] {
                if let Some(binding) = bindings.get(target_id) {
                    // Coerce literal numbers/integers to bool so the runtime
                    // side gets a proper boolean. Non-literal values pass
                    // through and get coerced runtime-side via the same
                    // js_jsvalue_to_string-style helper as setText.
                    let hidden_arg = match &args[1] {
                        Expr::Bool(b) => Expr::Bool(*b),
                        Expr::Number(n) => Expr::Bool(*n != 0.0),
                        Expr::Integer(n) => Expr::Bool(*n != 0),
                        other => other.clone(),
                    };
                    *e = Expr::NativeMethodCall {
                        module: "perry/arkts".to_string(),
                        class_name: None,
                        object: None,
                        method: "setVisibility".to_string(),
                        args: vec![Expr::String(binding.synth_id.clone()), hidden_arg],
                    };
                    return;
                }
            }
        }
    }
    // Recurse into all sub-expressions so closures, nested calls, etc.
    // get their setHidden calls rewritten too.
    match e {
        Expr::Closure { body, .. } => {
            rewrite_set_hidden_calls_in_stmts(body, bindings);
        }
        Expr::Call { callee, args, .. } => {
            rewrite_set_hidden_in_expr(callee, bindings);
            for a in args.iter_mut() {
                rewrite_set_hidden_in_expr(a, bindings);
            }
        }
        Expr::NativeMethodCall { object, args, .. } => {
            if let Some(o) = object {
                rewrite_set_hidden_in_expr(o, bindings);
            }
            for a in args.iter_mut() {
                rewrite_set_hidden_in_expr(a, bindings);
            }
        }
        Expr::PropertyGet { object, .. } => {
            rewrite_set_hidden_in_expr(object, bindings);
        }
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            rewrite_set_hidden_in_expr(condition, bindings);
            rewrite_set_hidden_in_expr(then_expr, bindings);
            rewrite_set_hidden_in_expr(else_expr, bindings);
        }
        Expr::Binary { left, right, .. } | Expr::Logical { left, right, .. } => {
            rewrite_set_hidden_in_expr(left, bindings);
            rewrite_set_hidden_in_expr(right, bindings);
        }
        Expr::Unary { operand, .. } => {
            rewrite_set_hidden_in_expr(operand, bindings);
        }
        Expr::Array(items) => {
            for i in items.iter_mut() {
                rewrite_set_hidden_in_expr(i, bindings);
            }
        }
        Expr::Object(props) => {
            for (_, v) in props.iter_mut() {
                rewrite_set_hidden_in_expr(v, bindings);
            }
        }
        Expr::New { args, .. } => {
            for a in args.iter_mut() {
                rewrite_set_hidden_in_expr(a, bindings);
            }
        }
        _ => {}
    }
}

/// Variant that ONLY recurses into closures inside module.init, leaving
/// the top-level Stmts alone. Used when we want closure-body widgetSetHidden
/// calls rewritten (so taps push to drain) but module-init top-level calls
/// preserved (their initial value is captured statically by
/// `collect_visibility_bindings` Pass 2).
fn rewrite_set_hidden_in_closures_in_stmts(
    stmts: &mut Vec<Stmt>,
    bindings: &HashMap<LocalId, VisibilityBinding>,
) {
    for stmt in stmts.iter_mut() {
        rewrite_set_hidden_in_closures_in_stmt(stmt, bindings);
    }
}

fn rewrite_set_hidden_in_closures_in_stmt(
    stmt: &mut Stmt,
    bindings: &HashMap<LocalId, VisibilityBinding>,
) {
    match stmt {
        Stmt::Expr(e) | Stmt::Return(Some(e)) => {
            rewrite_set_hidden_in_closures_in_expr(e, bindings);
        }
        Stmt::Let { init: Some(e), .. } => {
            rewrite_set_hidden_in_closures_in_expr(e, bindings);
        }
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            rewrite_set_hidden_in_closures_in_expr(condition, bindings);
            rewrite_set_hidden_in_closures_in_stmts(then_branch, bindings);
            if let Some(eb) = else_branch {
                rewrite_set_hidden_in_closures_in_stmts(eb, bindings);
            }
        }
        Stmt::While {
            condition, body, ..
        }
        | Stmt::DoWhile {
            body, condition, ..
        } => {
            rewrite_set_hidden_in_closures_in_expr(condition, bindings);
            rewrite_set_hidden_in_closures_in_stmts(body, bindings);
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
            ..
        } => {
            if let Some(i) = init {
                rewrite_set_hidden_in_closures_in_stmt(i.as_mut(), bindings);
            }
            if let Some(c) = condition {
                rewrite_set_hidden_in_closures_in_expr(c, bindings);
            }
            if let Some(u) = update {
                rewrite_set_hidden_in_closures_in_expr(u, bindings);
            }
            rewrite_set_hidden_in_closures_in_stmts(body, bindings);
        }
        _ => {}
    }
}

fn rewrite_set_hidden_in_closures_in_expr(
    e: &mut Expr,
    bindings: &HashMap<LocalId, VisibilityBinding>,
) {
    // When we hit a closure, its body IS the call site — recurse there
    // with the full rewriter (which treats every level as a target).
    if let Expr::Closure { body, .. } = e {
        rewrite_set_hidden_calls_in_stmts(body, bindings);
        return;
    }
    // Otherwise descend into sub-exprs without rewriting top-level
    // widgetSetHidden calls.
    match e {
        Expr::Call { callee, args, .. } => {
            rewrite_set_hidden_in_closures_in_expr(callee, bindings);
            for a in args.iter_mut() {
                rewrite_set_hidden_in_closures_in_expr(a, bindings);
            }
        }
        Expr::NativeMethodCall { object, args, .. } => {
            if let Some(o) = object {
                rewrite_set_hidden_in_closures_in_expr(o, bindings);
            }
            for a in args.iter_mut() {
                rewrite_set_hidden_in_closures_in_expr(a, bindings);
            }
        }
        Expr::PropertyGet { object, .. } => {
            rewrite_set_hidden_in_closures_in_expr(object, bindings);
        }
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            rewrite_set_hidden_in_closures_in_expr(condition, bindings);
            rewrite_set_hidden_in_closures_in_expr(then_expr, bindings);
            rewrite_set_hidden_in_closures_in_expr(else_expr, bindings);
        }
        Expr::Binary { left, right, .. } | Expr::Logical { left, right, .. } => {
            rewrite_set_hidden_in_closures_in_expr(left, bindings);
            rewrite_set_hidden_in_closures_in_expr(right, bindings);
        }
        Expr::Unary { operand, .. } => {
            rewrite_set_hidden_in_closures_in_expr(operand, bindings);
        }
        Expr::Array(items) => {
            for i in items.iter_mut() {
                rewrite_set_hidden_in_closures_in_expr(i, bindings);
            }
        }
        Expr::Object(props) => {
            for (_, v) in props.iter_mut() {
                rewrite_set_hidden_in_closures_in_expr(v, bindings);
            }
        }
        Expr::New { args, .. } => {
            for a in args.iter_mut() {
                rewrite_set_hidden_in_closures_in_expr(a, bindings);
            }
        }
        _ => {}
    }
}

/// Issue #408 — pre-walk for `widgetAddChild` / `scrollviewSetChild` /
/// `setPadding` / `setCornerRadius` / `widgetSet*` etc. mutator calls.
/// Walks every top-level statement (and into if/else branches) recording
/// each mutator against its target widget local.
///
/// Closures, loops, and nested function bodies are intentionally NOT
/// walked: mutators inside loops can't be statically traced (we'd need
/// to know how many iterations) and mutators inside closure bodies fire
/// at callback time, after the harvest has already produced the page.
/// The "out of scope" section of #408 explicitly calls these out as
/// fallback cases.
///
/// The `cond_group_counter` makes each top-level `if (...) { ... } else
/// { ... }` produce a unique group id so emitter can collapse mutations
/// from the same if statement back into a single `if/else` block — even
/// if the if appears alongside unconditional mutators.
fn collect_mutations(
    init: &[Stmt],
    bindings: &HashMap<LocalId, Expr>,
    compile_time_consts: &HashMap<LocalId, f64>,
) -> HashMap<LocalId, Vec<MutationEntry>> {
    let mut out: HashMap<LocalId, Vec<MutationEntry>> = HashMap::new();
    let mut group_counter: u32 = 0;
    for stmt in init {
        collect_mutations_in_stmt(
            stmt,
            None,
            &mut out,
            &mut group_counter,
            bindings,
            compile_time_consts,
        );
    }
    out
}

fn collect_mutations_in_stmt(
    stmt: &Stmt,
    enclosing: Option<MutationCondition>,
    out: &mut HashMap<LocalId, Vec<MutationEntry>>,
    group_counter: &mut u32,
    bindings: &HashMap<LocalId, Expr>,
    compile_time_consts: &HashMap<LocalId, f64>,
) {
    match stmt {
        Stmt::Expr(e) => collect_mutations_in_expr(e, enclosing.as_ref(), out, bindings),
        Stmt::Let { init: Some(e), .. } => {
            collect_mutations_in_expr(e, enclosing.as_ref(), out, bindings)
        }
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            // Issue #413 — try to constant-fold the condition. When every
            // operand bottoms out in literals (after resolving through
            // compile_time_consts and bindings), the resulting `if (9 ===
            // 1) { ... }` would be rejected by ArkTS's strict-mode
            // overlap checker. Drop the dead branch entirely so the
            // emitted source contains only the live mutators.
            if let Some(folded) = evaluate_condition(condition, bindings, compile_time_consts) {
                let live: &[Stmt] = if folded {
                    then_branch
                } else {
                    else_branch.as_deref().unwrap_or(&[])
                };
                // Inherit the *enclosing* condition (None if we're at
                // the top level) so the live branch's mutations look
                // identical to user code that didn't write the dead
                // `if` at all. No new group id is allocated — the
                // resolved branch isn't a real if/else from the
                // emitter's perspective.
                for s in live {
                    collect_mutations_in_stmt(
                        s,
                        enclosing.clone(),
                        out,
                        group_counter,
                        bindings,
                        compile_time_consts,
                    );
                }
                return;
            }
            // v0.5.490 — when evaluate_condition can't fold but the
            // condition wouldn't cleanly serialize either (PropertyGet
            // on unresolvable LocalGet, function call, etc.),
            // serialize_condition will degrade the emit to `if (true)
            // {...} else {...}`. Both branches would render — and the
            // else-branch is dead source-wise. Walk only the then-
            // branch in this case to avoid duplicate-content emission
            // (Mango: the welcome-card branch's CTA button + the
            // connection-list branch's addMoreBtn both rendering as
            // "+ New Connection"). Same heuristic as the
            // Expr::Conditional emit_widget pick-then-branch fallback
            // and the v0.5.487 unresolvable-LocalGet "true" fallback,
            // unified.
            if !is_cleanly_serializable_condition(condition, bindings, compile_time_consts) {
                for s in then_branch {
                    collect_mutations_in_stmt(
                        s,
                        enclosing.clone(),
                        out,
                        group_counter,
                        bindings,
                        compile_time_consts,
                    );
                }
                return;
            }
            // Each top-level if gets its own group id so the emitter can
            // collapse all mutations from the same if into a single
            // `if (cond) { ... } else { ... }` block.
            //
            // If we're already inside a conditional context, we still
            // carry the OUTER condition forward — nested conditions are
            // out of scope for v0 (they'd need a 2D group key); the
            // existing condition takes precedence.
            if enclosing.is_some() {
                // Nested-if fallback: walk both branches inheriting the
                // enclosing condition. Loses fidelity but doesn't crash.
                for s in then_branch {
                    collect_mutations_in_stmt(
                        s,
                        enclosing.clone(),
                        out,
                        group_counter,
                        bindings,
                        compile_time_consts,
                    );
                }
                if let Some(eb) = else_branch {
                    for s in eb {
                        collect_mutations_in_stmt(
                            s,
                            enclosing.clone(),
                            out,
                            group_counter,
                            bindings,
                            compile_time_consts,
                        );
                    }
                }
            } else {
                let cond_str = serialize_condition(condition, bindings, compile_time_consts);
                let group = *group_counter;
                *group_counter += 1;
                let then_cond = MutationCondition {
                    cond_str: cond_str.clone(),
                    branch: Branch::Then,
                    group,
                };
                for s in then_branch {
                    collect_mutations_in_stmt(
                        s,
                        Some(then_cond.clone()),
                        out,
                        group_counter,
                        bindings,
                        compile_time_consts,
                    );
                }
                if let Some(eb) = else_branch {
                    let else_cond = MutationCondition {
                        cond_str,
                        branch: Branch::Else,
                        group,
                    };
                    for s in eb {
                        collect_mutations_in_stmt(
                            s,
                            Some(else_cond.clone()),
                            out,
                            group_counter,
                            bindings,
                            compile_time_consts,
                        );
                    }
                }
            }
        }
        // Loops, switches, try, throw, return — out of scope per #408.
        // We could descend into switch cases analogously to if/else but
        // that's a v1 follow-up.
        _ => {}
    }
}

/// If `expr` is a recognized perry/ui mutator call, record an entry
/// against its target widget local. Mutator calls show up as
/// `Expr::NativeMethodCall { module: "perry/ui", method: "widgetAddChild",
/// args: [LocalGet(parent), LocalGet(child), ...] }` (the first arg is
/// always the receiver widget).
///
/// `bindings` is consulted by axis-aware mutators (e.g. `stackSetAlignment`)
/// to look up the target widget's constructor — VStack vs HStack picks
/// `HorizontalAlign.X` vs `VerticalAlign.X` per ArkUI's enum convention.
fn collect_mutations_in_expr(
    expr: &Expr,
    cond: Option<&MutationCondition>,
    out: &mut HashMap<LocalId, Vec<MutationEntry>>,
    bindings: &HashMap<LocalId, Expr>,
) {
    let Expr::NativeMethodCall {
        module: m,
        method,
        args,
        ..
    } = expr
    else {
        return;
    };
    if m != "perry/ui" {
        return;
    }
    let Some(target_id) = mutator_target_local_id(args) else {
        return;
    };
    let push_mut = |mu: Mutation,
                    out: &mut HashMap<LocalId, Vec<MutationEntry>>,
                    cond: Option<&MutationCondition>| {
        out.entry(target_id).or_default().push(MutationEntry {
            mutation: mu,
            condition: cond.cloned(),
        });
    };
    match method.as_str() {
        // ---- Children ----
        "widgetAddChild" => {
            if let Some(child) = args.get(1) {
                push_mut(Mutation::AddChild(child.clone()), out, cond);
            }
        }
        "widgetAddChildAt" => {
            // v0: positional insertion is treated as plain AddChild —
            // the `index` arg is dropped because the harvest model can't
            // re-order ArkUI children mid-build. Fidelity loss documented
            // as a v1 follow-up.
            if let Some(child) = args.get(1) {
                push_mut(Mutation::AddChild(child.clone()), out, cond);
            }
        }
        "widgetClearChildren" => {
            push_mut(Mutation::ClearChildren, out, cond);
        }
        "scrollviewSetChild" | "scrollViewSetChild" => {
            if let Some(child) = args.get(1) {
                push_mut(Mutation::SetScrollChild(child.clone()), out, cond);
            }
        }
        // ---- Styling modifiers ----
        "widgetSetBackgroundColor" => {
            if let Some(modifier) = mutator_background_color(&args[1..], bindings) {
                push_mut(Mutation::Modifier(modifier), out, cond);
            }
        }
        "widgetSetBackgroundGradient" => {
            // Args: (widget, r1, g1, b1, a1, r2, g2, b2, a2, direction)
            // — Perry passes two RGBA endpoints in 0..1 channel space
            // plus a direction flag (0 = vertical / top→bottom, 1 =
            // horizontal / left→right). Map to ArkUI `.linearGradient(
            // { angle, colors: [[hex, stop], ...] })`. ArkUI's `angle`
            // is degrees — 0 = top→bottom (Perry direction 0); 90 =
            // left→right (Perry direction 1). Resolves channel args
            // through bindings so theme-bound calls like
            // `widgetSetBackgroundGradient(box, moR, moG, moB, ...)`
            // work — Mango's exact pattern.
            //
            // If any channel can't be resolved, fall back to a comment
            // — the previous behavior emitted `'#ffffff'→'#000000'`
            // which produced white-on-white invisible text and was
            // worse than no gradient at all.
            let chans = (0..8)
                .map(|i| numeric_arg_resolved(&args[1..], i, bindings))
                .collect::<Option<Vec<_>>>();
            let Some(chans) = chans else {
                push_mut(
                    Mutation::Comment(
                        "widgetSetBackgroundGradient: channels unresolved, skipped".to_string(),
                    ),
                    out,
                    cond,
                );
                return;
            };
            let direction = numeric_arg_resolved(&args[1..], 8, bindings).unwrap_or(0.0);
            let to_hex = |r: f64, g: f64, b: f64| {
                let r = (r * 255.0).round().clamp(0.0, 255.0) as u8;
                let g = (g * 255.0).round().clamp(0.0, 255.0) as u8;
                let b = (b * 255.0).round().clamp(0.0, 255.0) as u8;
                format!("#{:02x}{:02x}{:02x}", r, g, b)
            };
            let c1 = to_hex(chans[0], chans[1], chans[2]);
            let c2 = to_hex(chans[4], chans[5], chans[6]);
            let angle = if direction == 0.0 { 0 } else { 90 };
            push_mut(
                Mutation::Modifier(format!(
                    ".linearGradient({{ angle: {}, colors: [['{}', 0.0], ['{}', 1.0]] }})",
                    angle, c1, c2
                )),
                out,
                cond,
            );
        }
        "setPadding" | "widgetSetEdgeInsets" => {
            // Args: (widget, top, right, bottom, left)
            // Resolve through bindings so Mango's `setPadding(box, isIOS
            // ? 52 : 12, mobile ? 16 : 24, ...)` ternary-and-binding
            // chain resolves to literal numbers; default to 0 only if
            // the leaf truly isn't a number (function call etc).
            let top = numeric_arg_resolved(&args[1..], 0, bindings).unwrap_or(0.0);
            let right = numeric_arg_resolved(&args[1..], 1, bindings).unwrap_or(0.0);
            let bottom = numeric_arg_resolved(&args[1..], 2, bindings).unwrap_or(0.0);
            let left = numeric_arg_resolved(&args[1..], 3, bindings).unwrap_or(0.0);
            push_mut(
                Mutation::Modifier(format!(
                    ".padding({{ top: {}, right: {}, bottom: {}, left: {} }})",
                    fmt_num(top),
                    fmt_num(right),
                    fmt_num(bottom),
                    fmt_num(left)
                )),
                out,
                cond,
            );
        }
        "setCornerRadius" => {
            let n = numeric_arg_resolved(&args[1..], 0, bindings).unwrap_or(0.0);
            push_mut(
                Mutation::Modifier(format!(".borderRadius({})", fmt_num(n))),
                out,
                cond,
            );
        }
        "widgetSetHidden" => {
            // Phase 2 v3.5 — if pre-seeded with a VisibilityBinding for this
            // target, the widget is bound to a `@State hidden_<id>` field
            // and the modifier comes via the binding's `Mutation::VisibilityBinding`
            // entry. Skip the static modifier emit so we don't double-bind.
            let has_binding = out
                .get(&target_id)
                .map(|v| {
                    v.iter()
                        .any(|e| matches!(e.mutation, Mutation::VisibilityBinding(_)))
                })
                .unwrap_or(false);
            if has_binding {
                return;
            }
            // Truthy second arg → Hidden, falsy → Visible.
            let hide = match args.get(1) {
                Some(Expr::Bool(true)) => true,
                Some(Expr::Number(n)) => *n != 0.0,
                Some(Expr::Integer(n)) => *n != 0,
                _ => false,
            };
            let v = if hide { "Hidden" } else { "Visible" };
            push_mut(
                Mutation::Modifier(format!(".visibility(Visibility.{})", v)),
                out,
                cond,
            );
        }
        // ---- Issue #669 Chart mutators ----
        "chartAddDataPoint" => {
            // Args: (chart, label, value). Both label + value must resolve
            // through bindings — the static fold can't bake in a runtime
            // computed value. When either is unresolvable, drop the point
            // and emit a comment so the user can see the gap. (Matching
            // the textSetFontFamily behavior for un-resolvable strings.)
            let label = args.get(1).and_then(|e| resolve_string_arg(e, bindings));
            let value = numeric_arg_resolved(&args[1..], 1, bindings);
            match (label, value) {
                (Some(l), Some(v)) => {
                    push_mut(Mutation::ChartAddDataPoint(l, v), out, cond);
                }
                _ => {
                    push_mut(
                        Mutation::Comment(
                            "chartAddDataPoint: non-literal label/value, point dropped".to_string(),
                        ),
                        out,
                        cond,
                    );
                }
            }
        }
        "chartClearData" => {
            push_mut(Mutation::ChartClearData, out, cond);
        }
        "chartSetTitle" => {
            let title = args
                .get(1)
                .and_then(|e| resolve_string_arg(e, bindings))
                .unwrap_or_default();
            push_mut(Mutation::ChartSetTitle(title), out, cond);
        }
        "chartReload" => {
            push_mut(Mutation::ChartReload, out, cond);
        }
        // ---- Issue #670 TreeView mutators ----
        "treeNodeAddChild" => {
            if let Some(child) = args.get(1) {
                push_mut(Mutation::TreeAddChild(child.clone()), out, cond);
            }
        }
        // ---- Text styling mutators (#408 follow-up) ----
        // All four resolve their numeric args through `bindings` so calls
        // with bound locals (`textSetFontSize(w, size)` where `size` is a
        // const-bound literal — Mango's pattern) work. When a value can't
        // be resolved (closure-captured, prop-access, etc.) we skip the
        // modifier emit entirely — better to leave the default styling
        // than to emit `.fontSize(0)` which makes the text invisible.
        "textSetFontSize" => {
            let Some(n) = numeric_arg_resolved(&args[1..], 0, bindings) else {
                return;
            };
            push_mut(
                Mutation::Modifier(format!(".fontSize({})", fmt_num(n))),
                out,
                cond,
            );
        }
        "textSetFontWeight" => {
            // Perry's signature is `(widget, size: number, weight: number)`
            // — mirroring Apple's `systemFont(ofSize: weight:)` API where
            // `weight` is a 0..1 normalized scale (0 = thin/100, 0.5 =
            // regular/400, 1.0 = bold/900). The pre-fix here read the
            // SIZE arg as the weight, emitting `.fontWeight(24)` etc.
            // which is below ArkUI's valid 100..900 range — ArkUI
            // clamped to 100 (lightest), making text appear translucent
            // (Mango's "Welcome to Mango" was the visible symptom).
            //
            // Resolve both args; map weight into 100..900 (rounded to
            // the nearest 100 for FontWeight-enum compatibility); emit
            // BOTH .fontSize() and .fontWeight() so the size always
            // matches even if a prior textSetFontSize call set it
            // earlier (the chain order is "last write wins" in ArkUI).
            let Some(size) = numeric_arg_resolved(&args[1..], 0, bindings) else {
                return;
            };
            let weight_scale = numeric_arg_resolved(&args[1..], 1, bindings).unwrap_or(0.5);
            // Map 0..1 → 100..900, rounded to nearest 100.
            let weight = (100.0 + 800.0 * weight_scale).clamp(100.0, 900.0);
            let weight_int = ((weight / 100.0).round() as i64) * 100;
            push_mut(
                Mutation::Modifier(format!(
                    ".fontSize({}).fontWeight({})",
                    fmt_num(size),
                    weight_int
                )),
                out,
                cond,
            );
        }
        "textSetFontFamily" => {
            // Args: (widget, family). Family must resolve to a string
            // literal — most theme code passes a const-bound string.
            let mut cur = match args.get(1) {
                Some(e) => e,
                None => return,
            };
            for _ in 0..16 {
                match cur {
                    Expr::String(s) => {
                        push_mut(
                            Mutation::Modifier(format!(".fontFamily({})", arkts_string_lit(s))),
                            out,
                            cond,
                        );
                        return;
                    }
                    Expr::LocalGet(id) => {
                        cur = match bindings.get(id) {
                            Some(b) => b,
                            None => return,
                        };
                    }
                    _ => return,
                }
            }
        }
        "textSetColor" => {
            // Args: (widget, r, g, b, a?) where each channel is 0..1.
            // Reuses the same mapping as widgetSetBackgroundColor.
            let Some(r) = numeric_arg_resolved(&args[1..], 0, bindings) else {
                return;
            };
            let Some(g) = numeric_arg_resolved(&args[1..], 1, bindings) else {
                return;
            };
            let Some(b) = numeric_arg_resolved(&args[1..], 2, bindings) else {
                return;
            };
            let a = numeric_arg_resolved(&args[1..], 3, bindings).unwrap_or(1.0);
            let r255 = (r * 255.0).round() as i64;
            let g255 = (g * 255.0).round() as i64;
            let b255 = (b * 255.0).round() as i64;
            push_mut(
                Mutation::Modifier(format!(
                    ".fontColor('rgba({}, {}, {}, {})')",
                    r255,
                    g255,
                    b255,
                    fmt_num(a)
                )),
                out,
                cond,
            );
        }
        // ---- Button styling mutators ----
        "buttonSetTextColor" => {
            // Args: (widget, r, g, b, a?) — same shape as textSetColor /
            // widgetSetBackgroundColor. ArkUI's Button accepts
            // `.fontColor(...)` to set the label text color, distinct
            // from `.backgroundColor()` for the button surface.
            let Some(r) = numeric_arg_resolved(&args[1..], 0, bindings) else {
                return;
            };
            let Some(g) = numeric_arg_resolved(&args[1..], 1, bindings) else {
                return;
            };
            let Some(b) = numeric_arg_resolved(&args[1..], 2, bindings) else {
                return;
            };
            let a = numeric_arg_resolved(&args[1..], 3, bindings).unwrap_or(1.0);
            let r255 = (r * 255.0).round() as i64;
            let g255 = (g * 255.0).round() as i64;
            let b255 = (b * 255.0).round() as i64;
            push_mut(
                Mutation::Modifier(format!(
                    ".fontColor('rgba({}, {}, {}, {})')",
                    r255,
                    g255,
                    b255,
                    fmt_num(a)
                )),
                out,
                cond,
            );
        }
        "buttonSetBordered" => {
            // Args: (widget, bordered: number) — 0 = no border (flat
            // button), non-zero = with border. ArkUI's default Button
            // is non-bordered (Capsule type); to get a flat / borderless
            // appearance we set `.backgroundColor(Color.Transparent)`
            // when bordered=0. When bordered=1 (or default), we leave
            // the default ArkUI styling in place. Mango uses
            // `buttonSetBordered(btn, 0)` extensively for ghost-style
            // buttons — without this they'd inherit the blue-pill
            // default.
            let bordered = match args.get(1) {
                Some(Expr::Bool(true)) => true,
                Some(Expr::Number(n)) => *n != 0.0,
                Some(Expr::Integer(n)) => *n != 0,
                _ => true,
            };
            if !bordered {
                push_mut(
                    Mutation::Modifier(".backgroundColor(Color.Transparent)".to_string()),
                    out,
                    cond,
                );
            }
            // bordered=true: no-op, default Button styling applies.
        }
        "buttonSetTitle" => {
            // Args: (widget, title). Updates the button label at
            // runtime. The harvest can't follow runtime mutations
            // through the page-struct state machinery without a
            // reactive binding, but we can at least emit a comment so
            // the user knows the call is recognized. TODO: hook into
            // the v3.2 reactive-Text setText machinery for buttons.
            let _ = args; // intentionally silenced
        }
        "textSetWraps" => {
            // truthy → wrap, falsy → ellipsis. ArkUI's analog is
            // `.maxLines(0)` for unlimited / `.textOverflow({overflow:
            // TextOverflow.Ellipsis})` for ellipsis. Map to maxLines.
            let wraps = match args.get(1) {
                Some(Expr::Bool(true)) => true,
                Some(Expr::Number(n)) => *n != 0.0,
                Some(Expr::Integer(n)) => *n != 0,
                _ => true,
            };
            // 0 = unlimited; 1 = single-line + ellipsis (set via overflow).
            let modifier = if wraps {
                ".maxLines(0)".to_string()
            } else {
                ".maxLines(1).textOverflow({ overflow: TextOverflow.Ellipsis })".to_string()
            };
            push_mut(Mutation::Modifier(modifier), out, cond);
        }
        "widgetMatchParentWidth" => {
            push_mut(Mutation::Modifier(".width('100%')".to_string()), out, cond);
        }
        "widgetMatchParentHeight" => {
            push_mut(Mutation::Modifier(".height('100%')".to_string()), out, cond);
        }
        "widgetSetWidth" => {
            // Skip-on-unresolved: emitting `.width(0)` zeros the widget.
            // Mango's pattern: `widgetSetWidth(logo, mobile ? 40 : 44)`
            // — needs binding-resolution + ternary-fold (handled by
            // numeric_arg_resolved).
            let Some(n) = numeric_arg_resolved(&args[1..], 0, bindings) else {
                return;
            };
            push_mut(
                Mutation::Modifier(format!(".width({})", fmt_num(n))),
                out,
                cond,
            );
        }
        "widgetSetHeight" => {
            let Some(n) = numeric_arg_resolved(&args[1..], 0, bindings) else {
                return;
            };
            push_mut(
                Mutation::Modifier(format!(".height({})", fmt_num(n))),
                out,
                cond,
            );
        }
        "widgetSetHugging" => {
            // ArkUI's closest equivalent is `.flexShrink(0)` — the widget
            // refuses to shrink below its intrinsic size.
            push_mut(Mutation::Modifier(".flexShrink(0)".to_string()), out, cond);
        }
        "stackSetDistribution" => {
            // 0..N → ArkUI FlexAlign enum buckets. The mapping mirrors
            // perry-ui-* native (Start/Center/End/SpaceBetween/SpaceAround/
            // SpaceEvenly).
            let n = numeric_arg_resolved(&args[1..], 0, bindings).unwrap_or(0.0) as i64;
            let v = match n {
                0 => "Start",
                1 => "Center",
                2 => "End",
                3 => "SpaceBetween",
                4 => "SpaceAround",
                5 => "SpaceEvenly",
                _ => "Start",
            };
            push_mut(
                Mutation::Modifier(format!(".justifyContent(FlexAlign.{})", v)),
                out,
                cond,
            );
        }
        "stackSetAlignment" => {
            let n = numeric_arg_resolved(&args[1..], 0, bindings).unwrap_or(0.0) as i64;
            // Issue #413 — ArkUI's cross-axis enum is axis-dependent:
            // Column (= VStack) takes `HorizontalAlign.X`,
            // Row (= HStack) takes `VerticalAlign.X`. Emitting the
            // wrong enum produces an ArkTS strict-mode type error
            // "Argument of type 'HorizontalAlign' is not assignable to
            // parameter of type 'VerticalAlign'". We look up the
            // target widget's constructor through `bindings` to pick
            // the right one. Defaults to `HorizontalAlign` (the
            // Column case) when the binding can't be resolved — same
            // as v0.5.480 behavior, preserves backwards compatibility
            // for VStack which is the common case.
            //
            // The value names also differ per enum:
            //   HorizontalAlign: Start | Center | End
            //   VerticalAlign:   Top   | Center | Bottom
            // Picking `Start`/`End` on `VerticalAlign` is also a
            // strict-mode error ("Property 'Start' does not exist on
            // type 'typeof VerticalAlign'") — so we map the same
            // semantic input value (0=start, 1=center, 2=end) to the
            // axis-correct value-name.
            let enum_name = stack_axis_align_enum(target_id, bindings);
            let v = match (enum_name, n) {
                ("VerticalAlign", 0) => "Top",
                ("VerticalAlign", 2) => "Bottom",
                (_, 1) => "Center",
                ("HorizontalAlign", 2) => "End",
                _ => "Start",
            };
            push_mut(
                Mutation::Modifier(format!(".alignItems({}.{})", enum_name, v)),
                out,
                cond,
            );
        }
        // Issue #479 — `widgetSetRichTooltip(target, content, hoverDelayMs)`
        // → ArkUI `.bindPopup(...)` modifier. ArkUI's bindPopup ships
        // both a simple-message variant (`{message: string}`) and a
        // builder variant (`{builder: () => CustomBuilder}`); the
        // simple-message form is the fidelity-correct match for a
        // text-only tooltip, the builder form would be needed for
        // truly rich custom-widget tooltips (v1.1 follow-up — wiring
        // the builder requires access to emit_widget machinery which
        // collect_mutations_in_expr doesn't have).
        //
        // For v1 we extract a plain-text payload from the content
        // widget when it resolves to a simple `Text(string)` call;
        // anything more complex degrades to a comment marker so the
        // user can see the gap. Hover-delay arg is documented but
        // not honored — ArkUI's popup show-trigger is implicit
        // (long-press / click), matching the android long-press
        // semantics the brief calls out as acceptable.
        "widgetSetRichTooltip" => {
            let content_text = args
                .get(1)
                .and_then(|content| resolve_tooltip_text(content, bindings));
            if let Some(text) = content_text {
                push_mut(
                    Mutation::Modifier(format!(
                        ".bindPopup(false, {{ message: {} }})",
                        arkts_string_lit(&text)
                    )),
                    out,
                    cond,
                );
            } else {
                push_mut(
                    Mutation::Comment(
                        "widgetSetRichTooltip: non-static content widget, simple-popup fallback skipped (v1.1 follow-up)"
                            .to_string(),
                    ),
                    out,
                    cond,
                );
            }
        }
        // Unrecognized mutator on a known target — log a comment so the
        // user can see the gap. Avoids silent fidelity loss.
        other => {
            // Silently skip the obviously-not-a-mutator perry/ui calls
            // (App, VStack, HStack, Text, Button — these CREATE widgets,
            // they don't mutate). Anything else is presumably a missed
            // mutator; flag it.
            if !is_widget_factory(other) {
                push_mut(
                    Mutation::Comment(format!(
                        "perry/ui mutator `{}` not yet handled by codegen-arkts (Issue #408 follow-up)",
                        other
                    )),
                    out,
                    cond,
                );
            }
        }
    }
}

/// Heuristic: known widget-factory names that should NOT be flagged as
/// missed mutators. Kept loose — false positives only produce extra
/// comments, not bugs.
fn is_widget_factory(name: &str) -> bool {
    matches!(
        name,
        "App"
            | "Text"
            | "Button"
            | "VStack"
            | "HStack"
            | "ZStack"
            | "ScrollView"
            | "LazyVStack"
            | "Spacer"
            | "Divider"
            | "Image"
            | "ImageFile"
            | "ImageSymbol"
            | "TextField"
            | "TextArea"
            | "Toggle"
            | "Slider"
            | "Picker"
            | "Combobox"
            | "RichTextEditor"
            | "Calendar"
            | "ProgressView"
            | "Section"
            | "Tabs"
            | "Modal"
            | "Dialog"
            | "Menu"
            | "ContextMenu"
            | "Grid"
            | "NavStack"
            | "Chart"
            | "TreeView"
            | "TreeNode"
            | "showToast"
            | "setText"
            | "state"
            | "stateCreate"
    )
}

/// Inspect the first arg of a perry/ui method call. If it's a
/// `LocalGet(id)` (the canonical "mutate a widget bound to a local"
/// shape), return the LocalId. Anything else (transient widget without
/// a binding, complex expression) returns None and the mutator is
/// dropped — the user code couldn't mutate something un-named anyway.
fn mutator_target_local_id(args: &[Expr]) -> Option<LocalId> {
    match args.first() {
        Some(Expr::LocalGet(id)) => Some(*id),
        _ => None,
    }
}

/// Issue #479 — extract a plain-text payload from a tooltip content
/// widget expression. The harvest can fold runtime-built widget trees
/// into ArkUI source via emit_widget, but the modifier pre-walk runs
/// before that pass and only has access to `bindings`. For v1 we
/// recognize the canonical shape `Text(literal-string)`, both as a
/// direct call and as a `LocalGet(id)` that resolves through bindings
/// to one. Anything else (HStack/VStack content, dynamic strings,
/// closure-captured locals) returns None and the caller falls back
/// to a comment.
fn resolve_tooltip_text(expr: &Expr, bindings: &HashMap<LocalId, Expr>) -> Option<String> {
    let mut cur = expr;
    for _ in 0..16 {
        match cur {
            Expr::NativeMethodCall {
                module,
                method,
                args,
                ..
            } if module == "perry/ui" && method == "Text" => {
                if let Some(Expr::String(s)) = args.first() {
                    return Some(s.clone());
                }
                return None;
            }
            Expr::LocalGet(id) => {
                cur = bindings.get(id)?;
            }
            _ => return None,
        }
    }
    None
}

/// Issue #413 — return the ArkUI cross-axis alignment enum name for the
/// stack target. Column (= VStack) takes `HorizontalAlign`; Row (=
/// HStack) takes `VerticalAlign`. Looks up the binding for the local
/// to discover the constructor; falls back to `HorizontalAlign` (the
/// VStack default) when the binding can't be resolved or doesn't name
/// a recognized stack constructor.
fn stack_axis_align_enum(target_id: LocalId, bindings: &HashMap<LocalId, Expr>) -> &'static str {
    let Some(init) = bindings.get(&target_id) else {
        return "HorizontalAlign";
    };
    if let Expr::NativeMethodCall {
        module: m, method, ..
    } = init
    {
        if m == "perry/ui" {
            return match method.as_str() {
                "HStack" => "VerticalAlign",
                _ => "HorizontalAlign",
            };
        }
    }
    "HorizontalAlign"
}

/// Build the `.backgroundColor('rgba(R, G, B, A)')` modifier string from
/// the 4 channel args of a `widgetSetBackgroundColor(w, r, g, b, a)` call.
/// Channels are 0..1 floats matching the perry-ui-* TS surface.
fn mutator_background_color(args: &[Expr], bindings: &HashMap<LocalId, Expr>) -> Option<String> {
    // Resolve through bindings so theme-bound calls work — Mango's
    // `widgetSetBackgroundColor(btn, moR, moG, moB, 1.0)` where moR/G/B
    // are const-bound brand-color numbers needed `numeric_arg_resolved`,
    // not the literal-only `numeric_arg`.
    let r = numeric_arg_resolved(args, 0, bindings)?;
    let g = numeric_arg_resolved(args, 1, bindings)?;
    let b = numeric_arg_resolved(args, 2, bindings)?;
    let a = numeric_arg_resolved(args, 3, bindings).unwrap_or(1.0);
    let r255 = (r * 255.0).round() as i64;
    let g255 = (g * 255.0).round() as i64;
    let b255 = (b * 255.0).round() as i64;
    Some(format!(
        ".backgroundColor('rgba({}, {}, {}, {})')",
        r255,
        g255,
        b255,
        fmt_num(a)
    ))
}

/// Stringify a condition expression for emission in an ArkUI
/// `if (<cond>)` predicate. Handles the canonical comparison + logical
/// shapes the harvest can statically rewrite, plus a few literal forms.
/// Falls back to a `true` predicate (so the then-branch always renders)
/// for shapes the emitter can't safely render.
/// Issue #410 — serialize a condition expression to ArkTS source. The
/// emitted string is interpolated into `if (...)` blocks (for conditional
/// AddChild mutations) and into `/* if (...) */` comment markers (for
/// conditional Modifier mutations) in the generated Index.ets. Two
/// invariants must hold for the emitted ArkTS to compile:
///
/// 1. **No `*/` substring anywhere in the returned string.** When the
///    caller wraps the result in `/* if ((<cond>)) */`, any `*/` inside
///    `<cond>` would close the outer comment early and leak the rest as
///    code (see #410 line-82 cascade). Every branch of this function is
///    audited to ensure that — string literals route through
///    `arkts_string_lit` (single-quoted, so `*/` can't appear unescaped),
///    operator strings come from a closed enum, and the bottom-fallback
///    returns `"true"` (literally — not `"true /* unsupported */"`).
///
/// 2. **No `__local_N` placeholders.** `Expr::LocalGet(id)` references
///    must resolve via `bindings` (top-level `let x = <init>` HIR shape)
///    to a real, ArkTS-bindable expression. If the local can't be
///    resolved (closure-captured, loop-mutated, or a `declare const`
///    without an `init`), we degrade gracefully to `"true"` — losing the
///    conditionality but keeping the build green. Compile-time platform
///    constants like `__platform__` are inlined as numeric literals via
///    the `compile_time_consts` map.
///
/// Issue #413 — defensive parenthesization on nested operators. When a
/// resolved binding contains a Binary/Logical/Unary expression and that
/// expression is the operand of an outer Binary/Logical/Unary, ArkTS's
/// precedence rules can invert the user's intent. Concretely,
/// `mobile = __platform__ === 1 || __platform__ === 2 || (!isIOS && x)`
/// inlined as `9 === 1 || 9 === 2 && !9 === 1 && true === 1` (where
/// `!9 === 1` parses as `(!9) === 1` instead of `!(9 === 1)`). The fix
/// is to wrap any non-leaf serialized operand in parentheses before
/// splicing into the parent operator string. Leaf shapes (literals,
/// LocalGet that resolved to a literal, PropertyGet) don't need wrapping.
/// On HarmonyOS, the v0.5.477 build.rs-generated stubs return 0/false
/// for every perry/system + perry/ui FFI symbol that's not implemented
/// natively. The harvest's constant folder treats calls to these
/// functions as Lit::Num(0.0) so theme-switching code like
/// `const dark = isDarkMode()` folds to `dark = false` at codegen
/// time, picking the light-mode branch. Without this, the unfoldable-
/// LocalGet heuristic-pick-then-branch fallback selects the dark-mode
/// branch and Mango renders translucent light-text-on-light-background.
fn is_harmonyos_zero_fn(name: &str) -> bool {
    matches!(
        name,
        "isDarkMode"
            | "getDeviceIdiom"
            | "getDeviceModel"
            | "getDeviceOSVersion"
            | "isHighContrast"
            | "isReducedMotion"
            | "getNotchHeight"
    )
}

/// Returns true iff every leaf in the expression is either a literal,
/// a compile-time-const LocalGet, or a binding-resolvable LocalGet
/// whose underlying init is itself cleanly serializable. PropertyGets,
/// function calls, and unresolvable LocalGets all return false — those
/// can't be safely interpolated as ArkTS condition source without
/// emitting an undeclared identifier (#410) or a type-mismatched
/// expression like `true.length === 0` (#413 follow-up).
fn is_cleanly_serializable_condition(
    e: &Expr,
    bindings: &HashMap<LocalId, Expr>,
    compile_time_consts: &HashMap<LocalId, f64>,
) -> bool {
    match e {
        Expr::Bool(_) | Expr::Number(_) | Expr::Integer(_) | Expr::String(_) => true,
        Expr::Null | Expr::Undefined => true,
        Expr::LocalGet(id) => {
            if compile_time_consts.contains_key(id) {
                return true;
            }
            match bindings.get(id) {
                Some(init) => {
                    is_cleanly_serializable_condition(init, bindings, compile_time_consts)
                }
                None => false,
            }
        }
        Expr::Compare { left, right, .. } => {
            is_cleanly_serializable_condition(left, bindings, compile_time_consts)
                && is_cleanly_serializable_condition(right, bindings, compile_time_consts)
        }
        Expr::Logical { left, right, .. } => {
            is_cleanly_serializable_condition(left, bindings, compile_time_consts)
                && is_cleanly_serializable_condition(right, bindings, compile_time_consts)
        }
        Expr::Unary { operand, .. } => {
            is_cleanly_serializable_condition(operand, bindings, compile_time_consts)
        }
        // PropertyGet, Call, NativeMethodCall, etc. — can't serialize.
        // Caller falls back to `true` so the conditional always
        // renders its then-branch (matching the v0.5.487 unresolvable-
        // LocalGet heuristic).
        _ => false,
    }
}

fn serialize_condition(
    e: &Expr,
    bindings: &HashMap<LocalId, Expr>,
    compile_time_consts: &HashMap<LocalId, f64>,
) -> String {
    use perry_hir::ir::{CompareOp, LogicalOp};

    // Pessimistic safety gate: if the expression contains anything that
    // can't be cleanly serialized into ArkTS source (PropertyGet on an
    // unresolvable LocalGet, function calls, complex member chains), the
    // current per-node fallbacks would produce gibberish like
    // `true.length === 0` or `true === connectionNames`. ArkTS strict
    // mode rejects both. Degrade the entire condition to `true` (always-
    // render the then-branch) — same heuristic as the unresolvable-
    // LocalGet fallback at the leaf level, just lifted to the root so
    // wrapping shapes (PropertyGet, Comparison-with-non-foldable-side)
    // don't leak.
    if !is_cleanly_serializable_condition(e, bindings, compile_time_consts) {
        return "true".to_string();
    }
    // Wrap a sub-expression's serialized form in parentheses if the
    // sub-expression is a Binary/Logical/Unary shape (post-resolve), so
    // splicing into a parent operator string can't invert precedence.
    // LocalGet recurses through resolution (compile_time_consts then
    // bindings), so we test the *resolved* expression to decide.
    fn needs_parens(e: &Expr, bindings: &HashMap<LocalId, Expr>) -> bool {
        let resolved = match e {
            Expr::LocalGet(id) => bindings.get(id).cloned(),
            _ => None,
        };
        let target = resolved.as_ref().unwrap_or(e);
        matches!(
            target,
            Expr::Compare { .. } | Expr::Logical { .. } | Expr::Unary { .. }
        )
    }
    fn wrap(e: &Expr, bindings: &HashMap<LocalId, Expr>, ts: String) -> String {
        if needs_parens(e, bindings) {
            format!("({})", ts)
        } else {
            ts
        }
    }
    match e {
        Expr::Bool(true) => "true".to_string(),
        Expr::Bool(false) => "false".to_string(),
        Expr::Compare { op, left, right } => {
            let op_str = match op {
                CompareOp::Eq => " === ",
                CompareOp::Ne => " !== ",
                CompareOp::LooseEq => " == ",
                CompareOp::LooseNe => " != ",
                CompareOp::Lt => " < ",
                CompareOp::Le => " <= ",
                CompareOp::Gt => " > ",
                CompareOp::Ge => " >= ",
            };
            let l = serialize_condition(left, bindings, compile_time_consts);
            let r = serialize_condition(right, bindings, compile_time_consts);
            format!(
                "{}{}{}",
                wrap(left, bindings, l),
                op_str,
                wrap(right, bindings, r)
            )
        }
        Expr::Logical { op, left, right } => {
            let op_str = match op {
                LogicalOp::And => " && ",
                LogicalOp::Or => " || ",
                LogicalOp::Coalesce => " ?? ",
            };
            let l = serialize_condition(left, bindings, compile_time_consts);
            let r = serialize_condition(right, bindings, compile_time_consts);
            format!(
                "{}{}{}",
                wrap(left, bindings, l),
                op_str,
                wrap(right, bindings, r)
            )
        }
        Expr::Unary { op, operand } => {
            use perry_hir::ir::UnaryOp;
            let op_str = match op {
                UnaryOp::Not => "!",
                UnaryOp::Neg => "-",
                UnaryOp::Pos => "+",
                UnaryOp::BitNot => "~",
            };
            let inner = serialize_condition(operand, bindings, compile_time_consts);
            format!("{}{}", op_str, wrap(operand, bindings, inner))
        }
        Expr::String(s) => arkts_string_lit(s),
        Expr::Number(n) => fmt_num(*n),
        Expr::Integer(n) => format!("{}", n),
        Expr::LocalGet(id) => {
            // Compile-time platform constants (e.g. `declare const
            // __platform__: number`) are inlined as numeric literals.
            // For the harmonyos codegen path this is always 9.0; the
            // map is populated by `collect_compile_time_constants`.
            if let Some(v) = compile_time_consts.get(id) {
                return fmt_num(*v);
            }
            // Try to resolve through const-bindings. For
            // `let mobile = (__platform__ === 1)` the resolved condition
            // is `(__platform__ === 1)` which then recurses through this
            // same function and inlines the platform literal.
            if let Some(init) = bindings.get(id) {
                return serialize_condition(init, bindings, compile_time_consts);
            }
            // Unresolvable LocalGet — degrade to `true` so the emitted
            // ArkTS compiles cleanly. Conditionality is lost; the
            // mutation always renders as if the predicate were truthy.
            // Emitting `__local_N` here would leak as an undeclared
            // identifier into the page struct (see #410 lines 48/52/68).
            "true".to_string()
        }
        Expr::PropertyGet { object, property } => {
            // `obj.prop` shape — used commonly in conditions like
            // `props.mobile`. Recursively stringify the object access
            // chain. Keeps the predicate syntactically valid; the
            // user-side reference may not actually exist at the ArkTS
            // page-struct scope, in which case ArkTS's compiler
            // surfaces it as a separate error during emission.
            format!(
                "{}.{}",
                serialize_condition(object, bindings, compile_time_consts),
                property
            )
        }
        // Fallback: emit `true` (literally — no diagnostic comment, since
        // the comment's `*/` would close any wrapping block-comment
        // marker, see #410 line-82 cascade). Conditionality is lost but
        // the build stays green.
        _ => "true".to_string(),
    }
}

/// Issue #413 — try to constant-fold a condition expression. Returns
/// `Some(true)`/`Some(false)` when every operand bottoms out in a
/// literal (Bool/Number/Integer/String/Null/Undefined) and resolves
/// fully through `bindings` and `compile_time_consts`. Returns `None`
/// when any non-literal leaf is reached (e.g. PropertyGet on a runtime
/// value, an unresolved LocalGet, a Call/NativeMethodCall, etc.).
///
/// The caller in `collect_mutations_in_stmt` uses this to drop dead
/// `if` branches at harvest time. Without this, expressions like
/// `__platform__ === 1` (after `__platform__` inlines to 9) would emit
/// as ArkTS `if (9 === 1) { ... }` — which strict-mode ArkTS rejects
/// with a "comparison appears to be unintentional because the types
/// '9' and '1' have no overlap" error. By folding to `Some(false)` and
/// dropping the `if`, we keep the emitted source legal.
fn evaluate_condition(
    e: &Expr,
    bindings: &HashMap<LocalId, Expr>,
    compile_time_consts: &HashMap<LocalId, f64>,
) -> Option<bool> {
    use perry_hir::ir::{CompareOp, LogicalOp, UnaryOp};
    /// Inner repr of a fully-resolved literal value the constant-folder
    /// can reason about. Anything not representable here returns None
    /// from `to_lit` and propagates as the caller's None.
    #[derive(Debug, Clone, PartialEq)]
    enum Lit {
        Bool(bool),
        Num(f64),
        Str(String),
        Null,
        Undefined,
    }
    fn to_lit(
        e: &Expr,
        bindings: &HashMap<LocalId, Expr>,
        compile_time_consts: &HashMap<LocalId, f64>,
    ) -> Option<Lit> {
        match e {
            Expr::Bool(b) => Some(Lit::Bool(*b)),
            Expr::Number(n) => Some(Lit::Num(*n)),
            Expr::Integer(n) => Some(Lit::Num(*n as f64)),
            Expr::String(s) => Some(Lit::Str(s.clone())),
            Expr::Null => Some(Lit::Null),
            Expr::Undefined => Some(Lit::Undefined),
            Expr::LocalGet(id) => {
                if let Some(v) = compile_time_consts.get(id) {
                    return Some(Lit::Num(*v));
                }
                if let Some(init) = bindings.get(id) {
                    return to_lit(init, bindings, compile_time_consts);
                }
                None
            }
            // Known stubbed perry/system + perry/ui functions that
            // return 0 / false on HarmonyOS (the v0.5.477 build.rs
            // auto-stubs all return zero values). Treating them as
            // 0 here makes `dark = isDarkMode()` fold to `dark = 0`
            // at codegen time, which then propagates through
            // `dark ? darkColor : lightColor` to pick the light-mode
            // branch. Without this, the heuristic-pick-then-branch
            // fallback selects darkColor and Mango renders translucent
            // light-on-light text.
            Expr::Call { callee, .. } => match callee.as_ref() {
                Expr::ExternFuncRef { name, .. } if is_harmonyos_zero_fn(name) => {
                    Some(Lit::Num(0.0))
                }
                Expr::FuncRef(_) => None,
                _ => None,
            },
            // perry/system.isDarkMode() may also surface as a
            // NativeMethodCall — same treatment.
            Expr::NativeMethodCall { module, method, .. }
                if module == "perry/system" && is_harmonyos_zero_fn(method) =>
            {
                Some(Lit::Num(0.0))
            }
            Expr::Compare { op, left, right } => {
                let l = to_lit(left, bindings, compile_time_consts)?;
                let r = to_lit(right, bindings, compile_time_consts)?;
                let res = match op {
                    CompareOp::Eq => lit_strict_eq(&l, &r),
                    CompareOp::Ne => !lit_strict_eq(&l, &r),
                    CompareOp::LooseEq => lit_loose_eq(&l, &r),
                    CompareOp::LooseNe => !lit_loose_eq(&l, &r),
                    CompareOp::Lt | CompareOp::Le | CompareOp::Gt | CompareOp::Ge => {
                        let (Lit::Num(a), Lit::Num(b)) = (&l, &r) else {
                            return None;
                        };
                        match op {
                            CompareOp::Lt => a < b,
                            CompareOp::Le => a <= b,
                            CompareOp::Gt => a > b,
                            CompareOp::Ge => a >= b,
                            _ => unreachable!(),
                        }
                    }
                };
                Some(Lit::Bool(res))
            }
            Expr::Logical { op, left, right } => {
                let l = to_lit(left, bindings, compile_time_consts)?;
                match op {
                    LogicalOp::And => {
                        if !lit_truthy(&l) {
                            Some(l)
                        } else {
                            to_lit(right, bindings, compile_time_consts)
                        }
                    }
                    LogicalOp::Or => {
                        if lit_truthy(&l) {
                            Some(l)
                        } else {
                            to_lit(right, bindings, compile_time_consts)
                        }
                    }
                    LogicalOp::Coalesce => {
                        if matches!(l, Lit::Null | Lit::Undefined) {
                            to_lit(right, bindings, compile_time_consts)
                        } else {
                            Some(l)
                        }
                    }
                }
            }
            Expr::Unary { op, operand } => {
                let v = to_lit(operand, bindings, compile_time_consts)?;
                match op {
                    UnaryOp::Not => Some(Lit::Bool(!lit_truthy(&v))),
                    UnaryOp::Neg => match v {
                        Lit::Num(n) => Some(Lit::Num(-n)),
                        _ => None,
                    },
                    UnaryOp::Pos => match v {
                        Lit::Num(n) => Some(Lit::Num(n)),
                        _ => None,
                    },
                    UnaryOp::BitNot => match v {
                        Lit::Num(n) => Some(Lit::Num((!(n as i32)) as f64)),
                        _ => None,
                    },
                }
            }
            _ => None,
        }
    }
    fn lit_truthy(l: &Lit) -> bool {
        match l {
            Lit::Bool(b) => *b,
            Lit::Num(n) => *n != 0.0 && !n.is_nan(),
            Lit::Str(s) => !s.is_empty(),
            Lit::Null | Lit::Undefined => false,
        }
    }
    fn lit_strict_eq(a: &Lit, b: &Lit) -> bool {
        match (a, b) {
            (Lit::Bool(x), Lit::Bool(y)) => x == y,
            (Lit::Num(x), Lit::Num(y)) => x == y,
            (Lit::Str(x), Lit::Str(y)) => x == y,
            (Lit::Null, Lit::Null) => true,
            (Lit::Undefined, Lit::Undefined) => true,
            _ => false,
        }
    }
    fn lit_loose_eq(a: &Lit, b: &Lit) -> bool {
        // `null == undefined` per spec, plus strict-eq for matching kinds.
        // Cross-type numeric/string coercion is intentionally not
        // implemented here — we only resolve the cases we can do safely.
        match (a, b) {
            (Lit::Null, Lit::Undefined) | (Lit::Undefined, Lit::Null) => true,
            _ => lit_strict_eq(a, b),
        }
    }
    let l = to_lit(e, bindings, compile_time_consts)?;
    Some(lit_truthy(&l))
}

/// Walk a Vec<Stmt> and rewrite any `state.set(v)` calls (where state's
/// LocalId is in the registry) to `setText(synth_id, v)` calls. Recurses
/// into closure bodies, blocks, control flow.
fn rewrite_state_calls_in_stmts(stmts: &mut Vec<Stmt>, reg: &HashMap<LocalId, StateBinding>) {
    for stmt in stmts.iter_mut() {
        rewrite_state_in_stmt(stmt, reg);
    }
}

fn rewrite_state_in_stmt(stmt: &mut Stmt, reg: &HashMap<LocalId, StateBinding>) {
    match stmt {
        Stmt::Expr(e) => rewrite_state_in_expr(e, reg),
        Stmt::Let { init: Some(e), .. } => rewrite_state_in_expr(e, reg),
        Stmt::Return(Some(e)) => rewrite_state_in_expr(e, reg),
        Stmt::If {
            condition,
            then_branch,
            else_branch,
            ..
        } => {
            rewrite_state_in_expr(condition, reg);
            rewrite_state_calls_in_stmts(then_branch, reg);
            if let Some(else_branch) = else_branch {
                rewrite_state_calls_in_stmts(else_branch, reg);
            }
        }
        Stmt::While {
            condition, body, ..
        }
        | Stmt::DoWhile {
            body, condition, ..
        } => {
            rewrite_state_in_expr(condition, reg);
            rewrite_state_calls_in_stmts(body, reg);
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
            ..
        } => {
            if let Some(init) = init {
                rewrite_state_in_stmt(init.as_mut(), reg);
            }
            if let Some(c) = condition {
                rewrite_state_in_expr(c, reg);
            }
            if let Some(u) = update {
                rewrite_state_in_expr(u, reg);
            }
            rewrite_state_calls_in_stmts(body, reg);
        }
        _ => {}
    }
}

fn rewrite_state_in_expr(e: &mut Expr, reg: &HashMap<LocalId, StateBinding>) {
    // Detect `state.set(v)` first (most specific shape).
    if let Expr::Call { callee, args, .. } = e {
        if args.len() == 1 {
            if let Expr::PropertyGet { object, property } = callee.as_ref() {
                if property == "set" {
                    if let Expr::LocalGet(state_id) = object.as_ref() {
                        if let Some(binding) = reg.get(state_id) {
                            let value_expr = args[0].clone();
                            *e = Expr::NativeMethodCall {
                                module: "perry/ui".to_string(),
                                class_name: None,
                                object: None,
                                method: "setText".to_string(),
                                args: vec![Expr::String(binding.synth_id.clone()), value_expr],
                            };
                            return;
                        }
                    }
                }
            }
        }
    }
    // Recurse into ALL expression children so nested state.set(v) calls
    // inside method args / object literals / closure bodies / etc. are
    // also rewritten. Each variant unrolls its sub-Exprs explicitly so
    // we don't miss any HIR shape.
    match e {
        Expr::Call { callee, args, .. } => {
            rewrite_state_in_expr(callee, reg);
            for a in args.iter_mut() {
                rewrite_state_in_expr(a, reg);
            }
        }
        Expr::NativeMethodCall { object, args, .. } => {
            if let Some(o) = object {
                rewrite_state_in_expr(o, reg);
            }
            for a in args.iter_mut() {
                rewrite_state_in_expr(a, reg);
            }
        }
        Expr::Object(props) => {
            for (_, v) in props.iter_mut() {
                rewrite_state_in_expr(v, reg);
            }
        }
        Expr::Array(items) => {
            for v in items.iter_mut() {
                rewrite_state_in_expr(v, reg);
            }
        }
        Expr::Closure { body, .. } => {
            rewrite_state_calls_in_stmts(body, reg);
        }
        Expr::PropertyGet { object, .. } => {
            rewrite_state_in_expr(object, reg);
        }
        Expr::PropertySet { object, value, .. } => {
            rewrite_state_in_expr(object, reg);
            rewrite_state_in_expr(value, reg);
        }
        Expr::IndexGet { object, index } => {
            rewrite_state_in_expr(object, reg);
            rewrite_state_in_expr(index, reg);
        }
        Expr::Binary { left, right, .. } => {
            rewrite_state_in_expr(left, reg);
            rewrite_state_in_expr(right, reg);
        }
        Expr::ArrayMap { array, callback } => {
            rewrite_state_in_expr(array, reg);
            rewrite_state_in_expr(callback, reg);
        }
        Expr::New { args, .. } => {
            for a in args.iter_mut() {
                rewrite_state_in_expr(a, reg);
            }
        }
        // Leaf/other variants don't carry rewriteable sub-Exprs (or are
        // rare enough that v6 deferring them is fine — file as v6.5
        // follow-up if anyone hits a real-world miss).
        _ => {}
    }
}

/// Find the first top-level `App({body: <expr>})` call in `module.init`,
/// **return its body by-value**, and replace the entire statement with a
/// no-op `Stmt::Expr(Expr::Number(0.0))`. Other statements are untouched
/// so logic before/after `App(...)` still runs in `perryEntry.run()`.
fn find_and_strip_app(init: &mut [Stmt], classes: &[Class]) -> Option<Expr> {
    for stmt in init.iter_mut() {
        if let Stmt::Expr(Expr::NativeMethodCall {
            module: m,
            method,
            object: None,
            args,
            ..
        }) = stmt
        {
            if m == "perry/ui" && method == "App" && args.len() == 1 {
                let body = extract_body_field(&mut args[0], classes);
                if body.is_some() {
                    *stmt = Stmt::Expr(Expr::Number(0.0));
                    return body;
                }
            }
        }
    }
    None
}

/// Pull out the `body:` field's expression from either a plain
/// `Expr::Object` or a `__AnonShape_*` `Expr::New`. Returns the body by
/// value (cloned for the New case since we can't move out of args[idx]
/// without disturbing the rest of the args array, but the strip below
/// throws the whole call away anyway).
fn extract_body_field(arg: &mut Expr, classes: &[Class]) -> Option<Expr> {
    match arg {
        Expr::Object(props) => {
            let idx = props.iter().position(|(k, _)| k == "body")?;
            let (_, body) = props.remove(idx);
            Some(body)
        }
        Expr::New {
            class_name, args, ..
        } if class_name.starts_with("__AnonShape_") => {
            let class = classes.iter().find(|c| &c.name == class_name)?;
            let body_idx = class.fields.iter().position(|f| f.name == "body")?;
            args.get(body_idx).cloned()
        }
        _ => None,
    }
}

/// Snapshot read-only top-level `let x = <expr>;` so widget walks can
/// follow `Expr::LocalGet(x)` back to the init expression. We index by
/// LocalId rather than name because perry-hir's identifier resolution
/// runs by id — names are debug aids only.
///
/// Phase 2 v1.5 only follows TOP-level inits; nested let-bindings inside
/// blocks would need a wider analysis pass (the code path is only invoked
/// via `App({body: x})` which itself is top-level, so the binding it
/// references is also top-level — works for the common case).
fn collect_const_bindings(init: &[Stmt]) -> HashMap<LocalId, Expr> {
    let mut map = HashMap::new();
    walk_collect_const_bindings(init, &mut map);
    map
}

/// Recursive helper for `collect_const_bindings` — walks `Stmt::If` /
/// `Stmt::Block` bodies so const bindings created in conditional branches
/// are visible to the harvest. Mango's pattern:
///
/// ```ts
/// if (mobile) {
///     const connInfoBtn = Button(...);
///     widgetAddChild(connBody, HStack([Spacer(), connInfoBtn, Spacer()]));
/// }
/// ```
///
/// — the `widgetAddChild` mutation gets recorded by `collect_mutations`
/// with its enclosing condition. The inner Button construction needs
/// `connInfoBtn` to be in `bindings` when the harvest emits the inner
/// HStack children — otherwise it falls through to `[unrecognized body]`.
///
/// Limitation: if two if-branches both `const foo = ...` with different
/// RHS, the last branch's binding wins. Mango doesn't hit this (each
/// branch defines unique names) and the alternative — full scoped
/// resolution — is meaningfully more complex. Acceptable trade-off for
/// the procedural-construction use case.
fn walk_collect_const_bindings(stmts: &[Stmt], map: &mut HashMap<LocalId, Expr>) {
    for stmt in stmts {
        match stmt {
            Stmt::Let {
                id,
                init: Some(expr),
                mutable: false,
                ..
            } => {
                map.insert(*id, expr.clone());
            }
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                walk_collect_const_bindings(then_branch, map);
                if let Some(eb) = else_branch {
                    walk_collect_const_bindings(eb, map);
                }
            }
            _ => {}
        }
    }
}

/// Issue #410 — discover `declare const __platform__: number;` style
/// compile-time constants. The HIR shape is `Stmt::Let { name, init: None }`
/// (matches `crates/perry-codegen/src/codegen.rs::compile_time_constants`).
/// Used by `serialize_condition` to inline `__platform__ === N`
/// comparisons at codegen time so the emitted ArkTS doesn't reference
/// undeclared identifiers.
///
/// This codegen path is harmonyos-only (the compile.rs harvest at
/// line 1071 only fires on `--target harmonyos[-simulator]`), so
/// `__platform__` is always 9.0 here. The platform-id table lives in
/// `crates/perry-codegen/src/codegen.rs:672-700`.
fn collect_compile_time_constants(init: &[Stmt]) -> HashMap<LocalId, f64> {
    let mut map = HashMap::new();
    for stmt in init {
        if let Stmt::Let {
            id,
            name,
            init: None,
            ..
        } = stmt
        {
            // Mirror codegen.rs::compile_time_constants — only the
            // canonical names are recognized. Anything else is a regular
            // hoisted let binding that resolves through the normal
            // `bindings` map.
            match name.as_str() {
                "__platform__" => {
                    // This codegen is harmonyos-only — see emit_index_ets
                    // call site in crates/perry/src/commands/compile.rs.
                    map.insert(*id, 9.0);
                }
                "__plugins__" => {
                    // No harmonyos-specific plugin set today; default to 0.
                    map.insert(*id, 0.0);
                }
                _ => {}
            }
        }
    }
    map
}

/// Resolve `Expr::LocalGet(id)` to its bound init expression if available.
/// Returns the original expression for any non-LocalGet shape so callers
/// can use it as a transparent identity-or-deref helper.
fn resolve(expr: &Expr, bindings: &HashMap<LocalId, Expr>) -> Expr {
    // Chase chains of LocalGet → LocalGet → ... → real expr. Phase B
    // of the inliner introduces aliasing chains: a top-level
    // `const disconnectBtn = makeDangerBtn(...)` becomes
    // `const disconnectBtn = LocalGet(remapped_btn)` after the call
    // gets inlined and substituted. emit_widget needs to chase past
    // these aliases to find the actual NativeMethodCall(Button, ...).
    // 16-hop cap mirrors numeric_arg_resolved / resolve_string_arg's
    // safety bound.
    let mut cur = expr.clone();
    for _ in 0..16 {
        let next = match &cur {
            Expr::LocalGet(id) => bindings.get(id).cloned(),
            _ => return cur,
        };
        match next {
            Some(e) => cur = e,
            None => return cur,
        }
    }
    cur
}

/// Emit an ArkUI expression for a perry/ui widget call. Returns the inner
/// `build()`-block content (no wrapping component). `depth` controls
/// indentation when emitting nested children. `callbacks` accumulates
/// closure expressions that need runtime registration; each push assigns
/// the next slot id (= callbacks.len() before push).
///
/// Unrecognized widgets degrade to a comment + a placeholder Text — never
/// errors out, since emit-time errors would leave the user without any UI.
#[allow(clippy::too_many_arguments)]
fn emit_widget(
    expr: &Expr,
    bindings: &HashMap<LocalId, Expr>,
    depth: usize,
    callbacks: &mut Vec<Expr>,
    text_slots: &mut Vec<TextSlot>,
    arkts_locals: &HashMap<LocalId, String>,
    classes: &[Class],
    state_registry: &HashMap<LocalId, StateBinding>,
    lazy_sources: &mut Vec<LazyDataSource>,
    extras: &mut HarvestExtras,
    mutations: &HashMap<LocalId, Vec<MutationEntry>>,
    // `outer_local_hint` is set when the caller already knows the
    // top-level LocalId we're emitting for — used by recursive calls
    // from emit_stack into a child position that may itself be a
    // LocalGet of another widget local. Always None at the entry point.
    outer_local_hint: Option<LocalId>,
) -> String {
    // Issue #408 — extract LocalId hint before resolving so we can later
    // look up procedural mutations recorded against this widget binding.
    // `outer_local_hint` overrides nothing: if expr is itself a LocalGet,
    // its id wins over a caller-supplied hint.
    let local_hint = match expr {
        Expr::LocalGet(id) => Some(*id),
        _ => outer_local_hint,
    };
    // Phase 2 v6 — `state.text()` shape: Expr::Call { callee: PropertyGet
    // { obj: LocalGet(state_id), property: "text" }, args: [] } where
    // state_id is in the registry. Emit a reactive Text using the
    // registered synth_id + initial value (uses the v3.2 path).
    if let Expr::Call { callee, args, .. } = expr {
        if args.is_empty() {
            if let Expr::PropertyGet { object, property } = callee.as_ref() {
                if property == "text" {
                    if let Expr::LocalGet(state_id) = object.as_ref() {
                        if let Some(binding) = state_registry.get(state_id) {
                            text_slots.push(TextSlot {
                                original_id: binding.synth_id.clone(),
                                field_id: sanitize_text_id(&binding.synth_id),
                                initial: binding.initial_str.clone(),
                            });
                            return format!(
                                "Text(this.text_{}).fontSize(20)",
                                sanitize_text_id(&binding.synth_id)
                            );
                        }
                    }
                }
            }
        }
    }
    let resolved = resolve(expr, bindings);
    match &resolved {
        Expr::NativeMethodCall {
            module: m,
            method,
            args,
            ..
        } if m == "perry/ui" => {
            let core = match method.as_str() {
                "Text" => emit_text(args, text_slots, arkts_locals, bindings),
                "VStack" => emit_stack(
                    "Column",
                    args,
                    bindings,
                    depth,
                    callbacks,
                    text_slots,
                    arkts_locals,
                    classes,
                    state_registry,
                    lazy_sources,
                    extras,
                    mutations,
                    local_hint,
                ),
                "HStack" => emit_stack(
                    "Row",
                    args,
                    bindings,
                    depth,
                    callbacks,
                    text_slots,
                    arkts_locals,
                    classes,
                    state_registry,
                    lazy_sources,
                    extras,
                    mutations,
                    local_hint,
                ),
                "Button" => emit_button(args, callbacks),
                "TextField" => emit_textfield(args, callbacks),
                "Toggle" => emit_toggle(args, callbacks),
                "Slider" => emit_slider(args, callbacks),
                "Spacer" => "Blank()".to_string(),
                "Divider" => "Divider()".to_string(),
                "Image" | "ImageFile" => emit_image(args, bindings),
                "ScrollView" => emit_scrollview(
                    args,
                    bindings,
                    depth,
                    callbacks,
                    text_slots,
                    arkts_locals,
                    classes,
                    state_registry,
                    lazy_sources,
                    extras,
                    mutations,
                    local_hint,
                ),
                "LazyVStack" => emit_lazy_vstack(
                    args,
                    bindings,
                    depth,
                    callbacks,
                    text_slots,
                    arkts_locals,
                    classes,
                    state_registry,
                    lazy_sources,
                    extras,
                    mutations,
                ),
                "Picker" => emit_picker(args, callbacks),
                // Issue #475 — Combobox(initial, onChange) maps to ArkUI
                // Select. Runtime-added items (`comboboxAddItem`) are
                // currently not folded into the static options array;
                // the emitted Select shows the `initial` value as its
                // only option. Tracked for v1.1 follow-up.
                "Combobox" => emit_combobox(args, callbacks),
                // Issue #478 — RichTextEditor(width, height, onChange)
                // maps to ArkUI RichEditor. HTML round-trip + per-span
                // bold/italic/underline toggles are #478 v1.1.
                "RichTextEditor" => emit_richtexteditor(args, callbacks),
                // Issue #481 — Calendar(year, month, onChange) maps to
                // ArkUI's CalendarPicker. Per the #481 v1 brief, the
                // picker variant is simpler than the full Calendar
                // (which would need a builder for cell rendering).
                "Calendar" => emit_calendar(args, callbacks),
                "ProgressView" => emit_progressview(args),
                "Section" => emit_section(
                    args,
                    bindings,
                    depth,
                    callbacks,
                    text_slots,
                    arkts_locals,
                    classes,
                    state_registry,
                    lazy_sources,
                    extras,
                    mutations,
                    local_hint,
                ),
                // Phase 2 v12 widgets.
                "Tabs" => emit_tabs(
                    args,
                    bindings,
                    depth,
                    callbacks,
                    text_slots,
                    arkts_locals,
                    classes,
                    state_registry,
                    lazy_sources,
                    extras,
                    mutations,
                ),
                "Modal" | "Dialog" => emit_modal(args, callbacks),
                "Menu" | "ContextMenu" => emit_menu(args, callbacks),
                "Grid" => emit_grid(
                    args,
                    bindings,
                    depth,
                    callbacks,
                    text_slots,
                    arkts_locals,
                    classes,
                    state_registry,
                    lazy_sources,
                    extras,
                    mutations,
                ),
                // Phase 2 v11: state-driven multi-page nav.
                "NavStack" => emit_nav_stack(
                    args,
                    bindings,
                    depth,
                    callbacks,
                    text_slots,
                    arkts_locals,
                    classes,
                    state_registry,
                    lazy_sources,
                    extras,
                    mutations,
                ),
                // Issue #669 — Chart(kind, w, h) → ArkUI Canvas with the
                // mutator-folded data baked into the draw closure.
                "Chart" => emit_chart(args, bindings, mutations, local_hint, extras),
                // Issue #670 — TreeView(root, onSelect) → ArkUI List with
                // a recursive flatten over a static node graph + @State
                // expanded/selected fields.
                "TreeView" => emit_treeview(args, bindings, callbacks, mutations, extras),
                // Issue #670 — TreeNode(id, label) is a node-graph
                // builder, not a renderable widget. Its only legal
                // appearance in the widget tree is as a TreeView's
                // first arg (handled above). Falling here means the
                // user used TreeNode in an unsupported position —
                // emit a comment + placeholder so the build still works.
                "TreeNode" => format!(
                    "// TreeNode used outside TreeView's first arg — not renderable\n\
                     Text('[TreeNode misplaced]').fontSize(14).fontColor('#888888')"
                ),
                other => format!(
                    "// unsupported perry/ui widget: {} (Phase 2 v12)\n\
                     Text('[unsupported: {}]').fontSize(16).fontColor('#888888')",
                    other, other
                ),
            };
            // Phase 2 v5: detect a trailing StyleProps object and append
            // its modifier chain. Disambiguates Text's 2nd-arg id-vs-style
            // by checking whether the last arg is an object (style) or a
            // plain string (id) — Text("hi", "id") leaves args.last() as
            // a String which extract_style_object returns None for.
            let style_props = args.last().and_then(|a| extract_style_object(a, classes));
            let mut out = if let Some(props) = style_props {
                let modifiers = emit_style_modifiers(&props);
                if !modifiers.is_empty() {
                    format!("{}{}", core, modifiers)
                } else {
                    core
                }
            } else {
                core
            };
            // Issue #408 — append modifier mutations recorded against this
            // widget local. Stack/ScrollView/Section emitters fold AddChild
            // / SetScrollChild / ClearChildren mutations into their bodies
            // directly (see those functions); Modifier mutations are
            // append-only and apply to *every* widget kind so we handle
            // them here unconditionally.
            if let Some(id) = local_hint {
                if let Some(muts) = mutations.get(&id) {
                    out.push_str(&emit_modifier_mutations(muts));
                }
            }
            out
        }
        // Phase 2 v5: ForEach via array.map. When a widget position
        // contains `array.map(item => widgetExpr)`, lower it to ArkUI's
        // ForEach with the closure body emitted in a fresh local-scope
        // env where the closure's param resolves to `__item`.
        Expr::ArrayMap { array, callback } => emit_for_each(
            array,
            callback,
            bindings,
            depth,
            callbacks,
            text_slots,
            arkts_locals,
            classes,
            state_registry,
            lazy_sources,
            extras,
            mutations,
        ),
        // Issue #408 follow-up — ternary `cond ? thenWidget : elseWidget`
        // (HIR `Expr::Conditional`). Mango's pattern:
        //
        //     const toolbarRow = mobile
        //       ? HStack(10, [...mobileChildren])
        //       : HStack(10, [...desktopChildren]);
        //
        // Try to const-fold the condition first — if it resolves to a
        // literal bool, emit the corresponding branch unconditionally.
        // If the condition involves runtime values (function calls,
        // unresolved props), we can't reliably pick — default to the
        // then-branch (the heuristic that picks the "primary" / first-
        // listed case, matching what users typically write first).
        // Without this arm Conditional widget refs fell through to
        // `[unrecognized body]` even though both branches are real
        // widget calls the harvest CAN emit.
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            let folded = evaluate_condition(condition, bindings, &HashMap::new());
            let chosen = match folded {
                Some(false) => else_expr,
                _ => then_expr, // true OR unresolved → take the then-branch
            };
            emit_widget(
                chosen,
                bindings,
                depth,
                callbacks,
                text_slots,
                arkts_locals,
                classes,
                state_registry,
                lazy_sources,
                extras,
                mutations,
                local_hint,
            )
        }
        _ => format!(
            "// unrecognized body expression (must be a perry/ui widget call)\n\
             Text('[unrecognized body]').fontSize(16).fontColor('#888888')"
        ),
    }
}

/// Issue #408 — emit the modifier-only entries from a mutation list as
/// a `.<mod>(...).<mod>(...)` chain. `AddChild` / `ClearChildren` /
/// `SetScrollChild` are skipped here (the structural emitters absorb
/// them); only `Modifier` and `Comment` entries surface.
///
/// Conditional mutations are emitted inline as a JS-style ternary chain:
///   `.padding(8) /* if cond */`
/// since ArkUI's modifier chain is a method-call sequence and we can't
/// inject a multi-statement `if` mid-chain. This is a fidelity loss vs
/// child mutations which DO get full if/else expansion. Document tracked
/// as a v0 limitation; users wanting conditional modifiers can author
/// the trailing `style: {...}` arg directly with the v5 inline-style path.
fn emit_modifier_mutations(muts: &[MutationEntry]) -> String {
    let mut out = String::new();
    for entry in muts {
        match &entry.mutation {
            Mutation::Modifier(s) => {
                if let Some(cond) = &entry.condition {
                    let branch = match cond.branch {
                        Branch::Then => "",
                        Branch::Else => "!",
                    };
                    // Issue #410 — defensive: strip any `*/` from cond_str
                    // before splicing into a `/* ... */` block-comment
                    // marker. `serialize_condition` is audited never to
                    // emit `*/`, but a future change there could
                    // reintroduce the line-82 nested-comment cascade.
                    // Belt-and-braces; the substring check is O(n) on a
                    // string that's already been built.
                    let cond_safe = sanitize_for_block_comment(&cond.cond_str);
                    out.push_str(&format!(
                        " /* if ({branch}({c})) */ {m}",
                        branch = branch,
                        c = cond_safe,
                        m = s
                    ));
                } else {
                    out.push_str(s);
                }
            }
            Mutation::Comment(c) => {
                // #408 follow-up: must be an inline block comment, NOT a
                // `// line comment to EOL`. emit_modifier_mutations is
                // called between modifier chain entries (e.g.
                // `.padding(...) <here> .visibility(...)`), so any
                // line-comment swallows the following `.modifier()` call
                // — `\n// foo.visibility(...)` parses as a single comment
                // line and the `.visibility` is silently dropped.
                // Inline block comments don't have that problem; sanitize
                // for the same `*/`-leaks-out-of-comment hazard #410
                // already flags on cond_str.
                let safe = sanitize_for_block_comment(c);
                out.push_str(&format!(" /* {} */", safe));
            }
            // Phase 2 v3.5 — leaf-mutator binding for `widgetSetHidden`.
            // Emits `.visibility(this.hidden_<id> ? Visibility.Hidden :
            // Visibility.Visible)` so ArkUI re-renders the widget when
            // ArkTS pumps the runtime drain queue and flips the
            // `@State hidden_<id>` field. Conditional bindings are not
            // expected (the binding is a single emit per widget), so we
            // ignore the condition here.
            Mutation::VisibilityBinding(synth_id) => {
                out.push_str(&format!(
                    ".visibility(this.hidden_{id} ? Visibility.Hidden : Visibility.Visible)",
                    id = synth_id
                ));
            }
            // Structural mutations are handled by the per-widget emitters.
            Mutation::AddChild(_) | Mutation::ClearChildren | Mutation::SetScrollChild(_) => {}
            // Issue #669/#670 — Chart + TreeView mutations are folded by
            // `emit_chart` / `emit_treeview` directly. They're already
            // consumed; nothing more to emit as a modifier chain entry.
            Mutation::ChartAddDataPoint(_, _)
            | Mutation::ChartClearData
            | Mutation::ChartSetTitle(_)
            | Mutation::ChartReload
            | Mutation::TreeAddChild(_) => {}
        }
    }
    out
}

/// Issue #410 — replace any `*/` substring with `*\u{200b}/` (an inserted
/// zero-width space) so the result can be safely spliced inside a `/* ... */`
/// block comment marker without closing the outer comment early. The
/// zero-width space renders invisibly in editor diagnostics so the comment
/// stays human-readable. Also handles the `*//` edge case (where two
/// adjacent close-comment markers would survive a single replacement).
fn sanitize_for_block_comment(s: &str) -> String {
    if !s.contains("*/") {
        return s.to_string();
    }
    s.replace("*/", "*\u{200b}/")
}

/// Issue #408 — return the list of effective AddChild expressions for a
/// widget local, after honoring `ClearChildren` (which drops earlier
/// AddChild entries from the same condition group + branch).
///
/// Returns `(unconditional_children, conditional_groups)` where each
/// conditional_group is `(cond_str, then_children, else_children)`.
/// All three lists hold the user-supplied child Expr references in
/// source order.
#[allow(clippy::type_complexity)]
fn fold_child_mutations(
    muts: &[MutationEntry],
) -> (Vec<Expr>, Vec<(String, Vec<Expr>, Vec<Expr>, Vec<String>)>) {
    let mut unconditional: Vec<Expr> = Vec::new();
    // group_id → (cond_str, then_children, else_children, comments)
    let mut groups: Vec<(u32, String, Vec<Expr>, Vec<Expr>, Vec<String>)> = Vec::new();
    let group_idx = |groups: &mut Vec<(u32, String, Vec<Expr>, Vec<Expr>, Vec<String>)>,
                     id: u32,
                     cond_str: &str|
     -> usize {
        if let Some(i) = groups.iter().position(|(g, _, _, _, _)| *g == id) {
            i
        } else {
            groups.push((id, cond_str.to_string(), Vec::new(), Vec::new(), Vec::new()));
            groups.len() - 1
        }
    };
    for entry in muts {
        match (&entry.mutation, &entry.condition) {
            (Mutation::AddChild(child), None) => unconditional.push(child.clone()),
            (Mutation::AddChild(child), Some(cond)) => {
                let i = group_idx(&mut groups, cond.group, &cond.cond_str);
                match cond.branch {
                    Branch::Then => groups[i].2.push(child.clone()),
                    Branch::Else => groups[i].3.push(child.clone()),
                }
            }
            (Mutation::ClearChildren, None) => unconditional.clear(),
            (Mutation::ClearChildren, Some(cond)) => {
                let i = group_idx(&mut groups, cond.group, &cond.cond_str);
                match cond.branch {
                    Branch::Then => groups[i].2.clear(),
                    Branch::Else => groups[i].3.clear(),
                }
            }
            (Mutation::Comment(c), None) => unconditional_push_comment(&mut unconditional, c),
            (Mutation::Comment(c), Some(cond)) => {
                let i = group_idx(&mut groups, cond.group, &cond.cond_str);
                groups[i].4.push(c.clone());
            }
            // Modifier mutations don't affect children, SetScrollChild is
            // handled by emit_scrollview directly.
            _ => {}
        }
    }
    let conds: Vec<(String, Vec<Expr>, Vec<Expr>, Vec<String>)> = groups
        .into_iter()
        .map(|(_id, cs, t, e, c)| (cs, t, e, c))
        .collect();
    (unconditional, conds)
}

/// Helper: push a comment Expr by smuggling it through a sentinel that
/// downstream emitters recognize. Today comments are dropped at child
/// emission since `emit_widget` requires a real widget call. We instead
/// surface comments at the post-mutation modifier-chain emit site.
/// This helper is here as the API but currently a no-op — kept to make
/// the design explicit.
fn unconditional_push_comment(_out: &mut Vec<Expr>, _comment: &str) {
    // Intentionally empty: comments are surfaced via emit_modifier_mutations'
    // post-emit `// ...` lines, not as fake child widgets.
}

/// Issue #408 — emit a string of ArkUI children (already-rendered) for
/// the unconditional + conditional groups produced by fold_child_mutations.
/// Each conditional group emits as `if (cond) { thenA(); thenB(); } else { elseA(); }`,
/// inlined into the parent's body alongside the unconditional siblings.
///
/// Caller is responsible for indenting the result appropriately. Returns
/// an empty string if no children registered, so callers can short-circuit.
#[allow(clippy::too_many_arguments)]
fn emit_mutation_children(
    muts: &[MutationEntry],
    bindings: &HashMap<LocalId, Expr>,
    depth: usize,
    callbacks: &mut Vec<Expr>,
    text_slots: &mut Vec<TextSlot>,
    arkts_locals: &HashMap<LocalId, String>,
    classes: &[Class],
    state_registry: &HashMap<LocalId, StateBinding>,
    lazy_sources: &mut Vec<LazyDataSource>,
    extras: &mut HarvestExtras,
    mutations: &HashMap<LocalId, Vec<MutationEntry>>,
) -> Vec<String> {
    let (unconditional, conds) = fold_child_mutations(muts);
    let mut out: Vec<String> = Vec::new();
    for child in &unconditional {
        out.push(emit_widget(
            child,
            bindings,
            depth,
            callbacks,
            text_slots,
            arkts_locals,
            classes,
            state_registry,
            lazy_sources,
            extras,
            mutations,
            None,
        ));
    }
    for (cond_str, then_kids, else_kids, comments) in &conds {
        let inner_indent = "    ".repeat(depth + 1);
        let outer_indent = "    ".repeat(depth);
        let then_lines: Vec<String> = then_kids
            .iter()
            .map(|c| {
                emit_widget(
                    c,
                    bindings,
                    depth + 1,
                    callbacks,
                    text_slots,
                    arkts_locals,
                    classes,
                    state_registry,
                    lazy_sources,
                    extras,
                    mutations,
                    None,
                )
            })
            .collect();
        let else_lines: Vec<String> = else_kids
            .iter()
            .map(|c| {
                emit_widget(
                    c,
                    bindings,
                    depth + 1,
                    callbacks,
                    text_slots,
                    arkts_locals,
                    classes,
                    state_registry,
                    lazy_sources,
                    extras,
                    mutations,
                    None,
                )
            })
            .collect();
        let comment_block = if comments.is_empty() {
            String::new()
        } else {
            comments
                .iter()
                .map(|c| format!("{}// {}\n", inner_indent, c))
                .collect()
        };
        let then_body = if then_lines.is_empty() {
            format!("{}// (no children)", inner_indent)
        } else {
            then_lines
                .iter()
                .map(|c| {
                    c.lines()
                        .map(|line| format!("{}{}", inner_indent, line))
                        .collect::<Vec<_>>()
                        .join("\n")
                })
                .collect::<Vec<_>>()
                .join("\n")
        };
        let block = if else_lines.is_empty() {
            format!(
                "if ({cond}) {{\n\
                 {comments}{body}\n\
                 {outer}}}",
                cond = cond_str,
                comments = comment_block,
                body = then_body,
                outer = outer_indent,
            )
        } else {
            let else_body = else_lines
                .iter()
                .map(|c| {
                    c.lines()
                        .map(|line| format!("{}{}", inner_indent, line))
                        .collect::<Vec<_>>()
                        .join("\n")
                })
                .collect::<Vec<_>>()
                .join("\n");
            format!(
                "if ({cond}) {{\n\
                 {comments}{body}\n\
                 {outer}}} else {{\n\
                 {else_body}\n\
                 {outer}}}",
                cond = cond_str,
                comments = comment_block,
                body = then_body,
                outer = outer_indent,
                else_body = else_body,
            )
        };
        out.push(block);
    }
    out
}

/// Phase 2 v5: emit ArkUI `ForEach(<array>, (__item) => { <body> })`
/// from a `Expr::ArrayMap { array, callback }` HIR node. The callback's
/// closure parameter is bound to `__item` in arkts_locals so any
/// `LocalGet(param_id)` inside the body resolves correctly.
///
/// The array source must be a literal `Expr::Array` or a `LocalGet`
/// that resolves to a top-level binding (via `bindings`). Other shapes
/// (e.g., complex computed expressions) fall back to a degraded inline
/// emit so the build doesn't break.
#[allow(clippy::too_many_arguments)]
fn emit_for_each(
    array: &Expr,
    callback: &Expr,
    bindings: &HashMap<LocalId, Expr>,
    depth: usize,
    callbacks: &mut Vec<Expr>,
    text_slots: &mut Vec<TextSlot>,
    arkts_locals: &HashMap<LocalId, String>,
    classes: &[Class],
    state_registry: &HashMap<LocalId, StateBinding>,
    lazy_sources: &mut Vec<LazyDataSource>,
    extras: &mut HarvestExtras,
    mutations: &HashMap<LocalId, Vec<MutationEntry>>,
) -> String {
    let array_src = arkts_array_source(array, bindings);
    let (param_id, body_expr) = match callback {
        Expr::Closure { params, body, .. } if !params.is_empty() => {
            // The closure body is a Vec<Stmt>; we expect a single return-
            // expr or expression-statement. Take the first Expr we find.
            let body_expr = body.iter().find_map(|s| match s {
                Stmt::Return(Some(e)) => Some(e.clone()),
                Stmt::Expr(e) => Some(e.clone()),
                _ => None,
            });
            (Some(params[0].id), body_expr)
        }
        _ => (None, None),
    };
    let inner_indent = "    ".repeat(depth + 1);
    let outer_indent = "    ".repeat(depth);
    let (param_name, body_str) = match (param_id, body_expr) {
        (Some(pid), Some(body)) => {
            let mut locals = arkts_locals.clone();
            locals.insert(pid, "__item".to_string());
            let inner = emit_widget(
                &body,
                bindings,
                depth + 1,
                callbacks,
                text_slots,
                &locals,
                classes,
                state_registry,
                lazy_sources,
                extras,
                mutations,
                None,
            );
            ("__item".to_string(), inner)
        }
        _ => (
            "__item".to_string(),
            "Text('[non-closure ForEach body]').fontSize(16).fontColor('#888888')".to_string(),
        ),
    };
    let indented_body = body_str
        .lines()
        .map(|l| format!("{}{}", inner_indent, l))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "ForEach({arr}, ({pname}: any) => {{\n\
         {body}\n\
         {outer}}}, ({pname}: any) => {pname})",
        arr = array_src,
        pname = param_name,
        body = indented_body,
        outer = outer_indent,
    )
}

/// Emit a TS expression for the array source of a ForEach. Supports
/// literal `Expr::Array(items)` (serialized inline) and `Expr::LocalGet`
/// resolved to a top-level binding's name. Other shapes fall back to
/// an empty `[]` with a note comment.
fn arkts_array_source(e: &Expr, bindings: &HashMap<LocalId, Expr>) -> String {
    match e {
        Expr::Array(items) => {
            let parts: Vec<String> = items.iter().map(arkts_value_literal).collect();
            format!("[{}]", parts.join(", "))
        }
        Expr::LocalGet(_id) => {
            // Look up the binding's init expr; if it's an Array literal,
            // serialize. Otherwise fall through to empty.
            if let Expr::LocalGet(id) = e {
                if let Some(Expr::Array(items)) = bindings.get(id) {
                    let parts: Vec<String> = items.iter().map(arkts_value_literal).collect();
                    return format!("[{}]", parts.join(", "));
                }
            }
            // Phase 2 v5 limitation: complex array sources need real
            // ArkTS-side state binding. Emit a placeholder.
            "[/* unresolved ForEach source — needs Phase 2 v6 state binding */]".to_string()
        }
        _ => "[/* unsupported ForEach source */]".to_string(),
    }
}

/// Serialize a literal-shaped `Expr` to TS source for inline array lit.
fn arkts_value_literal(e: &Expr) -> String {
    match e {
        Expr::String(s) => arkts_string_lit(s),
        Expr::Number(n) => fmt_num(*n),
        Expr::Integer(n) => format!("{}", n),
        Expr::Bool(b) => format!("{}", b),
        _ => "null".to_string(),
    }
}

/// `Text("hi")` → `Text('hi').fontSize(20)`.
///
/// Phase 2 v3 Option 2: `Text("hi", "id")` → registers a reactive slot.
/// The widget emits `Text(this.text_<id>)` instead of a string literal,
/// and `wrap_index_page` adds `@State text_<id>: string = 'hi'` to the
/// page struct. User code calls `setText("id", newValue)` from inside
/// a closure to update.
///
/// Non-string-literal args fall back to a placeholder so unsupported
/// shapes don't break the build.
fn emit_text(
    args: &[Expr],
    text_slots: &mut Vec<TextSlot>,
    arkts_locals: &HashMap<LocalId, String>,
    bindings: &HashMap<LocalId, Expr>,
) -> String {
    // Phase 2 v5: inside a ForEach body, `Text(item)` where `item` is
    // the closure's loop param resolves via arkts_locals → `Text(__item)`.
    let first = args.first();
    let content_str = match first {
        Some(Expr::String(content)) => Some(arkts_string_lit(content)),
        Some(Expr::LocalGet(id)) if arkts_locals.contains_key(id) => arkts_locals.get(id).cloned(),
        // Fallback: try to resolve through bindings + Conditional +
        // I18nString (perry/i18n's `t('key')` lowers to I18nString).
        // This catches Mango's `Text(t('Welcome to Mango'))` shape.
        Some(other) => resolve_string_arg(other, bindings).map(|s| arkts_string_lit(&s)),
        None => None,
    };
    let Some(content_arg) = content_str else {
        return "Text('[non-literal Text arg]').fontSize(20).fontColor('#888888')".to_string();
    };
    if let Some(Expr::String(id)) = args.get(1) {
        // Reactive Text. Sanitize the id so it's a valid ArkTS field-
        // name suffix (alphanumeric + underscore). The original id stays
        // alongside it for the runtime-side switch match.
        // Only the literal-string form is reactive — ForEach's __item
        // binding is per-iteration and doesn't persist to a slot.
        if let Some(Expr::String(initial)) = first {
            let safe = sanitize_text_id(id);
            text_slots.push(TextSlot {
                original_id: id.clone(),
                field_id: safe.clone(),
                initial: initial.clone(),
            });
            return format!("Text(this.text_{}).fontSize(20)", safe);
        }
    }
    format!("Text({}).fontSize(20)", content_arg)
}

/// Extract a `style: {...}` object from a widget arg. Handles both
/// `Expr::Object(props)` (open shape) and Perry's closed-shape
/// optimization `Expr::New { class_name: "__AnonShape_*", args }` where
/// the class's fields list correlates positionally with args. Used by
/// `emit_style_modifiers` to map StyleProps into ArkUI modifiers.
///
/// Phase 2 v5 — ergonomic parity with macOS/iOS/etc inline styling.
fn extract_style_object(arg: &Expr, classes: &[Class]) -> Option<Vec<(String, Expr)>> {
    match arg {
        Expr::Object(props) => Some(props.clone()),
        Expr::New {
            class_name, args, ..
        } if class_name.starts_with("__AnonShape_") => {
            let class = classes.iter().find(|c| &c.name == class_name)?;
            // Pair each field with its positional arg; missing args fall through.
            let pairs: Vec<(String, Expr)> = class
                .fields
                .iter()
                .enumerate()
                .filter_map(|(i, f)| args.get(i).map(|a| (f.name.clone(), a.clone())))
                .collect();
            Some(pairs)
        }
        _ => None,
    }
}

/// Map a Perry color expression to an ArkUI color string.
///   - `Expr::String("blue")` / `"#3B82F6"` → quoted string passthrough
///   - `Expr::Object([(r,…),(g,…),(b,…),(a,…)])` (PerryColor) → `'rgba(R,G,B,A)'`
///     where channels are scaled to 0..255 / 0..1 per CSS rgba() convention
fn arkts_color_value(e: &Expr) -> String {
    match e {
        Expr::String(s) => arkts_string_lit(s),
        Expr::Object(props) => {
            let chan = |name: &str, default: f64| -> f64 {
                props
                    .iter()
                    .find(|(k, _)| k == name)
                    .and_then(|(_, v)| match v {
                        Expr::Number(n) => Some(*n),
                        Expr::Integer(n) => Some(*n as f64),
                        _ => None,
                    })
                    .unwrap_or(default)
            };
            let r = (chan("r", 0.0) * 255.0).round() as i64;
            let g = (chan("g", 0.0) * 255.0).round() as i64;
            let b = (chan("b", 0.0) * 255.0).round() as i64;
            let a = chan("a", 1.0);
            format!("'rgba({}, {}, {}, {})'", r, g, b, fmt_num(a))
        }
        _ => "'#000000'".to_string(),
    }
}

/// Phase 2 v13 — map a CSS-style curve string to ArkUI's `Curve` enum.
/// ArkUI `Curve` lives at `@ohos.curves` and the values match the W3C
/// timing-function names with PascalCase (`Curve.Linear`, `Curve.Ease`,
/// `Curve.EaseInOut`, etc.). Unrecognized values fall back to `Curve.Ease`.
fn arkts_curve_value(s: &str) -> String {
    let name = match s {
        "linear" => "Linear",
        "ease" => "Ease",
        "ease-in" | "easeIn" => "EaseIn",
        "ease-out" | "easeOut" => "EaseOut",
        "ease-in-out" | "easeInOut" => "EaseInOut",
        "fast-out-slow-in" => "FastOutSlowIn",
        "linear-out-slow-in" => "LinearOutSlowIn",
        "fast-out-linear-in" => "FastOutLinearIn",
        "extreme-deceleration" => "ExtremeDeceleration",
        "sharp" => "Sharp",
        "rhythm" => "Rhythm",
        "smooth" => "Smooth",
        "friction" => "Friction",
        _ => "Ease",
    };
    format!("Curve.{}", name)
}

/// Map a `StyleProps` object to an ArkUI modifier chain like
/// `.backgroundColor('blue').borderRadius(8).opacity(0.95)`.
///
/// Phase 2 v5 covers the high-traffic props: backgroundColor, color,
/// fontSize, fontWeight, fontFamily, borderRadius, padding, opacity,
/// hidden, borderColor + borderWidth (as combined `.border({...})`).
/// Skipped (complex / multi-arg ArkUI shape): shadow, gradient,
/// textDecoration, tooltip, animation, transition — these would each
/// need their own ArkUI modifier and are deferred to Phase 2 v13.
fn emit_style_modifiers(props: &[(String, Expr)]) -> String {
    let mut out = String::new();
    let mut border_color: Option<String> = None;
    let mut border_width: Option<String> = None;
    for (k, v) in props {
        match k.as_str() {
            "backgroundColor" => {
                out.push_str(&format!(".backgroundColor({})", arkts_color_value(v)));
            }
            "color" => {
                // ArkUI's `.fontColor` works on Text; non-text widgets
                // silently ignore it.
                out.push_str(&format!(".fontColor({})", arkts_color_value(v)));
            }
            "fontSize" => {
                if let Some(n) = numeric_expr(v) {
                    out.push_str(&format!(".fontSize({})", fmt_num(n)));
                }
            }
            "fontWeight" => {
                if let Some(n) = numeric_expr(v) {
                    out.push_str(&format!(".fontWeight({})", fmt_num(n)));
                }
            }
            "fontFamily" => {
                if let Expr::String(s) = v {
                    out.push_str(&format!(".fontFamily({})", arkts_string_lit(s)));
                }
            }
            "borderRadius" => {
                if let Some(n) = numeric_expr(v) {
                    out.push_str(&format!(".borderRadius({})", fmt_num(n)));
                }
            }
            "borderColor" => {
                border_color = Some(arkts_color_value(v));
            }
            "borderWidth" => {
                if let Some(n) = numeric_expr(v) {
                    border_width = Some(fmt_num(n));
                }
            }
            "padding" => match v {
                Expr::Number(n) => out.push_str(&format!(".padding({})", fmt_num(*n))),
                Expr::Integer(n) => out.push_str(&format!(".padding({})", *n)),
                Expr::Object(sides) => {
                    let side = |name: &str| -> Option<f64> {
                        sides
                            .iter()
                            .find(|(k, _)| k == name)
                            .and_then(|(_, v)| numeric_expr(v))
                    };
                    let parts: Vec<String> = ["top", "right", "bottom", "left"]
                        .iter()
                        .filter_map(|s| side(s).map(|n| format!("{}: {}", s, fmt_num(n))))
                        .collect();
                    if !parts.is_empty() {
                        out.push_str(&format!(".padding({{ {} }})", parts.join(", ")));
                    }
                }
                _ => {}
            },
            "opacity" => {
                if let Some(n) = numeric_expr(v) {
                    out.push_str(&format!(".opacity({})", fmt_num(n)));
                }
            }
            "hidden" => {
                let is_hidden = matches!(v, Expr::Bool(true));
                if is_hidden {
                    out.push_str(".visibility(Visibility.Hidden)");
                }
            }
            // Phase 2 v13 — animation/transition/shadow/textDecoration.
            "animation" => {
                if let Expr::Object(props) = v {
                    let mut parts: Vec<String> = Vec::new();
                    for (k2, v2) in props {
                        match k2.as_str() {
                            "duration" => {
                                if let Some(n) = numeric_expr(v2) {
                                    parts.push(format!("duration: {}", fmt_num(n)));
                                }
                            }
                            "curve" => {
                                if let Expr::String(s) = v2 {
                                    parts.push(format!("curve: {}", arkts_curve_value(s)));
                                }
                            }
                            "delay" => {
                                if let Some(n) = numeric_expr(v2) {
                                    parts.push(format!("delay: {}", fmt_num(n)));
                                }
                            }
                            "iterations" => {
                                if let Some(n) = numeric_expr(v2) {
                                    parts.push(format!("iterations: {}", fmt_num(n)));
                                }
                            }
                            _ => {}
                        }
                    }
                    if !parts.is_empty() {
                        out.push_str(&format!(".animation({{ {} }})", parts.join(", ")));
                    }
                }
            }
            "shadow" => {
                if let Expr::Object(props) = v {
                    let mut parts: Vec<String> = Vec::new();
                    for (k2, v2) in props {
                        match k2.as_str() {
                            "color" => {
                                parts.push(format!("color: {}", arkts_color_value(v2)));
                            }
                            "blur" => {
                                if let Some(n) = numeric_expr(v2) {
                                    parts.push(format!("radius: {}", fmt_num(n)));
                                }
                            }
                            "offsetX" => {
                                if let Some(n) = numeric_expr(v2) {
                                    parts.push(format!("offsetX: {}", fmt_num(n)));
                                }
                            }
                            "offsetY" => {
                                if let Some(n) = numeric_expr(v2) {
                                    parts.push(format!("offsetY: {}", fmt_num(n)));
                                }
                            }
                            _ => {}
                        }
                    }
                    if !parts.is_empty() {
                        out.push_str(&format!(".shadow({{ {} }})", parts.join(", ")));
                    }
                }
            }
            "textDecoration" => {
                if let Expr::String(s) = v {
                    let kind = match s.as_str() {
                        "underline" => Some("Underline"),
                        "strikethrough" | "line-through" => Some("LineThrough"),
                        "overline" => Some("Overline"),
                        "none" => Some("None"),
                        _ => None,
                    };
                    if let Some(k) = kind {
                        out.push_str(&format!(
                            ".decoration({{ type: TextDecorationType.{} }})",
                            k
                        ));
                    }
                }
            }
            // Phase 2 v13 deferred: gradient, transition, tooltip — these
            // each need more complex ArkUI shapes (linearGradient, multi-
            // part transition config, custom-component popup) and are
            // tracked as v13.5 follow-ups.
            _ => {}
        }
    }
    // Joint border: ArkUI's `.border({color, width})` is one modifier
    // taking a config object; emit only if at least one was set.
    if border_color.is_some() || border_width.is_some() {
        let mut parts: Vec<String> = Vec::new();
        if let Some(w) = border_width {
            parts.push(format!("width: {}", w));
        }
        if let Some(c) = border_color {
            parts.push(format!("color: {}", c));
        }
        out.push_str(&format!(".border({{ {} }})", parts.join(", ")));
    }
    out
}

/// Extract a Number / Integer expression as `f64`. Returns None for
/// anything else (including `Expr::String` parseable numerals — those
/// are intentionally rejected because StyleProps forbids them).
fn numeric_expr(e: &Expr) -> Option<f64> {
    match e {
        Expr::Number(n) => Some(*n),
        Expr::Integer(n) => Some(*n as f64),
        _ => None,
    }
}

/// Sanitize an arbitrary string id into a valid ArkTS field-name suffix.
/// Replaces non-[a-zA-Z0-9_] with `_`. Front-pads with `x` if it starts
/// with a digit. Empty input → `default`.
fn sanitize_text_id(s: &str) -> String {
    if s.is_empty() {
        return "default".to_string();
    }
    let mut out: String = s
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if out
        .chars()
        .next()
        .map(|c| c.is_ascii_digit())
        .unwrap_or(false)
    {
        out.insert(0, 'x');
    }
    out
}

/// VStack/HStack: detect (Array, ...) vs (Number, Array, ...) signatures.
/// Recurse into the children array via `emit_widget`. Spacing prop
/// becomes `Column({space: <n>})` / `Row({space: <n>})`. ArkUI's default
/// of 0 makes spacing-less stacks look cramped, so we default to 8 which
/// matches the perry-ui-macos default.
///
/// Issue #408 — when `local_hint` is set and `mutations` has recorded
/// AddChild / ClearChildren entries against this widget, the recorded
/// children are appended after the explicit children list (and ClearChildren
/// from the mutator side drops earlier explicit children too). Conditional
/// children become `if (cond) { ChildA() } else { ChildB() }` blocks.
#[allow(clippy::too_many_arguments)]
fn emit_stack(
    arkui_kind: &str,
    args: &[Expr],
    bindings: &HashMap<LocalId, Expr>,
    depth: usize,
    callbacks: &mut Vec<Expr>,
    text_slots: &mut Vec<TextSlot>,
    arkts_locals: &HashMap<LocalId, String>,
    classes: &[Class],
    state_registry: &HashMap<LocalId, StateBinding>,
    lazy_sources: &mut Vec<LazyDataSource>,
    extras: &mut HarvestExtras,
    mutations: &HashMap<LocalId, Vec<MutationEntry>>,
    local_hint: Option<LocalId>,
) -> String {
    // First-arg shape detection — same logic as lower_call/native.rs:91.
    let (spacing, children_idx) = match args.first() {
        Some(Expr::Array(_)) | Some(Expr::ArrayMap { .. }) => (8.0, 0),
        Some(Expr::Number(n)) => (*n, 1),
        Some(Expr::Integer(n)) => (*n as f64, 1),
        _ => (8.0, 0),
    };

    let mut children = match args.get(children_idx) {
        Some(Expr::Array(items)) => items
            .iter()
            .map(|child| {
                emit_widget(
                    child,
                    bindings,
                    depth + 1,
                    callbacks,
                    text_slots,
                    arkts_locals,
                    classes,
                    state_registry,
                    lazy_sources,
                    extras,
                    mutations,
                    None,
                )
            })
            .collect::<Vec<_>>(),
        // Phase 2 v5: stack(items.map(item => Widget)) — the children
        // arg IS the array.map. Emit a single ForEach as the only child
        // of the Column/Row.
        Some(am @ Expr::ArrayMap { .. }) => vec![emit_widget(
            am,
            bindings,
            depth + 1,
            callbacks,
            text_slots,
            arkts_locals,
            classes,
            state_registry,
            lazy_sources,
            extras,
            mutations,
            None,
        )],
        Some(_) => vec![format!(
            "// children arg wasn't an array literal — Phase 2 v1.5 limitation\n\
             Text('[non-array children]').fontSize(16).fontColor('#888888')"
        )],
        None => vec![],
    };

    // Issue #408 — fold AddChild + ClearChildren mutations.
    if let Some(id) = local_hint {
        if let Some(muts) = mutations.get(&id) {
            // ClearChildren at the unconditional level wipes the explicit
            // children list emitted from the constructor's `Array(children)`.
            // We approximate this by checking the mutation list for any
            // unconditional ClearChildren — if found, drop existing children.
            let has_unconditional_clear = muts
                .iter()
                .any(|e| matches!(e.mutation, Mutation::ClearChildren) && e.condition.is_none());
            if has_unconditional_clear {
                children.clear();
            }
            let extra = emit_mutation_children(
                muts,
                bindings,
                depth + 1,
                callbacks,
                text_slots,
                arkts_locals,
                classes,
                state_registry,
                lazy_sources,
                extras,
                mutations,
            );
            children.extend(extra);
        }
    }

    let inner_indent = "    ".repeat(depth + 1);
    let outer_indent = "    ".repeat(depth);

    let body = if children.is_empty() {
        String::new()
    } else {
        children
            .iter()
            .map(|c| {
                c.lines()
                    .map(|line| format!("{}{}", inner_indent, line))
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .collect::<Vec<_>>()
            .join("\n")
    };

    format!(
        "{kind}({{ space: {space} }}) {{\n{body}\n{outer}}}",
        kind = arkui_kind,
        space = fmt_num(spacing),
        body = body,
        outer = outer_indent,
    )
}

/// `Button("label", onPress)` → `Button('label').onClick(() => { ... })`.
/// The onClick body invokes the registered closure via NAPI then drains
/// the toast queue (Phase 2 v3 Option 1):
///
/// ```text
/// perryEntry.invokeCallback(<idx>);
/// let __t = perryEntry.drainToast();
/// while (__t !== undefined) {
///     promptAction.showToast({ message: __t });
///     __t = perryEntry.drainToast();
/// }
/// ```
///
/// The drain loop runs unconditionally — most closures don't enqueue
/// toasts, so it's a single fast `drainToast()` returning undefined.
/// When the user calls `showToast("Saved!")` from inside the closure,
/// the message lands on the queue and pops out here as a popup banner.
///
/// Non-closure second args (or absent) emit a label-only Button with no
/// onClick — preserves v1.5 behavior for simpler tests.
fn emit_button(args: &[Expr], callbacks: &mut Vec<Expr>) -> String {
    let label = first_string_arg(args).unwrap_or_else(|| "Button".to_string());
    let onclick_attached = match args.get(1) {
        Some(closure @ Expr::Closure { .. }) => {
            let idx = callbacks.len();
            callbacks.push(closure.clone());
            format!(
                ".onClick(() => {{\n    \
                 perryEntry.invokeCallback({});\n    \
                 {drain}\
                 }})",
                idx,
                drain = drain_loop_body()
            )
        }
        _ => String::new(),
    };
    format!(
        "Button({}).fontSize(16){}",
        arkts_string_lit(&label),
        onclick_attached
    )
}

/// Multi-pass drain after a closure body returns. Used by Button.onClick
/// (Phase 2 v2) and Toggle/TextField/Slider.onChange (v2.5):
///   1. drainToast loop → promptAction.showToast({message})
///   2. drainTextUpdate loop → this.applyTextUpdate(id, value)
///   3. drainVisibilityUpdate loop → this.applyVisibilityUpdate(id, hidden)  [v3.5]
/// `invokeCallback` itself is emitted by the caller because it varies
/// (callN with N-arg widgets, plus ArkUI's per-widget onChange shape).
fn drain_loop_body() -> String {
    "let __t = perryEntry.drainToast();\n    \
     while (__t !== undefined) { \
     promptAction.showToast({ message: __t }); \
     __t = perryEntry.drainToast(); \
     }\n    \
     let __u = perryEntry.drainTextUpdate();\n    \
     while (__u !== undefined) { \
     this.applyTextUpdate(__u.id, __u.value); \
     __u = perryEntry.drainTextUpdate(); \
     }\n    \
     let __v = perryEntry.drainVisibilityUpdate();\n    \
     while (__v !== undefined) { \
     this.applyVisibilityUpdate(__v.id, __v.hidden); \
     __v = perryEntry.drainVisibilityUpdate(); \
     }\n    \
     let __c = perryEntry.drainContentViewUpdate();\n    \
     while (__c !== undefined) { \
     this.applyContentViewUpdate(__c.id, __c.view); \
     __c = perryEntry.drainContentViewUpdate(); \
     }\n  "
        .to_string()
}

/// `TextField(placeholder, onChange)` → `TextInput(...).onChange(...)`.
/// Phase 2 v2.5: when `onChange` is a closure, register it in the slot
/// table and emit an `onChange((value: string) => perryEntry.invokeCallback1(idx, value))`
/// handler that also drains toast + text-update queues.
fn emit_textfield(args: &[Expr], callbacks: &mut Vec<Expr>) -> String {
    let placeholder = first_string_arg(args).unwrap_or_default();
    let onchange = match args.get(1) {
        Some(closure @ Expr::Closure { .. }) => {
            let idx = callbacks.len();
            callbacks.push(closure.clone());
            format!(
                ".onChange((value: string) => {{\n    \
                 perryEntry.invokeCallback1({}, value);\n    \
                 {drain}\
                 }})",
                idx,
                drain = drain_loop_body()
            )
        }
        _ => String::new(),
    };
    format!(
        "TextInput({{ placeholder: {} }}){}",
        arkts_string_lit(&placeholder),
        onchange,
    )
}

/// `Toggle(label, onChange)` → label as a sibling Text + ArkUI's Toggle
/// in a Row. Phase 2 v2.5: closure receives `(isOn: boolean)`.
fn emit_toggle(args: &[Expr], callbacks: &mut Vec<Expr>) -> String {
    let label = first_string_arg(args).unwrap_or_default();
    let onchange = match args.get(1) {
        Some(closure @ Expr::Closure { .. }) => {
            let idx = callbacks.len();
            callbacks.push(closure.clone());
            format!(
                ".onChange((isOn: boolean) => {{\n    \
                 perryEntry.invokeCallback1({}, isOn);\n    \
                 {drain}\
                 }})",
                idx,
                drain = drain_loop_body()
            )
        }
        _ => String::new(),
    };
    if label.is_empty() {
        format!(
            "Toggle({{ type: ToggleType.Switch, isOn: false }}){}",
            onchange
        )
    } else {
        format!(
            "Row({{ space: 8 }}) {{\n\
             \x20\x20\x20\x20Text({}).fontSize(16)\n\
             \x20\x20\x20\x20Toggle({{ type: ToggleType.Switch, isOn: false }}){}\n\
             }}",
            arkts_string_lit(&label),
            onchange,
        )
    }
}

/// `Slider(min, max, onChange)` → ArkUI Slider with onChange. Phase 2
/// v2.5: closure receives `(value: number)`. ArkUI's onChange callback
/// is `(value: number, mode: SliderChangeMode)` — we ignore `mode` and
/// only forward `value`.
fn emit_slider(args: &[Expr], callbacks: &mut Vec<Expr>) -> String {
    let min = numeric_arg(args, 0).unwrap_or(0.0);
    let max = numeric_arg(args, 1).unwrap_or(100.0);
    let onchange = match args.get(2) {
        Some(closure @ Expr::Closure { .. }) => {
            let idx = callbacks.len();
            callbacks.push(closure.clone());
            format!(
                ".onChange((value: number, _mode: SliderChangeMode) => {{\n    \
                 perryEntry.invokeCallback1({}, value);\n    \
                 {drain}\
                 }})",
                idx,
                drain = drain_loop_body()
            )
        }
        _ => String::new(),
    };
    format!(
        "Slider({{ value: {min}, min: {min}, max: {max}, step: 1, style: SliderStyle.OutSet }}){onchange}",
        min = fmt_num(min),
        max = fmt_num(max),
        onchange = onchange,
    )
}

/// `Image(src)` / `ImageFile(src)` → `Image('src').width('100%').height(200)`.
/// Default sizing matches the perry-ui-* native default of "fill width,
/// 200pt tall"; users can wrap in further sizing via container modifiers
/// later (Phase 2 v5 will likely accept a `style: { ... }` trailing arg).
///
/// Resolves the src arg through `bindings` so common patterns work:
/// - `ImageFile('assets/icon.png')` — direct literal
/// - `ImageFile(LOGO_PATH)` where `const LOGO_PATH = 'assets/...'`
/// - `ImageFile(mobile ? 'path-mobile' : 'path-desktop')` — ternary,
///   evaluates the condition when foldable, otherwise picks the
///   then-branch (mirrors the `Expr::Conditional` widget heuristic).
///
/// Falls back to a placeholder Text only when the chain bottoms out
/// at a non-string leaf (function call, prop access, etc.).
fn emit_image(args: &[Expr], bindings: &HashMap<LocalId, Expr>) -> String {
    let Some(first) = args.first() else {
        return "Text('[non-literal Image src]').fontSize(16).fontColor('#888888')".to_string();
    };
    let Some(src) = resolve_string_arg(first, bindings) else {
        return "Text('[non-literal Image src]').fontSize(16).fontColor('#888888')".to_string();
    };
    // Phase 2 v13 — recognize the `@app.media/<name>` resource path
    // shape and emit ArkUI's `$r('app.media.<name>')` accessor instead
    // of a quoted string literal. Plain URLs / file paths still pass
    // through as quoted strings.
    //
    // `assets/X.png` paths (Mango's convention) translate to
    // `$rawfile('X.png')` — the HAP build (`harmonyos_hap.rs::copy_
    // assets_to_rawfile`) copies the project's `assets/` directory
    // verbatim into `resources/rawfile/`, and ArkUI's Image accepts
    // `$rawfile()` for raw resource references.
    let src_arg = if let Some(name) = src.strip_prefix("@app.media/") {
        // ArkUI's $r() takes a dot-path string, NOT a slash-path.
        format!("$r('app.media.{}')", name)
    } else if let Some(name) = src.strip_prefix("@app.icon/") {
        format!("$r('app.icon.{}')", name)
    } else if let Some(rest) = src.strip_prefix("assets/") {
        format!("$rawfile('{}')", rest)
    } else {
        arkts_string_lit(&src)
    };
    format!("Image({}).width('100%').height(200)", src_arg)
}

/// Walk a string-typed argument through bindings + ternary branches to
/// find the underlying string literal. Returns None when the chain
/// bottoms out at a non-string leaf. Same shape as
/// `numeric_arg_resolved` but for strings.
fn resolve_string_arg(expr: &Expr, bindings: &HashMap<LocalId, Expr>) -> Option<String> {
    let mut cur = expr;
    for _ in 0..16 {
        match cur {
            Expr::String(s) => return Some(s.clone()),
            Expr::LocalGet(id) => {
                cur = bindings.get(id)?;
            }
            Expr::Conditional {
                condition,
                then_expr,
                else_expr,
            } => {
                // Same heuristic as the widget Conditional emit: if the
                // condition const-folds, pick the corresponding branch;
                // otherwise default to the then-branch (the "primary"
                // case the author wrote first).
                cur = match evaluate_condition(condition, bindings, &HashMap::new()) {
                    Some(false) => else_expr,
                    _ => then_expr,
                };
            }
            // `t('key')` from `perry/i18n` lowers to an
            // `Expr::I18nString { key, ... }`. Use the key as the
            // string fallback — for Mango (and most apps using Perry's
            // i18n) the English source text doubles as the key, so
            // emitting the key gives the user readable English text on
            // platforms where dynamic i18n hasn't been wired yet.
            // Future: thread a locale lookup table through the harvest
            // and pick the matching translation.
            Expr::I18nString { key, .. } => return Some(key.clone()),
            // `t('key')` may also surface as a NativeMethodCall to
            // `perry/i18n` if the caller used the destructured-import
            // form. Unwrap to the inner I18nString (or plain string)
            // arg.
            Expr::NativeMethodCall {
                module,
                method,
                args,
                ..
            } if module == "perry/i18n" && method == "t" => {
                cur = args.first()?;
            }
            _ => return None,
        }
    }
    None
}

/// `ScrollView(children)` → `Scroll() { Column({space: 8}) { ... } }`.
/// ArkUI's `Scroll` is a single-child container that scrolls vertically by
/// default; we wrap in a `Column` so multiple children stack the way users
/// expect from the perry-ui-* native ScrollView wiring. Empty / non-array
/// children degrade to an empty Scroll just like the native variant.
///
/// Issue #408 — when `local_hint` resolves to a recorded set of mutations
/// against this scroll local, `scrollviewSetChild(scroll, content)` calls
/// inject the content as a child of the inner Column (latest one wins),
/// and `widgetAddChild(scroll, child)` calls also append into the inner
/// Column.
#[allow(clippy::too_many_arguments)]
fn emit_scrollview(
    args: &[Expr],
    bindings: &HashMap<LocalId, Expr>,
    depth: usize,
    callbacks: &mut Vec<Expr>,
    text_slots: &mut Vec<TextSlot>,
    arkts_locals: &HashMap<LocalId, String>,
    classes: &[Class],
    state_registry: &HashMap<LocalId, StateBinding>,
    lazy_sources: &mut Vec<LazyDataSource>,
    extras: &mut HarvestExtras,
    mutations: &HashMap<LocalId, Vec<MutationEntry>>,
    local_hint: Option<LocalId>,
) -> String {
    let inner_indent = "    ".repeat(depth + 2);
    let mid_indent = "    ".repeat(depth + 1);
    let outer_indent = "    ".repeat(depth);

    let mut children: Vec<String> = match args.first() {
        Some(Expr::Array(items)) => items
            .iter()
            .map(|c| {
                emit_widget(
                    c,
                    bindings,
                    depth + 2,
                    callbacks,
                    text_slots,
                    arkts_locals,
                    classes,
                    state_registry,
                    lazy_sources,
                    extras,
                    mutations,
                    None,
                )
            })
            .collect(),
        Some(am @ Expr::ArrayMap { .. }) => vec![emit_widget(
            am,
            bindings,
            depth + 2,
            callbacks,
            text_slots,
            arkts_locals,
            classes,
            state_registry,
            lazy_sources,
            extras,
            mutations,
            None,
        )],
        _ => vec![],
    };

    // Issue #408 — fold scroll-specific mutations.
    // SetScrollChild semantics: latest wins, replaces ALL prior children
    // (matches the native `scrollviewSetChild` behavior). AddChild on a
    // ScrollView is rare but supported — appends inside the inner Column.
    if let Some(id) = local_hint {
        if let Some(muts) = mutations.get(&id) {
            // Find the LAST unconditional SetScrollChild — that wins.
            let last_set = muts.iter().rposition(|e| {
                matches!(e.mutation, Mutation::SetScrollChild(_)) && e.condition.is_none()
            });
            if let Some(idx) = last_set {
                if let Mutation::SetScrollChild(content) = &muts[idx].mutation {
                    children = vec![emit_widget(
                        content,
                        bindings,
                        depth + 2,
                        callbacks,
                        text_slots,
                        arkts_locals,
                        classes,
                        state_registry,
                        lazy_sources,
                        extras,
                        mutations,
                        None,
                    )];
                }
            }
            // Append AddChild + conditional groups (built from BOTH AddChild
            // and SetScrollChild — the latter is essentially "replace + add").
            // For conditional SetScrollChild, treat each set as a single-child
            // override INSIDE its branch: we synthesize an AddChild-style
            // entry so emit_mutation_children can render it as an `if` block.
            let synthesized: Vec<MutationEntry> = muts
                .iter()
                .filter_map(|e| match (&e.mutation, &e.condition) {
                    (Mutation::AddChild(_), _) => Some(e.clone()),
                    (Mutation::ClearChildren, _) => Some(e.clone()),
                    (Mutation::SetScrollChild(c), Some(_)) => Some(MutationEntry {
                        mutation: Mutation::AddChild(c.clone()),
                        condition: e.condition.clone(),
                    }),
                    _ => None,
                })
                .collect();
            let extra = emit_mutation_children(
                &synthesized,
                bindings,
                depth + 2,
                callbacks,
                text_slots,
                arkts_locals,
                classes,
                state_registry,
                lazy_sources,
                extras,
                mutations,
            );
            children.extend(extra);
        }
    }

    let body = if children.is_empty() {
        String::new()
    } else {
        children
            .iter()
            .map(|c| {
                c.lines()
                    .map(|line| format!("{}{}", inner_indent, line))
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .collect::<Vec<_>>()
            .join("\n")
    };

    format!(
        "Scroll() {{\n\
         {mid}Column({{ space: 8 }}) {{\n\
         {body}\n\
         {mid}}}\n\
         {outer}}}",
        mid = mid_indent,
        body = body,
        outer = outer_indent,
    )
}

/// `LazyVStack(children)` → for now just emit `Column({space: 8}) { ... }`.
/// Real lazy rendering needs ArkUI's `LazyForEach` + a custom `IDataSource`
/// implementation, which doesn't fit the static-tree harvest model — the
/// children would have to be a function `(index) => Widget` evaluated per
/// row, which isn't expressible in the harvest pass without a runtime
/// callback bridge. Deferred to a future Phase 2 v5; today users write the
/// expanded children list explicitly and pay the eager-render cost.
#[allow(clippy::too_many_arguments)]
fn emit_lazy_vstack(
    args: &[Expr],
    bindings: &HashMap<LocalId, Expr>,
    depth: usize,
    callbacks: &mut Vec<Expr>,
    text_slots: &mut Vec<TextSlot>,
    arkts_locals: &HashMap<LocalId, String>,
    classes: &[Class],
    state_registry: &HashMap<LocalId, StateBinding>,
    lazy_sources: &mut Vec<LazyDataSource>,
    extras: &mut HarvestExtras,
    mutations: &HashMap<LocalId, Vec<MutationEntry>>,
) -> String {
    let inner_indent = "    ".repeat(depth + 1);
    let outer_indent = "    ".repeat(depth);

    // Phase 2 v10 — Real LazyVStack: when args[0] is `Expr::ArrayMap`,
    // emit ArkUI's `List() { LazyForEach(this.<src>, item => { ListItem() {<inner>} }, item => item) }`
    // and register a `PerryListDataSource`-backed `@State` field on the
    // page struct. wrap_index_page emits the IDataSource helper class +
    // the per-source field decls.
    if let Some(Expr::ArrayMap { array, callback }) = args.first() {
        let items_source = arkts_array_source(array, bindings);
        let field_id = format!("lazy_source_{}", lazy_sources.len());
        // Lower the closure body in a fresh arkts_locals scope so
        // LocalGet(param_id) resolves to `__item`.
        let (param_name, body_str) = match callback.as_ref() {
            Expr::Closure { params, body, .. } if !params.is_empty() => {
                let body_expr = body.iter().find_map(|s| match s {
                    Stmt::Return(Some(e)) => Some(e.clone()),
                    Stmt::Expr(e) => Some(e.clone()),
                    _ => None,
                });
                if let Some(body) = body_expr {
                    let mut locals = arkts_locals.clone();
                    locals.insert(params[0].id, "__item".to_string());
                    let inner = emit_widget(
                        &body,
                        bindings,
                        depth + 3,
                        callbacks,
                        text_slots,
                        &locals,
                        classes,
                        state_registry,
                        lazy_sources,
                        extras,
                        mutations,
                        None,
                    );
                    ("__item".to_string(), inner)
                } else {
                    (
                        "__item".to_string(),
                        "Text('[empty body]').fontSize(16)".to_string(),
                    )
                }
            }
            _ => (
                "__item".to_string(),
                "Text('[non-closure ForEach body]').fontSize(16)".to_string(),
            ),
        };
        // Push the source AFTER recursive emit_widget to maintain a
        // deterministic ordering (outermost-last so nested LazyVStacks
        // get inner ids before outer).
        lazy_sources.push(LazyDataSource {
            field_id: field_id.clone(),
            items_source,
        });
        let item_indent = "    ".repeat(depth + 3);
        let body_indented = body_str
            .lines()
            .map(|l| format!("{}{}", item_indent, l))
            .collect::<Vec<_>>()
            .join("\n");
        let mid_indent = "    ".repeat(depth + 2);
        return format!(
            "List() {{\n\
             {inner}LazyForEach(this.{field}, ({pname}: any) => {{\n\
             {mid}ListItem() {{\n\
             {body}\n\
             {mid}}}\n\
             {inner}}}, ({pname}: any) => {pname})\n\
             {outer}}}",
            inner = inner_indent,
            mid = mid_indent,
            field = field_id,
            pname = param_name,
            body = body_indented,
            outer = outer_indent,
        );
    }

    // Fall-through (v4 behavior): non-ArrayMap children render eagerly
    // as a plain Column. Preserves backwards compat for explicit-list
    // LazyVStack callers.
    let children: Vec<String> = match args.first() {
        Some(Expr::Array(items)) => items
            .iter()
            .map(|c| {
                emit_widget(
                    c,
                    bindings,
                    depth + 1,
                    callbacks,
                    text_slots,
                    arkts_locals,
                    classes,
                    state_registry,
                    lazy_sources,
                    extras,
                    mutations,
                    None,
                )
            })
            .collect(),
        _ => vec![],
    };
    let body = if children.is_empty() {
        String::new()
    } else {
        children
            .iter()
            .map(|c| {
                c.lines()
                    .map(|line| format!("{}{}", inner_indent, line))
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    format!(
        "// LazyVStack with explicit children: rendered eagerly as Column.\n\
         {outer}// For real lazy rendering, pass `items.map(item => Widget)`.\n\
         {outer}Column({{ space: 8 }}) {{\n\
         {body}\n\
         {outer}}}",
        outer = outer_indent,
        body = body,
    )
}

/// `Picker(options, onChange)` → ArkUI `TextPicker({range, value: range[0]}).onChange(...)`.
/// Closure receives `(idx: number)` matching the perry-ui-* TS surface.
/// ArkUI's onChange has the shape `(value: string, index: number)` — we
/// forward only `index` since that's what the Perry callback expects.
/// Same drain pattern as Toggle/Slider.
fn emit_picker(args: &[Expr], callbacks: &mut Vec<Expr>) -> String {
    let options = match args.first() {
        Some(Expr::Array(items)) => {
            let strs: Vec<String> = items
                .iter()
                .filter_map(|item| match item {
                    Expr::String(s) => Some(arkts_string_lit(s)),
                    _ => None,
                })
                .collect();
            format!("[{}]", strs.join(", "))
        }
        _ => "[]".to_string(),
    };
    // ArkUI requires a `value` field set to a member of `range`; falling
    // back to an empty string is safe when options is empty.
    let initial = match args.first() {
        Some(Expr::Array(items)) => match items.first() {
            Some(Expr::String(s)) => arkts_string_lit(s),
            _ => "''".to_string(),
        },
        _ => "''".to_string(),
    };

    let onchange = match args.get(1) {
        Some(closure @ Expr::Closure { .. }) => {
            let idx = callbacks.len();
            callbacks.push(closure.clone());
            format!(
                ".onChange((_value: string, index: number) => {{\n    \
                 perryEntry.invokeCallback1({}, index);\n    \
                 {drain}\
                 }})",
                idx,
                drain = drain_loop_body()
            )
        }
        _ => String::new(),
    };

    format!(
        "TextPicker({{ range: {opts}, value: {init} }}){onchange}",
        opts = options,
        init = initial,
        onchange = onchange,
    )
}

/// Issue #475 — `Combobox(initial, onChange)` → ArkUI `Select([...])` with
/// `.value()` / `.selected()` / `.onSelect()`. ArkUI's Select takes a
/// `SelectOption[]` (each option is `{value: string}`); `.value()` sets
/// the currently displayed label; `.onSelect((index, value) => ...)` fires
/// when the user picks an item.
///
/// v1 limitation: runtime `comboboxAddItem(widget, value)` calls aren't
/// folded into the static options list — only the `initial` string seeds
/// the dropdown. The macOS path's NSComboBox completion behavior doesn't
/// have a direct ArkUI analog; Select shows the dropdown on tap, which
/// is acceptable for the v1 milestone (#475).
fn emit_combobox(args: &[Expr], callbacks: &mut Vec<Expr>) -> String {
    let initial = first_string_arg(args).unwrap_or_default();
    let initial_lit = arkts_string_lit(&initial);

    let onchange = match args.get(1) {
        Some(closure @ Expr::Closure { .. }) => {
            let idx = callbacks.len();
            callbacks.push(closure.clone());
            format!(
                ".onSelect((_index: number, value: string) => {{\n    \
                 perryEntry.invokeCallback1({}, value);\n    \
                 {drain}\
                 }})",
                idx,
                drain = drain_loop_body()
            )
        }
        _ => String::new(),
    };

    // ArkUI Select requires a non-empty options array. Seed it with the
    // initial value so the dropdown isn't empty on first render. Real
    // app code that calls `comboboxAddItem` repeatedly will need a v1.1
    // mutator-style fold (similar to widgetAddChild) — out of scope #475 v1.
    format!(
        "Select([{{ value: {init} }}]).selected(0).value({init}){onchange}",
        init = initial_lit,
        onchange = onchange,
    )
}

/// Issue #478 — `RichTextEditor(width, height, onChange)` → ArkUI
/// `RichEditor({controller})`. ArkUI's RichEditor takes a
/// `RichEditorController` for programmatic content manipulation and fires
/// `.aboutToIMEInput` / `.onIMEInputComplete` lifecycle callbacks.
///
/// v1 mapping decisions:
///   - `width` / `height` flow through to `.width()` / `.height()` modifiers.
///   - `onChange` fires from `.onIMEInputComplete` — ArkUI's closest analog
///     to NSTextView's didChange notification. Forwards the plain-text view
///     of the editor content via `invokeCallback1(idx, plainText)`.
///   - `richTextToggleBold` / `Italic` / `Underline` would map to
///     `RichEditorController.updateSpanStyle({textStyle: ...})` on a
///     stored controller. v1 emits the editor + the controller field;
///     the mutator dispatch for the toggles is a v1.1 follow-up so the
///     emitted ArkTS still compiles and the editor itself renders.
///   - `richTextSetString` / `richTextGetString` map to the controller's
///     `addTextSpan` / `getSpans`; `setHtml` / `getHtml` are still
///     TODOs (ArkUI RichEditor doesn't ship a 1:1 HTML round-trip).
fn emit_richtexteditor(args: &[Expr], callbacks: &mut Vec<Expr>) -> String {
    let width = numeric_arg(args, 0).unwrap_or(0.0);
    let height = numeric_arg(args, 1).unwrap_or(0.0);

    let onchange = match args.get(2) {
        Some(closure @ Expr::Closure { .. }) => {
            let idx = callbacks.len();
            callbacks.push(closure.clone());
            // RichEditor's onIMEInputComplete gives us a TextRange/value
            // shape; for v1 we forward an empty string placeholder so the
            // TS side sees a valid arg slot. Wiring the actual plain-text
            // payload requires reading the controller post-event — v1.1.
            format!(
                ".onIMEInputComplete(() => {{\n    \
                 perryEntry.invokeCallback1({}, '');\n    \
                 {drain}\
                 }})",
                idx,
                drain = drain_loop_body()
            )
        }
        _ => String::new(),
    };

    // Width / height modifiers only emitted when non-zero so a caller
    // passing 0 (TS default) doesn't zero out the editor's intrinsic size.
    let mut sizing = String::new();
    if width > 0.0 {
        sizing.push_str(&format!(".width({})", fmt_num(width)));
    }
    if height > 0.0 {
        sizing.push_str(&format!(".height({})", fmt_num(height)));
    }

    format!(
        "RichEditor({{ controller: new RichEditorController() }}){sizing}{onchange}",
        sizing = sizing,
        onchange = onchange,
    )
}

/// Issue #481 — `Calendar(year, month, onChange)` → ArkUI
/// `CalendarPicker({selected: new Date(year, month-1, 1)}).onChange(...)`.
/// CalendarPicker fires `.onChange((value: Date) => ...)` when the user
/// picks a date. Perry's `onChange` receives an ISO `yyyy-MM-dd` string
/// (POSIX-locale), so the forwarded payload is `value.toISOString().split('T')[0]`.
///
/// JavaScript's `Date` constructor takes `(year, monthIndex, day)` where
/// monthIndex is 0-based; Perry passes a 1-based month per its TS
/// signature, so we emit `month - 1` literally when both args resolve.
/// When the args don't resolve to literals, we fall back to today's
/// date (`new Date()`) — the user can still pick a date even if the
/// initial highlight is wrong, which is strictly better than emitting
/// an invalid Date object.
fn emit_calendar(args: &[Expr], callbacks: &mut Vec<Expr>) -> String {
    let year = numeric_arg(args, 0);
    let month = numeric_arg(args, 1);
    let selected = match (year, month) {
        (Some(y), Some(m)) => {
            // ArkUI's Date constructor wants (year, monthIndex, day).
            let m_idx = (m as i64).saturating_sub(1).max(0);
            format!("new Date({}, {}, 1)", y as i64, m_idx)
        }
        _ => "new Date()".to_string(),
    };

    let onchange = match args.get(2) {
        Some(closure @ Expr::Closure { .. }) => {
            let idx = callbacks.len();
            callbacks.push(closure.clone());
            // ArkUI's CalendarPicker.onChange hands us a `Date`; convert
            // to the ISO yyyy-MM-dd shape Perry's TS surface promises.
            // .toISOString() returns "yyyy-MM-ddTHH:mm:ss.sssZ" — split
            // at 'T' and take the date half.
            format!(
                ".onChange((value: Date) => {{\n    \
                 const __iso = value.toISOString().split('T')[0];\n    \
                 perryEntry.invokeCallback1({}, __iso);\n    \
                 {drain}\
                 }})",
                idx,
                drain = drain_loop_body()
            )
        }
        _ => String::new(),
    };

    format!(
        "CalendarPicker({{ selected: {sel} }}){onchange}",
        sel = selected,
        onchange = onchange,
    )
}

/// `ProgressView(value?, total?)` → ArkUI `Progress({value, total, type: ProgressType.Linear})`.
/// Defaults: value=0, total=100. Both args optional — leaf widget, no
/// callbacks, no children.
fn emit_progressview(args: &[Expr]) -> String {
    let value = numeric_arg(args, 0).unwrap_or(0.0);
    let total = numeric_arg(args, 1).unwrap_or(100.0);
    format!(
        "Progress({{ value: {value}, total: {total}, type: ProgressType.Linear }})",
        value = fmt_num(value),
        total = fmt_num(total),
    )
}

/// Issue #669 — `Chart(kind, width, height)` → ArkUI `Canvas` backed by a
/// per-instance `CanvasRenderingContext2D` class field. The data points,
/// title and kind are baked into the page at codegen time by folding
/// the chart's mutator stream (`chartAddDataPoint`, `chartClearData`,
/// `chartSetTitle`). The draw closure is registered via `.onReady` and
/// runs once per build() — `chartReload` doesn't need an explicit
/// re-trigger because every closure-time data mutation routes through
/// ArkUI's `@State` re-render cycle (the static fold is the v1 baseline
/// matching `comboboxAddItem`'s limitation — dynamic-data charts are a
/// follow-up).
///
/// The math mirrors the Android impl
/// (`crates/perry-ui-android/template/.../PerryBridge.kt::PerryChartView`):
/// - 0=line: stroke path connecting data points + point dots + x-axis labels
/// - 1=bar:  filled rectangles + x-axis labels
/// - 2=pie:  filled arcs + legend column on the left
/// padding/colors copied 1:1 so the harmonyos rendering matches the
/// other platforms.
fn emit_chart(
    args: &[Expr],
    bindings: &HashMap<LocalId, Expr>,
    mutations: &HashMap<LocalId, Vec<MutationEntry>>,
    local_hint: Option<LocalId>,
    extras: &mut HarvestExtras,
) -> String {
    let kind = numeric_arg_resolved(args, 0, bindings).unwrap_or(1.0) as i64;
    let width = numeric_arg_resolved(args, 1, bindings).unwrap_or(0.0);
    let height = numeric_arg_resolved(args, 2, bindings).unwrap_or(0.0);

    // Fold the chart's mutator stream into a static (data, title) pair.
    let (points, title) = fold_chart_mutations(local_hint, mutations);

    // Register a per-instance ctx field; the field id is a simple
    // sequence number so emitted source is deterministic regardless of
    // the user's local-id remapping.
    let field_id = format!("{}", extras.chart_instances.len());
    extras.chart_instances.push(ChartInstance {
        field_id: field_id.clone(),
        kind,
        width,
        height,
        points: points.clone(),
        title: title.clone(),
    });

    // Inline data + title literals so the draw closure doesn't need to
    // walk class state — keeps the closure self-contained per build().
    let data_lit = points
        .iter()
        .map(|(l, v)| {
            format!(
                "{{ label: {}, value: {} }}",
                arkts_string_lit(l),
                fmt_num(*v)
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    let title_lit = arkts_string_lit(&title);

    let mut sizing = String::new();
    if width > 0.0 {
        sizing.push_str(&format!(".width({})", fmt_num(width)));
    }
    if height > 0.0 {
        sizing.push_str(&format!(".height({})", fmt_num(height)));
    }

    let draw_body = chart_draw_body(kind);

    // Resolve the actual canvas pixel size — the draw closure reads
    // `cw` / `ch` from the rendering context to compute scaled
    // coordinates. When the user passed width/height we know those
    // numbers; otherwise fall back to the closure's intrinsic
    // measurement at draw time.
    let cw_decl = if width > 0.0 {
        format!("        const cw: number = {};\n", fmt_num(width))
    } else {
        "        const cw: number = ctx.width;\n".to_string()
    };
    let ch_decl = if height > 0.0 {
        format!("        const ch: number = {};\n", fmt_num(height))
    } else {
        "        const ch: number = ctx.height;\n".to_string()
    };

    format!(
        "Canvas(this.__chart_{id}_ctx){sizing}.backgroundColor('#ffffff').onReady(() => {{\n\
         \x20\x20\x20\x20\x20\x20\x20\x20const ctx = this.__chart_{id}_ctx;\n\
         {cw}\
         {ch}\
         \x20\x20\x20\x20\x20\x20\x20\x20const data: Array<{{ label: string, value: number }}> = [{data}];\n\
         \x20\x20\x20\x20\x20\x20\x20\x20const title: string = {title};\n\
         \x20\x20\x20\x20\x20\x20\x20\x20ctx.clearRect(0, 0, cw, ch);\n\
         {draw}\
         \x20\x20\x20\x20}})",
        id = field_id,
        sizing = sizing,
        cw = cw_decl,
        ch = ch_decl,
        data = data_lit,
        title = title_lit,
        draw = draw_body,
    )
}

/// Fold a chart's recorded mutator stream into a `(points, title)` pair.
/// Order matters: `chartClearData` resets the points list, then later
/// `chartAddDataPoint` entries refill it; the last `chartSetTitle` wins.
/// Conditional mutations (recorded with `MutationEntry::condition: Some`)
/// are flattened by treating them as unconditional — the v1 fold doesn't
/// emit per-branch chart configurations. That's fine because the issue
/// #669 scope explicitly punts dynamic data; charts are static.
fn fold_chart_mutations(
    local_hint: Option<LocalId>,
    mutations: &HashMap<LocalId, Vec<MutationEntry>>,
) -> (Vec<(String, f64)>, String) {
    let mut points: Vec<(String, f64)> = Vec::new();
    let mut title = String::new();
    let Some(id) = local_hint else {
        return (points, title);
    };
    let Some(entries) = mutations.get(&id) else {
        return (points, title);
    };
    for entry in entries {
        match &entry.mutation {
            Mutation::ChartAddDataPoint(label, value) => {
                points.push((label.clone(), *value));
            }
            Mutation::ChartClearData => {
                points.clear();
            }
            Mutation::ChartSetTitle(t) => {
                title = t.clone();
            }
            Mutation::ChartReload => { /* no-op at codegen time */ }
            _ => {}
        }
    }
    (points, title)
}

/// Generate the ArkTS draw closure body for the chosen chart kind. All
/// three branches use ctx.* (CanvasRenderingContext2D) so the same body
/// works for any per-instance ctx the caller binds. Lines indented at
/// 8 spaces so they slot under `Canvas(...).onReady(() => { ... })`.
fn chart_draw_body(kind: i64) -> String {
    // Common: title bar at the top — drawn for every kind so the chart
    // always has its label visible regardless of which kind renders below.
    // (Matches the Android impl, where titleHeight is reserved up front.)
    let title_block = "\
        const titleHeight: number = title.length > 0 ? 28 : 0;\n\
        if (title.length > 0) {\n\
            ctx.fillStyle = '#222222';\n\
            ctx.font = 'bold 18px sans-serif';\n\
            ctx.textAlign = 'center';\n\
            ctx.fillText(title, cw / 2, 22);\n\
        }\n\
        if (data.length === 0) { return; }\n";
    let body = match kind {
        0 => chart_draw_line(),
        2 => chart_draw_pie(),
        // bar is the default for unknown kinds (matches Android's
        // `else -> drawBar(...)` branch).
        _ => chart_draw_bar(),
    };
    format!(
        "\x20\x20\x20\x20\x20\x20\x20\x20{title}{body}",
        title = title_block.replace('\n', "\n        "),
        body = body,
    )
}

fn chart_draw_bar() -> String {
    "\
const padL: number = 36;\n\
const padR: number = 12;\n\
const padB: number = 36;\n\
const padT: number = titleHeight + 12;\n\
const plotW: number = cw - padL - padR;\n\
const plotH: number = ch - padT - padB;\n\
if (plotW <= 0 || plotH <= 0) { return; }\n\
let maxV: number = 1e-9;\n\
for (const p of data) { if (p.value > maxV) { maxV = p.value; } }\n\
const step: number = plotW / data.length;\n\
const barWidth: number = step * 0.7;\n\
ctx.strokeStyle = '#888888';\n\
ctx.lineWidth = 2;\n\
ctx.beginPath();\n\
ctx.moveTo(padL, ch - padB);\n\
ctx.lineTo(cw - padR, ch - padB);\n\
ctx.stroke();\n\
ctx.fillStyle = '#4287F5';\n\
for (let i = 0; i < data.length; i++) {\n\
    const p = data[i];\n\
    const cx = padL + step * (i + 0.5);\n\
    const barH = p.value / maxV * plotH;\n\
    ctx.fillRect(cx - barWidth / 2, (ch - padB) - barH, barWidth, barH);\n\
}\n\
ctx.fillStyle = '#222222';\n\
ctx.font = '12px sans-serif';\n\
ctx.textAlign = 'center';\n\
for (let i = 0; i < data.length; i++) {\n\
    const cx = padL + step * (i + 0.5);\n\
    ctx.fillText(data[i].label, cx, ch - padB + 18);\n\
}\n"
    .replace('\n', "\n        ")
}

fn chart_draw_line() -> String {
    "\
const padL: number = 36;\n\
const padR: number = 12;\n\
const padB: number = 36;\n\
const padT: number = titleHeight + 12;\n\
const plotW: number = cw - padL - padR;\n\
const plotH: number = ch - padT - padB;\n\
if (plotW <= 0 || plotH <= 0) { return; }\n\
let maxV: number = 1e-9;\n\
for (const p of data) { if (p.value > maxV) { maxV = p.value; } }\n\
const step: number = data.length <= 1 ? 0 : plotW / (data.length - 1);\n\
ctx.strokeStyle = '#888888';\n\
ctx.lineWidth = 2;\n\
ctx.beginPath();\n\
ctx.moveTo(padL, ch - padB);\n\
ctx.lineTo(cw - padR, ch - padB);\n\
ctx.stroke();\n\
ctx.strokeStyle = '#4287F5';\n\
ctx.lineWidth = 3;\n\
ctx.beginPath();\n\
for (let i = 0; i < data.length; i++) {\n\
    const cx = padL + step * i;\n\
    const cy = (ch - padB) - (data[i].value / maxV * plotH);\n\
    if (i === 0) { ctx.moveTo(cx, cy); } else { ctx.lineTo(cx, cy); }\n\
}\n\
ctx.stroke();\n\
ctx.fillStyle = '#1B5FB8';\n\
for (let i = 0; i < data.length; i++) {\n\
    const cx = padL + step * i;\n\
    const cy = (ch - padB) - (data[i].value / maxV * plotH);\n\
    ctx.beginPath();\n\
    ctx.arc(cx, cy, 4, 0, Math.PI * 2);\n\
    ctx.fill();\n\
}\n\
ctx.fillStyle = '#222222';\n\
ctx.font = '12px sans-serif';\n\
ctx.textAlign = 'center';\n\
for (let i = 0; i < data.length; i++) {\n\
    const cx = padL + step * i;\n\
    ctx.fillText(data[i].label, cx, ch - padB + 18);\n\
}\n"
    .replace('\n', "\n        ")
}

fn chart_draw_pie() -> String {
    "\
const colors: string[] = ['#4287F5', '#E54545', '#34A853', '#FBBC05', '#9C27B0', '#00ACC1', '#FF7043', '#8D6E63'];\n\
let sum: number = 0;\n\
for (const p of data) { sum += p.value; }\n\
if (sum <= 0) { return; }\n\
const padT: number = titleHeight + 12;\n\
const plotH: number = ch - padT - 12;\n\
const radius: number = Math.max(8, Math.min(cw, plotH) / 2 - 12);\n\
const cx: number = cw / 2;\n\
const cy: number = padT + plotH / 2;\n\
let startAngle: number = -Math.PI / 2;\n\
for (let i = 0; i < data.length; i++) {\n\
    const sweep = data[i].value / sum * Math.PI * 2;\n\
    ctx.fillStyle = colors[i % colors.length];\n\
    ctx.beginPath();\n\
    ctx.moveTo(cx, cy);\n\
    ctx.arc(cx, cy, radius, startAngle, startAngle + sweep);\n\
    ctx.closePath();\n\
    ctx.fill();\n\
    startAngle += sweep;\n\
}\n\
ctx.font = '12px sans-serif';\n\
ctx.textAlign = 'left';\n\
let ly: number = padT + 16;\n\
for (let i = 0; i < data.length; i++) {\n\
    ctx.fillStyle = colors[i % colors.length];\n\
    ctx.fillRect(8, ly - 12, 16, 12);\n\
    ctx.fillStyle = '#222222';\n\
    ctx.fillText(data[i].label, 32, ly);\n\
    ly += 18;\n\
}\n"
        .replace('\n', "\n        ")
}

/// Issue #670 — `TreeView(rootNode, onSelect)` → ArkUI `List` with
/// a recursive flatten. The node graph is rebuilt at codegen time by
/// chasing `rootNode` back through `bindings` to its `TreeNode(id,
/// label)` constructor + accumulated `treeNodeAddChild` mutations.
/// The result lives as a `private __tree_<n>_nodes: TreeNodeData[]`
/// constant on the @Component; expand/collapse state is
/// `@State __tree_<n>_expanded: Set<string>`; the currently selected
/// id is `@State __tree_<n>_selectedId: string`. A helper method
/// `__tree_<n>_flatten()` walks the graph using the expanded set and
/// returns the visible rows ArkUI's `ForEach` iterates over.
///
/// Tapping a row that has children toggles the chevron and re-renders
/// (ArkUI reacts to the Set mutation via the `=` assignment helper).
/// Tapping any row fires the user `onSelect` closure with the row's id.
/// `treeViewExpandAll` / `treeViewCollapseAll` / `treeViewGetSelectedId`
/// route through the existing perry_arkts drain queue at runtime — out
/// of scope for v1; the @State surface is in place so a v1.1 NAPI
/// drain bridge can flip the set without touching codegen.
fn emit_treeview(
    args: &[Expr],
    bindings: &HashMap<LocalId, Expr>,
    callbacks: &mut Vec<Expr>,
    mutations: &HashMap<LocalId, Vec<MutationEntry>>,
    extras: &mut HarvestExtras,
) -> String {
    let root_arg = args.first();

    let on_select_idx = match args.get(1) {
        Some(closure @ Expr::Closure { .. }) => {
            let idx = callbacks.len();
            callbacks.push(closure.clone());
            Some(idx)
        }
        _ => None,
    };

    // Build the static tree by walking from root_arg through bindings.
    let root = root_arg.and_then(|e| build_tree_node(e, bindings, mutations, 0));
    let Some(root_node) = root else {
        // Couldn't resolve the root TreeNode call — degrade to a comment
        // + placeholder so the user can see the gap.
        return format!(
            "// TreeView: couldn't resolve root TreeNode call (non-literal binding)\n\
             Text('[TreeView: unresolved root]').fontSize(14).fontColor('#888888')"
        );
    };

    let field_id = format!("{}", extras.tree_view_instances.len());
    extras.tree_view_instances.push(TreeViewInstance {
        field_id: field_id.clone(),
        root: root_node,
        on_select_idx,
    });

    let select_call = match on_select_idx {
        Some(idx) => format!(
            "perryEntry.invokeCallback1({}, row.id); {drain}",
            idx,
            drain = drain_loop_body()
        ),
        None => String::new(),
    };

    // ForEach key extractor uses row.id so ArkUI can diff between
    // pre/post-expand flattenings. The chevron leaf-vs-branch logic
    // mirrors the Android indentation glyphs ("▾"/"▸"/blank).
    format!(
        "List({{ space: 0 }}) {{\n\
         \x20\x20\x20\x20ForEach(this.__tree_{id}_flatten(), (row: {{ id: string, label: string, depth: number, hasChildren: boolean, expanded: boolean }}) => {{\n\
         \x20\x20\x20\x20\x20\x20\x20\x20ListItem() {{\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20Row({{ space: 4 }}) {{\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20Text(row.hasChildren ? (row.expanded ? '\u{25be}' : '\u{25b8}') : ' ').fontSize(14).width(16)\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20Text(row.label).fontSize(14)\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20}}\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20.padding({{ left: row.depth * 16, top: 4, bottom: 4 }})\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20.onClick(() => {{\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20this.__tree_{id}_selectedId = row.id;\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20if (row.hasChildren) {{\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20const next = new Set<string>(this.__tree_{id}_expanded);\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20if (next.has(row.id)) {{ next.delete(row.id); }} else {{ next.add(row.id); }}\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20this.__tree_{id}_expanded = next;\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20}}\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20{select}\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20}})\n\
         \x20\x20\x20\x20\x20\x20\x20\x20}}\n\
         \x20\x20\x20\x20}}, (row: {{ id: string }}) => row.id)\n\
         }}",
        id = field_id,
        select = select_call,
    )
}

/// Walk an `Expr` (presumably a `LocalGet` referring to a `TreeNode(id,
/// label)` constructor) through bindings + recorded `treeNodeAddChild`
/// mutations to produce a static [`TreeViewNode`] tree. Bounded depth
/// (16) protects against pathological binding cycles.
fn build_tree_node(
    expr: &Expr,
    bindings: &HashMap<LocalId, Expr>,
    mutations: &HashMap<LocalId, Vec<MutationEntry>>,
    depth: usize,
) -> Option<TreeViewNode> {
    if depth > 16 {
        return None;
    }
    let (id_str, label_str, target_local) = match expr {
        Expr::LocalGet(id) => {
            let init = bindings.get(id)?;
            // The init must itself be a TreeNode(id, label) call.
            let (i, l) = treenode_id_label(init)?;
            (i, l, Some(*id))
        }
        _ => {
            let (i, l) = treenode_id_label(expr)?;
            (i, l, None)
        }
    };
    let mut children: Vec<TreeViewNode> = Vec::new();
    if let Some(lid) = target_local {
        if let Some(entries) = mutations.get(&lid) {
            for entry in entries {
                if let Mutation::TreeAddChild(child_expr) = &entry.mutation {
                    if let Some(c) = build_tree_node(child_expr, bindings, mutations, depth + 1) {
                        children.push(c);
                    }
                }
            }
        }
    }
    Some(TreeViewNode {
        id: id_str,
        label: label_str,
        children,
    })
}

/// Inspect an expr for the `TreeNode(id, label)` factory call shape and
/// extract the literal id + label strings. Returns None for any other
/// shape (unbound local, non-literal args, etc.).
fn treenode_id_label(expr: &Expr) -> Option<(String, String)> {
    let Expr::NativeMethodCall {
        module,
        method,
        args,
        ..
    } = expr
    else {
        return None;
    };
    if module != "perry/ui" || method != "TreeNode" {
        return None;
    }
    let id = match args.first()? {
        Expr::String(s) => s.clone(),
        _ => return None,
    };
    let label = match args.get(1)? {
        Expr::String(s) => s.clone(),
        _ => return None,
    };
    Some((id, label))
}

/// Emit ArkTS source for `chart_decls`, `tree_view_decls`, and
/// `tree_view_methods` consumed by `wrap_index_page`. Splits Chart's
/// field decls from TreeView's so the @Component layout stays readable
/// (charts come first, then tree views, then media glue).
fn chart_and_tree_glue(extras: &HarvestExtras) -> (String, String, String) {
    let mut chart_decls = String::new();
    for inst in &extras.chart_instances {
        chart_decls.push_str(&format!(
            "    private __chart_{id}_settings: RenderingContextSettings = new RenderingContextSettings(true);\n\
             \x20\x20\x20\x20private __chart_{id}_ctx: CanvasRenderingContext2D = new CanvasRenderingContext2D(this.__chart_{id}_settings);\n",
            id = inst.field_id,
        ));
    }

    let mut tree_view_decls = String::new();
    let mut tree_view_methods = String::new();
    for inst in &extras.tree_view_instances {
        let nodes_lit = render_tree_data_literal(&inst.root);
        tree_view_decls.push_str(&format!(
            "    private __tree_{id}_nodes: Array<{{ id: string, label: string, children: any[] }}> = [{lit}];\n\
             \x20\x20\x20\x20@State __tree_{id}_expanded: Set<string> = new Set<string>();\n\
             \x20\x20\x20\x20@State __tree_{id}_selectedId: string = '';\n",
            id = inst.field_id,
            lit = nodes_lit,
        ));
        tree_view_methods.push_str(&format!(
            "    __tree_{id}_flatten(): Array<{{ id: string, label: string, depth: number, hasChildren: boolean, expanded: boolean }}> {{\n\
             \x20\x20\x20\x20\x20\x20\x20\x20const out: Array<{{ id: string, label: string, depth: number, hasChildren: boolean, expanded: boolean }}> = [];\n\
             \x20\x20\x20\x20\x20\x20\x20\x20const expanded = this.__tree_{id}_expanded;\n\
             \x20\x20\x20\x20\x20\x20\x20\x20const walk = (nodes: Array<{{ id: string, label: string, children: any[] }}>, depth: number): void => {{\n\
             \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20for (const n of nodes) {{\n\
             \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20const has = n.children && n.children.length > 0;\n\
             \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20const isOpen = expanded.has(n.id);\n\
             \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20out.push({{ id: n.id, label: n.label, depth: depth, hasChildren: has, expanded: isOpen }});\n\
             \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20if (has && isOpen) {{ walk(n.children, depth + 1); }}\n\
             \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20}}\n\
             \x20\x20\x20\x20\x20\x20\x20\x20}};\n\
             \x20\x20\x20\x20\x20\x20\x20\x20walk(this.__tree_{id}_nodes, 0);\n\
             \x20\x20\x20\x20\x20\x20\x20\x20return out;\n\
             \x20\x20\x20\x20}}\n",
            id = inst.field_id,
        ));
    }

    (chart_decls, tree_view_decls, tree_view_methods)
}

/// Render a `TreeViewNode` (and its children, recursively) as an ArkTS
/// object-literal entry. Used by `chart_and_tree_glue` to bake the
/// static graph into the @Component's `__tree_<n>_nodes` field.
fn render_tree_data_literal(node: &TreeViewNode) -> String {
    let children = node
        .children
        .iter()
        .map(render_tree_data_literal)
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "{{ id: {id}, label: {label}, children: [{kids}] }}",
        id = arkts_string_lit(&node.id),
        label = arkts_string_lit(&node.label),
        kids = children,
    )
}

/// `Section(title, children)` → labeled vertical group.
/// Emits `Column({space: 4}) { Text('<title>').fontSize(14).fontColor('#888888'); <children> }`.
/// The greyed-out small label header matches the iOS UITableView section
/// header convention; no native ArkUI primitive maps 1:1, so we hand-roll.
#[allow(clippy::too_many_arguments)]
fn emit_section(
    args: &[Expr],
    bindings: &HashMap<LocalId, Expr>,
    depth: usize,
    callbacks: &mut Vec<Expr>,
    text_slots: &mut Vec<TextSlot>,
    arkts_locals: &HashMap<LocalId, String>,
    classes: &[Class],
    state_registry: &HashMap<LocalId, StateBinding>,
    lazy_sources: &mut Vec<LazyDataSource>,
    extras: &mut HarvestExtras,
    mutations: &HashMap<LocalId, Vec<MutationEntry>>,
    local_hint: Option<LocalId>,
) -> String {
    let title = first_string_arg(args).unwrap_or_default();

    let inner_indent = "    ".repeat(depth + 1);
    let outer_indent = "    ".repeat(depth);

    let mut children: Vec<String> = match args.get(1) {
        Some(Expr::Array(items)) => items
            .iter()
            .map(|c| {
                emit_widget(
                    c,
                    bindings,
                    depth + 1,
                    callbacks,
                    text_slots,
                    arkts_locals,
                    classes,
                    state_registry,
                    lazy_sources,
                    extras,
                    mutations,
                    None,
                )
            })
            .collect(),
        Some(am @ Expr::ArrayMap { .. }) => vec![emit_widget(
            am,
            bindings,
            depth + 1,
            callbacks,
            text_slots,
            arkts_locals,
            classes,
            state_registry,
            lazy_sources,
            extras,
            mutations,
            None,
        )],
        _ => vec![],
    };

    // Issue #408 — fold AddChild + ClearChildren mutations.
    if let Some(id) = local_hint {
        if let Some(muts) = mutations.get(&id) {
            let has_unconditional_clear = muts
                .iter()
                .any(|e| matches!(e.mutation, Mutation::ClearChildren) && e.condition.is_none());
            if has_unconditional_clear {
                children.clear();
            }
            let extra = emit_mutation_children(
                muts,
                bindings,
                depth + 1,
                callbacks,
                text_slots,
                arkts_locals,
                classes,
                state_registry,
                lazy_sources,
                extras,
                mutations,
            );
            children.extend(extra);
        }
    }

    // Always emit the title Text at the top, regardless of children count.
    let title_line = format!(
        "{}Text({}).fontSize(14).fontColor('#888888')",
        inner_indent,
        arkts_string_lit(&title)
    );

    let body = if children.is_empty() {
        title_line
    } else {
        let kids = children
            .iter()
            .map(|c| {
                c.lines()
                    .map(|line| format!("{}{}", inner_indent, line))
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .collect::<Vec<_>>()
            .join("\n");
        format!("{}\n{}", title_line, kids)
    };

    format!(
        "Column({{ space: 4 }}) {{\n\
         {body}\n\
         {outer}}}",
        body = body,
        outer = outer_indent,
    )
}

/// Wrap a widget body expression in a complete ArkUI `@Entry @Component
// ----- Phase 2 v12 widgets -----

/// `Tabs([{label: "A", body: ...}, {label: "B", body: ...}])` →
/// ArkUI `Tabs() { TabContent() {...}.tabBar('A'); TabContent() {...}.tabBar('B') }`.
/// Each tab's body harvests like a normal sub-widget tree. Closure-bearing
/// children compose with the v2 callback registry transparently.
#[allow(clippy::too_many_arguments)]
fn emit_tabs(
    args: &[Expr],
    bindings: &HashMap<LocalId, Expr>,
    depth: usize,
    callbacks: &mut Vec<Expr>,
    text_slots: &mut Vec<TextSlot>,
    arkts_locals: &HashMap<LocalId, String>,
    classes: &[Class],
    state_registry: &HashMap<LocalId, StateBinding>,
    lazy_sources: &mut Vec<LazyDataSource>,
    extras: &mut HarvestExtras,
    mutations: &HashMap<LocalId, Vec<MutationEntry>>,
) -> String {
    let tab_specs: Vec<&Expr> = match args.first() {
        Some(Expr::Array(items)) => items.iter().collect(),
        _ => Vec::new(),
    };
    let inner_indent = "    ".repeat(depth + 1);
    let outer_indent = "    ".repeat(depth);
    let tab_blocks: Vec<String> = tab_specs
        .iter()
        .map(|spec| {
            // Each spec is `{label: string, body: Widget}`. Handle both
            // open Object and closed-shape New, same pattern as styles.
            let pairs: Option<Vec<(String, Expr)>> = match spec {
                Expr::Object(props) => Some(props.clone()),
                Expr::New {
                    class_name, args, ..
                } if class_name.starts_with("__AnonShape_") => {
                    classes.iter().find(|c| &c.name == class_name).map(|cls| {
                        cls.fields
                            .iter()
                            .enumerate()
                            .filter_map(|(i, f)| args.get(i).map(|a| (f.name.clone(), a.clone())))
                            .collect()
                    })
                }
                _ => None,
            };
            let Some(pairs) = pairs else {
                return format!(
                    "{ind}// tab spec wasn't an object\n\
                     {ind}TabContent() {{\n\
                     {ind}    Text('[invalid tab]').fontSize(16)\n\
                     {ind}}}.tabBar('?')",
                    ind = inner_indent
                );
            };
            let label = pairs
                .iter()
                .find(|(k, _)| k == "label")
                .and_then(|(_, v)| match v {
                    Expr::String(s) => Some(s.clone()),
                    _ => None,
                })
                .unwrap_or_else(|| "Tab".to_string());
            let body = pairs
                .iter()
                .find(|(k, _)| k == "body")
                .map(|(_, v)| {
                    emit_widget(
                        v,
                        bindings,
                        depth + 2,
                        callbacks,
                        text_slots,
                        arkts_locals,
                        classes,
                        state_registry,
                        lazy_sources,
                        extras,
                        mutations,
                        None,
                    )
                })
                .unwrap_or_else(|| "Text('[empty tab]').fontSize(16)".to_string());
            // Indent the body inside TabContent { ... }.
            let body_indent = "    ".repeat(depth + 2);
            let body_indented = body
                .lines()
                .map(|l| format!("{}{}", body_indent, l))
                .collect::<Vec<_>>()
                .join("\n");
            format!(
                "{ind}TabContent() {{\n\
                 {body}\n\
                 {ind}}}.tabBar({lbl})",
                ind = inner_indent,
                body = body_indented,
                lbl = arkts_string_lit(&label),
            )
        })
        .collect();
    let body = tab_blocks.join("\n");
    format!(
        "Tabs() {{\n\
         {body}\n\
         {outer}}}",
        body = body,
        outer = outer_indent,
    )
}

/// `Modal(title, body, [{label, action}])` → emits a small wrapper widget.
/// Real ArkUI `AlertDialog.show({...})` is fired imperatively; harvest-time
/// emission can only stage the dialog config. Phase 2 v12 emits a
/// placeholder Text + comment documenting the runtime-side wiring (a
/// proper `showDialog(...)` runtime FFI is the v12.5 follow-up).
fn emit_modal(_args: &[Expr], _callbacks: &mut Vec<Expr>) -> String {
    "// Modal: configure with `showDialog(...)` from a closure body \
     (Phase 2 v12.5 — needs runtime FFI bridge to AlertDialog.show)\n\
     Text('[Modal — call showDialog() instead]').fontSize(16).fontColor('#888888')"
        .to_string()
}

/// `Menu([{label, action}])` → ArkUI menu shape. ArkUI's `.bindMenu(...)` is
/// a modifier on a triggering widget, not a standalone widget. Phase 2 v12
/// emits the menu as a `Column { Button(label) }` for each item — visible
/// + functional via the v2 callback registry — and the user can wrap it
/// in any container they want. Real `.bindMenu()` modifier integration is
/// v12.5.
fn emit_menu(args: &[Expr], callbacks: &mut Vec<Expr>) -> String {
    let items: Vec<&Expr> = match args.first() {
        Some(Expr::Array(items)) => items.iter().collect(),
        _ => Vec::new(),
    };
    let buttons: Vec<String> = items
        .iter()
        .map(|item| {
            let pairs: Option<Vec<(String, Expr)>> = match item {
                Expr::Object(props) => Some(props.clone()),
                _ => None,
            };
            let Some(pairs) = pairs else {
                return "Text('[invalid menu item]').fontSize(14).fontColor('#888888')".to_string();
            };
            let label = pairs
                .iter()
                .find(|(k, _)| k == "label")
                .and_then(|(_, v)| match v {
                    Expr::String(s) => Some(s.clone()),
                    _ => None,
                })
                .unwrap_or_else(|| "Item".to_string());
            let action = pairs.iter().find(|(k, _)| k == "action").map(|(_, v)| v);
            // Reuse Button's emit shape so action closures register
            // correctly via the v2 callback pipeline.
            let pseudo_args: Vec<Expr> = vec![
                Expr::String(label.clone()),
                action.cloned().unwrap_or(Expr::Number(0.0)),
            ];
            emit_button(&pseudo_args, callbacks)
        })
        .collect();
    format!(
        "Column({{ space: 4 }}) {{\n    {}\n}}",
        buttons.join("\n    "),
    )
}

/// `Grid(columns, items)` → ArkUI `Grid() { GridItem() {...} }` with
/// `.columnsTemplate('1fr 1fr ...')` for the column count.
#[allow(clippy::too_many_arguments)]
fn emit_grid(
    args: &[Expr],
    bindings: &HashMap<LocalId, Expr>,
    depth: usize,
    callbacks: &mut Vec<Expr>,
    text_slots: &mut Vec<TextSlot>,
    arkts_locals: &HashMap<LocalId, String>,
    classes: &[Class],
    state_registry: &HashMap<LocalId, StateBinding>,
    lazy_sources: &mut Vec<LazyDataSource>,
    extras: &mut HarvestExtras,
    mutations: &HashMap<LocalId, Vec<MutationEntry>>,
) -> String {
    let columns = numeric_arg(args, 0).unwrap_or(2.0) as i64;
    let columns = columns.clamp(1, 12);
    let template = (0..columns).map(|_| "1fr").collect::<Vec<_>>().join(" ");
    let items: Vec<&Expr> = match args.get(1) {
        Some(Expr::Array(items)) => items.iter().collect(),
        _ => Vec::new(),
    };
    let inner_indent = "    ".repeat(depth + 1);
    let outer_indent = "    ".repeat(depth);
    let grid_items: Vec<String> = items
        .iter()
        .map(|child| {
            let body = emit_widget(
                child,
                bindings,
                depth + 2,
                callbacks,
                text_slots,
                arkts_locals,
                classes,
                state_registry,
                lazy_sources,
                extras,
                mutations,
                None,
            );
            let body_indent = "    ".repeat(depth + 2);
            let body_indented = body
                .lines()
                .map(|l| format!("{}{}", body_indent, l))
                .collect::<Vec<_>>()
                .join("\n");
            format!(
                "{ind}GridItem() {{\n{body}\n{ind}}}",
                ind = inner_indent,
                body = body_indented,
            )
        })
        .collect();
    format!(
        "Grid() {{\n\
         {body}\n\
         {outer}}}.columnsTemplate('{template}')",
        body = grid_items.join("\n"),
        outer = outer_indent,
        template = template,
    )
}

/// Phase 2 v11: `NavStack(state, [{name, body}, ...])` for multi-page
/// navigation. Composes on the v6 state<T> + v3.2 reactive-Text bridge
/// instead of ArkUI's heavier `Navigation` + `NavPathStack` + @Builder
/// pattern — the user holds a `state<string>("home")` for the active
/// route, and `route.set("detail")` from any closure flips the visible
/// branch via the existing setText drain queue. Zero new runtime FFIs.
///
/// Native ArkUI back-gesture integration (proper `Navigation` +
/// `NavDestination` + `pageStack.pop()` on Android-style hardware-back)
/// is the v11.5 follow-up — it needs the @Builder-based pattern that
/// requires real navigator state on the page struct, not a string state.
/// The state-driven if/elseif emission shipped here covers the canonical
/// "tap button → forward; tap back button → state.set(prev)" happy
/// path, which is what most apps actually need.
///
/// Emit shape:
/// ```ets
/// Column() {
///     if (this.text_<sid> === 'home') {
///         <home body>
///     } else if (this.text_<sid> === 'detail') {
///         <detail body>
///     }
/// }
/// ```
///
/// `args[0]` must be `Expr::LocalGet(state_id)` referring to a
/// state<string> binding harvested by `collect_state_bindings`. If it's
/// not, emit a placeholder comment + use the first route as fallback so
/// the page still renders something.
#[allow(clippy::too_many_arguments)]
fn emit_nav_stack(
    args: &[Expr],
    bindings: &HashMap<LocalId, Expr>,
    depth: usize,
    callbacks: &mut Vec<Expr>,
    text_slots: &mut Vec<TextSlot>,
    arkts_locals: &HashMap<LocalId, String>,
    classes: &[Class],
    state_registry: &HashMap<LocalId, StateBinding>,
    lazy_sources: &mut Vec<LazyDataSource>,
    extras: &mut HarvestExtras,
    mutations: &HashMap<LocalId, Vec<MutationEntry>>,
) -> String {
    let inner_indent = "    ".repeat(depth + 1);
    let outer_indent = "    ".repeat(depth);

    // Resolve the state arg — must be a LocalGet whose id is registered
    // in state_registry (v6 collect_state_bindings handles this on init).
    // Register the synth_id with a text_slot so wrap_index_page emits
    // the @State decl + applyTextUpdate dispatch arm.
    let state_field = match args.first() {
        Some(Expr::LocalGet(id)) => state_registry.get(id).map(|b| {
            let field_id = sanitize_text_id(&b.synth_id);
            // Avoid double-registering if the user *also* called
            // route.text() somewhere else in the tree — text_slots is
            // de-duped by original_id at wrap_index_page emission time.
            text_slots.push(TextSlot {
                original_id: b.synth_id.clone(),
                field_id: field_id.clone(),
                initial: b.initial_str.clone(),
            });
            field_id
        }),
        _ => None,
    };

    // Routes array: each elem is `{name: string, body: Widget}` (open
    // Object) or `__AnonShape_*` New (Perry's closed-shape form).
    let route_specs: Vec<&Expr> = match args.get(1) {
        Some(Expr::Array(items)) => items.iter().collect(),
        _ => Vec::new(),
    };

    if route_specs.is_empty() {
        return format!(
            "Column() {{\n\
             {ind}// NavStack: empty routes array\n\
             {outer}}}",
            ind = inner_indent,
            outer = outer_indent,
        );
    }

    // No state binding — fall back to rendering only the first route so
    // the page still has something visible. Emit a developer-facing hint
    // comment so the lapse is discoverable.
    let Some(state_field) = state_field else {
        let first_body = extract_route_body(route_specs[0], classes)
            .map(|body| {
                emit_widget(
                    &body,
                    bindings,
                    depth + 1,
                    callbacks,
                    text_slots,
                    arkts_locals,
                    classes,
                    state_registry,
                    lazy_sources,
                    extras,
                    mutations,
                    None,
                )
            })
            .unwrap_or_else(|| "Text('[invalid route body]').fontSize(16)".to_string());
        let body_indent = "    ".repeat(depth + 1);
        let first_body_indented = first_body
            .lines()
            .map(|l| format!("{}{}", body_indent, l))
            .collect::<Vec<_>>()
            .join("\n");
        return format!(
            "Column() {{\n\
             {ind}// NavStack: first arg must be a `state<string>(...)` local — \
             rendering first route only\n\
             {body}\n\
             {outer}}}",
            ind = inner_indent,
            body = first_body_indented,
            outer = outer_indent,
        );
    };

    // Per-route emission: each gets an `if/else if` arm keyed on the
    // state field's current value. The first route is the `if`; the rest
    // are `else if`. We don't add a final `else` — if the state holds an
    // unknown route name, nothing renders, which is the expected
    // behavior for a cleared/unset route.
    let mut arms: Vec<String> = Vec::new();
    for (idx, spec) in route_specs.iter().enumerate() {
        let name = extract_route_name(spec, classes).unwrap_or_else(|| format!("route_{}", idx));
        let body_expr = extract_route_body(spec, classes);
        let body_str = body_expr
            .as_ref()
            .map(|b| {
                emit_widget(
                    b,
                    bindings,
                    depth + 2,
                    callbacks,
                    text_slots,
                    arkts_locals,
                    classes,
                    state_registry,
                    lazy_sources,
                    extras,
                    mutations,
                    None,
                )
            })
            .unwrap_or_else(|| "Text('[empty route]').fontSize(16)".to_string());
        let body_indent = "    ".repeat(depth + 2);
        let body_indented = body_str
            .lines()
            .map(|l| format!("{}{}", body_indent, l))
            .collect::<Vec<_>>()
            .join("\n");
        let keyword = if idx == 0 { "if" } else { "else if" };
        arms.push(format!(
            "{ind}{kw} (this.text_{field} === {lit}) {{\n\
             {body}\n\
             {ind}}}",
            ind = inner_indent,
            kw = keyword,
            field = state_field,
            lit = arkts_string_lit(&name),
            body = body_indented,
        ));
    }

    format!(
        "Column() {{\n\
         {body}\n\
         {outer}}}",
        body = arms.join(" "),
        outer = outer_indent,
    )
}

/// Extract the `name` field from a route spec object — handles open
/// `Expr::Object` and Perry's closed-shape `Expr::New { __AnonShape_* }`.
fn extract_route_name(spec: &Expr, classes: &[Class]) -> Option<String> {
    let pairs: Vec<(String, Expr)> = match spec {
        Expr::Object(props) => props.clone(),
        Expr::New {
            class_name, args, ..
        } if class_name.starts_with("__AnonShape_") => {
            classes.iter().find(|c| &c.name == class_name).map(|cls| {
                cls.fields
                    .iter()
                    .enumerate()
                    .filter_map(|(i, f)| args.get(i).map(|a| (f.name.clone(), a.clone())))
                    .collect()
            })?
        }
        _ => return None,
    };
    pairs
        .into_iter()
        .find(|(k, _)| k == "name")
        .and_then(|(_, v)| match v {
            Expr::String(s) => Some(s),
            _ => None,
        })
}

/// Extract the `body` field from a route spec object.
fn extract_route_body(spec: &Expr, classes: &[Class]) -> Option<Expr> {
    let pairs: Vec<(String, Expr)> = match spec {
        Expr::Object(props) => props.clone(),
        Expr::New {
            class_name, args, ..
        } if class_name.starts_with("__AnonShape_") => {
            classes.iter().find(|c| &c.name == class_name).map(|cls| {
                cls.fields
                    .iter()
                    .enumerate()
                    .filter_map(|(i, f)| args.get(i).map(|a| (f.name.clone(), a.clone())))
                    .collect()
            })?
        }
        _ => return None,
    };
    pairs.into_iter().find(|(k, _)| k == "body").map(|(_, v)| v)
}

/// struct Index { build() { Column() { ... } } }` page.
///
/// The leading imports make `perryEntry.invokeCallback` (Phase 2 v2),
/// `perryEntry.drainToast` + `promptAction.showToast` (v3 Option 1),
/// and `perryEntry.drainTextUpdate` (v3 Option 2) available to the
/// auto-emitted `.onClick(...)` handlers.
///
/// `text_slots` is the list of reactive `Text(content, id)` registrations
/// collected during the widget walk. For each slot we emit:
///   - `@State text_<id>: string = '<initial>'` field decl
///   - a switch arm in `applyTextUpdate(id, value)` that assigns to
///     the matching field
fn wrap_index_page(
    widget_body: &str,
    text_slots: &[TextSlot],
    lazy_sources: &[LazyDataSource],
    uses_media: bool,
    visibility_bindings: &HashMap<LocalId, VisibilityBinding>,
    view_builders: &[ViewBuilder],
    extras: &HarvestExtras,
) -> String {
    let indented = widget_body
        .lines()
        .map(|line| format!("            {}", line))
        .collect::<Vec<_>>()
        .join("\n");

    // @State decls (one per registered reactive Text). Field names use
    // the sanitized id; literals come straight from the user's TS.
    let state_decls: String = text_slots
        .iter()
        .map(|slot| {
            format!(
                "    @State text_{}: string = {};\n",
                slot.field_id,
                arkts_string_lit(&slot.initial)
            )
        })
        .collect();

    // Phase 2 v10 — `@State <id>: PerryListDataSource = new PerryListDataSource(<items>)`
    // for each LazyVStack(items.map(...)) in the harvested tree.
    let lazy_decls: String = lazy_sources
        .iter()
        .map(|src| {
            format!(
                "    @State {}: PerryListDataSource = new PerryListDataSource({});\n",
                src.field_id, src.items_source,
            )
        })
        .collect();

    // Phase 2 v10 — boilerplate IDataSource class. Emitted once per page
    // if any LazyVStack registered a source. Idempotent (no-op if none).
    let lazy_class = if lazy_sources.is_empty() {
        String::new()
    } else {
        "\
class PerryListDataSource implements IDataSource {\n\
    private items: any[];\n\
    private listeners: DataChangeListener[] = [];\n\
    constructor(items: any[]) { this.items = items; }\n\
    totalCount(): number { return this.items.length; }\n\
    getData(idx: number): any { return this.items[idx]; }\n\
    registerDataChangeListener(listener: DataChangeListener): void { this.listeners.push(listener); }\n\
    unregisterDataChangeListener(listener: DataChangeListener): void { this.listeners = this.listeners.filter(l => l !== listener); }\n\
}\n\n"
            .to_string()
    };

    // applyTextUpdate(id, value) switch arms. Always emit the method,
    // even with zero slots, so the auto-generated onClick body's call
    // resolves at ArkTS compile time. The switch matches the ORIGINAL
    // id (what the runtime queues from `setText("user-name", ...)`)
    // and assigns to the SANITIZED field name.
    let switch_arms: String = text_slots
        .iter()
        .map(|slot| {
            format!(
                "            case {}: this.text_{} = value; break;\n",
                arkts_string_lit(&slot.original_id),
                slot.field_id
            )
        })
        .collect();
    let apply_method = format!(
        "    applyTextUpdate(id: string, value: string): void {{\n\
         \x20\x20\x20\x20\x20\x20\x20\x20switch (id) {{\n\
         {arms}\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20default: break;\n\
         \x20\x20\x20\x20\x20\x20\x20\x20}}\n\
         \x20\x20\x20\x20}}\n",
        arms = switch_arms
    );

    // Phase 2 v3.5 — `@State hidden_<id>: boolean = <initial>` declarations
    // and applyVisibilityUpdate switch method. Iteration order is the
    // BTreeSet ordering from `collect_visibility_bindings` so the emitted
    // bytes are stable across re-runs.
    let mut sorted_visibility: Vec<(&LocalId, &VisibilityBinding)> =
        visibility_bindings.iter().collect();
    sorted_visibility.sort_by_key(|(id, _)| **id);
    let visibility_decls: String = sorted_visibility
        .iter()
        .map(|(_, binding)| {
            format!(
                "    @State hidden_{}: boolean = {};\n",
                binding.synth_id, binding.initial_hidden
            )
        })
        .collect();
    let visibility_arms: String = sorted_visibility
        .iter()
        .map(|(_, binding)| {
            format!(
                "            case {}: this.hidden_{} = hidden; break;\n",
                arkts_string_lit(&binding.synth_id),
                binding.synth_id
            )
        })
        .collect();
    let apply_visibility_method = format!(
        "    applyVisibilityUpdate(id: string, hidden: boolean): void {{\n\
         \x20\x20\x20\x20\x20\x20\x20\x20switch (id) {{\n\
         {arms}\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20default: break;\n\
         \x20\x20\x20\x20\x20\x20\x20\x20}}\n\
         \x20\x20\x20\x20}}\n",
        arms = visibility_arms
    );

    // Phase 2 v3.6 — `@State contentView_<target_synth>: string = 'default'`
    // declarations. One per UNIQUE target_synth (multiple view-builders
    // for the same target share the same @State). 'default' as the empty
    // initial value matches the `if (this.contentView_X === 'Y')`
    // condition: at startup, no view is active, so the lifted branches
    // don't render — the unconditional default content from module init
    // shows.
    let mut content_view_targets: std::collections::BTreeMap<String, ()> =
        std::collections::BTreeMap::new();
    for b in view_builders {
        content_view_targets.insert(b.target_synth.clone(), ());
    }
    let content_view_decls: String = content_view_targets
        .keys()
        .map(|target_synth| {
            format!(
                "    @State contentView_{}: string = 'default';\n",
                target_synth
            )
        })
        .collect();
    // applyContentViewUpdate switch: matches by target_synth and
    // assigns view_id to the corresponding @State field. Always emit
    // the method even with zero builders so the auto-emitted onClick
    // body's call resolves at ArkTS compile time.
    let mut sorted_targets: Vec<&String> = content_view_targets.keys().collect();
    sorted_targets.sort();
    let content_view_arms: String = sorted_targets
        .iter()
        .map(|target_synth| {
            format!(
                "            case {}: this.contentView_{} = view; break;\n",
                arkts_string_lit(target_synth),
                target_synth
            )
        })
        .collect();
    let apply_content_view_method = format!(
        "    applyContentViewUpdate(id: string, view: string): void {{\n\
         \x20\x20\x20\x20\x20\x20\x20\x20switch (id) {{\n\
         {arms}\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20default: break;\n\
         \x20\x20\x20\x20\x20\x20\x20\x20}}\n\
         \x20\x20\x20\x20}}\n",
        arms = content_view_arms
    );

    // Issue #369 — perry/media drain glue. Emitted only when the harvest
    // walker saw any `perry/media` NativeMethodCall in the module. The
    // pump runs on the ArkTS UI thread (setInterval is bound to the
    // current ability's run loop), so the AVPlayer dispatches and the
    // pushMediaState callback land on the same thread Perry's main()
    // runs on — closures fired from `media_playback::push_media_state`
    // can safely allocate into the per-thread arena.
    let (media_imports, media_decls, media_methods, media_pump) = if uses_media {
        media_glue()
    } else {
        (String::new(), String::new(), String::new(), String::new())
    };

    // Issue #669 / #670 — Chart + TreeView class-level state.
    let (chart_decls, tree_view_decls, tree_view_methods) = chart_and_tree_glue(extras);

    format!(
        "// Auto-generated by Perry (perry-codegen-arkts) — do not edit.\n\
         // Regenerated every `perry compile --target harmonyos`.\n\
         //\n\
         // Source of truth is the `App({{body: ...}})` call in your\n\
         // TypeScript entry. Edit there; this file is overwritten.\n\
         import perryEntry from 'libentry.so';\n\
         import promptAction from '@ohos.promptAction';\n\
         {media_imports}\
         \n\
         {lazy_class}\
         @Entry\n\
         @Component\n\
         struct Index {{\n\
         {states}\
         {visibility_decls}\
         {content_view_decls}\
         {lazy_decls}\
         {chart_decls}\
         {tree_view_decls}\
         {media_decls}\
         {apply}\
         {apply_visibility}\
         {apply_content_view}\
         {tree_view_methods}\
         {media_methods}\
         \x20\x20\x20\x20build() {{\n\
         \x20\x20\x20\x20\x20\x20\x20\x20Column() {{\n\
         {body}\n\
         \x20\x20\x20\x20\x20\x20\x20\x20}}\n\
         \x20\x20\x20\x20\x20\x20\x20\x20.width('100%')\n\
         \x20\x20\x20\x20\x20\x20\x20\x20.height('100%')\n\
         \x20\x20\x20\x20\x20\x20\x20\x20.justifyContent(FlexAlign.Center)\n\
         {media_pump}\
         \x20\x20\x20\x20}}\n\
         }}\n",
        states = state_decls,
        visibility_decls = visibility_decls,
        content_view_decls = content_view_decls,
        lazy_class = lazy_class,
        lazy_decls = lazy_decls,
        chart_decls = chart_decls,
        tree_view_decls = tree_view_decls,
        tree_view_methods = tree_view_methods,
        apply = apply_method,
        apply_visibility = apply_visibility_method,
        apply_content_view = apply_content_view_method,
        body = indented,
        media_imports = media_imports,
        media_decls = media_decls,
        media_methods = media_methods,
        media_pump = media_pump,
    )
}

/// Issue #369 — emit four pieces of ArkTS glue for the perry/media
/// drain bridge: the imports, the per-instance Map of AVPlayer handles,
/// the drain methods, and the `aboutToAppear` lifecycle hook that kicks
/// off the 100 ms `setInterval` pump.
///
/// The pump dispatches one entry per drain per tick (matches the NAPI
/// shape — `drainMediaCreate` / `drainMediaControl` / `drainNowPlaying`
/// each pop a single intent). On a steady-state app the queues are
/// empty, so each tick is three NAPI roundtrips returning undefined —
/// cheap.
///
/// AVSession lock-screen integration is deferred — the now-playing
/// drain pulls the metadata but only calls `console.info(...)` on it
/// for now. Full AVSession plumbing requires the user's hap manifest
/// to declare `ohos.permission.AVSESSION` and is tracked as a follow-up
/// to #369.
fn media_glue() -> (String, String, String, String) {
    let imports = "\
         import media from '@ohos.multimedia.media';\n"
        .to_string();

    let decls = "\
    // perry/media — issue #369. Map<handle, AVPlayer> populated as
    // ArkTS drains createPlayer requests from the runtime queue.
    private mediaPlayers: Map<number, media.AVPlayer> = new Map();\n\
    private mediaPumpHandle: number = -1;\n"
        .to_string();

    // The `runMediaPump` method drives all three drains per tick. We
    // intentionally call each drain in a `while` loop so a multi-op
    // burst (e.g. user taps play+volume+seek in rapid succession) gets
    // coalesced into one tick of work rather than waiting one 100 ms
    // interval per op.
    let methods = "\
    aboutToAppear() {\n\
        this.mediaPumpHandle = setInterval(() => { this.runMediaPump(); }, 100);\n\
    }\n\
    \n\
    aboutToDisappear() {\n\
        if (this.mediaPumpHandle !== -1) { clearInterval(this.mediaPumpHandle); this.mediaPumpHandle = -1; }\n\
        this.mediaPlayers.forEach((p) => { try { p.release(); } catch (e) {} });\n\
        this.mediaPlayers.clear();\n\
    }\n\
    \n\
    runMediaPump() {\n\
        // 1) Allocate AVPlayers for each pending createPlayer.\n\
        let createReq: any = perryEntry.drainMediaCreate();\n\
        while (createReq !== undefined) {\n\
            this.allocPlayer(createReq.handle, createReq.url);\n\
            createReq = perryEntry.drainMediaCreate();\n\
        }\n\
        // 2) Dispatch every queued control op against its handle.\n\
        let cmd: any = perryEntry.drainMediaControl();\n\
        while (cmd !== undefined) {\n\
            this.dispatchControl(cmd);\n\
            cmd = perryEntry.drainMediaControl();\n\
        }\n\
        // 3) Now-playing metadata — best-effort. Wired to AVSession in\n\
        //    a follow-up; for now we just surface it in hilog so the\n\
        //    user can verify the bridge is alive.\n\
        let np: any = perryEntry.drainNowPlaying();\n\
        while (np !== undefined) {\n\
            console.info(`perry/media now-playing handle=${np.handle} title=${np.title} artist=${np.artist}`);\n\
            np = perryEntry.drainNowPlaying();\n\
        }\n\
    }\n\
    \n\
    allocPlayer(handle: number, url: string) {\n\
        media.createAVPlayer().then((player: media.AVPlayer) => {\n\
            this.mediaPlayers.set(handle, player);\n\
            player.on('stateChange', (state: string, _reason: any) => {\n\
                let cur: number = (player.currentTime !== undefined ? player.currentTime / 1000 : 0);\n\
                let dur: number = (player.duration !== undefined && player.duration > 0 ? player.duration / 1000 : 0);\n\
                perryEntry.pushMediaState(handle, state, cur, dur);\n\
                if (state === 'initialized') {\n\
                    player.prepare();\n\
                }\n\
            });\n\
            player.on('timeUpdate', (timeMs: number) => {\n\
                let cur: number = timeMs / 1000;\n\
                let dur: number = (player.duration !== undefined && player.duration > 0 ? player.duration / 1000 : 0);\n\
                perryEntry.pushMediaState(handle, 'playing', cur, dur);\n\
            });\n\
            player.on('error', (err: any) => {\n\
                console.error(`perry/media error handle=${handle} code=${err && err.code} msg=${err && err.message}`);\n\
                perryEntry.pushMediaState(handle, 'error', 0, 0);\n\
            });\n\
            player.on('endOfStream', () => {\n\
                perryEntry.pushMediaState(handle, 'completed', 0, 0);\n\
            });\n\
            player.url = url;\n\
        }).catch((err: any) => {\n\
            console.error(`perry/media createAVPlayer failed handle=${handle} url=${url} err=${err}`);\n\
            perryEntry.pushMediaState(handle, 'error', 0, 0);\n\
        });\n\
    }\n\
    \n\
    dispatchControl(cmd: any) {\n\
        const player: media.AVPlayer | undefined = this.mediaPlayers.get(cmd.handle);\n\
        if (player === undefined) { return; }\n\
        try {\n\
            switch (cmd.op) {\n\
                case 'play': player.play(); break;\n\
                case 'pause': player.pause(); break;\n\
                case 'stop': player.stop(); break;\n\
                case 'seek': player.seek(Math.floor(cmd.seconds * 1000)); break;\n\
                case 'setVolume': player.setVolume(cmd.volume); break;\n\
                case 'setRate':\n\
                    // AVPlayer.setSpeed takes an enum (0..6 mapped to 0.75x..2x).\n\
                    // Map the raw rate to the closest enum bucket.\n\
                    if (cmd.rate <= 0.5) { player.setSpeed(0); }\n\
                    else if (cmd.rate <= 0.875) { player.setSpeed(1); }\n\
                    else if (cmd.rate <= 1.125) { player.setSpeed(2); }\n\
                    else if (cmd.rate <= 1.375) { player.setSpeed(3); }\n\
                    else if (cmd.rate <= 1.75) { player.setSpeed(4); }\n\
                    else { player.setSpeed(5); }\n\
                    break;\n\
                case 'destroy':\n\
                    player.release();\n\
                    this.mediaPlayers.delete(cmd.handle);\n\
                    break;\n\
                default: break;\n\
            }\n\
        } catch (e) {\n\
            console.error(`perry/media dispatch failed op=${cmd.op} handle=${cmd.handle} err=${e}`);\n\
        }\n\
    }\n"
        .to_string();

    // Pump is started/stopped via the lifecycle methods above (declared
    // in `media_methods`), so there's nothing extra to add inside
    // `build()`. Returned slot stays as the empty string.
    let pump = String::new();

    (imports, decls, methods, pump)
}

// ----- helpers -----

/// First arg matched as a string literal. Returns None if absent or
/// non-literal so callers can pick a sensible default.
fn first_string_arg(args: &[Expr]) -> Option<String> {
    match args.first() {
        Some(Expr::String(s)) => Some(s.clone()),
        _ => None,
    }
}

/// Get arg at `idx` as a Number, supporting both Integer and Number HIR
/// variants since perry-hir distinguishes them.
fn numeric_arg(args: &[Expr], idx: usize) -> Option<f64> {
    match args.get(idx) {
        Some(Expr::Number(n)) => Some(*n),
        Some(Expr::Integer(n)) => Some(*n as f64),
        _ => None,
    }
}

/// Like `numeric_arg`, but resolves `Expr::LocalGet(id)` through `bindings`
/// — e.g. `let size = 28; textSetFontSize(t, size)` resolves to `28`.
/// Returns `None` only when the chain bottoms out in a non-numeric leaf
/// (function call, prop-access, etc.) or hits an unbound local. Mango
/// uses this pattern heavily for theme-controlled values.
fn numeric_arg_resolved(
    args: &[Expr],
    idx: usize,
    bindings: &HashMap<LocalId, Expr>,
) -> Option<f64> {
    let mut cur = args.get(idx)?;
    // Bound the walk to avoid pathological binding cycles.
    for _ in 0..16 {
        match cur {
            Expr::Number(n) => return Some(*n),
            Expr::Integer(n) => return Some(*n as f64),
            Expr::Bool(true) => return Some(1.0),
            Expr::Bool(false) => return Some(0.0),
            Expr::LocalGet(id) => {
                cur = bindings.get(id)?;
            }
            // `cond ? a : b` — same heuristic as the widget Conditional
            // emitter and resolve_string_arg: const-fold the condition,
            // pick the resolved branch; default to then-branch when
            // unresolvable (Mango: `widgetSetWidth(logo, mobile ? 40 :
            // 44)` resolves through the ternary to the numeric leaf).
            Expr::Conditional {
                condition,
                then_expr,
                else_expr,
            } => {
                cur = match evaluate_condition(condition, bindings, &HashMap::new()) {
                    Some(false) => else_expr,
                    _ => then_expr,
                };
            }
            // HarmonyOS-stubbed perry/system functions return 0 (see
            // is_harmonyos_zero_fn). Treating them as 0 here makes
            // theme-color resolution like `txR = dark ? 0.91 : 0.17`
            // pick the else-branch (light mode) when dark is bound to
            // `isDarkMode()`.
            Expr::Call { callee, .. } => match callee.as_ref() {
                Expr::ExternFuncRef { name, .. } if is_harmonyos_zero_fn(name) => return Some(0.0),
                _ => return None,
            },
            Expr::NativeMethodCall { module, method, .. }
                if module == "perry/system" && is_harmonyos_zero_fn(method) =>
            {
                return Some(0.0);
            }
            _ => return None,
        }
    }
    None
}

/// Format a float as ArkTS source. Whole numbers emit without a decimal
/// (`8`, not `8.0`) to match ArkUI's idiomatic style.
fn fmt_num(n: f64) -> String {
    if n == n.trunc() && n.abs() < 1e15 {
        format!("{}", n as i64)
    } else {
        format!("{}", n)
    }
}

/// Escape a Rust string into an ArkTS single-quoted string literal.
/// ArkTS shares JS string-literal rules — escape backslash + single quote.
fn arkts_string_lit(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\'' => out.push_str("\\'"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out.push('\'');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_module() -> Module {
        Module {
            name: "test".to_string(),
            imports: vec![],
            exports: vec![],
            classes: vec![],
            interfaces: vec![],
            type_aliases: vec![],
            enums: vec![],
            globals: vec![],
            functions: vec![],
            init: vec![],
            exported_native_instances: vec![],
            exported_func_return_native_instances: vec![],
            exported_objects: vec![],
            exported_functions: vec![],
            widgets: vec![],
            uses_fetch: false,
            init_was_unrolled: false,
            extern_funcs: vec![],
        }
    }

    fn nmc(method: &str, args: Vec<Expr>) -> Expr {
        Expr::NativeMethodCall {
            module: "perry/ui".to_string(),
            class_name: None,
            object: None,
            method: method.to_string(),
            args,
        }
    }

    fn app_with_body(body: Expr) -> Stmt {
        Stmt::Expr(Expr::NativeMethodCall {
            module: "perry/ui".to_string(),
            class_name: None,
            object: None,
            method: "App".to_string(),
            args: vec![Expr::Object(vec![("body".to_string(), body)])],
        })
    }

    fn closure_stub() -> Expr {
        Expr::Closure {
            func_id: 0 as perry_types::FuncId,
            params: vec![],
            return_type: perry_types::Type::Any,
            body: vec![],
            captures: vec![],
            mutable_captures: vec![],
            captures_this: false,
            enclosing_class: None,
            is_async: false,
        }
    }

    #[test]
    fn emits_none_for_empty_module() {
        let mut m = empty_module();
        assert!(emit_index_ets(&mut m).unwrap().is_none());
    }

    #[test]
    fn text_strips_app_call() {
        let mut m = empty_module();
        m.init
            .push(app_with_body(nmc("Text", vec![Expr::String("hi".into())])));
        let r = emit_index_ets(&mut m).unwrap().unwrap();
        assert!(r.ets_source.contains("Text('hi').fontSize(20)"));
        assert!(matches!(m.init[0], Stmt::Expr(Expr::Number(_))));
        assert_eq!(r.callbacks.len(), 0);
    }

    #[test]
    fn vstack_with_text_children() {
        let mut m = empty_module();
        m.init.push(app_with_body(nmc(
            "VStack",
            vec![Expr::Array(vec![
                nmc("Text", vec![Expr::String("a".into())]),
                nmc("Text", vec![Expr::String("b".into())]),
            ])],
        )));
        let r = emit_index_ets(&mut m).unwrap().unwrap();
        assert!(r.ets_source.contains("Column({ space: 8 })"));
        assert!(r.ets_source.contains("Text('a').fontSize(20)"));
        assert!(r.ets_source.contains("Text('b').fontSize(20)"));
    }

    #[test]
    fn vstack_with_explicit_spacing() {
        let mut m = empty_module();
        m.init.push(app_with_body(nmc(
            "VStack",
            vec![
                Expr::Number(16.0),
                Expr::Array(vec![nmc("Text", vec![Expr::String("a".into())])]),
            ],
        )));
        let r = emit_index_ets(&mut m).unwrap().unwrap();
        assert!(r.ets_source.contains("Column({ space: 16 })"));
    }

    #[test]
    fn hstack_emits_row() {
        let mut m = empty_module();
        m.init.push(app_with_body(nmc(
            "HStack",
            vec![Expr::Array(vec![nmc("Spacer", vec![])])],
        )));
        let r = emit_index_ets(&mut m).unwrap().unwrap();
        assert!(r.ets_source.contains("Row({ space: 8 })"));
        assert!(r.ets_source.contains("Blank()"));
    }

    #[test]
    fn button_label_only_no_closure_drops_onclick() {
        let mut m = empty_module();
        m.init.push(app_with_body(nmc(
            "Button",
            vec![
                Expr::String("Save".into()),
                Expr::Number(0.0), // not a closure — placeholder
            ],
        )));
        let r = emit_index_ets(&mut m).unwrap().unwrap();
        assert!(r.ets_source.contains("Button('Save').fontSize(16)"));
        assert!(!r.ets_source.contains(".onClick"));
        assert_eq!(r.callbacks.len(), 0);
    }

    #[test]
    fn button_with_closure_emits_onclick_and_captures_callback() {
        // Phase 2 v2 + v3 headline test: Button("Save", () => {}) emits
        // an onClick that invokes the registered closure THEN drains the
        // toast queue (so `showToast(msg)` calls inside the closure body
        // produce visible popups).
        let mut m = empty_module();
        m.init.push(app_with_body(nmc(
            "Button",
            vec![Expr::String("Save".into()), closure_stub()],
        )));
        let r = emit_index_ets(&mut m).unwrap().unwrap();
        // v2: invokeCallback dispatches the registered closure.
        assert!(r.ets_source.contains("perryEntry.invokeCallback(0)"));
        // v3: drain loop dispatches queued toasts after the closure
        // returns. Single-line search avoids depending on whitespace.
        assert!(r.ets_source.contains("perryEntry.drainToast()"));
        assert!(r.ets_source.contains("promptAction.showToast"));
        assert_eq!(r.callbacks.len(), 1);
        assert!(matches!(r.callbacks[0], Expr::Closure { .. }));
        // Page wrapper imports both perryEntry and promptAction so the
        // auto-emitted onClick body resolves at ArkTS compile time.
        assert!(r
            .ets_source
            .contains("import perryEntry from 'libentry.so'"));
        assert!(r
            .ets_source
            .contains("import promptAction from '@ohos.promptAction'"));
    }

    #[test]
    fn multi_button_assigns_sequential_callback_slots() {
        // Two buttons in a VStack — slot 0 and slot 1 in declaration order.
        let mut m = empty_module();
        m.init.push(app_with_body(nmc(
            "VStack",
            vec![Expr::Array(vec![
                nmc("Button", vec![Expr::String("First".into()), closure_stub()]),
                nmc(
                    "Button",
                    vec![Expr::String("Second".into()), closure_stub()],
                ),
            ])],
        )));
        let r = emit_index_ets(&mut m).unwrap().unwrap();
        assert!(r.ets_source.contains("perryEntry.invokeCallback(0)"));
        assert!(r.ets_source.contains("perryEntry.invokeCallback(1)"));
        assert_eq!(r.callbacks.len(), 2);
    }

    #[test]
    fn textfield_placeholder() {
        let mut m = empty_module();
        m.init.push(app_with_body(nmc(
            "TextField",
            vec![Expr::String("Search…".into()), Expr::Number(0.0)],
        )));
        let r = emit_index_ets(&mut m).unwrap().unwrap();
        assert!(r
            .ets_source
            .contains("TextInput({ placeholder: 'Search…' })"));
    }

    #[test]
    fn toggle_with_label_emits_row() {
        let mut m = empty_module();
        m.init.push(app_with_body(nmc(
            "Toggle",
            vec![Expr::String("Notifications".into()), Expr::Number(0.0)],
        )));
        let r = emit_index_ets(&mut m).unwrap().unwrap();
        assert!(r.ets_source.contains("Row({ space: 8 })"));
        assert!(r.ets_source.contains("Text('Notifications')"));
        assert!(r
            .ets_source
            .contains("Toggle({ type: ToggleType.Switch, isOn: false })"));
    }

    #[test]
    fn slider_min_max() {
        let mut m = empty_module();
        m.init.push(app_with_body(nmc(
            "Slider",
            vec![
                Expr::Number(0.0),
                Expr::Number(100.0),
                Expr::Number(0.0), // would be closure
            ],
        )));
        let r = emit_index_ets(&mut m).unwrap().unwrap();
        assert!(r.ets_source.contains("min: 0"));
        assert!(r.ets_source.contains("max: 100"));
    }

    #[test]
    fn divider_no_args() {
        let mut m = empty_module();
        m.init.push(app_with_body(nmc("Divider", vec![])));
        let r = emit_index_ets(&mut m).unwrap().unwrap();
        assert!(r.ets_source.contains("Divider()"));
    }

    #[test]
    fn nested_vstack_in_hstack() {
        let mut m = empty_module();
        m.init.push(app_with_body(nmc(
            "VStack",
            vec![Expr::Array(vec![nmc(
                "HStack",
                vec![Expr::Array(vec![
                    nmc("Text", vec![Expr::String("L".into())]),
                    nmc("Text", vec![Expr::String("R".into())]),
                ])],
            )])],
        )));
        let r = emit_index_ets(&mut m).unwrap().unwrap();
        assert!(r.ets_source.contains("Column({ space: 8 })"));
        assert!(r.ets_source.contains("Row({ space: 8 })"));
        assert!(r.ets_source.contains("Text('L')"));
        assert!(r.ets_source.contains("Text('R')"));
    }

    #[test]
    fn local_get_escape_follows_const_binding() {
        let mut m = empty_module();
        // Simulate: const t = Text("via let"); App({body: t});
        m.init.push(Stmt::Let {
            id: 7,
            name: "t".to_string(),
            ty: perry_types::Type::Any,
            mutable: false,
            init: Some(nmc("Text", vec![Expr::String("via let".into())])),
        });
        m.init.push(app_with_body(Expr::LocalGet(7)));
        let r = emit_index_ets(&mut m).unwrap().unwrap();
        assert!(r.ets_source.contains("Text('via let')"));
    }

    #[test]
    fn text_with_id_registers_reactive_slot() {
        // Phase 2 v3 Option 2: Text("Count: 0", "counter") must:
        //   - emit @State text_counter: string = 'Count: 0' on the page
        //   - emit Text(this.text_counter) at the widget site
        //   - register a switch arm in applyTextUpdate
        let mut m = empty_module();
        m.init.push(app_with_body(nmc(
            "Text",
            vec![
                Expr::String("Count: 0".into()),
                Expr::String("counter".into()),
            ],
        )));
        let r = emit_index_ets(&mut m).unwrap().unwrap();
        assert!(r
            .ets_source
            .contains("@State text_counter: string = 'Count: 0'"));
        assert!(r.ets_source.contains("Text(this.text_counter)"));
        assert!(r
            .ets_source
            .contains("case 'counter': this.text_counter = value; break;"));
    }

    #[test]
    fn text_id_sanitization_drops_invalid_chars() {
        let mut m = empty_module();
        m.init.push(app_with_body(nmc(
            "Text",
            vec![
                Expr::String("hi".into()),
                Expr::String("user-name".into()), // hyphen → underscore
            ],
        )));
        let r = emit_index_ets(&mut m).unwrap().unwrap();
        assert!(r.ets_source.contains("@State text_user_name"));
        assert!(r.ets_source.contains("case 'user-name'"));
    }

    #[test]
    fn toggle_with_closure_emits_onchange_with_invokecallback1() {
        let mut m = empty_module();
        m.init.push(app_with_body(nmc(
            "Toggle",
            vec![Expr::String("Notify".into()), closure_stub()],
        )));
        let r = emit_index_ets(&mut m).unwrap().unwrap();
        assert!(r.ets_source.contains(".onChange((isOn: boolean) => {"));
        assert!(r.ets_source.contains("perryEntry.invokeCallback1(0, isOn)"));
        assert_eq!(r.callbacks.len(), 1);
    }

    #[test]
    fn textfield_with_closure_forwards_value_to_invokecallback1() {
        let mut m = empty_module();
        m.init.push(app_with_body(nmc(
            "TextField",
            vec![Expr::String("Search…".into()), closure_stub()],
        )));
        let r = emit_index_ets(&mut m).unwrap().unwrap();
        assert!(r.ets_source.contains(".onChange((value: string) => {"));
        assert!(r
            .ets_source
            .contains("perryEntry.invokeCallback1(0, value)"));
    }

    #[test]
    fn slider_with_closure_forwards_value_to_invokecallback1() {
        let mut m = empty_module();
        m.init.push(app_with_body(nmc(
            "Slider",
            vec![Expr::Number(0.0), Expr::Number(100.0), closure_stub()],
        )));
        let r = emit_index_ets(&mut m).unwrap().unwrap();
        assert!(r
            .ets_source
            .contains(".onChange((value: number, _mode: SliderChangeMode) => {"));
        assert!(r
            .ets_source
            .contains("perryEntry.invokeCallback1(0, value)"));
    }

    #[test]
    fn button_onclick_drains_both_toast_and_text_update_queues() {
        // The generated onClick body should drain BOTH queues so a
        // closure that calls showToast AND setText sees both effects.
        let mut m = empty_module();
        m.init.push(app_with_body(nmc(
            "Button",
            vec![Expr::String("Tap".into()), closure_stub()],
        )));
        let r = emit_index_ets(&mut m).unwrap().unwrap();
        assert!(r.ets_source.contains("perryEntry.drainToast()"));
        assert!(r.ets_source.contains("perryEntry.drainTextUpdate()"));
        assert!(r
            .ets_source
            .contains("this.applyTextUpdate(__u.id, __u.value)"));
    }

    // ----- Phase 2 v13: animation / shadow / textDecoration / image asset -----

    #[test]
    fn animation_modifier_maps_curve_string_to_curve_enum() {
        let mut m = empty_module();
        m.init.push(app_with_body(nmc(
            "Text",
            vec![
                Expr::String("hi".into()),
                Expr::Object(vec![(
                    "animation".into(),
                    Expr::Object(vec![
                        ("duration".into(), Expr::Number(300.0)),
                        ("curve".into(), Expr::String("ease-in".into())),
                    ]),
                )]),
            ],
        )));
        let r = emit_index_ets(&mut m).unwrap().unwrap();
        assert!(r
            .ets_source
            .contains(".animation({ duration: 300, curve: Curve.EaseIn })"));
    }

    #[test]
    fn shadow_modifier_maps_blur_to_radius_offsets_to_offsetXY() {
        let mut m = empty_module();
        m.init.push(app_with_body(nmc(
            "Text",
            vec![
                Expr::String("hi".into()),
                Expr::Object(vec![(
                    "shadow".into(),
                    Expr::Object(vec![
                        ("color".into(), Expr::String("black".into())),
                        ("blur".into(), Expr::Number(8.0)),
                        ("offsetX".into(), Expr::Number(2.0)),
                        ("offsetY".into(), Expr::Number(4.0)),
                    ]),
                )]),
            ],
        )));
        let r = emit_index_ets(&mut m).unwrap().unwrap();
        // ArkUI's shadow uses `radius` not `blur`; offsetX/Y match.
        assert!(r.ets_source.contains(".shadow({"));
        assert!(r.ets_source.contains("color: 'black'"));
        assert!(r.ets_source.contains("radius: 8"));
        assert!(r.ets_source.contains("offsetX: 2"));
        assert!(r.ets_source.contains("offsetY: 4"));
    }

    #[test]
    fn text_decoration_underline_maps_to_decoration_modifier() {
        let mut m = empty_module();
        m.init.push(app_with_body(nmc(
            "Text",
            vec![
                Expr::String("hi".into()),
                Expr::Object(vec![(
                    "textDecoration".into(),
                    Expr::String("underline".into()),
                )]),
            ],
        )));
        let r = emit_index_ets(&mut m).unwrap().unwrap();
        assert!(r
            .ets_source
            .contains(".decoration({ type: TextDecorationType.Underline })"));
    }

    #[test]
    fn text_decoration_strikethrough_maps_to_linethrough() {
        let mut m = empty_module();
        m.init.push(app_with_body(nmc(
            "Text",
            vec![
                Expr::String("hi".into()),
                Expr::Object(vec![(
                    "textDecoration".into(),
                    Expr::String("strikethrough".into()),
                )]),
            ],
        )));
        let r = emit_index_ets(&mut m).unwrap().unwrap();
        assert!(r
            .ets_source
            .contains(".decoration({ type: TextDecorationType.LineThrough })"));
    }

    #[test]
    fn image_app_media_path_maps_to_resource_accessor() {
        let mut m = empty_module();
        m.init.push(app_with_body(nmc(
            "Image",
            vec![Expr::String("@app.media/icon".into())],
        )));
        let r = emit_index_ets(&mut m).unwrap().unwrap();
        // `$r('app.media.icon')` (no quotes around the $r() arg).
        assert!(r.ets_source.contains("Image($r('app.media.icon'))"));
        // Plain string passthrough still works for HTTP URLs etc.
        assert!(!r.ets_source.contains("'@app.media/icon'"));
    }

    #[test]
    fn image_plain_url_passes_through_as_string() {
        let mut m = empty_module();
        m.init.push(app_with_body(nmc(
            "Image",
            vec![Expr::String("https://example.com/foo.png".into())],
        )));
        let r = emit_index_ets(&mut m).unwrap().unwrap();
        assert!(r
            .ets_source
            .contains("Image('https://example.com/foo.png')"));
    }

    // ----- Phase 2 v5: inline style + ForEach -----

    #[test]
    fn inline_style_object_emits_arkui_modifier_chain() {
        // Button("Save", () => {}, { backgroundColor: "blue", borderRadius: 8, opacity: 0.9 })
        let mut m = empty_module();
        m.init.push(app_with_body(nmc(
            "Button",
            vec![
                Expr::String("Save".into()),
                closure_stub(),
                Expr::Object(vec![
                    ("backgroundColor".into(), Expr::String("blue".into())),
                    ("borderRadius".into(), Expr::Number(8.0)),
                    ("opacity".into(), Expr::Number(0.9)),
                ]),
            ],
        )));
        let r = emit_index_ets(&mut m).unwrap().unwrap();
        assert!(r.ets_source.contains(".backgroundColor('blue')"));
        assert!(r.ets_source.contains(".borderRadius(8)"));
        assert!(r.ets_source.contains(".opacity(0.9)"));
    }

    #[test]
    fn inline_style_color_object_emits_rgba() {
        // Text("hi", { color: { r: 0.2, g: 0.5, b: 0.95, a: 1 } })
        let mut m = empty_module();
        m.init.push(app_with_body(nmc(
            "Text",
            vec![
                Expr::String("hi".into()),
                Expr::Object(vec![(
                    "color".into(),
                    Expr::Object(vec![
                        ("r".into(), Expr::Number(0.2)),
                        ("g".into(), Expr::Number(0.5)),
                        ("b".into(), Expr::Number(0.95)),
                        ("a".into(), Expr::Number(1.0)),
                    ]),
                )]),
            ],
        )));
        let r = emit_index_ets(&mut m).unwrap().unwrap();
        // 0.2 * 255 = 51, 0.5 * 255 ≈ 128, 0.95 * 255 ≈ 242
        assert!(r.ets_source.contains(".fontColor('rgba(51, 128, 242, 1)')"));
    }

    #[test]
    fn inline_style_padding_per_side_object() {
        let mut m = empty_module();
        m.init.push(app_with_body(nmc(
            "Text",
            vec![
                Expr::String("hi".into()),
                Expr::Object(vec![(
                    "padding".into(),
                    Expr::Object(vec![
                        ("top".into(), Expr::Number(10.0)),
                        ("bottom".into(), Expr::Number(20.0)),
                    ]),
                )]),
            ],
        )));
        let r = emit_index_ets(&mut m).unwrap().unwrap();
        assert!(r.ets_source.contains(".padding({ top: 10, bottom: 20 })"));
    }

    #[test]
    fn inline_style_border_combines_color_and_width() {
        let mut m = empty_module();
        m.init.push(app_with_body(nmc(
            "Text",
            vec![
                Expr::String("hi".into()),
                Expr::Object(vec![
                    ("borderColor".into(), Expr::String("red".into())),
                    ("borderWidth".into(), Expr::Number(2.0)),
                ]),
            ],
        )));
        let r = emit_index_ets(&mut m).unwrap().unwrap();
        // ArkUI's `.border({ width, color })` is one combined modifier.
        assert!(r.ets_source.contains(".border({ width: 2, color: 'red' })"));
    }

    #[test]
    fn text_with_id_string_is_NOT_treated_as_style() {
        // Text("Count: 0", "counter") — second string arg is the reactive
        // id, NOT a style object. extract_style_object returns None for
        // String args, so the v3.2 reactive path still wins.
        let mut m = empty_module();
        m.init.push(app_with_body(nmc(
            "Text",
            vec![
                Expr::String("Count: 0".into()),
                Expr::String("counter".into()),
            ],
        )));
        let r = emit_index_ets(&mut m).unwrap().unwrap();
        assert!(r.ets_source.contains("Text(this.text_counter)"));
        // Should NOT have any inline-style modifiers tacked on.
        assert!(!r.ets_source.contains(".backgroundColor"));
    }

    #[test]
    fn for_each_lowers_array_map_in_vstack() {
        // VStack(items.map(item => Text(item))) — the closure-param `item`
        // resolves via arkts_locals → __item in the emitted ForEach body.
        let mut m = empty_module();
        // Build `Expr::ArrayMap { array: ["a","b","c"], callback: (p) => Text(p) }`.
        let item_param = perry_hir::ir::Param {
            id: 42,
            name: "item".to_string(),
            ty: perry_types::Type::Any,
            default: None,
            is_rest: false,
        };
        let inner_text = nmc("Text", vec![Expr::LocalGet(42)]);
        let map_expr = Expr::ArrayMap {
            array: Box::new(Expr::Array(vec![
                Expr::String("a".into()),
                Expr::String("b".into()),
                Expr::String("c".into()),
            ])),
            callback: Box::new(Expr::Closure {
                func_id: 0 as perry_types::FuncId,
                params: vec![item_param],
                return_type: perry_types::Type::Any,
                body: vec![Stmt::Return(Some(inner_text))],
                captures: vec![],
                mutable_captures: vec![],
                captures_this: false,
                enclosing_class: None,
                is_async: false,
            }),
        };
        m.init.push(app_with_body(nmc(
            "VStack",
            vec![Expr::Array(vec![map_expr])],
        )));
        let r = emit_index_ets(&mut m).unwrap().unwrap();
        assert!(r
            .ets_source
            .contains("ForEach(['a', 'b', 'c'], (__item: any)"));
        // Body resolves `LocalGet(item_param.id)` → __item.
        assert!(r.ets_source.contains("Text(__item)"));
    }

    #[test]
    // ----- Phase 2 v12: Tabs / Modal / Menu / Grid -----
    #[test]
    fn tabs_emits_tabcontent_per_spec() {
        // Tabs([{label: "Home", body: Text("home content")}, {label: "Settings", body: Text("settings")}])
        let mut m = empty_module();
        let tab1 = Expr::Object(vec![
            ("label".into(), Expr::String("Home".into())),
            (
                "body".into(),
                nmc("Text", vec![Expr::String("home content".into())]),
            ),
        ]);
        let tab2 = Expr::Object(vec![
            ("label".into(), Expr::String("Settings".into())),
            (
                "body".into(),
                nmc("Text", vec![Expr::String("settings".into())]),
            ),
        ]);
        m.init.push(app_with_body(nmc(
            "Tabs",
            vec![Expr::Array(vec![tab1, tab2])],
        )));
        let r = emit_index_ets(&mut m).unwrap().unwrap();
        assert!(r.ets_source.contains("Tabs() {"));
        assert!(r.ets_source.contains(".tabBar('Home')"));
        assert!(r.ets_source.contains(".tabBar('Settings')"));
        assert!(r.ets_source.contains("Text('home content')"));
        assert!(r.ets_source.contains("Text('settings')"));
    }

    #[test]
    fn menu_emits_buttons_per_item() {
        let mut m = empty_module();
        let item1 = Expr::Object(vec![
            ("label".into(), Expr::String("Edit".into())),
            ("action".into(), closure_stub()),
        ]);
        let item2 = Expr::Object(vec![
            ("label".into(), Expr::String("Delete".into())),
            ("action".into(), closure_stub()),
        ]);
        m.init.push(app_with_body(nmc(
            "Menu",
            vec![Expr::Array(vec![item1, item2])],
        )));
        let r = emit_index_ets(&mut m).unwrap().unwrap();
        assert!(r.ets_source.contains("Button('Edit')"));
        assert!(r.ets_source.contains("Button('Delete')"));
        // Both action closures should register (slot 0 + slot 1).
        assert!(r.ets_source.contains("perryEntry.invokeCallback(0)"));
        assert!(r.ets_source.contains("perryEntry.invokeCallback(1)"));
        assert_eq!(r.callbacks.len(), 2);
    }

    #[test]
    fn grid_emits_columns_template_and_griditems() {
        // Grid(3, [Text("a"), Text("b"), Text("c")])
        let mut m = empty_module();
        m.init.push(app_with_body(nmc(
            "Grid",
            vec![
                Expr::Number(3.0),
                Expr::Array(vec![
                    nmc("Text", vec![Expr::String("a".into())]),
                    nmc("Text", vec![Expr::String("b".into())]),
                    nmc("Text", vec![Expr::String("c".into())]),
                ]),
            ],
        )));
        let r = emit_index_ets(&mut m).unwrap().unwrap();
        assert!(r.ets_source.contains("Grid() {"));
        assert!(r.ets_source.contains(".columnsTemplate('1fr 1fr 1fr')"));
        assert!(r.ets_source.contains("GridItem()"));
        assert!(r.ets_source.contains("Text('a')"));
        assert!(r.ets_source.contains("Text('c')"));
    }

    #[test]
    fn modal_emits_placeholder_with_runtime_hint() {
        let mut m = empty_module();
        m.init.push(app_with_body(nmc(
            "Modal",
            vec![Expr::String("Title".into())],
        )));
        let r = emit_index_ets(&mut m).unwrap().unwrap();
        // Phase 2 v12 emits a placeholder + comment pointing at the
        // showDialog runtime FFI follow-up.
        assert!(r.ets_source.contains("// Modal:"));
        assert!(r.ets_source.contains("showDialog"));
    }

    // ----- Phase 2 v11: NavStack multi-page navigation -----

    #[test]
    fn navstack_emits_state_driven_branches() {
        // const route = state("home");
        // App({body: NavStack(route, [
        //     {name: "home", body: Text("Home")},
        //     {name: "detail", body: Text("Detail")},
        // ])});
        let mut m = empty_module();
        m.init.push(Stmt::Let {
            id: 5,
            name: "route".to_string(),
            ty: perry_types::Type::Any,
            mutable: false,
            init: Some(state_call(Expr::String("home".into()))),
        });
        let routes = Expr::Array(vec![
            Expr::Object(vec![
                ("name".into(), Expr::String("home".into())),
                (
                    "body".into(),
                    nmc("Text", vec![Expr::String("Home".into())]),
                ),
            ]),
            Expr::Object(vec![
                ("name".into(), Expr::String("detail".into())),
                (
                    "body".into(),
                    nmc("Text", vec![Expr::String("Detail".into())]),
                ),
            ]),
        ]);
        m.init.push(app_with_body(nmc(
            "NavStack",
            vec![Expr::LocalGet(5), routes],
        )));
        let r = emit_index_ets(&mut m).unwrap().unwrap();
        // Should register an @State decl for the synth id (v6 path).
        assert!(
            r.ets_source.contains("@State text___state_0"),
            "missing v6 @State decl:\n{}",
            r.ets_source
        );
        // First arm is `if`, second is `else if`. The state field used
        // is `this.text___state_0` since the synth id (`__state_0`)
        // sanitizes to `__state_0` and gets prefixed with `text_`.
        assert!(
            r.ets_source.contains("if (this.text___state_0 === 'home')"),
            "missing if-arm for first route:\n{}",
            r.ets_source
        );
        assert!(
            r.ets_source
                .contains("else if (this.text___state_0 === 'detail')"),
            "missing else-if for second route:\n{}",
            r.ets_source
        );
        // Both bodies should be present.
        assert!(r.ets_source.contains("Text('Home')"));
        assert!(r.ets_source.contains("Text('Detail')"));
    }

    #[test]
    fn navstack_no_state_falls_back_to_first_route() {
        // NavStack(<plain non-state local>, [...]) — first arg isn't
        // registered in state_registry, so emit falls back to rendering
        // the first route only with a developer-facing hint comment.
        let mut m = empty_module();
        m.init.push(Stmt::Let {
            id: 7,
            name: "x".to_string(),
            ty: perry_types::Type::Any,
            mutable: false,
            init: Some(Expr::String("home".into())),
        });
        let routes = Expr::Array(vec![Expr::Object(vec![
            ("name".into(), Expr::String("home".into())),
            (
                "body".into(),
                nmc("Text", vec![Expr::String("Home".into())]),
            ),
        ])]);
        m.init.push(app_with_body(nmc(
            "NavStack",
            vec![Expr::LocalGet(7), routes],
        )));
        let r = emit_index_ets(&mut m).unwrap().unwrap();
        // Hint comment is in the output.
        assert!(
            r.ets_source
                .contains("first arg must be a `state<string>(...)` local"),
            "missing fallback hint:\n{}",
            r.ets_source
        );
        // Body of first route still rendered.
        assert!(r.ets_source.contains("Text('Home')"));
    }

    #[test]
    fn navstack_empty_routes_emits_empty_column_with_comment() {
        let mut m = empty_module();
        m.init.push(Stmt::Let {
            id: 5,
            name: "route".to_string(),
            ty: perry_types::Type::Any,
            mutable: false,
            init: Some(state_call(Expr::String("home".into()))),
        });
        m.init.push(app_with_body(nmc(
            "NavStack",
            vec![Expr::LocalGet(5), Expr::Array(vec![])],
        )));
        let r = emit_index_ets(&mut m).unwrap().unwrap();
        assert!(r.ets_source.contains("// NavStack: empty routes array"));
    }

    #[test]
    fn navstack_set_in_closure_rewrites_to_settext() {
        // const route = state("home");
        // Button("Detail", () => route.set("detail")) — the closure body
        // should rewrite via the existing v6 `state.set(v)` → setText
        // path so navigation actually triggers a re-render.
        let mut m = empty_module();
        m.init.push(Stmt::Let {
            id: 5,
            name: "route".to_string(),
            ty: perry_types::Type::Any,
            mutable: false,
            init: Some(state_call(Expr::String("home".into()))),
        });
        let nav_button = nmc(
            "Button",
            vec![
                Expr::String("Go".into()),
                Expr::Closure {
                    func_id: 0 as perry_types::FuncId,
                    params: vec![],
                    return_type: perry_types::Type::Any,
                    body: vec![Stmt::Expr(state_method_call(
                        5,
                        "set",
                        vec![Expr::String("detail".into())],
                    ))],
                    captures: vec![],
                    mutable_captures: vec![],
                    captures_this: false,
                    enclosing_class: None,
                    is_async: false,
                },
            ],
        );
        let routes = Expr::Array(vec![Expr::Object(vec![
            ("name".into(), Expr::String("home".into())),
            ("body".into(), nav_button),
        ])]);
        m.init.push(app_with_body(nmc(
            "NavStack",
            vec![Expr::LocalGet(5), routes],
        )));
        let r = emit_index_ets(&mut m).unwrap().unwrap();
        // Exactly one callback registered (the Button's onClick).
        assert_eq!(r.callbacks.len(), 1);
        // The closure's body should now be a setText call (rewritten by
        // the v6 pre-walk that also runs for NavStack-nested closures).
        let captured = &r.callbacks[0];
        if let Expr::Closure { body, .. } = captured {
            let has_settext = body.iter().any(|s| {
                matches!(
                    s,
                    Stmt::Expr(Expr::NativeMethodCall {
                        module,
                        method,
                        ..
                    }) if module == "perry/ui" && method == "setText"
                )
            });
            assert!(
                has_settext,
                "expected setText rewrite, got body: {:?}",
                body
            );
        } else {
            panic!("expected Closure callback");
        }
    }

    // ----- Phase 2 v6: state<T> reactive container -----

    fn state_call(initial: Expr) -> Expr {
        Expr::NativeMethodCall {
            module: "perry/ui".to_string(),
            class_name: None,
            object: None,
            method: "state".to_string(),
            args: vec![initial],
        }
    }

    fn state_method_call(state_id: u32, method: &str, args: Vec<Expr>) -> Expr {
        Expr::Call {
            callee: Box::new(Expr::PropertyGet {
                object: Box::new(Expr::LocalGet(state_id)),
                property: method.to_string(),
            }),
            args,
            type_args: vec![],
        }
    }

    #[test]
    fn state_text_emits_reactive_text_with_synth_id() {
        // const count = state(0); App({body: count.text()});
        let mut m = empty_module();
        m.init.push(Stmt::Let {
            id: 5,
            name: "count".to_string(),
            ty: perry_types::Type::Any,
            mutable: false,
            init: Some(state_call(Expr::Number(0.0))),
        });
        m.init
            .push(app_with_body(state_method_call(5, "text", vec![])));
        let r = emit_index_ets(&mut m).unwrap().unwrap();
        // Synth id is __state_0; sanitized to __state_0 (already valid).
        assert!(r.ets_source.contains("Text(this.text___state_0)"));
        // @State decl with initial value 0.
        assert!(r.ets_source.contains("@State text___state_0: string = '0'"));
    }

    #[test]
    fn state_set_in_closure_rewrites_to_settext() {
        // const count = state(0);
        // App({body: Button("+", () => count.set(5))});
        let mut m = empty_module();
        m.init.push(Stmt::Let {
            id: 5,
            name: "count".to_string(),
            ty: perry_types::Type::Any,
            mutable: false,
            init: Some(state_call(Expr::Number(0.0))),
        });
        // Closure body: Stmt::Expr(count.set(5))
        let closure = Expr::Closure {
            func_id: 0 as perry_types::FuncId,
            params: vec![],
            return_type: perry_types::Type::Any,
            body: vec![Stmt::Expr(state_method_call(
                5,
                "set",
                vec![Expr::Number(5.0)],
            ))],
            captures: vec![],
            mutable_captures: vec![],
            captures_this: false,
            enclosing_class: None,
            is_async: false,
        };
        m.init.push(app_with_body(nmc(
            "Button",
            vec![Expr::String("+".into()), closure],
        )));
        let r = emit_index_ets(&mut m).unwrap().unwrap();
        // The closure body should now contain a setText call. Codegen-side
        // we can't directly assert on that — but we can verify the harvest
        // captured exactly 1 callback (the rewritten closure).
        assert_eq!(r.callbacks.len(), 1);
        // And confirm the rewritten HIR has the setText shape inside.
        let captured = &r.callbacks[0];
        if let Expr::Closure { body, .. } = captured {
            let has_settext = body.iter().any(|s| {
                matches!(s, Stmt::Expr(Expr::NativeMethodCall { method, .. }) if method == "setText")
            });
            assert!(
                has_settext,
                "closure body should have been rewritten to setText"
            );
        } else {
            panic!("expected Closure in callback registry");
        }
    }

    #[test]
    fn multiple_state_decls_get_unique_ids() {
        let mut m = empty_module();
        m.init.push(Stmt::Let {
            id: 1,
            name: "count".to_string(),
            ty: perry_types::Type::Any,
            mutable: false,
            init: Some(state_call(Expr::Number(0.0))),
        });
        m.init.push(Stmt::Let {
            id: 2,
            name: "name".to_string(),
            ty: perry_types::Type::Any,
            mutable: false,
            init: Some(state_call(Expr::String("Alice".into()))),
        });
        m.init.push(app_with_body(nmc(
            "VStack",
            vec![Expr::Array(vec![
                state_method_call(1, "text", vec![]),
                state_method_call(2, "text", vec![]),
            ])],
        )));
        let r = emit_index_ets(&mut m).unwrap().unwrap();
        assert!(r.ets_source.contains("@State text___state_0: string = '0'"));
        assert!(r
            .ets_source
            .contains("@State text___state_1: string = 'Alice'"));
        assert!(r.ets_source.contains("Text(this.text___state_0)"));
        assert!(r.ets_source.contains("Text(this.text___state_1)"));
    }

    #[test]
    fn unsupported_widget_degrades_with_comment_not_error() {
        // Use a widget that's intentionally NOT yet supported so this
        // test stays valid as the supported set grows. As of v4 we
        // still don't emit anything for `Canvas` / `Window` / `TabBar`.
        let mut m = empty_module();
        m.init.push(app_with_body(nmc(
            "Canvas",
            vec![Expr::Number(100.0), Expr::Number(100.0)],
        )));
        let r = emit_index_ets(&mut m).unwrap().unwrap();
        assert!(r
            .ets_source
            .contains("// unsupported perry/ui widget: Canvas"));
        assert!(r.ets_source.contains("Text('[unsupported: Canvas]')"));
    }

    #[test]
    fn image_with_src() {
        let mut m = empty_module();
        m.init.push(app_with_body(nmc(
            "Image",
            vec![Expr::String("logo.png".into())],
        )));
        let r = emit_index_ets(&mut m).unwrap().unwrap();
        assert!(r
            .ets_source
            .contains("Image('logo.png').width('100%').height(200)"));
    }

    #[test]
    fn imagefile_alias_emits_same_shape() {
        // ImageFile is the existing perry-ui-* TS surface name; both must
        // route through the same emitter for cross-platform parity.
        let mut m = empty_module();
        m.init.push(app_with_body(nmc(
            "ImageFile",
            vec![Expr::String("photo.jpg".into())],
        )));
        let r = emit_index_ets(&mut m).unwrap().unwrap();
        assert!(r.ets_source.contains("Image('photo.jpg')"));
    }

    #[test]
    fn scrollview_with_children() {
        let mut m = empty_module();
        m.init.push(app_with_body(nmc(
            "ScrollView",
            vec![Expr::Array(vec![
                nmc("Text", vec![Expr::String("a".into())]),
                nmc("Text", vec![Expr::String("b".into())]),
            ])],
        )));
        let r = emit_index_ets(&mut m).unwrap().unwrap();
        assert!(r.ets_source.contains("Scroll() {"));
        assert!(r.ets_source.contains("Column({ space: 8 })"));
        assert!(r.ets_source.contains("Text('a').fontSize(20)"));
        assert!(r.ets_source.contains("Text('b').fontSize(20)"));
    }

    #[test]
    fn lazyvstack_emits_column_with_deferral_comment() {
        let mut m = empty_module();
        m.init.push(app_with_body(nmc(
            "LazyVStack",
            vec![Expr::Array(vec![
                nmc("Text", vec![Expr::String("row 0".into())]),
                nmc("Text", vec![Expr::String("row 1".into())]),
            ])],
        )));
        let r = emit_index_ets(&mut m).unwrap().unwrap();
        // Phase 2 v10: explicit-children variant (non-ArrayMap) still
        // renders eagerly as a plain Column for backwards compat. The
        // real lazy path triggers only on `LazyVStack(items.map(...))`.
        assert!(r
            .ets_source
            .contains("LazyVStack with explicit children: rendered eagerly as Column"));
        assert!(r.ets_source.contains("Column({ space: 8 })"));
        assert!(r.ets_source.contains("Text('row 0')"));
    }

    // ----- Phase 2 v10: real LazyVStack with LazyForEach + IDataSource -----

    #[test]
    fn lazyvstack_with_array_map_emits_lazy_for_each() {
        // LazyVStack(items.map(item => Text(item)))
        let mut m = empty_module();
        let item_param = perry_hir::ir::Param {
            id: 99,
            name: "item".to_string(),
            ty: perry_types::Type::Any,
            default: None,
            is_rest: false,
        };
        let inner_text = nmc("Text", vec![Expr::LocalGet(99)]);
        let map_expr = Expr::ArrayMap {
            array: Box::new(Expr::Array(vec![
                Expr::String("a".into()),
                Expr::String("b".into()),
            ])),
            callback: Box::new(Expr::Closure {
                func_id: 0 as perry_types::FuncId,
                params: vec![item_param],
                return_type: perry_types::Type::Any,
                body: vec![Stmt::Return(Some(inner_text))],
                captures: vec![],
                mutable_captures: vec![],
                captures_this: false,
                enclosing_class: None,
                is_async: false,
            }),
        };
        m.init
            .push(app_with_body(nmc("LazyVStack", vec![map_expr])));
        let r = emit_index_ets(&mut m).unwrap().unwrap();
        // ArkUI shape: List() { LazyForEach(this.lazy_source_0, ...) }
        assert!(r.ets_source.contains("List() {"));
        assert!(r.ets_source.contains("LazyForEach(this.lazy_source_0"));
        assert!(r.ets_source.contains("ListItem()"));
        // Inner widget body resolves item to __item.
        assert!(r.ets_source.contains("Text(__item)"));
        // IDataSource boilerplate emitted at module top.
        assert!(r
            .ets_source
            .contains("class PerryListDataSource implements IDataSource"));
        // @State field decl on the page.
        assert!(r.ets_source.contains(
            "@State lazy_source_0: PerryListDataSource = new PerryListDataSource(['a', 'b'])"
        ));
    }

    #[test]
    fn lazyvstack_no_array_map_skips_lazy_class_emission() {
        // Eager-mode (explicit Array) variant should NOT emit the
        // PerryListDataSource boilerplate.
        let mut m = empty_module();
        m.init.push(app_with_body(nmc(
            "LazyVStack",
            vec![Expr::Array(vec![nmc(
                "Text",
                vec![Expr::String("hi".into())],
            )])],
        )));
        let r = emit_index_ets(&mut m).unwrap().unwrap();
        assert!(!r.ets_source.contains("class PerryListDataSource"));
        assert!(!r.ets_source.contains("LazyForEach"));
    }

    #[test]
    fn picker_with_options_and_closure() {
        let mut m = empty_module();
        m.init.push(app_with_body(nmc(
            "Picker",
            vec![
                Expr::Array(vec![
                    Expr::String("Red".into()),
                    Expr::String("Green".into()),
                    Expr::String("Blue".into()),
                ]),
                closure_stub(),
            ],
        )));
        let r = emit_index_ets(&mut m).unwrap().unwrap();
        assert!(r
            .ets_source
            .contains("TextPicker({ range: ['Red', 'Green', 'Blue'], value: 'Red' })"));
        assert!(r
            .ets_source
            .contains(".onChange((_value: string, index: number) => {"));
        assert!(r
            .ets_source
            .contains("perryEntry.invokeCallback1(0, index)"));
        assert_eq!(r.callbacks.len(), 1);
    }

    #[test]
    fn combobox_emits_arkui_select() {
        // Issue #475 — Combobox(initial, onChange) → Select with onSelect.
        // Asserts the canonical patterns: Select( + .onSelect( + the
        // initial value used as both .value() and the only seed option.
        let mut m = empty_module();
        m.init.push(app_with_body(nmc(
            "Combobox",
            vec![Expr::String("Apple".into()), closure_stub()],
        )));
        let r = emit_index_ets(&mut m).unwrap().unwrap();
        assert!(r.ets_source.contains("Select("));
        assert!(r.ets_source.contains(".value('Apple')"));
        assert!(r.ets_source.contains(".onSelect("));
        assert!(r
            .ets_source
            .contains("perryEntry.invokeCallback1(0, value)"));
        // Drain is wired so showToast / setText inside the closure body
        // surface after onSelect returns.
        assert!(r.ets_source.contains("perryEntry.drainToast()"));
        assert_eq!(r.callbacks.len(), 1);
    }

    #[test]
    fn rich_text_editor_emits_arkui_richeditor() {
        // Issue #478 — RichTextEditor(width, height, onChange) emits
        // an ArkUI RichEditor with a fresh controller; width/height
        // flow through to sizing modifiers; the onChange closure is
        // captured and routed through onIMEInputComplete.
        let mut m = empty_module();
        m.init.push(app_with_body(nmc(
            "RichTextEditor",
            vec![Expr::Number(320.0), Expr::Number(200.0), closure_stub()],
        )));
        let r = emit_index_ets(&mut m).unwrap().unwrap();
        assert!(r.ets_source.contains("RichEditor("));
        assert!(r.ets_source.contains("new RichEditorController()"));
        assert!(r.ets_source.contains(".width(320)"));
        assert!(r.ets_source.contains(".height(200)"));
        assert!(r.ets_source.contains(".onIMEInputComplete("));
        assert!(r.ets_source.contains("perryEntry.invokeCallback1(0, ''"));
        assert_eq!(r.callbacks.len(), 1);
    }

    #[test]
    fn calendar_emits_arkui_calendar_picker() {
        // Issue #481 — Calendar(2026, 5, onChange) → CalendarPicker
        // with selected = new Date(2026, 4, 1) (month is 0-indexed in
        // JS Date) and an onChange that converts the Date payload to
        // an ISO yyyy-MM-dd string before invoking the TS callback.
        let mut m = empty_module();
        m.init.push(app_with_body(nmc(
            "Calendar",
            vec![Expr::Number(2026.0), Expr::Number(5.0), closure_stub()],
        )));
        let r = emit_index_ets(&mut m).unwrap().unwrap();
        assert!(r.ets_source.contains("CalendarPicker("));
        // 1-based month 5 (May) → 0-based monthIndex 4
        assert!(r.ets_source.contains("new Date(2026, 4, 1)"));
        assert!(r.ets_source.contains(".onChange((value: Date) => {"));
        assert!(r.ets_source.contains("value.toISOString().split('T')[0]"));
        assert!(r
            .ets_source
            .contains("perryEntry.invokeCallback1(0, __iso)"));
        assert_eq!(r.callbacks.len(), 1);
    }

    #[test]
    fn calendar_without_literal_args_falls_back_to_today() {
        // Calendar(yearLocal, monthLocal, _) — args don't resolve to
        // numeric literals, so the selected date defaults to `new Date()`.
        let mut m = empty_module();
        m.init.push(app_with_body(nmc(
            "Calendar",
            vec![
                Expr::String("not-a-number".into()),
                Expr::String("nope".into()),
                Expr::Number(0.0),
            ],
        )));
        let r = emit_index_ets(&mut m).unwrap().unwrap();
        assert!(r.ets_source.contains("CalendarPicker("));
        assert!(r.ets_source.contains("selected: new Date()"));
    }

    #[test]
    fn rich_text_editor_zero_size_skips_width_height_modifiers() {
        // 0 width/height means "use intrinsic" — emitting .width(0)
        // would zero the editor. Test confirms the elision.
        let mut m = empty_module();
        m.init.push(app_with_body(nmc(
            "RichTextEditor",
            vec![Expr::Number(0.0), Expr::Number(0.0), Expr::Number(0.0)],
        )));
        let r = emit_index_ets(&mut m).unwrap().unwrap();
        assert!(r.ets_source.contains("RichEditor("));
        assert!(!r.ets_source.contains(".width(0)"));
        assert!(!r.ets_source.contains(".height(0)"));
    }

    #[test]
    fn progressview_with_default_value_and_total() {
        let mut m = empty_module();
        m.init.push(app_with_body(nmc("ProgressView", vec![])));
        let r = emit_index_ets(&mut m).unwrap().unwrap();
        assert!(r
            .ets_source
            .contains("Progress({ value: 0, total: 100, type: ProgressType.Linear })"));
    }

    #[test]
    fn progressview_with_explicit_value() {
        let mut m = empty_module();
        m.init.push(app_with_body(nmc(
            "ProgressView",
            vec![Expr::Number(42.0), Expr::Number(200.0)],
        )));
        let r = emit_index_ets(&mut m).unwrap().unwrap();
        assert!(r
            .ets_source
            .contains("Progress({ value: 42, total: 200, type: ProgressType.Linear })"));
    }

    #[test]
    fn section_with_title_and_children() {
        let mut m = empty_module();
        m.init.push(app_with_body(nmc(
            "Section",
            vec![
                Expr::String("Personal Info".into()),
                Expr::Array(vec![
                    nmc("Text", vec![Expr::String("name".into())]),
                    nmc("Text", vec![Expr::String("email".into())]),
                ]),
            ],
        )));
        let r = emit_index_ets(&mut m).unwrap().unwrap();
        assert!(r.ets_source.contains("Column({ space: 4 })"));
        assert!(r
            .ets_source
            .contains("Text('Personal Info').fontSize(14).fontColor('#888888')"));
        assert!(r.ets_source.contains("Text('name').fontSize(20)"));
        assert!(r.ets_source.contains("Text('email').fontSize(20)"));
    }

    #[test]
    fn string_literal_escaping() {
        assert_eq!(arkts_string_lit("hi"), "'hi'");
        assert_eq!(arkts_string_lit("he's there"), "'he\\'s there'");
        assert_eq!(arkts_string_lit("a\\b"), "'a\\\\b'");
        assert_eq!(arkts_string_lit("line1\nline2"), "'line1\\nline2'");
    }

    #[test]
    fn fmt_num_drops_decimal_for_whole_numbers() {
        assert_eq!(fmt_num(8.0), "8");
        assert_eq!(fmt_num(16.0), "16");
        assert_eq!(fmt_num(1.5), "1.5");
        assert_eq!(fmt_num(-3.0), "-3");
    }

    // ─── #369 perry/media drain glue ────────────────────────────────

    fn media_call(method: &str, args: Vec<Expr>) -> Expr {
        Expr::NativeMethodCall {
            module: "perry/media".to_string(),
            class_name: None,
            object: None,
            method: method.to_string(),
            args,
        }
    }

    #[test]
    fn no_media_use_omits_media_glue() {
        let mut m = empty_module();
        m.init
            .push(app_with_body(nmc("Text", vec![Expr::String("hi".into())])));
        let r = emit_index_ets(&mut m).unwrap().unwrap();
        assert!(!r.ets_source.contains("@ohos.multimedia.media"));
        assert!(!r.ets_source.contains("mediaPlayers"));
        assert!(!r.ets_source.contains("runMediaPump"));
    }

    #[test]
    fn createplayer_in_init_emits_media_glue() {
        // `createPlayer(url)` is a top-level call (not inside App body),
        // typical media-app shape: `const p = createPlayer(url); App({body: ...})`.
        let mut m = empty_module();
        m.init.push(Stmt::Expr(media_call(
            "createPlayer",
            vec![Expr::String("https://e.x/a.mp3".into())],
        )));
        m.init
            .push(app_with_body(nmc("Text", vec![Expr::String("hi".into())])));
        let r = emit_index_ets(&mut m).unwrap().unwrap();
        // Imports.
        assert!(r
            .ets_source
            .contains("import media from '@ohos.multimedia.media'"));
        // Per-instance state.
        assert!(r
            .ets_source
            .contains("private mediaPlayers: Map<number, media.AVPlayer>"));
        // Lifecycle pump.
        assert!(r.ets_source.contains("aboutToAppear()"));
        assert!(r
            .ets_source
            .contains("setInterval(() => { this.runMediaPump(); }, 100)"));
        // Three drain loops.
        assert!(r.ets_source.contains("perryEntry.drainMediaCreate()"));
        assert!(r.ets_source.contains("perryEntry.drainMediaControl()"));
        assert!(r.ets_source.contains("perryEntry.drainNowPlaying()"));
        // State pushback.
        assert!(r.ets_source.contains("perryEntry.pushMediaState"));
        // AVPlayer dispatch.
        assert!(r.ets_source.contains("media.createAVPlayer()"));
        assert!(r.ets_source.contains("player.play()"));
        assert!(r.ets_source.contains("player.pause()"));
        assert!(r.ets_source.contains("player.seek("));
        assert!(r.ets_source.contains("player.setVolume("));
        assert!(r.ets_source.contains("player.release()"));
    }

    #[test]
    fn media_call_inside_button_closure_also_triggers_glue() {
        // Critical for play/pause buttons: the perry/media calls live
        // inside Button's onClick closure, not in module.init. The
        // walker must descend into Closure bodies via stmt_uses → Closure.
        let mut m = empty_module();
        let play_closure = Expr::Closure {
            func_id: 0 as perry_types::FuncId,
            params: vec![],
            return_type: perry_types::Type::Any,
            body: vec![Stmt::Expr(media_call("play", vec![Expr::Number(1.0)]))],
            captures: vec![],
            mutable_captures: vec![],
            captures_this: false,
            enclosing_class: None,
            is_async: false,
        };
        m.init.push(app_with_body(nmc(
            "Button",
            vec![Expr::String("Play".into()), play_closure],
        )));
        let r = emit_index_ets(&mut m).unwrap().unwrap();
        assert!(r
            .ets_source
            .contains("import media from '@ohos.multimedia.media'"));
        assert!(r.ets_source.contains("runMediaPump"));
    }

    // ─── #408 procedural mutation tracking ─────────────────────────────

    /// Helper: Let-bind a widget to a LocalId so mutator calls can target it.
    fn let_widget(id: LocalId, name: &str, init: Expr) -> Stmt {
        Stmt::Let {
            id,
            name: name.to_string(),
            ty: perry_types::Type::Any,
            mutable: false,
            init: Some(init),
        }
    }

    /// Helper: a perry/ui mutator call expression, e.g. widgetAddChild(parent, child).
    fn mutator_stmt(method: &str, args: Vec<Expr>) -> Stmt {
        Stmt::Expr(Expr::NativeMethodCall {
            module: "perry/ui".to_string(),
            class_name: None,
            object: None,
            method: method.to_string(),
            args,
        })
    }

    #[test]
    fn issue_408_hstack_with_widget_add_child_appends_children() {
        // const toolbar = HStack(0, []);
        // widgetAddChild(toolbar, button1);
        // widgetAddChild(toolbar, button2);
        // App({body: toolbar});
        let mut m = empty_module();
        let toolbar_id: LocalId = 10;
        let btn_a_id: LocalId = 11;
        let btn_b_id: LocalId = 12;
        m.init.push(let_widget(
            toolbar_id,
            "toolbar",
            nmc("HStack", vec![Expr::Number(0.0), Expr::Array(vec![])]),
        ));
        m.init.push(let_widget(
            btn_a_id,
            "btn_a",
            nmc("Button", vec![Expr::String("A".into())]),
        ));
        m.init.push(let_widget(
            btn_b_id,
            "btn_b",
            nmc("Button", vec![Expr::String("B".into())]),
        ));
        m.init.push(mutator_stmt(
            "widgetAddChild",
            vec![Expr::LocalGet(toolbar_id), Expr::LocalGet(btn_a_id)],
        ));
        m.init.push(mutator_stmt(
            "widgetAddChild",
            vec![Expr::LocalGet(toolbar_id), Expr::LocalGet(btn_b_id)],
        ));
        m.init.push(app_with_body(Expr::LocalGet(toolbar_id)));
        let r = emit_index_ets(&mut m).unwrap().unwrap();
        assert!(
            r.ets_source.contains("Row({ space: 0 })"),
            "expected Row container:\n{}",
            r.ets_source
        );
        // Both children must appear inside the body. They show up after
        // the explicit empty array's children (none) so they're the only
        // contents of Row.
        assert!(
            r.ets_source.contains("Button('A')"),
            "missing Button A:\n{}",
            r.ets_source
        );
        assert!(
            r.ets_source.contains("Button('B')"),
            "missing Button B:\n{}",
            r.ets_source
        );
        // Order: A appears before B in the source.
        let pos_a = r.ets_source.find("Button('A')").unwrap();
        let pos_b = r.ets_source.find("Button('B')").unwrap();
        assert!(pos_a < pos_b, "child order swapped:\n{}", r.ets_source);
    }

    #[test]
    fn issue_408_scrollview_set_child_replaces_body() {
        // const screen = ScrollView();
        // const content = VStack([Text("hello")]);
        // scrollviewSetChild(screen, content);
        // App({body: screen});
        let mut m = empty_module();
        let screen_id: LocalId = 20;
        let content_id: LocalId = 21;
        m.init
            .push(let_widget(screen_id, "screen", nmc("ScrollView", vec![])));
        m.init.push(let_widget(
            content_id,
            "content",
            nmc(
                "VStack",
                vec![Expr::Array(vec![nmc(
                    "Text",
                    vec![Expr::String("hello".into())],
                )])],
            ),
        ));
        m.init.push(mutator_stmt(
            "scrollviewSetChild",
            vec![Expr::LocalGet(screen_id), Expr::LocalGet(content_id)],
        ));
        m.init.push(app_with_body(Expr::LocalGet(screen_id)));
        let r = emit_index_ets(&mut m).unwrap().unwrap();
        assert!(
            r.ets_source.contains("Scroll() {"),
            "expected Scroll wrapper:\n{}",
            r.ets_source
        );
        // Child content is rendered inside the inner Column.
        assert!(
            r.ets_source.contains("Text('hello')"),
            "missing scroll child content:\n{}",
            r.ets_source
        );
    }

    #[test]
    fn issue_408_set_padding_emits_modifier_chain() {
        // const card = VStack([]);
        // setPadding(card, 8, 12, 8, 12);
        // setCornerRadius(card, 16);
        // widgetSetBackgroundColor(card, 0.2, 0.5, 0.95, 1);
        // App({body: card});
        let mut m = empty_module();
        let card_id: LocalId = 30;
        m.init.push(let_widget(
            card_id,
            "card",
            nmc("VStack", vec![Expr::Array(vec![])]),
        ));
        m.init.push(mutator_stmt(
            "setPadding",
            vec![
                Expr::LocalGet(card_id),
                Expr::Number(8.0),
                Expr::Number(12.0),
                Expr::Number(8.0),
                Expr::Number(12.0),
            ],
        ));
        m.init.push(mutator_stmt(
            "setCornerRadius",
            vec![Expr::LocalGet(card_id), Expr::Number(16.0)],
        ));
        m.init.push(mutator_stmt(
            "widgetSetBackgroundColor",
            vec![
                Expr::LocalGet(card_id),
                Expr::Number(0.2),
                Expr::Number(0.5),
                Expr::Number(0.95),
                Expr::Number(1.0),
            ],
        ));
        m.init.push(app_with_body(Expr::LocalGet(card_id)));
        let r = emit_index_ets(&mut m).unwrap().unwrap();
        assert!(
            r.ets_source
                .contains(".padding({ top: 8, right: 12, bottom: 8, left: 12 })"),
            "expected padding modifier:\n{}",
            r.ets_source
        );
        assert!(
            r.ets_source.contains(".borderRadius(16)"),
            "expected borderRadius:\n{}",
            r.ets_source
        );
        // 0.2*255=51, 0.5*255≈128, 0.95*255≈242
        assert!(
            r.ets_source
                .contains(".backgroundColor('rgba(51, 128, 242, 1)')"),
            "expected rgba background:\n{}",
            r.ets_source
        );
    }

    #[test]
    fn issue_479_widget_set_rich_tooltip_emits_bind_popup_modifier() {
        // const btn = Button("Save");
        // const tip = Text("Press to save now");
        // widgetSetRichTooltip(btn, tip, 500);
        // App({body: btn});
        //
        // Asserts the tooltip lowers to ArkUI's `.bindPopup(false, {
        // message: '...' })` modifier chained off the trigger widget.
        // The hover delay is documented but not honored — ArkUI's
        // popup show-trigger is implicit (long-press / click).
        let mut m = empty_module();
        let btn_id: LocalId = 100;
        let tip_id: LocalId = 101;
        m.init.push(let_widget(
            btn_id,
            "btn",
            nmc("Button", vec![Expr::String("Save".into())]),
        ));
        m.init.push(let_widget(
            tip_id,
            "tip",
            nmc("Text", vec![Expr::String("Press to save now".into())]),
        ));
        m.init.push(mutator_stmt(
            "widgetSetRichTooltip",
            vec![
                Expr::LocalGet(btn_id),
                Expr::LocalGet(tip_id),
                Expr::Number(500.0),
            ],
        ));
        m.init.push(app_with_body(Expr::LocalGet(btn_id)));
        let r = emit_index_ets(&mut m).unwrap().unwrap();
        assert!(
            r.ets_source
                .contains(".bindPopup(false, { message: 'Press to save now' })"),
            "expected bindPopup modifier:\n{}",
            r.ets_source
        );
    }

    #[test]
    fn issue_479_widget_set_rich_tooltip_with_inline_text_content() {
        // Same as above but the content widget is constructed inline,
        // without an intervening LocalGet binding — exercises the
        // direct-call branch of resolve_tooltip_text.
        let mut m = empty_module();
        let btn_id: LocalId = 110;
        m.init.push(let_widget(
            btn_id,
            "btn",
            nmc("Button", vec![Expr::String("Save".into())]),
        ));
        m.init.push(mutator_stmt(
            "widgetSetRichTooltip",
            vec![
                Expr::LocalGet(btn_id),
                nmc("Text", vec![Expr::String("inline tip".into())]),
                Expr::Number(0.0),
            ],
        ));
        m.init.push(app_with_body(Expr::LocalGet(btn_id)));
        let r = emit_index_ets(&mut m).unwrap().unwrap();
        assert!(
            r.ets_source
                .contains(".bindPopup(false, { message: 'inline tip' })"),
            "expected bindPopup modifier:\n{}",
            r.ets_source
        );
    }

    #[test]
    fn issue_408_conditional_widget_add_child_emits_if_else() {
        // const screen = VStack([]);
        // const btn_phone = Button("phone");
        // const btn_desktop = Button("desktop");
        // if (props.isMobile) { widgetAddChild(screen, btn_phone); }
        // else { widgetAddChild(screen, btn_desktop); }
        // App({body: screen});
        //
        // The condition uses a PropertyGet, which can't be statically
        // folded by the #413 evaluator (only literal-leaf expressions
        // fold). The harvest emits a real `if (...) { ... } else { ... }`
        // block in the ArkTS source.
        let mut m = empty_module();
        let screen_id: LocalId = 40;
        let phone_id: LocalId = 41;
        let desktop_id: LocalId = 42;
        m.init.push(let_widget(
            screen_id,
            "screen",
            nmc("VStack", vec![Expr::Array(vec![])]),
        ));
        m.init.push(let_widget(
            phone_id,
            "btn_phone",
            nmc("Button", vec![Expr::String("phone".into())]),
        ));
        m.init.push(let_widget(
            desktop_id,
            "btn_desktop",
            nmc("Button", vec![Expr::String("desktop".into())]),
        ));
        // v0.5.490: dead-branch elim now fires when the condition isn't
        // cleanly serializable. The original PropertyGet(LocalGet(9999),
        // "isMobile") shape would have rendered both branches under
        // `if (true) { ... } else { ... }` — but the else-branch is
        // dead source-wise and Mango exposed this as the "+ New
        // Connection" duplicate-content bug. New behavior: walk only
        // the then-branch when the condition can't be serialized
        // (matches the then-branch heuristic from v0.5.487's
        // Expr::Conditional emit_widget arm).
        m.init.push(Stmt::If {
            condition: Expr::PropertyGet {
                object: Box::new(Expr::LocalGet(9999)),
                property: "isMobile".to_string(),
            },
            then_branch: vec![mutator_stmt(
                "widgetAddChild",
                vec![Expr::LocalGet(screen_id), Expr::LocalGet(phone_id)],
            )],
            else_branch: Some(vec![mutator_stmt(
                "widgetAddChild",
                vec![Expr::LocalGet(screen_id), Expr::LocalGet(desktop_id)],
            )]),
        });
        m.init.push(app_with_body(Expr::LocalGet(screen_id)));
        let r = emit_index_ets(&mut m).unwrap().unwrap();
        // Then-branch is the only one emitted (heuristic-pick).
        assert!(
            r.ets_source.contains("Button('phone')"),
            "expected then-branch (`Button('phone')`) emitted:\n{}",
            r.ets_source
        );
        // Else-branch is dropped — no `Button('desktop')`.
        assert!(
            !r.ets_source.contains("Button('desktop')"),
            "else-branch must be dropped (cleanly-serializable gate fired):\n{}",
            r.ets_source
        );
    }

    #[test]
    fn issue_408_widget_clear_children_drops_earlier_addchild() {
        // const stack = HStack(0, []);
        // widgetAddChild(stack, btn_a);
        // widgetClearChildren(stack);
        // widgetAddChild(stack, btn_b);
        // App({body: stack}); — only btn_b should render.
        let mut m = empty_module();
        let stack_id: LocalId = 50;
        let a_id: LocalId = 51;
        let b_id: LocalId = 52;
        m.init.push(let_widget(
            stack_id,
            "stack",
            nmc("HStack", vec![Expr::Number(0.0), Expr::Array(vec![])]),
        ));
        m.init.push(let_widget(
            a_id,
            "btn_a",
            nmc("Button", vec![Expr::String("dropped".into())]),
        ));
        m.init.push(let_widget(
            b_id,
            "btn_b",
            nmc("Button", vec![Expr::String("kept".into())]),
        ));
        m.init.push(mutator_stmt(
            "widgetAddChild",
            vec![Expr::LocalGet(stack_id), Expr::LocalGet(a_id)],
        ));
        m.init.push(mutator_stmt(
            "widgetClearChildren",
            vec![Expr::LocalGet(stack_id)],
        ));
        m.init.push(mutator_stmt(
            "widgetAddChild",
            vec![Expr::LocalGet(stack_id), Expr::LocalGet(b_id)],
        ));
        m.init.push(app_with_body(Expr::LocalGet(stack_id)));
        let r = emit_index_ets(&mut m).unwrap().unwrap();
        assert!(
            !r.ets_source.contains("Button('dropped')"),
            "Button('dropped') should have been cleared:\n{}",
            r.ets_source
        );
        assert!(
            r.ets_source.contains("Button('kept')"),
            "Button('kept') should remain:\n{}",
            r.ets_source
        );
    }

    #[test]
    fn issue_408_untraceable_parent_falls_back_without_crashing() {
        // widgetAddChild(<some unbound expression>, btn) — parent isn't
        // a LocalGet, so the mutation is dropped silently. The page still
        // emits cleanly.
        let mut m = empty_module();
        let stack_id: LocalId = 60;
        m.init.push(let_widget(
            stack_id,
            "stack",
            nmc("VStack", vec![Expr::Array(vec![])]),
        ));
        m.init.push(mutator_stmt(
            "widgetAddChild",
            vec![
                // First arg is NOT a LocalGet — typical "transient widget"
                // shape that the harvest can't statically trace. Should
                // not crash; should be silently skipped.
                nmc("Button", vec![Expr::String("orphan".into())]),
                nmc("Button", vec![Expr::String("child".into())]),
            ],
        ));
        m.init.push(app_with_body(Expr::LocalGet(stack_id)));
        let r = emit_index_ets(&mut m).unwrap().unwrap();
        // Stack still renders; mutation silently skipped.
        assert!(
            r.ets_source.contains("Column({ space: 8 })"),
            "stack still renders:\n{}",
            r.ets_source
        );
        // The orphan child shouldn't appear since the mutation didn't
        // resolve to a known parent.
        assert!(
            !r.ets_source.contains("Button('child')"),
            "untraceable child should not have been added:\n{}",
            r.ets_source
        );
    }

    #[test]
    fn issue_408_widget_set_hidden_emits_visibility_modifier() {
        let mut m = empty_module();
        let id: LocalId = 70;
        m.init.push(let_widget(
            id,
            "w",
            nmc("VStack", vec![Expr::Array(vec![])]),
        ));
        m.init.push(mutator_stmt(
            "widgetSetHidden",
            vec![Expr::LocalGet(id), Expr::Number(1.0)],
        ));
        m.init.push(app_with_body(Expr::LocalGet(id)));
        let r = emit_index_ets(&mut m).unwrap().unwrap();
        assert!(
            r.ets_source.contains(".visibility(Visibility.Hidden)"),
            "missing hidden modifier:\n{}",
            r.ets_source
        );
    }

    /// Phase 2 v3.5 — `widgetSetHidden` from a Button onClick closure
    /// triggers a `@State hidden_<id>` binding + `.visibility(...)` bound
    /// modifier. Mango's "+ New Connection" tap pattern.
    #[test]
    fn phase2_v35_widget_set_hidden_in_closure_emits_state_binding() {
        let mut m = empty_module();
        let target_id: LocalId = 100;
        // const formContainer = VStack(0, []);
        m.init.push(let_widget(
            target_id,
            "formContainer",
            nmc("VStack", vec![Expr::Number(0.0), Expr::Array(vec![])]),
        ));
        // widgetSetHidden(formContainer, 1);  // module-init initial = hidden
        m.init.push(mutator_stmt(
            "widgetSetHidden",
            vec![Expr::LocalGet(target_id), Expr::Number(1.0)],
        ));
        // App({body: VStack(0, [Button("Open", () => widgetSetHidden(formContainer, 0)),
        //                       formContainer])})
        let body_id: LocalId = 101;
        let onclick = Expr::Closure {
            func_id: 0,
            params: vec![],
            return_type: perry_types::Type::Any,
            body: vec![mutator_stmt(
                "widgetSetHidden",
                vec![Expr::LocalGet(target_id), Expr::Number(0.0)],
            )],
            captures: vec![],
            mutable_captures: vec![],
            captures_this: false,
            enclosing_class: None,
            is_async: false,
        };
        m.init.push(let_widget(
            body_id,
            "rootBody",
            nmc(
                "VStack",
                vec![
                    Expr::Number(0.0),
                    Expr::Array(vec![
                        nmc("Button", vec![Expr::String("Open".to_string()), onclick]),
                        Expr::LocalGet(target_id),
                    ]),
                ],
            ),
        ));
        m.init.push(app_with_body(Expr::LocalGet(body_id)));
        let r = emit_index_ets(&mut m).unwrap().unwrap();
        // @State decl emitted with module-init initial value (hidden=true).
        assert!(
            r.ets_source
                .contains("@State hidden_vis_0: boolean = true;"),
            "missing @State hidden_vis_0 decl:\n{}",
            r.ets_source
        );
        // applyVisibilityUpdate switch arm.
        assert!(
            r.ets_source
                .contains("case 'vis_0': this.hidden_vis_0 = hidden; break;"),
            "missing applyVisibilityUpdate arm for vis_0:\n{}",
            r.ets_source
        );
        // Bound modifier on the widget itself.
        assert!(
            r.ets_source.contains(
                ".visibility(this.hidden_vis_0 ? Visibility.Hidden : Visibility.Visible)"
            ),
            "missing bound .visibility modifier:\n{}",
            r.ets_source
        );
        // No static .visibility(Visibility.Hidden) — that path is replaced
        // by the binding when binding is in effect.
        assert!(
            !r.ets_source.contains(".visibility(Visibility.Hidden)"),
            "static visibility modifier should be replaced by binding:\n{}",
            r.ets_source
        );
        // Drain pump for the visibility queue lives in the onClick body.
        assert!(
            r.ets_source.contains("perryEntry.drainVisibilityUpdate"),
            "missing drainVisibilityUpdate in onClick:\n{}",
            r.ets_source
        );
        // Closure-time call rewritten to setVisibility.
        // (Indirectly verified by its absence as a static `widgetSetHidden`
        // call inside the closure body in the harvested HIR — the rewrite
        // happened in-place. We check the registered closure has had its
        // body modified by inspecting the harvest result's callbacks.)
        assert_eq!(r.callbacks.len(), 1, "expected one harvested closure");
        let cb = &r.callbacks[0];
        if let Expr::Closure { body, .. } = cb {
            // The rewritten closure body should contain a setVisibility
            // NativeMethodCall on perry/arkts (not the original
            // widgetSetHidden on perry/ui).
            let stmt0 = &body[0];
            if let Stmt::Expr(Expr::NativeMethodCall { module, method, .. }) = stmt0 {
                assert_eq!(module, "perry/arkts", "module not rewritten:\n{:?}", stmt0);
                assert_eq!(
                    method, "setVisibility",
                    "method not rewritten:\n{:?}",
                    stmt0
                );
            } else {
                panic!("closure body[0] not a NativeMethodCall: {:?}", stmt0);
            }
        } else {
            panic!("callback[0] not a Closure: {:?}", cb);
        }
    }

    #[test]
    fn issue_408_match_parent_size_emits_100pct_modifiers() {
        let mut m = empty_module();
        let id: LocalId = 80;
        m.init.push(let_widget(
            id,
            "w",
            nmc("VStack", vec![Expr::Array(vec![])]),
        ));
        m.init.push(mutator_stmt(
            "widgetMatchParentWidth",
            vec![Expr::LocalGet(id)],
        ));
        m.init.push(mutator_stmt(
            "widgetMatchParentHeight",
            vec![Expr::LocalGet(id)],
        ));
        m.init.push(app_with_body(Expr::LocalGet(id)));
        let r = emit_index_ets(&mut m).unwrap().unwrap();
        assert!(
            r.ets_source.contains(".width('100%')"),
            "missing width 100%:\n{}",
            r.ets_source
        );
        assert!(
            r.ets_source.contains(".height('100%')"),
            "missing height 100%:\n{}",
            r.ets_source
        );
    }

    #[test]
    fn issue_408_stack_distribution_and_alignment_emit_flexalign_modifiers() {
        // Uses HStack, so post-#413 the alignment enum is VerticalAlign
        // (Row's cross-axis is vertical). Pre-#413 this test asserted
        // HorizontalAlign.Center — which ArkTS strict-mode rejected at
        // assembleHap with "type 'HorizontalAlign' not assignable to
        // 'VerticalAlign'".
        let mut m = empty_module();
        let id: LocalId = 90;
        m.init.push(let_widget(
            id,
            "w",
            nmc("HStack", vec![Expr::Number(0.0), Expr::Array(vec![])]),
        ));
        m.init.push(mutator_stmt(
            "stackSetDistribution",
            vec![Expr::LocalGet(id), Expr::Number(3.0)], // SpaceBetween
        ));
        m.init.push(mutator_stmt(
            "stackSetAlignment",
            vec![Expr::LocalGet(id), Expr::Number(1.0)], // Center
        ));
        m.init.push(app_with_body(Expr::LocalGet(id)));
        let r = emit_index_ets(&mut m).unwrap().unwrap();
        assert!(
            r.ets_source
                .contains(".justifyContent(FlexAlign.SpaceBetween)"),
            "missing distribution modifier:\n{}",
            r.ets_source
        );
        assert!(
            r.ets_source.contains(".alignItems(VerticalAlign.Center)"),
            "missing alignment modifier (HStack should pick VerticalAlign):\n{}",
            r.ets_source
        );
        // Negative-pin: must NOT emit HorizontalAlign for HStack.
        assert!(
            !r.ets_source.contains("HorizontalAlign"),
            "HStack must not emit HorizontalAlign:\n{}",
            r.ets_source
        );
    }

    #[test]
    fn text_styling_mutators_emit_arkui_modifiers() {
        // #408 follow-up — `textSetFontSize` / `textSetColor` /
        // `textSetFontWeight` / `textSetFontFamily` had been falling
        // through to the unrecognized-mutator path, producing
        // `// not yet handled` comments instead of real ArkUI modifiers.
        // Mango uses these heavily for branded title styling — without
        // them the toolbar shows up as plain default-styled text.
        let mut m = empty_module();
        let id: LocalId = 50;
        m.init.push(let_widget(
            id,
            "title",
            nmc("Text", vec![Expr::String("Mango".into())]),
        ));
        m.init.push(mutator_stmt(
            "textSetFontSize",
            vec![Expr::LocalGet(id), Expr::Number(28.0)],
        ));
        m.init.push(mutator_stmt(
            "textSetFontWeight",
            // (widget, size, weight_scale) — matches Apple's
            // systemFont(ofSize: weight:) signature. weight_scale 0..1
            // maps to ArkUI's 100..900 (rounded to nearest 100). 1.0
            // → 900 (Bold-equivalent).
            vec![Expr::LocalGet(id), Expr::Number(28.0), Expr::Number(1.0)],
        ));
        m.init.push(mutator_stmt(
            "textSetFontFamily",
            vec![Expr::LocalGet(id), Expr::String("Inter".into())],
        ));
        m.init.push(mutator_stmt(
            "textSetColor",
            vec![
                Expr::LocalGet(id),
                Expr::Number(0.5),
                Expr::Number(0.25),
                Expr::Number(0.0),
                Expr::Number(1.0),
            ],
        ));
        m.init.push(app_with_body(Expr::LocalGet(id)));
        let r = emit_index_ets(&mut m).unwrap().unwrap();
        for must in [
            ".fontSize(28)",
            ".fontWeight(900)",
            ".fontFamily('Inter')",
            ".fontColor('rgba(128, 64, 0, 1)')",
        ] {
            assert!(
                r.ets_source.contains(must),
                "missing {must} in:\n{}",
                r.ets_source
            );
        }
        // Negative-pin: must NOT be in the unrecognized-mutator branch.
        assert!(
            !r.ets_source.contains("textSetFontSize` not yet handled"),
            "textSetFontSize should be handled, not flagged:\n{}",
            r.ets_source
        );
    }

    #[test]
    fn unrecognized_mutator_comment_does_not_swallow_following_modifier() {
        // #408 follow-up — `Mutation::Comment` previously emitted as
        // `\n// X`, which is a line comment runs to EOL. Modifier
        // mutations chain on the same physical line in the emitted
        // ArkTS (e.g. `}.padding(...).visibility(...)`); a `\n// X`
        // splice between two modifiers caused the second modifier to
        // be eaten by the comment:
        //   `}.padding(...)\n// X.visibility(...)`
        // ArkTS parses `// X.visibility(...)` as one comment line and
        // the `.visibility` modifier silently disappears. Fix: emit
        // unrecognized-mutator diagnostics as inline `/* X */` block
        // comments instead.
        let mut m = empty_module();
        let id: LocalId = 60;
        m.init.push(let_widget(
            id,
            "label",
            nmc("Text", vec![Expr::String("hi".into())]),
        ));
        // Sandwich an unrecognized mutator between two recognized ones
        // so we exercise the "comment between modifiers" shape.
        m.init.push(mutator_stmt(
            "textSetFontSize",
            vec![Expr::LocalGet(id), Expr::Number(20.0)],
        ));
        m.init.push(mutator_stmt(
            "totallyMadeUpMutator",
            vec![Expr::LocalGet(id), Expr::Number(99.0)],
        ));
        m.init.push(mutator_stmt(
            "widgetSetHidden",
            vec![Expr::LocalGet(id), Expr::Number(1.0)],
        ));
        m.init.push(app_with_body(Expr::LocalGet(id)));
        let r = emit_index_ets(&mut m).unwrap().unwrap();
        // Both modifiers AROUND the unrecognized one must be present
        // and not swallowed.
        assert!(
            r.ets_source.contains(".fontSize(20)"),
            "fontSize should be present:\n{}",
            r.ets_source
        );
        assert!(
            r.ets_source.contains(".visibility(Visibility.Hidden)"),
            "visibility should be present after the comment:\n{}",
            r.ets_source
        );
        // The comment itself must use inline block-comment shape.
        assert!(
            r.ets_source
                .contains("/* perry/ui mutator `totallyMadeUpMutator`"),
            "comment should be inline /* */, not //:\n{}",
            r.ets_source
        );
        // Negative-pin: no `\n// ` patterns in the modifier section
        // (which would re-introduce the swallow bug).
        assert!(
            !r.ets_source.contains("\n// perry/ui mutator"),
            "comments must not be line comments anymore:\n{}",
            r.ets_source
        );
    }

    #[test]
    fn stack_alignment_value_names_match_axis_enum() {
        // #413 follow-up — `VerticalAlign` doesn't have `Start`/`End`
        // (those exist only on `HorizontalAlign`). It uses `Top`/`Bottom`.
        // Picking `VerticalAlign.Start` produces an ArkTS strict-mode
        // error: "Property 'Start' does not exist on type 'typeof
        // VerticalAlign'". Mango hit this on the browserContent HStack
        // with stackSetAlignment(0) (= start semantics).
        //
        // Same semantic input value (0=start, 1=center, 2=end) must map
        // to axis-correct value-names — Top/Bottom for VerticalAlign,
        // Start/End for HorizontalAlign.
        for (ctor, n_in, expected_modifier) in [
            ("HStack", 0.0, ".alignItems(VerticalAlign.Top)"),
            ("HStack", 1.0, ".alignItems(VerticalAlign.Center)"),
            ("HStack", 2.0, ".alignItems(VerticalAlign.Bottom)"),
            ("VStack", 0.0, ".alignItems(HorizontalAlign.Start)"),
            ("VStack", 1.0, ".alignItems(HorizontalAlign.Center)"),
            ("VStack", 2.0, ".alignItems(HorizontalAlign.End)"),
        ] {
            let mut m = empty_module();
            let id: LocalId = 90;
            m.init.push(let_widget(
                id,
                "w",
                nmc(ctor, vec![Expr::Number(0.0), Expr::Array(vec![])]),
            ));
            m.init.push(mutator_stmt(
                "stackSetAlignment",
                vec![Expr::LocalGet(id), Expr::Number(n_in)],
            ));
            m.init.push(app_with_body(Expr::LocalGet(id)));
            let r = emit_index_ets(&mut m).unwrap().unwrap();
            assert!(
                r.ets_source.contains(expected_modifier),
                "{ctor} stackSetAlignment({n_in}) should emit '{expected_modifier}':\n{src}",
                src = r.ets_source
            );
        }
    }

    #[test]
    fn issue_408_mango_three_screen_shape_renders_all_screens() {
        // Composite test mirroring the Mango shape from #408 — three
        // top-level screens built procedurally with widgetAddChild +
        // styling mutators, all wrapped in a single VStack.
        let mut m = empty_module();
        let root_id: LocalId = 100;
        let conn_id: LocalId = 101;
        let browser_id: LocalId = 102;
        let info_id: LocalId = 103;
        let conn_btn: LocalId = 110;
        let browser_btn: LocalId = 111;
        let info_btn: LocalId = 112;
        m.init.push(let_widget(
            root_id,
            "root",
            nmc("VStack", vec![Expr::Array(vec![])]),
        ));
        // Three screen containers
        m.init.push(let_widget(
            conn_id,
            "connectionScreen",
            nmc("VStack", vec![Expr::Array(vec![])]),
        ));
        m.init.push(let_widget(
            browser_id,
            "browserScreen",
            nmc("ScrollView", vec![]),
        ));
        m.init.push(let_widget(
            info_id,
            "infoScreen",
            nmc("HStack", vec![Expr::Number(8.0), Expr::Array(vec![])]),
        ));
        // Widget-level child buttons
        m.init.push(let_widget(
            conn_btn,
            "conn_btn",
            nmc("Button", vec![Expr::String("Connect".into())]),
        ));
        m.init.push(let_widget(
            browser_btn,
            "browser_btn",
            nmc("Button", vec![Expr::String("Browse".into())]),
        ));
        m.init.push(let_widget(
            info_btn,
            "info_btn",
            nmc("Button", vec![Expr::String("Info".into())]),
        ));
        // widgetAddChild calls — connection screen gets a button
        m.init.push(mutator_stmt(
            "widgetAddChild",
            vec![Expr::LocalGet(conn_id), Expr::LocalGet(conn_btn)],
        ));
        // browserScreen uses scrollviewSetChild + a wrapper VStack
        let browser_content_id: LocalId = 120;
        m.init.push(let_widget(
            browser_content_id,
            "browser_content",
            nmc(
                "VStack",
                vec![Expr::Array(vec![Expr::LocalGet(browser_btn)])],
            ),
        ));
        m.init.push(mutator_stmt(
            "scrollviewSetChild",
            vec![
                Expr::LocalGet(browser_id),
                Expr::LocalGet(browser_content_id),
            ],
        ));
        m.init.push(mutator_stmt(
            "widgetAddChild",
            vec![Expr::LocalGet(info_id), Expr::LocalGet(info_btn)],
        ));
        // Style the root
        m.init.push(mutator_stmt(
            "setPadding",
            vec![
                Expr::LocalGet(root_id),
                Expr::Number(16.0),
                Expr::Number(16.0),
                Expr::Number(16.0),
                Expr::Number(16.0),
            ],
        ));
        // Add screens to root
        m.init.push(mutator_stmt(
            "widgetAddChild",
            vec![Expr::LocalGet(root_id), Expr::LocalGet(conn_id)],
        ));
        m.init.push(mutator_stmt(
            "widgetAddChild",
            vec![Expr::LocalGet(root_id), Expr::LocalGet(browser_id)],
        ));
        m.init.push(mutator_stmt(
            "widgetAddChild",
            vec![Expr::LocalGet(root_id), Expr::LocalGet(info_id)],
        ));
        m.init.push(app_with_body(Expr::LocalGet(root_id)));
        let r = emit_index_ets(&mut m).unwrap().unwrap();
        // All three screens' contents must surface.
        assert!(
            r.ets_source.contains("Button('Connect')"),
            "missing Connect:\n{}",
            r.ets_source
        );
        assert!(
            r.ets_source.contains("Button('Browse')"),
            "missing Browse:\n{}",
            r.ets_source
        );
        assert!(
            r.ets_source.contains("Button('Info')"),
            "missing Info:\n{}",
            r.ets_source
        );
        assert!(
            r.ets_source
                .contains(".padding({ top: 16, right: 16, bottom: 16, left: 16 })"),
            "missing root padding:\n{}",
            r.ets_source
        );
        assert!(
            r.ets_source.contains("Scroll() {"),
            "missing browser scroll:\n{}",
            r.ets_source
        );
    }

    // ----------------------------------------------------------------
    // Issue #410 — emitted ArkUI must compile cleanly through ArkTS.
    //
    // The three bugs documented in the issue:
    //
    //   1. Nested block comments — `serialize_condition` fallback
    //      returned `"true /* unsupported condition */"` which closed
    //      the outer `/* if ((...)) */` wrapper early on line 82.
    //
    //   2. `__local_N` undeclared identifiers — `serialize_condition`
    //      emitted `__local_<id>` for `Expr::LocalGet`, leaking into
    //      the emitted ArkTS as `if (__local_2) { ... }`.
    //
    //   3. `__platform__` references — once Bug 2 resolves through
    //      bindings, `__platform__ === N` surfaced in emitted code
    //      where `__platform__` isn't declared on the page struct.
    //
    // The fix lives in `serialize_condition` + `collect_compile_time_constants`.
    // These regression tests pin the emitted-source invariants:
    //   - never the substring `__local_`
    //   - never a `*/` inside a `/* if ((...)) */` marker
    //   - `__platform__` comparisons inline as numeric literals (9 for
    //     harmonyos, the only target this codegen serves).
    // ----------------------------------------------------------------

    /// Helper: declare-const stmt for `__platform__` (the canonical HIR
    /// shape `Stmt::Let { name, init: None }` — the same shape
    /// `crates/perry-codegen/src/codegen.rs::compile_time_constants`
    /// recognizes).
    fn declare_const(id: LocalId, name: &str) -> Stmt {
        Stmt::Let {
            id,
            name: name.to_string(),
            ty: perry_types::Type::Any,
            mutable: false,
            init: None,
        }
    }

    #[test]
    fn issue_410_serialize_condition_fallback_has_no_block_comment_close() {
        // The fallback (any unrecognized condition shape) must never
        // produce a `*/` substring — which would close the outer
        // `/* if ((...)) */` wrapper used by emit_modifier_mutations.
        let bindings = HashMap::new();
        let consts = HashMap::new();
        // A Call expression isn't recognized by serialize_condition's
        // match arms, so it lands in the fallback.
        let unrecognized = Expr::Call {
            callee: Box::new(Expr::LocalGet(99)),
            args: vec![],
            type_args: vec![],
        };
        let s = serialize_condition(&unrecognized, &bindings, &consts);
        assert!(
            !s.contains("*/"),
            "fallback emitted */ — bug 1 regressed: {}",
            s
        );
        assert_eq!(
            s, "true",
            "fallback should be the literal 'true', got: {}",
            s
        );
    }

    #[test]
    fn issue_410_local_get_resolves_through_bindings_not_placeholder() {
        // `let mobile = (props.screen === 'mobile')` — when a condition
        // references `mobile`, serialize_condition resolves the local
        // back to the init expression. The init contains a PropertyGet
        // on an unresolvable LocalGet — post-v0.5.489 the cleanly-
        // serializable gate at the top of serialize_condition catches
        // this and degrades the entire condition to `true` (the
        // unresolvable-LocalGet heuristic, lifted to root level).
        // Pre-fix this emitted `true.screen === 'mobile'` which ArkTS
        // strict-mode rejected with "Property 'screen' does not exist
        // on type 'true'".
        //
        // The original test name still applies: the emitted source
        // must NOT contain `__local_N` placeholder text. The exact
        // shape changed from "resolved condition" to "true" once the
        // root-level gate landed.
        let mobile_id: LocalId = 5;
        let init = Expr::Compare {
            op: perry_hir::ir::CompareOp::Eq,
            left: Box::new(Expr::PropertyGet {
                object: Box::new(Expr::LocalGet(99)), // unresolvable
                property: "screen".to_string(),
            }),
            right: Box::new(Expr::String("mobile".into())),
        };
        let mut bindings = HashMap::new();
        bindings.insert(mobile_id, init);
        let consts = HashMap::new();
        let s = serialize_condition(&Expr::LocalGet(mobile_id), &bindings, &consts);
        assert!(
            !s.contains("__local_"),
            "emitted __local_ placeholder — bug 2 regressed: {}",
            s
        );
        assert_eq!(
            s, "true",
            "PropertyGet on unresolvable LocalGet should degrade to 'true', got: {}",
            s
        );
    }

    #[test]
    fn issue_410_unresolvable_local_get_degrades_to_true_not_placeholder() {
        // A LocalGet that's not in bindings (e.g., closure-captured or
        // loop-mutated) degrades to `true` rather than leaking
        // `__local_N` into emitted ArkTS.
        let bindings = HashMap::new();
        let consts = HashMap::new();
        let s = serialize_condition(&Expr::LocalGet(42), &bindings, &consts);
        assert_eq!(
            s, "true",
            "unresolvable LocalGet should degrade to 'true', got: {}",
            s
        );
    }

    #[test]
    fn issue_410_platform_constant_inlines_as_number_literal() {
        // `__platform__ === 9` should serialize with the literal 9
        // inlined (since this codegen is harmonyos-only). Without the
        // compile_time_consts inlining, the LocalGet would resolve via
        // `bindings` and find no entry (declare-const has init: None),
        // ultimately leaking `__platform__` into emitted ArkTS.
        let plat_id: LocalId = 7;
        let bindings = HashMap::new();
        let mut consts = HashMap::new();
        consts.insert(plat_id, 9.0);
        let cmp = Expr::Compare {
            op: perry_hir::ir::CompareOp::Eq,
            left: Box::new(Expr::LocalGet(plat_id)),
            right: Box::new(Expr::Integer(9)),
        };
        let s = serialize_condition(&cmp, &bindings, &consts);
        assert!(
            !s.contains("__platform__"),
            "platform constant leaked: {}",
            s
        );
        assert!(
            !s.contains("__local_"),
            "platform local leaked as placeholder: {}",
            s
        );
        // 9 === 9 — both sides should be the literal 9.
        assert!(s.contains("9"), "expected platform value 9, got: {}", s);
    }

    #[test]
    fn issue_410_collect_compile_time_constants_picks_up_declare_const() {
        // `declare const __platform__: number;` lowers to
        // `Stmt::Let { name: "__platform__", init: None }`. The collector
        // must recognize this canonical shape and assign 9.0 (harmonyos).
        let init = vec![declare_const(11, "__platform__")];
        let map = collect_compile_time_constants(&init);
        assert_eq!(map.get(&11), Some(&9.0));
    }

    #[test]
    fn issue_410_conditional_addchild_emits_valid_arkts_if_block() {
        // The ternary-style shape from #410's "Implementation steps":
        // `if (mobile) widgetAddChild(parent, phone) else widgetAddChild(parent, desktop)`
        // where `mobile` is a top-level binding referencing `__platform__`.
        //
        // Post-#413, `__platform__ === 9` constant-folds to `true` (this
        // codegen path is harmonyos-only, where __platform__ inlines to
        // 9), so the entire `if/else` block evaporates and ONLY the
        // then-branch's `Button('phone')` is emitted as an
        // unconditional child. ArkTS strict-mode previously rejected
        // `if (9 === 9) { ... }` with a no-overlap warning; this
        // dead-branch elimination keeps the source legal.
        let mut m = empty_module();
        let plat_id: LocalId = 1;
        let mobile_id: LocalId = 2;
        let parent_id: LocalId = 3;
        let phone_id: LocalId = 4;
        let desktop_id: LocalId = 5;
        m.init.push(declare_const(plat_id, "__platform__"));
        // let mobile = (__platform__ === 9);
        m.init.push(let_widget(
            mobile_id,
            "mobile",
            Expr::Compare {
                op: perry_hir::ir::CompareOp::Eq,
                left: Box::new(Expr::LocalGet(plat_id)),
                right: Box::new(Expr::Integer(9)),
            },
        ));
        m.init.push(let_widget(
            parent_id,
            "parent",
            nmc("HStack", vec![Expr::Number(0.0), Expr::Array(vec![])]),
        ));
        m.init.push(let_widget(
            phone_id,
            "phoneToolbar",
            nmc("Button", vec![Expr::String("phone".into())]),
        ));
        m.init.push(let_widget(
            desktop_id,
            "desktopToolbar",
            nmc("Button", vec![Expr::String("desktop".into())]),
        ));
        m.init.push(Stmt::If {
            condition: Expr::LocalGet(mobile_id),
            then_branch: vec![mutator_stmt(
                "widgetAddChild",
                vec![Expr::LocalGet(parent_id), Expr::LocalGet(phone_id)],
            )],
            else_branch: Some(vec![mutator_stmt(
                "widgetAddChild",
                vec![Expr::LocalGet(parent_id), Expr::LocalGet(desktop_id)],
            )]),
        });
        m.init.push(app_with_body(Expr::LocalGet(parent_id)));
        let r = emit_index_ets(&mut m).unwrap().unwrap();
        let src = &r.ets_source;
        assert!(
            !src.contains("__local_"),
            "emitted source contains __local_ — bug 2 regressed:\n{}",
            src
        );
        assert!(
            !src.contains("__platform__"),
            "emitted source contains __platform__ — bug 3 regressed:\n{}",
            src
        );
        assert!(
            !src.contains("/* unsupported condition */"),
            "emitted source contains the bug-1 diagnostic comment:\n{}",
            src
        );
        // #413: dead-branch elimination — `9 === 9` folds to `true`, so
        // there's no `if (...)` block at all in the emitted source for
        // this widget; the then-branch's Button is unconditional.
        assert!(
            !src.contains("if (9 === 9)"),
            "literal-only `if (9 === 9)` must be folded out (#413):\n{}",
            src
        );
        assert!(
            src.contains("Button('phone')"),
            "missing then-branch (live after fold):\n{}",
            src
        );
        assert!(
            !src.contains("Button('desktop')"),
            "else-branch should be dead after fold (#413):\n{}",
            src
        );
        // Also pin: no nested */ pattern that would cascade-break ArkTS
        // parsing (Bug 1). We scan for any /* ... */ wrappers and
        // check that the opening `/*` only ever pairs with one `*/`.
        assert_no_nested_block_comments(src);
    }

    #[test]
    fn issue_410_conditional_modifier_chain_has_no_nested_block_comments() {
        // The procedural-mutation-with-conditional-modifier shape from
        // #410. Build a card with an unconditional modifier chain plus
        // a conditional one inside an `if` whose predicate would have
        // surfaced as `__local_N` pre-fix and broken on the fallback's
        // `*/` substring. Post-fix, both the predicate and the
        // surrounding /* if (...) */ comment must be safe.
        let mut m = empty_module();
        let card_id: LocalId = 200;
        let cond_id: LocalId = 201;
        // let isLarge = (something_unsupported_call())
        // → fallback to `true` post-fix; pre-fix would have emitted
        //   the nested-comment cascade.
        m.init.push(let_widget(
            cond_id,
            "isLarge",
            Expr::Call {
                callee: Box::new(Expr::LocalGet(999)),
                args: vec![],
                type_args: vec![],
            },
        ));
        m.init.push(let_widget(
            card_id,
            "card",
            nmc("VStack", vec![Expr::Array(vec![])]),
        ));
        m.init.push(mutator_stmt(
            "widgetSetBackgroundColor",
            vec![
                Expr::LocalGet(card_id),
                Expr::Number(0.5),
                Expr::Number(0.5),
                Expr::Number(0.5),
                Expr::Number(1.0),
            ],
        ));
        // Conditional padding mutator — emits as `/* if ((...)) */ .padding(...)`.
        m.init.push(Stmt::If {
            condition: Expr::LocalGet(cond_id),
            then_branch: vec![mutator_stmt(
                "setPadding",
                vec![
                    Expr::LocalGet(card_id),
                    Expr::Number(16.0),
                    Expr::Number(16.0),
                    Expr::Number(16.0),
                    Expr::Number(16.0),
                ],
            )],
            else_branch: None,
        });
        m.init.push(app_with_body(Expr::LocalGet(card_id)));
        let r = emit_index_ets(&mut m).unwrap().unwrap();
        let src = &r.ets_source;
        assert!(
            !src.contains("__local_"),
            "emitted source contains __local_ — bug 2 regressed:\n{}",
            src
        );
        assert!(
            !src.contains("/* unsupported condition */"),
            "emitted source contains the bug-1 diagnostic comment:\n{}",
            src
        );
        // The unconditional background modifier still applies.
        assert!(
            src.contains(".backgroundColor("),
            "expected unconditional background:\n{}",
            src
        );
        // Bug 1 acceptance bar: no nested /* ... */ patterns anywhere.
        assert_no_nested_block_comments(src);
    }

    /// Walk the source line-by-line and assert no line opens a `/*` that
    /// contains a second `*/` after the first one (which would break
    /// parsing). This is a tighter form of "no `*/` inside `/* ... */`":
    /// for every block-comment marker, count the number of `*/` between
    /// `/*` and the next `*/` — must be exactly one.
    fn assert_no_nested_block_comments(src: &str) {
        let mut i = 0;
        let bytes = src.as_bytes();
        while i + 1 < bytes.len() {
            if bytes[i] == b'/' && bytes[i + 1] == b'*' {
                // Found an opening `/*`. Find the matching close.
                let start = i;
                i += 2;
                let mut close = None;
                while i + 1 < bytes.len() {
                    if bytes[i] == b'*' && bytes[i + 1] == b'/' {
                        close = Some(i);
                        break;
                    }
                    i += 1;
                }
                let Some(close) = close else { return };
                // The comment body is bytes[start+2..close]. It must NOT
                // itself contain a `*/` (which would mean the original
                // close was actually the *second* close — impossible per
                // the inner-loop logic above, but the symmetric check
                // catches the other failure mode where serialize_condition
                // smuggled in a `*/` that was treated as the close.
                let body = &src[start + 2..close];
                assert!(
                    !body.contains("*/"),
                    "nested block comment found at {}: body={:?}\nfull source:\n{}",
                    start,
                    body,
                    src
                );
                i = close + 2;
            } else {
                i += 1;
            }
        }
    }

    // ─────────────────────────────────────────────────────────────────
    // Issue #413 — emitted ArkUI must compile through ArkTS strict mode.
    //
    // Two bugs documented in the issue:
    //
    //   1. Literal-only comparisons in conditions: with `__platform__`
    //      inlined to 9 (harmonyos codegen path) and bindings resolved,
    //      a condition like `__platform__ === 1` serialized to
    //      `9 === 1`, and ArkTS rejected `if (9 === 1) { ... }` with
    //      a "no overlap" error. Fix: constant-fold via
    //      `evaluate_condition` and drop dead branches at harvest time.
    //      Operator-precedence: when a binding's init expression is
    //      Binary/Logical/Unary and gets spliced into another such
    //      expression, parens prevent precedence inversion (e.g.
    //      `!isIOS` becoming `!9` then `=== 1` rather than
    //      `!(9 === 1)`).
    //
    //   2. Cross-axis alignment enum on HStack: ArkUI Row's cross-axis
    //      is vertical (uses `VerticalAlign`), Column's is horizontal
    //      (uses `HorizontalAlign`). v0.5.480's `stackSetAlignment`
    //      always emitted `HorizontalAlign.X`, which ArkTS rejected
    //      for HStack with a type-mismatch error.
    // ─────────────────────────────────────────────────────────────────

    #[test]
    fn issue_413_evaluate_condition_folds_literal_eq_false() {
        // 1 === 2 → Some(false)
        let bindings = HashMap::new();
        let consts = HashMap::new();
        let cmp = Expr::Compare {
            op: perry_hir::ir::CompareOp::Eq,
            left: Box::new(Expr::Integer(1)),
            right: Box::new(Expr::Integer(2)),
        };
        assert_eq!(evaluate_condition(&cmp, &bindings, &consts), Some(false));
    }

    #[test]
    fn issue_413_evaluate_condition_folds_literal_eq_true() {
        // 1 === 1 → Some(true)
        let bindings = HashMap::new();
        let consts = HashMap::new();
        let cmp = Expr::Compare {
            op: perry_hir::ir::CompareOp::Eq,
            left: Box::new(Expr::Integer(1)),
            right: Box::new(Expr::Integer(1)),
        };
        assert_eq!(evaluate_condition(&cmp, &bindings, &consts), Some(true));
    }

    #[test]
    fn issue_413_evaluate_condition_returns_none_for_runtime_value() {
        // PropertyGet on an unresolved local is non-foldable.
        let bindings = HashMap::new();
        let consts = HashMap::new();
        let prop = Expr::PropertyGet {
            object: Box::new(Expr::LocalGet(99)),
            property: "isMobile".to_string(),
        };
        assert_eq!(evaluate_condition(&prop, &bindings, &consts), None);
    }

    #[test]
    fn issue_413_evaluate_condition_resolves_through_compile_time_consts() {
        // __platform__ === 9 (with __platform__ as a compile-time
        // constant inlined to 9.0) → Some(true).
        let plat_id: LocalId = 7;
        let bindings = HashMap::new();
        let mut consts = HashMap::new();
        consts.insert(plat_id, 9.0);
        let cmp = Expr::Compare {
            op: perry_hir::ir::CompareOp::Eq,
            left: Box::new(Expr::LocalGet(plat_id)),
            right: Box::new(Expr::Integer(9)),
        };
        assert_eq!(evaluate_condition(&cmp, &bindings, &consts), Some(true));
    }

    #[test]
    fn issue_413_evaluate_condition_logical_or_short_circuits() {
        // (9 === 1) || (9 === 9) → Some(true) via short-circuit.
        let plat_id: LocalId = 7;
        let bindings = HashMap::new();
        let mut consts = HashMap::new();
        consts.insert(plat_id, 9.0);
        let cmp = Expr::Logical {
            op: perry_hir::ir::LogicalOp::Or,
            left: Box::new(Expr::Compare {
                op: perry_hir::ir::CompareOp::Eq,
                left: Box::new(Expr::LocalGet(plat_id)),
                right: Box::new(Expr::Integer(1)),
            }),
            right: Box::new(Expr::Compare {
                op: perry_hir::ir::CompareOp::Eq,
                left: Box::new(Expr::LocalGet(plat_id)),
                right: Box::new(Expr::Integer(9)),
            }),
        };
        assert_eq!(evaluate_condition(&cmp, &bindings, &consts), Some(true));
    }

    #[test]
    fn issue_413_evaluate_condition_unary_not_negates_literal() {
        // !true → Some(false)
        let bindings = HashMap::new();
        let consts = HashMap::new();
        let neg = Expr::Unary {
            op: perry_hir::ir::UnaryOp::Not,
            operand: Box::new(Expr::Bool(true)),
        };
        assert_eq!(evaluate_condition(&neg, &bindings, &consts), Some(false));
    }

    #[test]
    fn issue_413_literal_only_if_block_drops_dead_branch_emits_only_then() {
        // if (1 === 2) widgetAddChild(parent, btn_a) — 1 === 2 folds to
        // false, so the dead then-branch is dropped and nothing is
        // appended. The parent stays empty.
        let mut m = empty_module();
        let parent_id: LocalId = 80;
        let btn_a_id: LocalId = 81;
        m.init.push(let_widget(
            parent_id,
            "parent",
            nmc("VStack", vec![Expr::Array(vec![])]),
        ));
        m.init.push(let_widget(
            btn_a_id,
            "btn_a",
            nmc("Button", vec![Expr::String("dead".into())]),
        ));
        m.init.push(Stmt::If {
            condition: Expr::Compare {
                op: perry_hir::ir::CompareOp::Eq,
                left: Box::new(Expr::Integer(1)),
                right: Box::new(Expr::Integer(2)),
            },
            then_branch: vec![mutator_stmt(
                "widgetAddChild",
                vec![Expr::LocalGet(parent_id), Expr::LocalGet(btn_a_id)],
            )],
            else_branch: None,
        });
        m.init.push(app_with_body(Expr::LocalGet(parent_id)));
        let r = emit_index_ets(&mut m).unwrap().unwrap();
        let src = &r.ets_source;
        assert!(
            !src.contains("Button('dead')"),
            "dead-branch button should not be emitted:\n{}",
            src
        );
        // ArkTS strict-mode would have rejected `if (1 === 2)`. After
        // the fold it never appears in the source.
        assert!(
            !src.contains("if (1 === 2)") && !src.contains("if (1===2)"),
            "literal-only `if` predicate must be folded:\n{}",
            src
        );
    }

    #[test]
    fn issue_413_literal_only_if_block_keeps_then_inlines_no_if_wrapper() {
        // if (1 === 1) widgetAddChild(parent, btn_a) — 1 === 1 folds to
        // true, so the live then-branch's child is inlined as an
        // unconditional sibling and no `if (...)` wrapper is emitted.
        let mut m = empty_module();
        let parent_id: LocalId = 82;
        let btn_a_id: LocalId = 83;
        m.init.push(let_widget(
            parent_id,
            "parent",
            nmc("VStack", vec![Expr::Array(vec![])]),
        ));
        m.init.push(let_widget(
            btn_a_id,
            "btn_a",
            nmc("Button", vec![Expr::String("live".into())]),
        ));
        m.init.push(Stmt::If {
            condition: Expr::Compare {
                op: perry_hir::ir::CompareOp::Eq,
                left: Box::new(Expr::Integer(1)),
                right: Box::new(Expr::Integer(1)),
            },
            then_branch: vec![mutator_stmt(
                "widgetAddChild",
                vec![Expr::LocalGet(parent_id), Expr::LocalGet(btn_a_id)],
            )],
            else_branch: None,
        });
        m.init.push(app_with_body(Expr::LocalGet(parent_id)));
        let r = emit_index_ets(&mut m).unwrap().unwrap();
        let src = &r.ets_source;
        assert!(
            src.contains("Button('live')"),
            "live-branch button must be emitted:\n{}",
            src
        );
        assert!(
            !src.contains("if (1 === 1)") && !src.contains("if (1===1)"),
            "literal-only `if` predicate must be folded out of the source:\n{}",
            src
        );
    }

    #[test]
    fn issue_413_platform_const_eq_drops_dead_branch_in_addchild() {
        // Same shape as #410's repro but with __platform__ === 1 (the
        // mobile-style check that's false on harmonyos where
        // __platform__ === 9). Pre-#413 this serialized to
        // `if (9 === 1) { Button('phone') } else { Button('desktop') }`
        // which ArkTS rejected. Post-#413 it folds to `false` and only
        // the desktop branch survives.
        let mut m = empty_module();
        let plat_id: LocalId = 1;
        let parent_id: LocalId = 2;
        let phone_id: LocalId = 3;
        let desktop_id: LocalId = 4;
        m.init.push(declare_const(plat_id, "__platform__"));
        m.init.push(let_widget(
            parent_id,
            "parent",
            nmc("HStack", vec![Expr::Number(0.0), Expr::Array(vec![])]),
        ));
        m.init.push(let_widget(
            phone_id,
            "phoneToolbar",
            nmc("Button", vec![Expr::String("phone".into())]),
        ));
        m.init.push(let_widget(
            desktop_id,
            "desktopToolbar",
            nmc("Button", vec![Expr::String("desktop".into())]),
        ));
        m.init.push(Stmt::If {
            condition: Expr::Compare {
                op: perry_hir::ir::CompareOp::Eq,
                left: Box::new(Expr::LocalGet(plat_id)),
                right: Box::new(Expr::Integer(1)),
            },
            then_branch: vec![mutator_stmt(
                "widgetAddChild",
                vec![Expr::LocalGet(parent_id), Expr::LocalGet(phone_id)],
            )],
            else_branch: Some(vec![mutator_stmt(
                "widgetAddChild",
                vec![Expr::LocalGet(parent_id), Expr::LocalGet(desktop_id)],
            )]),
        });
        m.init.push(app_with_body(Expr::LocalGet(parent_id)));
        let r = emit_index_ets(&mut m).unwrap().unwrap();
        let src = &r.ets_source;
        assert!(
            !src.contains("Button('phone')"),
            "dead then-branch (9 === 1 is false) must be dropped:\n{}",
            src
        );
        assert!(
            src.contains("Button('desktop')"),
            "live else-branch must be emitted:\n{}",
            src
        );
        assert!(
            !src.contains("if (9 === 1)") && !src.contains("if (9===1)"),
            "literal `if (9 === 1)` must not appear:\n{}",
            src
        );
    }

    #[test]
    fn issue_413_local_get_resolves_through_binding_to_platform_compare() {
        // let mobile = __platform__ === 1;  (binding)
        // if (mobile) widgetAddChild(parent, phone) else widgetAddChild(parent, desktop);
        // Should fold the same as the inlined comparison: `mobile`
        // resolves to `9 === 1` which is `false`, so only the desktop
        // branch survives.
        let mut m = empty_module();
        let plat_id: LocalId = 1;
        let mobile_id: LocalId = 2;
        let parent_id: LocalId = 3;
        let phone_id: LocalId = 4;
        let desktop_id: LocalId = 5;
        m.init.push(declare_const(plat_id, "__platform__"));
        m.init.push(let_widget(
            mobile_id,
            "mobile",
            Expr::Compare {
                op: perry_hir::ir::CompareOp::Eq,
                left: Box::new(Expr::LocalGet(plat_id)),
                right: Box::new(Expr::Integer(1)),
            },
        ));
        m.init.push(let_widget(
            parent_id,
            "parent",
            nmc("HStack", vec![Expr::Number(0.0), Expr::Array(vec![])]),
        ));
        m.init.push(let_widget(
            phone_id,
            "btn_phone",
            nmc("Button", vec![Expr::String("phone".into())]),
        ));
        m.init.push(let_widget(
            desktop_id,
            "btn_desktop",
            nmc("Button", vec![Expr::String("desktop".into())]),
        ));
        m.init.push(Stmt::If {
            condition: Expr::LocalGet(mobile_id),
            then_branch: vec![mutator_stmt(
                "widgetAddChild",
                vec![Expr::LocalGet(parent_id), Expr::LocalGet(phone_id)],
            )],
            else_branch: Some(vec![mutator_stmt(
                "widgetAddChild",
                vec![Expr::LocalGet(parent_id), Expr::LocalGet(desktop_id)],
            )]),
        });
        m.init.push(app_with_body(Expr::LocalGet(parent_id)));
        let r = emit_index_ets(&mut m).unwrap().unwrap();
        let src = &r.ets_source;
        assert!(
            !src.contains("Button('phone')"),
            "dead then-branch (mobile = 9 === 1 = false) must be dropped:\n{}",
            src
        );
        assert!(
            src.contains("Button('desktop')"),
            "live else-branch must be emitted:\n{}",
            src
        );
    }

    #[test]
    fn issue_413_hstack_set_alignment_emits_vertical_align_enum() {
        // HStack (= ArkUI Row) cross-axis is vertical: must use
        // `VerticalAlign.Start`, not `HorizontalAlign.Start`.
        let mut m = empty_module();
        let id: LocalId = 100;
        m.init.push(let_widget(
            id,
            "row",
            nmc("HStack", vec![Expr::Number(0.0), Expr::Array(vec![])]),
        ));
        m.init.push(mutator_stmt(
            "stackSetAlignment",
            vec![Expr::LocalGet(id), Expr::Number(0.0)], // Start
        ));
        m.init.push(app_with_body(Expr::LocalGet(id)));
        let r = emit_index_ets(&mut m).unwrap().unwrap();
        let src = &r.ets_source;
        // v0.5.484 follow-up — `VerticalAlign` enum doesn't have a `Start`
        // member (only `Top` / `Center` / `Bottom`). Pre-v0.5.484 this
        // assertion pinned the broken `VerticalAlign.Start` shape that
        // ArkTS strict-mode rejected. Now the value-name is axis-correct.
        assert!(
            src.contains(".alignItems(VerticalAlign.Top)"),
            "HStack + start (0) must emit VerticalAlign.Top:\n{}",
            src
        );
        assert!(
            !src.contains("HorizontalAlign"),
            "HStack must NOT emit HorizontalAlign:\n{}",
            src
        );
    }

    #[test]
    fn issue_413_vstack_set_alignment_emits_horizontal_align_enum() {
        // VStack (= ArkUI Column) cross-axis is horizontal: must use
        // `HorizontalAlign.Start`. Regression-pin to ensure the new
        // axis-aware emit didn't accidentally flip the VStack arm.
        let mut m = empty_module();
        let id: LocalId = 101;
        m.init.push(let_widget(
            id,
            "col",
            nmc("VStack", vec![Expr::Array(vec![])]),
        ));
        m.init.push(mutator_stmt(
            "stackSetAlignment",
            vec![Expr::LocalGet(id), Expr::Number(0.0)], // Start
        ));
        m.init.push(app_with_body(Expr::LocalGet(id)));
        let r = emit_index_ets(&mut m).unwrap().unwrap();
        let src = &r.ets_source;
        assert!(
            src.contains(".alignItems(HorizontalAlign.Start)"),
            "VStack must emit HorizontalAlign.Start:\n{}",
            src
        );
        assert!(
            !src.contains("VerticalAlign"),
            "VStack must NOT emit VerticalAlign:\n{}",
            src
        );
    }

    #[test]
    fn issue_413_serialize_condition_parenthesizes_unary_of_compare() {
        // !mobile where mobile = (__platform__ === 1).
        // After binding-resolution, the unary `!` operates on the
        // serialized comparison. Without defensive parenthesization,
        // the result `!9 === 1` parses as `(!9) === 1` (false === 1 →
        // bool→num coercion → 0 === 1 → false) instead of the
        // intended `!(9 === 1)` (== !false → true). The parens fix
        // pins the precedence.
        let plat_id: LocalId = 7;
        let mobile_id: LocalId = 8;
        let bindings = {
            let mut b = HashMap::new();
            b.insert(
                mobile_id,
                Expr::Compare {
                    op: perry_hir::ir::CompareOp::Eq,
                    left: Box::new(Expr::LocalGet(plat_id)),
                    right: Box::new(Expr::Integer(1)),
                },
            );
            b
        };
        let mut consts = HashMap::new();
        consts.insert(plat_id, 9.0);
        let neg = Expr::Unary {
            op: perry_hir::ir::UnaryOp::Not,
            operand: Box::new(Expr::LocalGet(mobile_id)),
        };
        let s = serialize_condition(&neg, &bindings, &consts);
        // Must contain `!(...)` where `...` covers the comparison —
        // i.e. the `(` immediately after `!`. The internal contents
        // are `9 === 1` (whitespace from the operator string) so the
        // exact substring is `!(9 === 1)`.
        assert!(
            s.contains("!(9 === 1)") || s.contains("!(9===1)"),
            "expected unary-not to wrap binding-resolved comparison in parens, got: {}",
            s
        );
        // Negative-pin: the unparenthesized form `!9 === 1` must NOT
        // appear (which would parse as `(!9) === 1`).
        assert!(
            !s.contains("!9 === 1") && !s.contains("!9===1"),
            "unparenthesized `!9 === 1` precedence-inversion bug regressed: {}",
            s
        );
    }

    #[test]
    fn issue_413_serialize_condition_parenthesizes_or_chain_with_unary() {
        // mobile = __platform__ === 1 || __platform__ === 2 || (!isIOS && x)
        // where isIOS = __platform__ === 1 (so isIOS = false, and
        // !isIOS = true), and x is an unresolved PropertyGet so the
        // whole chain doesn't fold to a literal — it stays a runtime
        // condition. The serialized chain must parenthesize each
        // sub-Binary/Unary so precedence can't invert.
        let plat_id: LocalId = 7;
        let isios_id: LocalId = 9;
        let mut bindings = HashMap::new();
        bindings.insert(
            isios_id,
            Expr::Compare {
                op: perry_hir::ir::CompareOp::Eq,
                left: Box::new(Expr::LocalGet(plat_id)),
                right: Box::new(Expr::Integer(1)),
            },
        );
        let mut consts = HashMap::new();
        consts.insert(plat_id, 9.0);
        // (__platform__ === 1) || (__platform__ === 2) || (!isIOS && something)
        let chain = Expr::Logical {
            op: perry_hir::ir::LogicalOp::Or,
            left: Box::new(Expr::Logical {
                op: perry_hir::ir::LogicalOp::Or,
                left: Box::new(Expr::Compare {
                    op: perry_hir::ir::CompareOp::Eq,
                    left: Box::new(Expr::LocalGet(plat_id)),
                    right: Box::new(Expr::Integer(1)),
                }),
                right: Box::new(Expr::Compare {
                    op: perry_hir::ir::CompareOp::Eq,
                    left: Box::new(Expr::LocalGet(plat_id)),
                    right: Box::new(Expr::Integer(2)),
                }),
            }),
            right: Box::new(Expr::Unary {
                op: perry_hir::ir::UnaryOp::Not,
                operand: Box::new(Expr::LocalGet(isios_id)),
            }),
        };
        let s = serialize_condition(&chain, &bindings, &consts);
        // The buggy serialization documented in the issue:
        //     `9 === 1 || 9 === 2 || !9 === 1`
        // (note `!9 === 1` parses as `(!9) === 1`). Post-fix this
        // specific substring must NOT appear.
        assert!(
            !s.contains("!9 === 1") && !s.contains("!9===1"),
            "precedence-inverted `!9 === 1` regressed: {}",
            s
        );
        // Unary `!` must wrap the resolved comparison in parens.
        // (v0.5.489 note: dropped the `&& <unresolvable PropertyGet>`
        // tail from the chain — the new cleanly-serializable gate at
        // the root of serialize_condition would have degraded the whole
        // condition to `true` once any sub-expression hits an
        // unresolvable PropertyGet. The unary-paren behavior is still
        // exercised by the now-resolvable chain.)
        assert!(
            s.contains("!(9 === 1)") || s.contains("!(9===1)"),
            "expected unary-not paren-wrap: {}",
            s
        );
    }

    #[test]
    fn issue_490_unfoldable_unresolvable_condition_walks_only_then_branch() {
        // v0.5.490: when a condition is unfoldable AND not cleanly
        // serializable, dead-branch elim picks the then-branch. The
        // pre-v0.5.490 behavior emitted both branches under `if (true)
        // {...} else {...}` — Mango's `connectionNames.length === 0`
        // exposed this as the "+ New Connection" duplicate-content bug.
        let mut m = empty_module();
        let parent_id: LocalId = 110;
        let a_id: LocalId = 111;
        let b_id: LocalId = 112;
        m.init.push(let_widget(
            parent_id,
            "parent",
            nmc("VStack", vec![Expr::Array(vec![])]),
        ));
        m.init.push(let_widget(
            a_id,
            "btn_a",
            nmc("Button", vec![Expr::String("a".into())]),
        ));
        m.init.push(let_widget(
            b_id,
            "btn_b",
            nmc("Button", vec![Expr::String("b".into())]),
        ));
        m.init.push(Stmt::If {
            condition: Expr::PropertyGet {
                object: Box::new(Expr::LocalGet(9999)),
                property: "isMobile".to_string(),
            },
            then_branch: vec![mutator_stmt(
                "widgetAddChild",
                vec![Expr::LocalGet(parent_id), Expr::LocalGet(a_id)],
            )],
            else_branch: Some(vec![mutator_stmt(
                "widgetAddChild",
                vec![Expr::LocalGet(parent_id), Expr::LocalGet(b_id)],
            )]),
        });
        m.init.push(app_with_body(Expr::LocalGet(parent_id)));
        let r = emit_index_ets(&mut m).unwrap().unwrap();
        let src = &r.ets_source;
        // Then-branch only — heuristic pick.
        assert!(
            src.contains("Button('a')"),
            "then-branch must render:\n{}",
            src
        );
        assert!(
            !src.contains("Button('b')"),
            "else-branch must NOT render (dead-branch elim):\n{}",
            src
        );
    }

    // ------------------------------------------------------------------
    // Issue #669 — Chart on HarmonyOS (ArkUI Canvas backend).
    // ------------------------------------------------------------------

    #[test]
    fn chart_bar_with_data_points_emits_canvas_and_draw_calls() {
        // const c = Chart(1, 200, 150);
        // chartAddDataPoint(c, 'Q1', 10);
        // chartAddDataPoint(c, 'Q2', 20);
        // chartSetTitle(c, 'Sales');
        // App({ body: c });
        let mut m = empty_module();
        m.init.push(let_widget(
            42,
            "c",
            nmc(
                "Chart",
                vec![Expr::Integer(1), Expr::Number(200.0), Expr::Number(150.0)],
            ),
        ));
        m.init.push(mutator_stmt(
            "chartAddDataPoint",
            vec![
                Expr::LocalGet(42),
                Expr::String("Q1".into()),
                Expr::Number(10.0),
            ],
        ));
        m.init.push(mutator_stmt(
            "chartAddDataPoint",
            vec![
                Expr::LocalGet(42),
                Expr::String("Q2".into()),
                Expr::Number(20.0),
            ],
        ));
        m.init.push(mutator_stmt(
            "chartSetTitle",
            vec![Expr::LocalGet(42), Expr::String("Sales".into())],
        ));
        m.init.push(app_with_body(Expr::LocalGet(42)));

        let r = emit_index_ets(&mut m).unwrap().unwrap();
        let src = r.ets_source;

        // Canvas widget + per-instance ctx field.
        assert!(
            src.contains("Canvas(this.__chart_0_ctx)"),
            "Canvas with per-instance ctx must render:\n{}",
            src,
        );
        assert!(
            src.contains(
                "private __chart_0_settings: RenderingContextSettings = \
                 new RenderingContextSettings(true)"
            ),
            "RenderingContextSettings field missing:\n{}",
            src,
        );
        assert!(
            src.contains(
                "private __chart_0_ctx: CanvasRenderingContext2D = \
                 new CanvasRenderingContext2D(this.__chart_0_settings)"
            ),
            "CanvasRenderingContext2D field missing:\n{}",
            src,
        );
        // Size flowed through.
        assert!(src.contains(".width(200)"), "width missing:\n{}", src);
        assert!(src.contains(".height(150)"), "height missing:\n{}", src);
        // Data points folded.
        assert!(
            src.contains("{ label: 'Q1', value: 10 }"),
            "Q1 point missing:\n{}",
            src
        );
        assert!(
            src.contains("{ label: 'Q2', value: 20 }"),
            "Q2 point missing:\n{}",
            src
        );
        // Title folded.
        assert!(
            src.contains("const title: string = 'Sales'"),
            "title missing:\n{}",
            src
        );
        // 2D context draw calls present (bar branch uses fillRect for bars).
        assert!(
            src.contains("ctx.clearRect(0, 0, cw, ch)"),
            "clearRect missing:\n{}",
            src
        );
        assert!(src.contains("ctx.fillRect("), "fillRect missing:\n{}", src);
        assert!(
            src.contains("ctx.fillText(title, cw / 2, 22)"),
            "title fillText missing:\n{}",
            src
        );
    }

    #[test]
    fn chart_line_kind_emits_stroke_path() {
        let mut m = empty_module();
        m.init.push(let_widget(
            7,
            "c",
            nmc(
                "Chart",
                vec![Expr::Integer(0), Expr::Number(100.0), Expr::Number(100.0)],
            ),
        ));
        m.init.push(mutator_stmt(
            "chartAddDataPoint",
            vec![
                Expr::LocalGet(7),
                Expr::String("a".into()),
                Expr::Number(5.0),
            ],
        ));
        m.init.push(app_with_body(Expr::LocalGet(7)));
        let r = emit_index_ets(&mut m).unwrap().unwrap();
        let src = r.ets_source;
        // Line kind: lineTo + stroke + arc-dots.
        assert!(src.contains("ctx.lineTo("), "lineTo missing:\n{}", src);
        assert!(src.contains("ctx.stroke()"), "stroke() missing:\n{}", src);
        assert!(src.contains("ctx.arc("), "arc dot missing:\n{}", src);
    }

    #[test]
    fn chart_pie_kind_emits_arc_fill_and_legend() {
        let mut m = empty_module();
        m.init.push(let_widget(
            9,
            "c",
            nmc(
                "Chart",
                vec![Expr::Integer(2), Expr::Number(120.0), Expr::Number(120.0)],
            ),
        ));
        m.init.push(mutator_stmt(
            "chartAddDataPoint",
            vec![
                Expr::LocalGet(9),
                Expr::String("x".into()),
                Expr::Number(1.0),
            ],
        ));
        m.init.push(app_with_body(Expr::LocalGet(9)));
        let r = emit_index_ets(&mut m).unwrap().unwrap();
        let src = r.ets_source;
        assert!(
            src.contains("ctx.arc(cx, cy, radius"),
            "pie arc missing:\n{}",
            src
        );
        assert!(
            src.contains("ctx.closePath()"),
            "pie closePath missing:\n{}",
            src
        );
        assert!(src.contains("ctx.fill()"), "pie fill missing:\n{}", src);
    }

    #[test]
    fn chart_clear_data_resets_points() {
        // chartAddDataPoint then chartClearData then chartAddDataPoint —
        // only the last point should survive in the static fold.
        let mut m = empty_module();
        m.init.push(let_widget(
            5,
            "c",
            nmc(
                "Chart",
                vec![Expr::Integer(1), Expr::Number(100.0), Expr::Number(100.0)],
            ),
        ));
        m.init.push(mutator_stmt(
            "chartAddDataPoint",
            vec![
                Expr::LocalGet(5),
                Expr::String("dropped".into()),
                Expr::Number(99.0),
            ],
        ));
        m.init
            .push(mutator_stmt("chartClearData", vec![Expr::LocalGet(5)]));
        m.init.push(mutator_stmt(
            "chartAddDataPoint",
            vec![
                Expr::LocalGet(5),
                Expr::String("kept".into()),
                Expr::Number(7.0),
            ],
        ));
        m.init.push(app_with_body(Expr::LocalGet(5)));

        let r = emit_index_ets(&mut m).unwrap().unwrap();
        let src = r.ets_source;
        assert!(
            !src.contains("'dropped'"),
            "cleared point must not render:\n{}",
            src
        );
        assert!(
            src.contains("{ label: 'kept', value: 7 }"),
            "surviving point must render:\n{}",
            src
        );
    }

    // ------------------------------------------------------------------
    // Issue #670 — TreeView on HarmonyOS (ArkUI List backend).
    // ------------------------------------------------------------------

    #[test]
    fn treeview_static_graph_emits_list_foreach_and_state() {
        // const root  = TreeNode('root', 'Root');
        // const child = TreeNode('c1',   'Child 1');
        // treeNodeAddChild(root, child);
        // const tv = TreeView(root, () => {});
        // App({ body: tv });
        let mut m = empty_module();
        m.init.push(let_widget(
            10,
            "root",
            nmc(
                "TreeNode",
                vec![Expr::String("root".into()), Expr::String("Root".into())],
            ),
        ));
        m.init.push(let_widget(
            11,
            "child",
            nmc(
                "TreeNode",
                vec![Expr::String("c1".into()), Expr::String("Child 1".into())],
            ),
        ));
        m.init.push(mutator_stmt(
            "treeNodeAddChild",
            vec![Expr::LocalGet(10), Expr::LocalGet(11)],
        ));
        m.init.push(let_widget(
            12,
            "tv",
            nmc("TreeView", vec![Expr::LocalGet(10), closure_stub()]),
        ));
        m.init.push(app_with_body(Expr::LocalGet(12)));

        let r = emit_index_ets(&mut m).unwrap().unwrap();
        let src = r.ets_source;

        // List + ForEach with the flatten helper as its source.
        assert!(
            src.contains("List({ space: 0 })"),
            "List container missing:\n{}",
            src,
        );
        assert!(
            src.contains("ForEach(this.__tree_0_flatten(),"),
            "ForEach over flatten missing:\n{}",
            src,
        );
        // Static node data baked recursively (root holds child).
        assert!(
            src.contains(
                "{ id: 'root', label: 'Root', \
                 children: [{ id: 'c1', label: 'Child 1', children: [] }] }"
            ),
            "recursive node literal missing:\n{}",
            src,
        );
        // @State fields for expanded set + selected id.
        assert!(
            src.contains("@State __tree_0_expanded: Set<string> = new Set<string>()"),
            "expanded @State missing:\n{}",
            src,
        );
        assert!(
            src.contains("@State __tree_0_selectedId: string = ''"),
            "selectedId @State missing:\n{}",
            src,
        );
        // Flatten method emitted on the @Component.
        assert!(
            src.contains("__tree_0_flatten():"),
            "flatten helper missing:\n{}",
            src,
        );
        // Tap-handler wires invokeCallback1 with row.id.
        assert!(
            src.contains("perryEntry.invokeCallback1(0, row.id)"),
            "onSelect dispatch missing:\n{}",
            src,
        );
        assert_eq!(r.callbacks.len(), 1);
    }

    #[test]
    fn treeview_depth_padding_uses_row_depth_field() {
        // Verifies the ArkUI .padding({ left: row.depth * 16 }) shape so
        // children render with their indent. The actual numbers (16 px)
        // are a v1 layout choice — change requires test + code together.
        let mut m = empty_module();
        m.init.push(let_widget(
            20,
            "root",
            nmc(
                "TreeNode",
                vec![Expr::String("r".into()), Expr::String("R".into())],
            ),
        ));
        m.init.push(let_widget(
            21,
            "tv",
            nmc("TreeView", vec![Expr::LocalGet(20), closure_stub()]),
        ));
        m.init.push(app_with_body(Expr::LocalGet(21)));
        let r = emit_index_ets(&mut m).unwrap().unwrap();
        let src = r.ets_source;
        assert!(
            src.contains(".padding({ left: row.depth * 16,"),
            "depth-based padding missing:\n{}",
            src,
        );
    }
}
