// This module is part of the perry-codegen-arkts crate. It was
// mechanically split out of the former monolithic lib.rs (issue
// #1100). Pure code move — no logic changes.
#![allow(clippy::too_many_arguments)]
use crate::*;

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
pub(crate) fn collect_mutations(
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

pub(crate) fn collect_mutations_in_stmt(
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
pub(crate) fn collect_mutations_in_expr(
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
        "textSetTextAlignment" => {
            // Issue #3621. Canonical Perry/AppKit alignment values map to
            // ArkUI's direction-relative `TextAlign` enum: 0=left→Start,
            // 1=right→End, 2=center→Center, 3=justified→JUSTIFY,
            // 4=natural→Start (follows the locale's writing direction).
            let Some(n) = numeric_arg_resolved(&args[1..], 0, bindings) else {
                return;
            };
            let variant = match n as i64 {
                1 => "End",
                2 => "Center",
                3 => "JUSTIFY",
                _ => "Start",
            };
            push_mut(
                Mutation::Modifier(format!(".textAlign(TextAlign.{})", variant)),
                out,
                cond,
            );
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
pub(crate) fn emit_modifier_mutations(muts: &[MutationEntry]) -> String {
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

/// Issue #408 — return the list of effective AddChild expressions for a
/// widget local, after honoring `ClearChildren` (which drops earlier
/// AddChild entries from the same condition group + branch).
///
/// Returns `(unconditional_children, conditional_groups)` where each
/// conditional_group is `(cond_str, then_children, else_children)`.
/// All three lists hold the user-supplied child Expr references in
/// source order.
#[allow(clippy::type_complexity)]
pub(crate) fn fold_child_mutations(
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

/// Issue #408 — emit a string of ArkUI children (already-rendered) for
/// the unconditional + conditional groups produced by fold_child_mutations.
/// Each conditional group emits as `if (cond) { thenA(); thenB(); } else { elseA(); }`,
/// inlined into the parent's body alongside the unconditional siblings.
///
/// Caller is responsible for indenting the result appropriately. Returns
/// an empty string if no children registered, so callers can short-circuit.
#[allow(clippy::too_many_arguments)]
pub(crate) fn emit_mutation_children(
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

/// Helper: push a comment Expr by smuggling it through a sentinel that
/// downstream emitters recognize. Today comments are dropped at child
/// emission since `emit_widget` requires a real widget call. We instead
/// surface comments at the post-mutation modifier-chain emit site.
/// This helper is here as the API but currently a no-op — kept to make
/// the design explicit.
pub(crate) fn unconditional_push_comment(_out: &mut Vec<Expr>, _comment: &str) {
    // Intentionally empty: comments are surfaced via emit_modifier_mutations'
    // post-emit `// ...` lines, not as fake child widgets.
}
