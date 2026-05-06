// Type declarations for perry/ui — Perry's native UI framework
// These types are auto-written by `perry init` / `perry types` so IDEs
// and tsc can resolve `import { ... } from "perry/ui"`.

declare const __widget: unique symbol;

/**
 * Instance methods available on every Widget handle. The handle itself is
 * a NaN-boxed number at runtime; the compiler lowers these method calls to
 * `perry_ui_widget_*` FFI entries.
 */
export interface WidgetMethods {
    /**
     * Animate the widget's opacity to `target` over `durationSecs` seconds.
     * The animation starts from the widget's current opacity.
     */
    animateOpacity(target: number, durationSecs: number): void;
    /**
     * Animate the widget's position by `(dx, dy)` pixels over `durationSecs`
     * seconds, relative to its current position.
     */
    animatePosition(dx: number, dy: number, durationSecs: number): void;
}

/** Opaque handle to a native UI widget. */
export type Widget = number & WidgetMethods & { readonly [__widget]: void };

/**
 * 2D drawing methods available on a Canvas handle. Mirrors the stateful
 * HTML5 Canvas 2D context API. Color state is set with `setFillColor` /
 * `setStrokeColor` / `setLineWidth` before issuing draw calls.
 *
 * Native platform support: stubs exist on all targets; GTK4, Android, and
 * Windows have the path/gradient primitives. Full rasterization of the
 * stateful API is tracked in perry-ui-test (`U` → in progress).
 */
export interface CanvasMethods {
    setFillColor(r: number, g: number, b: number, a: number): void;
    setStrokeColor(r: number, g: number, b: number, a: number): void;
    setLineWidth(width: number): void;
    fillRect(x: number, y: number, width: number, height: number): void;
    strokeRect(x: number, y: number, width: number, height: number): void;
    clearRect(x: number, y: number, width: number, height: number): void;
    beginPath(): void;
    moveTo(x: number, y: number): void;
    lineTo(x: number, y: number): void;
    arc(x: number, y: number, radius: number, startAngle: number, endAngle: number): void;
    closePath(): void;
    fill(): void;
    stroke(): void;
    fillText(text: string, x: number, y: number): void;
    setFont(spec: string): void;
}

/** Opaque handle to a Canvas widget. Extends Widget with 2D drawing methods. */
export type Canvas = Widget & CanvasMethods;

/** Reactive state container. Generic over the value type it holds. */
export interface State<T = number> {
    /** Current value of the state. */
    readonly value: T;
    /** Set the state value and trigger bound UI updates. */
    set(value: T): void;
}

/** Native window with instance methods. */
export interface Window {
    show(): void;
    hide(): void;
    close(): void;
    setBody(body: Widget): void;
    setSize(width: number, height: number): void;
    onFocusLost(callback: () => void): void;
}

/**
 * RGBA color in 0..=1 floats.
 *
 * The FFI surface uses 4-float colors throughout (`(r, g, b, a)`), and the
 * `style: { ... }` API of issue #185 will accept these objects directly.
 * The string forms (CSS hex / rgb / hsl / named colors) are parsed by
 * `parseColor` from the `perry-styling` companion package.
 */
export interface PerryColor {
    r: number;
    g: number;
    b: number;
    a?: number;
}

/**
 * Cross-platform style descriptor for issue #185 Phase C.
 *
 * **Status:** Type surface only — the inline `Button("Save", onPress, { style })`
 * codegen pass is in development. Right now use the individual setters
 * (`widgetSetBackgroundColor`, `widgetSetBorderColor`, `setCornerRadius`,
 * etc.) and pass `StyleProps` shapes around as plain typed objects for
 * IDE autocomplete and future-compatibility — the same prop names map
 * 1:1 to setters, so code authored against `StyleProps` today will keep
 * working once the inline syntax lands.
 *
 * Every prop here is currently wired on macOS / iOS / tvOS / visionOS /
 * watchOS / Android / Web; GTK4 has 4 gaps (issue #202) and Windows has
 * 5 deferred-paint stubs (shadow, opacity, borders, text decoration —
 * tracked in `crates/perry-ui/src/styling_matrix.rs`).
 *
 * Color values currently accept either a raw `PerryColor` object or a
 * CSS string (hex / rgb / hsl / named) — string parsing happens at
 * widget-construction time via `parseColor`.
 */
export interface StyleProps {
    /** Solid background color. Maps to `widgetSetBackgroundColor`. */
    backgroundColor?: string | PerryColor;

    /** Foreground / text color. Maps to `textSetColor` (text widgets) or
     *  `buttonSetTextColor` (buttons). */
    color?: string | PerryColor;

    /** Border color. Maps to `widgetSetBorderColor`. Joint state with
     *  `borderWidth` — sets a default 1px width if width isn't also
     *  provided, so a single setter still produces a visible border. */
    borderColor?: string | PerryColor;

    /** Border width in pixels. Maps to `widgetSetBorderWidth`. Joint
     *  state with `borderColor`. */
    borderWidth?: number;

    /** Corner radius in pixels. Maps to `setCornerRadius`. */
    borderRadius?: number;

    /** Padding. A single number applies to all four sides; an object
     *  picks per-side (top / right / bottom / left). Maps to
     *  `widgetSetEdgeInsets`. */
    padding?: number | {
        top?: number;
        right?: number;
        bottom?: number;
        left?: number;
    };

    /** Font size in points. Maps to `textSetFontSize`. */
    fontSize?: number;

    /** Font weight (numeric, e.g. 400 = regular, 700 = bold). Maps to
     *  `textSetFontWeight`. */
    fontWeight?: number;

    /** Font family name (e.g. "Menlo", "system", "monospaced"). Maps to
     *  `textSetFontFamily`. */
    fontFamily?: string;

