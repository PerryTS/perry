// Shared data types for the perry-codegen-arkts crate. Mechanically
// split out of lib.rs (issue #1100). Pure code move.
use perry_hir::ir::Expr;

// LocalId is `u32` upstream; re-import directly so we don't carry a
// HIR owns the function identifier used by view builders.
pub(crate) type LocalId = u32;

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
pub(crate) struct TextSlot {
    pub(crate) original_id: String,
    pub(crate) field_id: String,
    pub(crate) initial: String,
}

/// Phase 2 v10 — Real LazyVStack registration. Each
/// `LazyVStack(items.map(item => widget))` allocates a
/// `PerryListDataSource`-backed `@State` field on the page struct. The
/// harvest collects these so `wrap_index_page` can emit the field decls +
/// the `PerryListDataSource` helper-class boilerplate once.
pub(crate) struct LazyDataSource {
    pub(crate) field_id: String,
    pub(crate) items_source: String,
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
pub(crate) struct ChartInstance {
    /// Per-page sequential id used to name `__chart_<n>_ctx`.
    pub(crate) field_id: String,
    /// 0 = line, 1 = bar, 2 = pie. Falls back to bar for unknown values.
    pub(crate) kind: i64,
    /// Layout hints — flow through to `.width(N).height(N)`. 0 means
    /// "use intrinsic / leave unset".
    pub(crate) width: f64,
    pub(crate) height: f64,
    /// Folded `(label, value)` pairs. `chartClearData` resets the list;
    /// `chartAddDataPoint` appends; later wins.
    pub(crate) points: Vec<(String, f64)>,
    /// Folded title string (last `chartSetTitle` wins). Empty = no title.
    pub(crate) title: String,
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
pub(crate) struct TreeViewInstance {
    /// Per-page sequential id used to name fields + helper method.
    pub(crate) field_id: String,
    /// Resolved tree built at codegen time. `Some` when the root walk
    /// succeeded; `None` is impossible here (we only push on success).
    pub(crate) root: TreeViewNode,
    /// Callback slot id for the user-supplied onSelect closure, if any.
    pub(crate) on_select_idx: Option<usize>,
}

/// Static tree node captured at codegen time. Mirrors `TreeNode(id, label)`
/// + its accumulated `treeNodeAddChild` children, recursively.
#[derive(Clone)]
pub(crate) struct TreeViewNode {
    pub(crate) id: String,
    pub(crate) label: String,
    pub(crate) children: Vec<TreeViewNode>,
}

/// Bundled holder for per-page state collectors that emit_widget grew
/// to need beyond text_slots / lazy_sources. Lives behind a single
/// `&mut HarvestExtras` param so adding a new widget that registers
/// @Component-level state doesn't require touching every emit_*
/// signature. Currently holds:
/// - chart_instances: see [`ChartInstance`] (issue #669)
/// - tree_view_instances: see [`TreeViewInstance`] (issue #670)
#[derive(Default)]
pub(crate) struct HarvestExtras {
    pub(crate) chart_instances: Vec<ChartInstance>,
    pub(crate) tree_view_instances: Vec<TreeViewInstance>,
}

/// Phase 2 v6 — `state<T>(initial)` registry. Each `let x = state(initial)`
/// declaration in `module.init` registers a synthetic id (`__state_<N>`)
/// + the initial value. Subsequent `x.text()` calls emit reactive Text
/// using the synth id; `x.set(v)` calls inside closures get rewritten to
/// `setText(synth_id, v)` calls (the runtime's `perry_arkts_set_text`
/// already coerces non-string args via `js_jsvalue_to_string`).
pub(crate) struct StateBinding {
    pub(crate) synth_id: String,
    pub(crate) initial_str: String,
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
pub(crate) struct VisibilityBinding {
    /// Synth identifier — `vis_0`, `vis_1`, … one per unique target LocalId.
    pub(crate) synth_id: String,
    /// Initial visibility from module init: `true` = hidden by default.
    /// Determined by the LAST literal `widgetSetHidden(target, V)` seen in
    /// `module.init` (latest wins). When no module-init call is found,
    /// defaults to `false` (visible) — matches the fact that widgets are
    /// born visible in Perry.
    pub(crate) initial_hidden: bool,
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
pub(crate) struct ViewBuilder {
    /// Function id (matches `Function::id` in the HIR).
    pub(crate) func_id: perry_hir::types::FuncId,
    /// Function name (used for the synth view id when sanitized).
    // #854: stashed on the harvest instance for a future diagnostic pass; the
    // view id is currently derived inline at emit time, not read back here.
    #[allow(dead_code)]
    pub(crate) func_name: String,
    /// Module-level container LocalId that this function adds children to
    /// via the terminal `widgetAddChild(LocalGet(target_id), X)` call.
    // #854: kept for layout/diagnostics; the addChild target is resolved
    // inline during fold, not read off this field yet.
    #[allow(dead_code)]
    pub(crate) target_id: LocalId,
    /// Synth identifier for the target — `cv_<n>`. Stable across re-runs.
    pub(crate) target_synth: String,
    /// Synth view id for THIS function — sanitized function name. Used as
    /// both the `@State contentView_<target_synth>` field's expected value
    /// and the case-arm key in `applyContentViewUpdate`.
    pub(crate) view_id: String,
    /// Mutation group id used to keep this view's lifted addChild +
    /// modifier mutations grouped together in `fold_child_mutations`.
    pub(crate) group_id: u32,
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
pub(crate) enum Mutation {
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
pub(crate) struct MutationEntry {
    pub(crate) mutation: Mutation,
    pub(crate) condition: Option<MutationCondition>,
}

#[derive(Debug, Clone)]
pub(crate) struct MutationCondition {
    /// String-serialized condition expression; reused as the ArkUI
    /// predicate. e.g. `"this.text___state_0 === 'mobile'"`.
    pub(crate) cond_str: String,
    /// Which branch this mutation lives in.
    pub(crate) branch: Branch,
    /// Group key — same source-statement id, so all mutations from one
    /// `if` statement share a key and can be grouped at emit time.
    pub(crate) group: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Branch {
    Then,
    Else,
}
