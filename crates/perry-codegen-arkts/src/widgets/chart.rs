// This module is part of the perry-codegen-arkts crate. It was
// mechanically split out of the former monolithic lib.rs (issue
// #1100). Pure code move — no logic changes.
#![allow(clippy::too_many_arguments)]
use crate::*;

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
pub(crate) fn emit_chart(
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
pub(crate) fn fold_chart_mutations(
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
pub(crate) fn chart_draw_body(kind: i64) -> String {
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

pub(crate) fn chart_draw_bar() -> String {
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

pub(crate) fn chart_draw_line() -> String {
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

pub(crate) fn chart_draw_pie() -> String {
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