    /** Opacity in 0.0..=1.0. Maps to `widgetSetOpacity`. */
    opacity?: number;

    /** Drop shadow. Maps to `widgetSetShadow`. `offset.y` is positive
     *  downward, matching CSS `box-shadow` and CALayer semantics. */
    shadow?: {
        color?: string | PerryColor;
        blur?: number;
        offsetX?: number;
        offsetY?: number;
    };

    /** Text decoration. Maps to `textSetDecoration`. */
    textDecoration?: "none" | "underline" | "strikethrough";

    /** Linear gradient. Maps to `widgetSetBackgroundGradient`. */
    gradient?: {
        angle: number;
        stops: Array<string | PerryColor>;
    };

    /** Whether the widget is hidden from layout. Maps to `widgetSetHidden`. */
    hidden?: boolean;

    /** Whether the widget accepts user interaction. Maps to
     *  `widgetSetEnabled`. */
    enabled?: boolean;

    /** Hover tooltip text. Maps to `widgetSetTooltip`. */
    tooltip?: string;
}

// ---------------------------------------------------------------------------
// Constructors
// ---------------------------------------------------------------------------

/** Create and run a native application. Blocks the main thread. */
export function App(config: {
    title: string;
    width: number;
    height: number;
    icon?: string;
    body: Widget;
}): void;

/** Vertical stack layout. */
export function VStack(children: Widget[]): Widget;
export function VStack(spacing: number, children: Widget[]): Widget;

/** Horizontal stack layout. */
export function HStack(children: Widget[]): Widget;
export function HStack(spacing: number, children: Widget[]): Widget;

/**
 * Static text label.
 *
 * Phase 2 v3 Option 2: passing a second `id` arg makes the Text
 * reactive — its content is bound to a `@State` field on the page.
 * Update from inside any closure via `setText(id, newValue)` to
 * trigger a UI rerender:
 *
 *     let count = 0;
 *     App({ body: VStack([
 *       Text("Count: 0", "counter"),
 *       Button("+", () => { count++; setText("counter", `Count: ${count}`); })
 *     ])});
 */
export function Text(content: string, id?: string): Widget;

/** Clickable button. */
export function Button(label: string, onPress: () => void): Widget;

/**
 * Update a reactive `Text(initial, id)` widget's content.
 *
 * On `--target harmonyos`, queues a `(id, value)` update that the
 * auto-emitted .ets onClick drains after the closure returns,
 * assigning to the matching `@State text_<id>: string` field — ArkUI
 * rerenders the Text. On other platforms this is currently a no-op.
 *
 * `id` must match exactly what was passed as the second arg to the
 * `Text()` call you want to update. Calls to `setText` for unregistered
 * ids are silently ignored (no Text widget binds to them).
 */
export function setText(id: string, value: string): void;

/**
 * A reactive state container. Wraps a value of type `T` and provides:
 *
 * - `value` / `get()` — read the current value
 * - `set(v)` — update the value and trigger any bound UI to rerender
 * - `text()` — return a reactive `Text` widget bound to this state
 *
 * Phase 2 v6 implementation desugars to the existing v3.2 setText/Text
 * reactive binding at compile time (perry-codegen-arkts) — each
 * `state(initial)` declaration registers a synthetic id; `state.text()`
 * emits a reactive `Text(initial.toString(), "<synth_id>")`, and
 * `state.set(v)` rewrites to `setText("<synth_id>", String(v))` inside
 * any closure body. No runtime FFIs change vs v3.2.
 *
 * Example:
 *
 *     import { App, VStack, Button, state } from "perry/ui";
 *
 *     const count = state(0);
 *
 *     App({ body: VStack([
 *       count.text(),
 *       Button("+", () => count.set(count.get() + 1)),
 *     ])});
 *
 * Cross-platform: harvest detection only fires on `--target harmonyos`
 * today. On other platforms the runtime side is a plain holder object —
 * `set()` updates the value but no UI binding fires (deferred to v6.5).
 */
export interface State<T> {
    readonly value: T;
    get(): T;
    set(value: T): void;
    text(): Widget;
}

export function state<T>(initial: T): State<T>;

/**
 * Show a transient banner/toast on supported platforms.
 *
 * On `--target harmonyos`, calls `promptAction.showToast({message})` via
 * a queue drained after each Button onClick — the message pops at the
 * bottom of the screen for ~3 seconds. On other platforms this is
 * currently a no-op (Phase 2 v3 only wires HarmonyOS); follow-ups will
 * route to NSAlert/UIAlertController/system notifications.
 */
export function showToast(message: string): void;

/** Single-line text input. */
export function TextField(placeholder: string, onChange: (value: string) => void): Widget;

/** Multi-line text input. */
export function TextArea(placeholder: string, onChange: (value: string) => void): Widget;

/** Password / secure text input. */
export function SecureField(placeholder: string, onChange: (value: string) => void): Widget;

/** Boolean toggle switch. */
export function Toggle(label: string, onChange: (value: boolean) => void): Widget;

/** Numeric slider. */
export function Slider(min: number, max: number, onChange: (value: number) => void): Widget;

/** Scrollable container. */
export function ScrollView(): Widget;

/** Flexible space. */
export function Spacer(): Widget;

/** Visual separator line. */
export function Divider(): Widget;

/** Indeterminate or determinate progress indicator. */
export function ProgressView(): Widget;

/** Depth stack (overlapping children). */
export function ZStack(): Widget;

/**
 * 2D drawing canvas. Returns a Canvas handle with drawing methods.
 * Compile-and-link supported on all native targets; visual rendering of the
 * stateful color/path API is tracked in perry-ui-test.
 */
export function Canvas(width: number, height: number): Canvas;

/** Dropdown picker. */
export function Picker(onChange: (index: number) => void): Widget;

/** Form section with a title. */
export function Section(title: string): Widget;

/**
 * Form section with a title and inline children. Convenience overload — emits
 * a labeled vertical group on HarmonyOS (`Column({space:4}) { Text(title); ... }`)
 * and falls through to the imperative `Section(title)` + `widgetAddChild`
 * pattern on every other platform.
 */
export function Section(title: string, children: Widget[]): Widget;

/**
 * Navigation stack for multi-page apps.
 *
 * **HarmonyOS Phase 2 v11**: state-driven shape — pass a `state<string>(...)`
 * holding the active route name, plus an array of `{ name, body }` route
 * specs. Navigation is just `route.set("detail")` from any closure; the
 * v6 setText drain queue swaps the visible branch.
 *
 * Example:
 * ```ts
 * const route = state("home");
 * App({
 *   body: NavStack(route, [
 *     { name: "home", body: VStack([
 *       Text("Welcome"),
 *       Button("Go to detail", () => route.set("detail")),
 *     ]) },
 *     { name: "detail", body: VStack([
 *       Text("Detail page"),
 *       Button("Back", () => route.set("home")),
 *     ]) },
 *   ]),
 * });
 * ```
 *
 * Native ArkUI `Navigation` + `NavPathStack` integration (hardware-back
 * gesture, `pageStack.pop()`) is the v11.5 follow-up. The state-driven
 * shape works today on every platform via the existing v6 + v3.2 bridge.
 *
 * The no-arg form is the legacy stub from Phase 1 — keep using it on
 * platforms that haven't shipped the multi-page emission yet.
 */
export function NavStack(): Widget;
export function NavStack(
  active: State<string>,
  routes: { name: string; body: Widget }[],
): Widget;

/**
 * Tab bar container.
 *
 * **Platform support:** fully implemented on iOS, tvOS, Android. On macOS,
 * Windows, and GTK4 this is currently a no-op stub (returns handle 0) and
 * watchOS silently substitutes a VStack. Prefer `NavStack` or a custom
 * segmented control on desktop until these land.
 */
export function TabBar(onChange: (index: number) => void): Widget;

/** Create a native window. */
export function Window(title: string, width: number, height: number): Window;

/**
 * Virtualized vertical list. `render(index)` is invoked lazily — on macOS,
 * backed by NSTableView, so only rows currently within the visible rect are
 * realized. Use this for lists of hundreds or thousands of items; for small
 * lists a plain `VStack` + `ForEach` is simpler.
 *
 * Row height defaults to 44pt (uniform). Override with `lazyvstackSetRowHeight`.
 * Call `lazyvstackUpdate(handle, newCount)` when the underlying data changes.
 */
export function LazyVStack(count: number, render: (index: number) => Widget): Widget;
export function lazyvstackUpdate(handle: Widget, count: number): void;
export function lazyvstackSetRowHeight(handle: Widget, height: number): void;

/** Vertical split view. */
export function SplitView(): Widget;

/** Image from a file path. */
export function ImageFile(path: string): Widget;

/** Image from a system symbol name (SF Symbols). */
export function ImageSymbol(name: string): Widget;

/** VStack with built-in edge insets. */
export function VStackWithInsets(spacing: number, top: number, left: number, bottom: number, right: number): Widget;

/** HStack with built-in edge insets. */
export function HStackWithInsets(spacing: number, top: number, left: number, bottom: number, right: number): Widget;

/** Reactive state container constructor. */
export function State<T>(initial: T): State<T>;

/**
 * Re-render a container's children from a count-driven state.
 *
 * `count` is a `State<number>` representing how many items to render.
 * Whenever the count changes, `render(i)` is invoked for `i = 0..count-1`
 * and the returned widgets replace the container's children. Pair this with
 * a separate array state that you keep in sync with the count.
 */
export function ForEach(count: State<number>, render: (index: number) => Widget): Widget;

// ---------------------------------------------------------------------------
// Text setters
// ---------------------------------------------------------------------------

export function textSetString(widget: Widget, text: string): void;
export function textSetColor(widget: Widget, r: number, g: number, b: number, a: number): void;
export function textSetFontSize(widget: Widget, size: number): void;
export function textSetFontWeight(widget: Widget, size: number, weight: number): void;
export function textSetFontFamily(widget: Widget, family: string): void;
export function textSetWraps(widget: Widget, maxWidth: number): void;
export function textSetSelectable(widget: Widget, selectable: number): void;
/**
 * Set text decoration on a Text widget (issue #185 Phase B).
 * `decoration`: 0 = none, 1 = underline, 2 = strikethrough.
 * Wired on every backend except Windows, which stores the value but
 * doesn't yet rebuild HFONTs to apply it visually.
 */
export function textSetDecoration(widget: Widget, decoration: number): void;

// ---------------------------------------------------------------------------
// Button setters
// ---------------------------------------------------------------------------

export function buttonSetBordered(widget: Widget, bordered: number): void;
export function buttonSetTitle(widget: Widget, title: string): void;
export function buttonSetTextColor(widget: Widget, r: number, g: number, b: number, a: number): void;
export function buttonSetImage(widget: Widget, symbolName: string): void;
export function buttonSetImagePosition(widget: Widget, position: number): void;
export function buttonSetContentTintColor(widget: Widget, r: number, g: number, b: number, a: number): void;

// ---------------------------------------------------------------------------
// Generic widget operations
// ---------------------------------------------------------------------------

export function widgetAddChild(parent: Widget, child: Widget): void;
export function widgetAddChildAt(parent: Widget, child: Widget, index: number): void;
export function widgetClearChildren(widget: Widget): void;
export function widgetRemoveChild(parent: Widget, child: Widget): void;
export function widgetReorderChild(widget: Widget, fromIndex: number, toIndex: number): void;
export function widgetSetWidth(widget: Widget, width: number): void;
export function widgetSetHeight(widget: Widget, height: number): void;
export function widgetSetHugging(widget: Widget, priority: number): void;
export function widgetSetHidden(widget: Widget, hidden: number): void;
export function widgetMatchParentWidth(widget: Widget): void;
export function widgetMatchParentHeight(widget: Widget): void;
export function widgetSetBackgroundColor(widget: Widget, r: number, g: number, b: number, a: number): void;
export function widgetSetBackgroundGradient(
    widget: Widget,
    r1: number, g1: number, b1: number, a1: number,
    r2: number, g2: number, b2: number, a2: number,
    angle: number,
): void;
export function widgetSetOpacity(widget: Widget, opacity: number): void;
export function widgetSetEnabled(widget: Widget, enabled: number): void;
export function widgetSetTooltip(widget: Widget, text: string): void;
/**
 * Attach a rich (widget-tree) tooltip to `widget`. The `content` widget
 * is presented in a borderless floating panel after `hoverDelayMs` of
 * mouse hover (default 500ms when ≤0). Currently wired on macOS via
 * `NSPanel` + `NSTrackingArea`; iOS/tvOS/visionOS/watchOS/Android/
 * Windows/GTK4 stub the FFI so calls compile but produce no overlay
 * yet — track in issue #479. For plain-text tooltips, prefer
 * `widgetSetTooltip` so the OS handles VoiceOver / a11y correctly.
 */
export function widgetSetRichTooltip(widget: Widget, content: Widget, hoverDelayMs: number): void;

// ---------------------------------------------------------------------------
// Combobox (issue #475) — editable text field with a filterable dropdown of
// suggestions. macOS uses NSComboBox with `setCompletes:YES` for as-you-type
// completion; iOS / tvOS / visionOS / watchOS / Android / Windows / GTK4
// stub the FFI today (text field falls back to a plain editable field).
// `onChange` fires with the current string value when the user picks from
// the dropdown or commits free text via Return.
// ---------------------------------------------------------------------------

export function Combobox(initial: string, onChange: (value: string) => void): Widget;
export function comboboxAddItem(widget: Widget, value: string): void;
export function comboboxSetValue(widget: Widget, value: string): void;
export function comboboxGetValue(widget: Widget): string;

// ---------------------------------------------------------------------------
// TreeView / outline view (issue #480) — hierarchical disclosure list. Build
// the topology bottom-up via TreeNode + treeNodeAddChild, then mount it via
// TreeView(rootNode, onSelect). macOS uses NSOutlineView; iOS/tvOS/visionOS/
// watchOS/Android/Windows/GTK4 stub the FFI today (selection always returns
// undefined). Out of scope this iteration: drag-and-drop, lazy children
// loader, multi-select, inline rename, icons.
// ---------------------------------------------------------------------------

export function TreeNode(id: string, label: string): Widget;
export function treeNodeAddChild(parent: Widget, child: Widget): void;
export function TreeView(rootNode: Widget, onSelect: (id: string) => void): Widget;
export function treeViewExpandAll(widget: Widget): void;
export function treeViewCollapseAll(widget: Widget): void;
export function treeViewGetSelectedId(widget: Widget): string;

// ---------------------------------------------------------------------------
// Calendar widget (issue #481, v1) — month-grid date picker. macOS uses
// NSDatePicker in graphical / clock-and-calendar style, elements limited
// to year-month-day so the clock face is hidden. iOS / tvOS / visionOS /
// watchOS / Android / Windows / GTK4 stub the FFI today (returns 0 on
// create, undefined on get-date).
//
// Out of scope v1: event blocks / dot indicators, week / day views,
// drag-to-create / drag-to-resize, overlap layout. The base widget
// plumbing lands first; richer modes follow in #481 follow-ups.
//
// `onChange` receives the selected date as an ISO `yyyy-MM-dd` string
// (POSIX-locale formatter, stable across user locales).
// ---------------------------------------------------------------------------

export function Calendar(year: number, month: number, onChange: (isoDate: string) => void): Widget;
export function calendarSetDate(widget: Widget, year: number, month: number, day: number): void;
export function calendarGetSelectedDate(widget: Widget): string;

// ---------------------------------------------------------------------------
// Chart widget (issue #474, v1) — line / bar / pie via CoreGraphics on macOS.
// `kind` is 0=line, 1=bar, 2=pie. iOS / tvOS / visionOS / watchOS / Android /
// Windows / GTK4 stub the FFI today (returns 0 on create, no-op on data
// updates). Apple Charts framework / SwiftUI Charts integration on iOS 16+ is
// a follow-up.
//
// Out of scope v1 (per #474 scope): multi-series line, grouped/stacked bars,
// donut, area, axis labels, legend, hover/tap tooltips, animated transitions,
// color theming. The base widget plumbing lands first; richer modes follow
// once the surface is used in TS apps.
// ---------------------------------------------------------------------------

export function Chart(kind: number, width: number, height: number): Widget;
export function chartAddDataPoint(widget: Widget, label: string, value: number): void;
export function chartClearData(widget: Widget): void;
export function chartSetTitle(widget: Widget, title: string): void;
export function chartReload(widget: Widget): void;

// ---------------------------------------------------------------------------
// Command palette (issue #477, v1) — ⌘K-style fuzzy command launcher.
// macOS: floating NSPanel with NSSearchField + NSTableView. iOS / tvOS /
// visionOS / watchOS / Android / Windows / GTK4 stub the FFI today.
//
// Out of scope v1: fuzzy ranking (substring match for now), recent /
// frequently-used boost, async command sources, command groups / section
// headers, OS-native menu-bar integration. Bind `commandPaletteShow()`
// to ⌘K via `addKeyboardShortcut` to wire the default hotkey.
// ---------------------------------------------------------------------------

export function commandPaletteRegister(
    id: string,
    label: string,
    subtitle: string,
    onRun: () => void,
): void;
export function commandPaletteUnregister(id: string): void;
export function commandPaletteClear(): void;
export function commandPaletteShow(): void;
export function commandPaletteHide(): void;

// ---------------------------------------------------------------------------
// Map widget (issue #517) — MKMapView on macOS / iOS / visionOS, stubs
// elsewhere. Pin styling is the default red drop-pin (MKPointAnnotation);
// custom annotation views are a follow-up.
//
// `mapType` enum: 0=standard, 1=satellite, 2=hybrid (matches MKMapType).
// `latSpan`/`lonSpan` are degrees — smaller = more zoomed in. A 0.05 span
// is roughly city-block scale; 1.0 span is a whole region.
//
// Out of scope this iteration: user-location tracking, custom annotation
// views, polylines/polygons, route directions, region-change callbacks.
// ---------------------------------------------------------------------------

export function MapView(width: number, height: number): Widget;
export function mapViewSetRegion(
    widget: Widget,
    lat: number,
    lon: number,
    latSpan: number,
    lonSpan: number,
): void;
export function mapViewAddPin(widget: Widget, lat: number, lon: number, title: string): void;
export function mapViewClearPins(widget: Widget): void;
export function mapViewSetMapType(widget: Widget, style: number): void;

// ---------------------------------------------------------------------------
// PDF viewer widget (issue #516) — wraps `PDFView` from PDFKit on macOS.
// `loadFile` returns 1 on success, 0 on failure (couldn't open path or
// PDFKit unavailable). Scale is a multiplier — 1.0 = 100%.
//
// Out of scope this iteration: programmatic PDF generation
// (`Pdf.create({...}).save(path)` style API), text-search highlighting,
// annotation editing, print-friendly rendering. Filed back into #516
// for follow-ups.
// ---------------------------------------------------------------------------

export function PdfView(width: number, height: number): Widget;
export function pdfViewLoadFile(widget: Widget, path: string): number;
export function pdfViewGetPageCount(widget: Widget): number;
export function pdfViewGoToPage(widget: Widget, pageIndex: number): void;
export function pdfViewGetCurrentPage(widget: Widget): number;
export function pdfViewSetScale(widget: Widget, scale: number): void;

// ---------------------------------------------------------------------------
// Rich text editor (issue #478, v1) — NSTextView with NSAttributedString
// storage. Plain-text + HTML round-trip cover persistence; bold/italic/
// underline cover inline formatting via NSResponder actions. Markdown
// round-trip, block formatting (headings/lists/blockquotes/code blocks),
// configurable toolbar, paste handling are #478 follow-ups.
// macOS: native NSTextView. iOS / tvOS / visionOS / watchOS / Android /
// Windows / GTK4 stub the FFI today.
// `setHtml` returns 1 on success, 0 on failure (e.g. malformed HTML).
// ---------------------------------------------------------------------------

export function RichTextEditor(
    width: number,
    height: number,
    onChange: (text: string) => void,
): Widget;
export function richTextSetString(widget: Widget, text: string): void;
export function richTextGetString(widget: Widget): string;
export function richTextSetHtml(widget: Widget, html: string): number;
export function richTextGetHtml(widget: Widget): string;
export function richTextToggleBold(widget: Widget): void;
export function richTextToggleItalic(widget: Widget): void;
export function richTextToggleUnderline(widget: Widget): void;
export function widgetSetControlSize(widget: Widget, size: number): void;
export function widgetSetEdgeInsets(widget: Widget, top: number, left: number, bottom: number, right: number): void;
export function widgetSetBorderColor(widget: Widget, r: number, g: number, b: number, a: number): void;
export function widgetSetBorderWidth(widget: Widget, width: number): void;
/**
 * Set a drop shadow on a widget. (r, g, b, a) is the shadow color in 0–1
 * — alpha rides on the layer's shadowOpacity so a non-1 alpha doesn't
 * double-multiply via the color's alpha. `blur` is the shadow radius.
 * `(offsetX, offsetY)` is the shadow offset, with positive y = downward
 * (matches HTML `box-shadow: x y blur color`). Issue #185 Phase B —
 * currently wired on macOS / iOS / tvOS / visionOS / watchOS; Android,
 * GTK4, Windows, Web closures coming next.
 */
export function widgetSetShadow(
    widget: Widget,
    r: number, g: number, b: number, a: number,
    blur: number, offsetX: number, offsetY: number
): void;
export function widgetSetContextMenu(widget: Widget, menu: Widget): void;
export function widgetAddOverlay(widget: Widget, overlay: Widget): void;
export function widgetSetOverlayFrame(widget: Widget, x: number, y: number, width: number, height: number): void;
export function widgetSetOnClick(widget: Widget, callback: () => void): void;
export function widgetSetOnHover(widget: Widget, callback: () => void): void;
export function widgetSetOnDoubleClick(widget: Widget, callback: () => void): void;
/** Animate opacity to `target` over `durationSecs` seconds. */
export function widgetAnimateOpacity(widget: Widget, target: number, durationSecs: number): void;
/** Animate position by `(dx, dy)` pixels over `durationSecs` seconds. */
export function widgetAnimatePosition(widget: Widget, dx: number, dy: number, durationSecs: number): void;

/** Set padding (edge insets) on a widget. */
export function setPadding(widget: Widget, top: number, left: number, bottom: number, right: number): void;

/** Set corner radius on a widget. */
export function setCornerRadius(widget: Widget, radius: number): void;

// ---------------------------------------------------------------------------
// TextField / TextArea
// ---------------------------------------------------------------------------

export function textfieldSetString(widget: Widget, text: string): void;
export function textfieldGetString(widget: Widget): string;
export function textfieldFocus(widget: Widget): void;
export function textfieldBlurAll(): void;
export function textfieldSetNextKeyView(widget: Widget, next: Widget): void;
export function textfieldSetOnSubmit(widget: Widget, callback: () => void): void;
export function textfieldSetOnFocus(widget: Widget, callback: () => void): void;
export function textfieldSetBackgroundColor(widget: Widget, r: number, g: number, b: number, a: number): void;
export function textfieldSetBorderless(widget: Widget, borderless: number): void;
export function textfieldSetFontSize(widget: Widget, size: number): void;
export function textfieldSetTextColor(widget: Widget, r: number, g: number, b: number, a: number): void;
export function textareaSetString(widget: Widget, text: string): void;
export function textareaGetString(widget: Widget): string;

// ---------------------------------------------------------------------------
// ScrollView
// ---------------------------------------------------------------------------

export function scrollviewSetChild(scrollView: Widget, child: Widget): void;
export function scrollViewSetChild(scrollView: Widget, child: Widget): void;
// Issue #391: lowercase-v aliases for the remaining ScrollView setters /
// getter so coverage is consistent across all five functions. The
// historical `scrollviewSetOffset(scrollView, y)` 1-arg-y form is still
// dispatched (for back-compat with code targeting older Perry versions),
// but the type stub only exposes the modern 2-arg `(x, y)` shape — old
// code calling `scrollviewSetOffset(sv, 100)` will need to migrate to
// `scrollviewSetOffset(sv, 0, 100)` or `scrollViewScrollTo(sv, 0, 100)`.
export function scrollviewGetOffset(scrollView: Widget): number;
export function scrollViewGetOffset(scrollView: Widget): number;
export function scrollviewSetOffset(scrollView: Widget, x: number, y: number): void;
export function scrollViewSetOffset(scrollView: Widget, x: number, y: number): void;
export function scrollviewScrollTo(scrollView: Widget, x: number, y: number): void;
export function scrollViewScrollTo(scrollView: Widget, x: number, y: number): void;

/**
 * Issue #390: native pull-to-refresh.
 *
 * Attach a refresh control to a ScrollView. The callback fires when the
 * user pulls down past the threshold; call `scrollviewEndRefreshing` to
 * dismiss the spinner once the refresh completes.
 *
 * Backed by `UIRefreshControl` on iOS / iPadOS / tvOS / visionOS,
 * `SwipeRefreshLayout` on Android, no-op on macOS / GTK4 / Windows /
 * watchOS / Web (the OS-provided pull gesture only exists on touch
 * platforms — desktop apps should add an explicit "Refresh" button).
 */
export function scrollviewSetRefreshControl(scrollView: Widget, onPull: () => void): void;
export function scrollViewSetRefreshControl(scrollView: Widget, onPull: () => void): void;
export function scrollviewEndRefreshing(scrollView: Widget): void;
export function scrollViewEndRefreshing(scrollView: Widget): void;

// ---------------------------------------------------------------------------
// Stack layout
// ---------------------------------------------------------------------------

export function stackSetAlignment(widget: Widget, alignment: number): void;
export function stackSetDistribution(widget: Widget, distribution: number): void;
export function stackSetDetachesHidden(widget: Widget, detach: number): void;

// ---------------------------------------------------------------------------
// State management (free-function API)
// ---------------------------------------------------------------------------

export function stateCreate(initial: number): State;
export function stateGet(state: State): number;
export function stateSet(state: State, value: number): void;
export function stateOnChange(state: State, callback: (value: number) => void): void;
export function stateBindTextNumeric(state: State, text: Widget, prefix: string, suffix: string): void;
export function stateBindSlider(state: State, slider: Widget): void;
export function stateBindToggle(state: State, toggle: Widget): void;
export function stateBindVisibility(state: State, showWidget: Widget, hideWidget: Widget): void;
export function stateBindTextfield(state: State<string>, textfield: Widget): void;

// ---------------------------------------------------------------------------
// Image
// ---------------------------------------------------------------------------

export function imageSetSize(image: Widget, width: number, height: number): void;
export function imageSetTint(image: Widget, r: number, g: number, b: number, a: number): void;

// ---------------------------------------------------------------------------
// ProgressView
// ---------------------------------------------------------------------------

export function progressviewSetValue(widget: Widget, value: number): void;

// ---------------------------------------------------------------------------
// Menu
// ---------------------------------------------------------------------------

export function menuCreate(): Widget;
export function menuAddItem(menu: Widget, title: string, callback: () => void): void;
export function menuAddSeparator(menu: Widget): void;
export function menuAddSubmenu(menu: Widget, title: string, submenu: Widget): void;
export function menuClear(menu: Widget): void;
export function menuAddItemWithShortcut(menu: Widget, title: string, shortcut: string, callback: () => void): void;
export function menuAddStandardAction(menu: Widget, action: string, title: string, shortcut: string): void;
export function menuBarCreate(): Widget;
export function menuBarAddMenu(menuBar: Widget, title: string, menu: Widget): void;
export function menuBarAttach(menuBar: Widget): void;

// ---------------------------------------------------------------------------
// Tray icon (system tray / menu-bar / notification area) — issue #490
// ---------------------------------------------------------------------------

/**
 * Create a system tray icon. Returns a tray handle.
 *
 * Per-platform behaviour:
 * - **macOS**: `NSStatusItem` in the menu bar (top-right of the screen).
 * - **Windows**: `Shell_NotifyIconW` in the notification area (bottom-right).
 * - **Linux/GTK4**: `StatusNotifierItem` (KSNI) over D-Bus — works on KDE,
 *   GNOME-with-extension, XFCE, Plasma. Logs a warning and no-ops on plain
 *   Wayland sessions without a status notifier host.
 * - **iOS / tvOS / visionOS / watchOS / Android / HarmonyOS**: no-op stub
 *   (these platforms have no tray concept).
 *
 * `iconPath` is a filesystem path to a PNG (or `.icns` on macOS, `.ico` on
 * Windows). Pass `""` to use a default placeholder.
 */
export function trayCreate(iconPath: string): Widget;

/** Update the tray icon image. */
export function traySetIcon(tray: Widget, iconPath: string): void;

/** Set the tooltip text shown when the user hovers the icon. */
export function traySetTooltip(tray: Widget, tooltip: string): void;

/**
 * Attach a context menu (built with `menuCreate` / `menuAddItem`) to the
 * tray icon. Right-click — or left-click on macOS — opens the menu.
 */
export function trayAttachMenu(tray: Widget, menu: Widget): void;

/**
 * Register a callback that fires on left-click of the tray icon. On macOS
 * the click opens the attached menu directly, so this handler is mostly
 * useful on Windows and Linux for "show the main window" buttons.
 */
export function trayOnClick(tray: Widget, callback: () => void): void;

/** Remove the tray icon. */
export function trayDestroy(tray: Widget): void;

// ---------------------------------------------------------------------------
// NavigationStack
// ---------------------------------------------------------------------------

export function navstackPush(navStack: Widget, view: Widget, title: string): void;
export function navstackPop(navStack: Widget): void;

// ---------------------------------------------------------------------------
// Picker
// ---------------------------------------------------------------------------

export function pickerAddItem(picker: Widget, title: string): void;
export function pickerGetSelected(picker: Widget): number;
export function pickerSetSelected(picker: Widget, index: number): void;

// ---------------------------------------------------------------------------
// TabBar
// ---------------------------------------------------------------------------

export function tabbarAddTab(tabBar: Widget, title: string, content: Widget): void;
export function tabbarSetSelected(tabBar: Widget, index: number): void;

// ---------------------------------------------------------------------------
// Table (issue #192)
// ---------------------------------------------------------------------------

/**
 * Multi-column scrollable table. Real implementation lives on **macOS**
 * (`NSTableView` + `NSScrollView`); the **Web** target uses an HTML
 * `<table>`. Other targets (iOS, Android, Linux/GTK4, Windows, tvOS,
 * visionOS, watchOS) link no-op stubs so cross-platform code compiles
 * everywhere — the table renders nothing and `tableGetSelectedRow`
 * returns `-1`.
 *
 * The render callback receives `(row, col)` and must return a `Widget`
 * (typically `Text(...)`). The runtime resolves the returned handle as
 * the cell view, which lets cells render images, stacks, or composites
 * — not just plain strings.
 *
 * Compare with `LazyVStack` (`Layout`) which is single-column but works
 * on every native target today.
 */
export function Table(rowCount: number, colCount: number, renderCell: (row: number, col: number) => Widget): Widget;

/** Set the header title of column `col` (0-based). */
export function tableSetColumnHeader(table: Widget, col: number, title: string): void;

/** Set the pixel width of column `col` (0-based). */
export function tableSetColumnWidth(table: Widget, col: number, width: number): void;

/** Update the total row count and reload the visible cells. */
export function tableUpdateRowCount(table: Widget, count: number): void;

/** Register a row-select callback. The callback receives the 0-based row index. */
export function tableSetOnRowSelect(table: Widget, callback: (row: number) => void): void;

/** Return the index of the currently selected row, or `-1` if none. */
export function tableGetSelectedRow(table: Widget): number;

// ---------------------------------------------------------------------------
// Data-table sort + filter + multi-select extensions (issue #473).
// macOS: NSTableView.sortDescriptors + selectedRowIndexes (real impls);
// other platforms: stubs returning safe defaults.
// ---------------------------------------------------------------------------

/**
 * Register a sort callback fired when the user clicks a column header.
 * Installing the callback also turns on per-column sort indicators.
 * `ascending` is `1` for ascending, `0` for descending.
 */
export function tableSetOnSortChange(
    table: Widget,
    callback: (colIndex: number, ascending: number) => void,
): void;

/** Toggle multi-row selection (⌘ / ⇧ click). `allow` is `1` to enable. */
export function tableSetAllowsMultipleSelection(table: Widget, allow: number): void;

/** Number of currently-selected rows in a multi-select table. */
export function tableGetSelectedRowsCount(table: Widget): number;

/** Index of the n-th selected row (0-based). Returns `-1` for out-of-range. */
export function tableGetSelectedRowAt(table: Widget, n: number): number;

/**
 * Store a filter text on the table. Passive — the user's TS code reads
 * this back via `tableGetFilterText` and adjusts `tableUpdateRowCount`
 * accordingly. Keeps the active row-hiding logic on the user side so
 * any reactive store can drive it.
 */
export function tableSetFilterText(table: Widget, text: string): void;
export function tableGetFilterText(table: Widget): string;

// ---------------------------------------------------------------------------
// Camera (issue #191)
// ---------------------------------------------------------------------------

/**
 * Live camera preview widget. Real capture is implemented on **iOS**
 * (AVCaptureSession) and **Android** (Camera2). Other targets (macOS,
 * Linux/GTK4, Windows, Web) link no-op stubs so cross-platform code
 * compiles everywhere; on those targets `cameraSampleColor` returns `-1`
 * and the start/stop/freeze setters are no-ops.
 *
 * The camera does not start automatically — call `cameraStart()` to begin
 * capture. On iOS, the camera permission dialog is shown automatically on
 * first use.
 */
export function CameraView(): Widget;

/** Start the live camera feed. */
export function cameraStart(camera: Widget): void;

/** Stop the camera feed and release the capture session. */
export function cameraStop(camera: Widget): void;

/** Pause the live preview while keeping the capture session active. */
export function cameraFreeze(camera: Widget): void;

/** Resume the live preview after a freeze. */
export function cameraUnfreeze(camera: Widget): void;

/**
 * Sample the pixel color at normalized coordinates (`x`, `y` in 0–1).
 * Returns packed RGB as a number — `r * 65536 + g * 256 + b` — or `-1` if
 * no frame is available. The color is averaged over a 5x5 pixel region
 * around the sample point for noise reduction.
 *
 * To extract individual channels:
 * ```text
 * const r = Math.floor(rgb / 65536);
 * const g = Math.floor((rgb % 65536) / 256);
 * const b = Math.floor(rgb % 256);
 * ```
 */
export function cameraSampleColor(x: number, y: number): number;

/**
 * Register a tap handler on the camera view. The callback receives the
 * normalized coordinates of the tap location, which can be passed
 * directly to `cameraSampleColor()`.
 */
export function cameraSetOnTap(camera: Widget, callback: (x: number, y: number) => void): void;

// ---------------------------------------------------------------------------
// Sheet
// ---------------------------------------------------------------------------

export function sheetCreate(body: Widget, width: number, height: number): Widget;
export function sheetPresent(sheet: Widget): void;
export function sheetDismiss(sheet: Widget): void;

// ---------------------------------------------------------------------------
// SplitView / FrameSplit
// ---------------------------------------------------------------------------

export function splitViewAddChild(splitView: Widget, child: Widget): void;
export function frameSplitCreate(dividerPosition: number): Widget;
export function frameSplitAddChild(frameSplit: Widget, child: Widget): void;

// ---------------------------------------------------------------------------
// Toolbar
// ---------------------------------------------------------------------------

export function toolbarCreate(): Widget;
export function toolbarAddItem(toolbar: Widget, identifier: string, label: string, callback: () => void): void;
export function toolbarAttach(toolbar: Widget, window: Widget): void;

// ---------------------------------------------------------------------------
// Clipboard
// ---------------------------------------------------------------------------

export function clipboardRead(): string;
export function clipboardWrite(text: string): void;

// ---------------------------------------------------------------------------
// Dialogs
// ---------------------------------------------------------------------------

export function alert(title: string, message: string): void;
/**
 * Show a modal alert with multiple labeled buttons. The callback is invoked
 * with the 0-based index of the button the user clicked.
 *
 * On macOS the first button becomes the default (Return key); on Windows the
 * native MessageBox API is used with OK/OKCancel/YesNoCancel layouts
 * depending on button count. Pass a `"destructive"` style via convention by
 * placing the destructive label last and checking the index in the callback.
 */
export function alertWithButtons(
  title: string,
  message: string,
  buttons: string[],
  callback: (index: number) => void,
): void;
export function openFileDialog(callback: (path: string) => void): void;
export function openFolderDialog(callback: (path: string) => void): void;
export function saveFileDialog(callback: (path: string) => void, defaultName: string, extension: string): void;
export function pollOpenFile(): string;

// ---------------------------------------------------------------------------
// Keyboard shortcuts
// ---------------------------------------------------------------------------

/**
 * Register a keyboard shortcut that fires `callback` when pressed.
 *
 * `modifiers` is a bitfield: `1 = Cmd` (Ctrl on Linux/Windows), `2 = Shift`,
 * `4 = Option/Alt`, `8 = Control`. Combine with bitwise OR — e.g. Cmd+Shift+S
 * is `1 | 2` (= `3`). Pass `0` for an unmodified key.
 *
 * Must be called before `App({...})` — registrations are buffered and
 * installed when the menu bar is created.
 */
export function addKeyboardShortcut(
    key: string,
    modifiers: number,
    callback: () => void,
): void;

/**
 * Register a system-wide hotkey that fires even when the app is backgrounded.
 *
 * **Modifier bits:** `1` = Cmd/Ctrl, `2` = Shift, `4` = Option/Alt, `8` =
 * Control (macOS only). Combine by adding — `3` = Cmd+Shift, etc.
 *
 * **Platform support:**
 * - macOS — real Carbon `RegisterEventHotKey` implementation.
 * - Linux / Windows / iOS / tvOS / visionOS / watchOS / Android — logs + no-op;
 *   global hotkeys require OS-level portal/hook APIs that differ per platform.
 */
export function registerGlobalHotkey(
    key: string,
    modifiers: number,
    callback: () => void,
): void;

// ---------------------------------------------------------------------------
// App lifecycle hooks
// ---------------------------------------------------------------------------

/**
 * Register a callback to run just before the app exits. The macOS backend
 * wires this to `applicationWillTerminate:`, GTK4 uses `connect_shutdown`,
 * and Windows uses `WM_DESTROY`. Typical use: flush state, close database
 * connections, write preferences.
 */
export function onTerminate(callback: () => void): void;

/**
 * Register a callback to run when the app becomes the frontmost app
 * (initial launch, dock click, cmd-tab). Runs once per activation. Use to
 * refresh data when the user returns to the app.
 */
export function onActivate(callback: () => void): void;

// ---------------------------------------------------------------------------
// Timer
// ---------------------------------------------------------------------------

/**
 * Schedule a recurring callback on the UI main thread (#389).
 *
 * Two forms:
 *  - `appSetTimer(intervalMs, callback)` — preferred, runs the callback on
 *    the platform's UI thread (NSTimer / Handler / etc.)
 *  - `appSetTimer(app, intervalMs, callback)` — historical 3-arg form. The
 *    `app` arg is ignored at runtime (the platform-specific implementation
 *    schedules against the running app instance, not the handle). Kept as
 *    an overload so older code that passed an app handle still typechecks.
 */
export function appSetTimer(intervalMs: number, callback: () => void): void;
export function appSetTimer(app: Widget, intervalMs: number, callback: () => void): void;

// ---------------------------------------------------------------------------
// Embed
// ---------------------------------------------------------------------------

/** Embed a raw NSView pointer as a widget. Advanced use only. */
export function embedNSView(pointer: number): Widget;
