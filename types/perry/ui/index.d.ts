// Type declarations for perry/ui — Perry's native UI framework
// These types are auto-written by `perry init` / `perry types` so IDEs
// and tsc can resolve `import { ... } from "perry/ui"`.

declare const __widget: unique symbol;
declare const __canvasImage: unique symbol;

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
    drawImage(image: Image, dx: number, dy: number): void;
    drawImage(image: Image, dx: number, dy: number, dWidth: number, dHeight: number): void;
    drawImage(
        image: Image,
        sx: number,
        sy: number,
        sWidth: number,
        sHeight: number,
        dx: number,
        dy: number,
        dWidth: number,
        dHeight: number,
    ): void;
}

/** Opaque handle to a Canvas widget. Extends Widget with 2D drawing methods. */
export type Canvas = Widget & CanvasMethods;

/** Decoded raster image asset returned by `loadImage` for Canvas blits. */
export interface Image {
    readonly [__canvasImage]: void;
    readonly width: number;
    readonly height: number;
    readonly ready: boolean;
}

/**
 * Load and decode an image asset for Canvas.drawImage. Repeated calls with the
 * same URL share the same decoded handle. Relative paths are document-relative
 * on web and bundle-relative on native.
 */
export function loadImage(url: string): Promise<Image>;

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
    /**
     * Initial window state. Default is `"normal"` — the window opens at
     * the requested `width`/`height`. `"maximized"` zooms the window to
     * fill the working area (taskbar/dock visible). `"fullscreen"` enters
     * native fullscreen on macOS, removes the title bar and fills the
     * monitor on Windows, and maps to `gtk_window_fullscreen` on GTK4.
     *
     * Issue #1280.
     */
    windowState?: "normal" | "maximized" | "fullscreen";
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

/**
 * Issue #710 — empty Text widget that accepts per-range styled appends.
 * Distinct from #478 (rich-text editor with toolbar/shortcuts) — this is
 * a static, attributed display surface for inline emphasis inside a
 * single wrapping paragraph (bold/italic/colored words mixed with
 * default-styled prose).
 *
 * @example
 *   const w = AttributedText();
 *   attributedTextAppend(w, "Tap ",   0, 0, 0, 0, 0,    0,    0,    0);
 *   attributedTextAppend(w, "here",   1, 0, 0, 0, 0.80, 0.07, 0.26, 1);
 *   attributedTextAppend(w, " to read more.", 0, 0, 0, 0, 0, 0, 0, 0);
 */
export function AttributedText(): Widget;

/**
 * Issue #710 — append one styled run to an AttributedText widget.
 *
 * Boolean flags use 0/1. `fontSize = 0` inherits the widget's default
 * size. Alpha `a = 0` keeps the inherited text color (omits the color
 * attribute entirely, so theme-aware label colors still apply).
 *
 * Maps to NSMutableAttributedString.appendAttributedString: on Apple
 * platforms; stubbed on GTK4 / Windows / Android / watchOS until the
 * platform-native attributed surfaces are wired up.
 */
export function attributedTextAppend(
  widget: Widget,
  text: string,
  bold: number,
  italic: number,
  underline: number,
  fontSize: number,
  r: number,
  g: number,
  b: number,
  a: number,
): void;

/** Issue #710 — reset the buffer to empty. */
export function attributedTextClear(widget: Widget): void;

/** Clickable button. */
export function Button(label: string, onPress: () => void): Widget;

/**
 * Update a reactive `Text(initial, id)` widget's content.
 *
 * Routes through the cross-platform setText handler registry on every
 * UI backend (macOS/iOS/tvOS/visionOS/watchOS/Android/GTK4/Windows);
 * on `--target harmonyos`, queues a `(id, value)` update that the
 * auto-emitted .ets onClick drains after the closure returns, assigning
 * to the matching `@State text_<id>: string` field — ArkUI rerenders.
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
 * Implementation desugars to the existing setText/Text reactive binding
 * at compile time — each `state(initial)` declaration registers a
 * synthetic id; `state.text()` emits a reactive
 * `Text(initial.toString(), "<synth_id>")`, and `state.set(v)` rewrites
 * to `setText("<synth_id>", String(v))` inside any closure body. On
 * `--target harmonyos`, perry-codegen-arkts owns the harvest; on every
 * other native target (macOS/iOS/tvOS/visionOS/watchOS/Android/GTK4/
 * Windows) the target-agnostic `state_desugar` HIR pass produces the
 * same shape and `js_state_init` / `js_state_get` / `js_state_set`
 * drive the bound widget through the registered set-text handler.
 *
 * The state value also drives `NavStack(state, routes)` visibility:
 * `state.set("detail")` flips the visible route on every backend that
 * registers `js_register_widget_hidden_handler` (currently all native
 * UI backends).
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
 * Limitations of the desugar pass: only top-level `state(...)`
 * declarations are tracked; only the canonical method-call shapes
 * (`x.set` / `x.get` / `x.value` / `x.text`) are rewritten. State
 * escaping through a function arg / array / object property won't
 * trigger UI updates at the escape site. `Text("prefix " + s.get())`
 * snapshots once at App-build time — use `s.text()` (or assemble a
 * derived string in `s.set` callers) for reactive concatenation.
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

/** Programmatically set a Toggle's on/off state. Pass `1` for on, `0` for off. */
export function toggleSetState(widget: Widget, on: number): void;

/** Numeric slider. */
export function Slider(min: number, max: number, onChange: (value: number) => void): Widget;

/** Scrollable container. */
export function ScrollView(): Widget;

/** Flexible space. */
export function Spacer(): Widget;

/** Visual separator line. */
export function Divider(): Widget;

/**
 * Banner ad size keys (#867). Values match Google Mobile Ads' standard
 * `AdSize` constants. `"adaptive"` requests an adaptive banner sized to
 * the device; the rest are fixed. A plain string-literal union (not a
 * runtime enum) so it needs no value backing in the native module.
 */
export type AdSizeKey =
  | "banner"
  | "large-banner"
  | "medium-rectangle"
  | "full-banner"
  | "leaderboard"
  | "adaptive";

/**
 * In-app banner ad (#867). Place it in a layout like any other widget:
 *
 * ```ts
 * VStack(0, [MyContent(), AdBanner("ca-app-pub-xxx/yyy", "banner")])
 * ```
 *
 * Reserves a banner-sized slot (`size`) in the layout on every platform.
 * Live ad rendering — a `GADBannerView` (iOS) / `AdView` (Android) — lands
 * when the platform Google Mobile Ads SDK is linked; until then (and
 * always on macOS, which has no Ads SDK) the widget is a layout
 * placeholder. Both arguments are required — pass an `AdSizeKey` for `size`.
 *
 * The App ID must be configured in `perry.toml` under `[ads]`
 * (`ios_app_id` / `android_app_id`); the compiler writes it into the
 * platform manifest at build time.
 */
export function AdBanner(unitId: string, size: AdSizeKey): Widget;

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

/**
 * Render-surface host for an external GPU renderer (issue #2395).
 *
 * `BloomView(width, height)` reserves a native render-surface view in the Perry
 * UI view tree. Perry UI does not draw into it — pass the handle returned by
 * `bloomViewGetNativeHandle(view)` to a renderer such as the Bloom engine
 * (`attachToNSView` / `attachToSurface` / …), which builds its surface on the
 * view and drives frames. Available on all native targets (Windows HWND,
 * macOS/iOS/visionOS native view, GTK4 widget, Android SurfaceView; tvOS real
 * view, watchOS links as a no-op).
 */
export function BloomView(width: number, height: number): Widget;

/**
 * The `BloomView`'s native render-surface handle as a number — the platform's
 * view/window pointer: `HWND` on Windows, `NSView*`/`UIView*` on Apple,
 * `GtkWidget*` on GTK4, `ANativeWindow*` on Android. Hand this to the Bloom
 * engine's attach call (`attachToNSView` / `attachToUIView` / `attachToSurface`,
 * all forwarding to `bloom_attach_native`).
 */
export function bloomViewGetNativeHandle(view: Widget): number;

/**
 * @deprecated Renamed to {@link bloomViewGetNativeHandle} in #5519 — the handle
 * is no longer Windows-only (it's an `NSView*`/`UIView*`/`GtkWidget*`/
 * `ANativeWindow*` on the other platforms). This alias still works and returns
 * the same value.
 */
export function bloomViewGetHwnd(view: Widget): number;

/**
 * Register a one-shot frame callback (requestAnimationFrame-style). The
 * callback receives `(timestampMs, deltaMs)`. Re-register from inside the
 * callback to keep a loop running. Returns an id usable with `cancelFrame`.
 */
export function onFrame(callback: (timestampMs: number, deltaMs: number) => void): number;

/** Cancel a pending `onFrame` callback by its id. */
export function cancelFrame(id: number): void;

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
 *
 * **Route shape requirements** (#1135): each route entry must be a 2-field
 * `{ name, body }` object literal at the call site (no spread, no extra
 * fields). The `name` value must be either a string literal or a
 * same-module `const X = "literal"` binding — the desugar pass const-folds
 * the local back to its initializer so the canonical "factor route names
 * into a shared constants file" pattern Just Works:
 *
 * ```ts
 * const ROUTE_HOME = "home";
 * const ROUTE_DETAIL = "detail";
 * NavStack(route, [
 *   { name: ROUTE_HOME, body: ... },     // OK — const-folded
 *   { name: ROUTE_DETAIL, body: ... },   // OK — const-folded
 * ])
 * ```
 *
 * Imported `const`s from a sibling module (`import { ROUTE_HOME } from
 * "./routes"`) are NOT resolved across modules today; they must be inlined
 * to a string literal at the NavStack call site. When the rewrite bails
 * for any reason, the compiler now emits a `Warning: NavStack(state,
 * routes) skipped state-driven lowering (…)` line — pre-#1135 the call
 * fell through silently to the 0-arg `NavStack()` stub and rendered as a
 * completely blank screen with no diagnostic.
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

/**
 * Image fetched from a remote URL (or `data:` URI). The widget appears
 * immediately as an empty box; the bytes are fetched on a background
 * queue and applied on the main thread once the response arrives.
 * Failed requests leave the widget empty (no crash).
 *
 * Real on macOS (NSImageView) + iOS (UIImageView). Other UI platforms
 * register an empty image placeholder so layout still works.
 *
 * `alt`, when provided, is used as the accessibility label.
 *
 * Either form is accepted:
 *
 * ```ts
 * Image("https://example.com/avatar.png");
 * Image({ url: "https://example.com/avatar.png", alt: "Avatar" });
 * Image({ systemName: "gear" }); // SF Symbol, same as ImageSymbol("gear")
 * ```
 *
 * (#635, #1495)
 */
export function Image(url: string, alt?: string): Widget;
export function Image(options: {
    url: string;
    alt?: string;
}): Widget;
export function Image(options: {
    systemName: string;
}): Widget;

/**
 * Embedded native WebView for auth flows / payment redirects / bounded HTML
 * pages. Backed by `WKWebView` on Apple platforms; `WebView2` on Windows
 * (post-Phase 1); `android.webkit.WebView` on Android (post-Phase 1);
 * `WebKitGTK 6.0` on Linux (post-Phase 1). Stub on tvOS / watchOS.
 *
 * Intentionally narrow scope — this is a "browser tab embedded in your
 * native widget tree" primitive, NOT a Tauri/Electron-style app shell.
 * If you need bidirectional native↔JS RPC, use Tauri or Electron.
 *
 * Common shape:
 *
 * ```ts
 * WebView({
 *   url: "https://accounts.google.com/o/oauth2/auth?...",
 *   allowedDomains: ["accounts.google.com", "myapp.com"],
 *   onShouldNavigate: (url) => {
 *     if (url.startsWith("https://myapp.com/oauth/callback?code=")) {
 *       const code = new URL(url).searchParams.get("code");
 *       exchangeCodeForToken(code);
 *       return false;  // don't actually navigate
 *     }
 *     return true;
 *   }
 * })
 * ```
 *
 * **Post-page-load hooks (cookies, JS values).** Use `webviewEvaluateJs`
 * to read data from the loaded page after `onLoaded` fires:
 *
 * ```ts
 * const wv = WebView({
 *   url: "https://example.com/auth/callback",
 *   onLoaded: (url) => {
 *     webviewEvaluateJs(wv, "document.cookie", (cookies) => {
 *       saveAuthSession(parseCookies(cookies));
 *     });
 *   }
 * });
 * ```
 *
 * **Cross-origin messaging (web target).** On the web target the
 * embedded page can `window.parent.postMessage(payload, "*")` and the
 * host can `window.addEventListener("message", e => ...)` to receive
 * frames. This is browser-platform-specific; native targets don't
 * expose `postMessage` (use `webviewEvaluateJs` to push state IN, and
 * navigation interception to pull state OUT — that's the Perry contract
 * across all platforms).
 *
 * (#658)
 */
export function WebView(options: {
    /** Initial URL to load. Use `webviewLoadUrl` to navigate later. */
    url: string;
    /**
     * Hard navigation allowlist. URLs whose host doesn't match any entry are
     * blocked at the native layer (no `onShouldNavigate` round-trip). Match
     * is exact OR subdomain — `["example.com"]` allows `example.com` and
     * `*.example.com`. Empty / omitted = no host restriction.
     */
    allowedDomains?: string[];
    /** Custom User-Agent header. Defaults to the platform WebKit UA. */
    userAgent?: string;
    /**
     * Cookie / storage isolation. Default `true` — auth flows reusing a
     * logged-in browser session is usually a footgun. Set `false` to
     * persist cookies across WebView dismissals.
     */
    ephemeral?: boolean;
    /**
     * Sync intercept invoked before each navigation. Return `false` to
     * cancel the load; return `true` (or omit a return) to allow it.
     * Most common use: extract OAuth callback `code=` from a known
     * redirect URL and cancel the actual navigation.
     */
    onShouldNavigate?: (url: string) => boolean | void;
    /** Fired once a page finishes loading. */
    onLoaded?: (url: string) => void;
    /** Fired on any load error (DNS, TLS, HTTP, navigation cancel). */
    onError?: (errorCode: number, message: string) => void;
    /** Pixel width hint. The widget tree's layout engine still controls final size. */
    width?: number;
    /** Pixel height hint. */
    height?: number;
}): Widget;

/** Replace the WebView's URL — re-navigates and re-paints. */
export function webviewLoadUrl(handle: Widget, url: string): void;
/** Reload the current page. */
export function webviewReload(handle: Widget): void;
/** Navigate back through the WebView's session history. */
export function webviewGoBack(handle: Widget): void;
/** Navigate forward through the WebView's session history. */
export function webviewGoForward(handle: Widget): void;
/** Returns 1 when there's history to go back to, 0 otherwise. */
export function webviewCanGoBack(handle: Widget): number;
/**
 * Run a one-shot JS expression in the WebView's content process. The
 * callback fires with the stringified result (empty string on null /
 * undefined / error). Use sparingly — this is for reading
 * `document.cookie` / `localStorage.getItem(...)` after a redirect, not
 * for general-purpose RPC. (#658)
 */
export function webviewEvaluateJs(handle: Widget, js: string, callback: (result: string) => void): void;
/** Wipe the WebView's cookies / local storage / IndexedDB. Use after auth. */
export function webviewClearCookies(handle: Widget): void;

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
 * Issue #707 — cap visible lines on a Text widget. `lines = 0` means
 * unlimited (the default). When `lines > 0` and the content overflows,
 * the runtime picks tail-truncation by default; use
 * `textSetTruncationMode` to choose head/middle/tail.
 *
 * Maps to `UILabel.numberOfLines` on iOS and `NSTextField.maximumNumberOfLines`
 * (+ cell wrapping/line-break-mode) on macOS. Stubbed on other platforms.
 */
export function textSetNumberOfLines(widget: Widget, lines: number): void;
/**
 * Issue #707 — control where the ellipsis appears when content overflows
 * the line cap set via `textSetNumberOfLines`.
 *
 * Modes: `0` = word-wrap (no ellipsis), `1` = head ("…foo"),
 * `2` = middle ("fo…ar"), `3` = tail ("foo…"). Tail is the most common.
 */
export function textSetTruncationMode(widget: Widget, mode: number): void;
/**
 * Set the horizontal text alignment of a Text widget (issue #3621),
 * equivalent to CSS `text-align`. Removes the need to wrap a `Text` in an
 * extra alignment container just to centre or right-align it.
 *
 * `alignment` values follow the macOS `NSTextAlignment` convention and are
 * translated to each platform's native enum so a single value renders the
 * same everywhere:
 * - `0` = left
 * - `1` = right
 * - `2` = center
 * - `3` = justified
 * - `4` = natural (follows the locale's writing direction)
 *
 * Wired on all native backends (macOS, iOS, tvOS, visionOS, watchOS,
 * Android, GTK4, Windows). `justified`/`natural` degrade gracefully to the
 * nearest supported alignment where a platform lacks a native equivalent.
 */
export function textSetTextAlignment(widget: Widget, alignment: number): void;
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
// DatePicker widget (issue #4772, v1) — compact field-style date picker, the
// space-saving complement to the month-grid `Calendar`. Backends use each
// platform's native compact date control:
//   macOS    NSDatePicker, text-field-and-stepper style (year-month-day)
//   iOS/visionOS  UIDatePicker, .compact style, .date mode
//   Windows  SysDateTimePick32 (DateTimePicker dropdown)
//   Android  android.widget.DatePicker
//   GTK4     GtkCalendar (GTK has no native compact date field, so the
//            month grid is reused; behavior is otherwise identical)
//   tvOS/watchOS  stub the FFI (returns 0 on create, undefined on get-date)
//
// `year` and `month` are 1-based; pass <=0 / out-of-range to default to
// 2026-01. `onChange` receives the selected date as an ISO `yyyy-MM-dd`
// string (POSIX-locale formatter, stable across user locales) — matching
// `Calendar`.
// ---------------------------------------------------------------------------

export function DatePicker(year: number, month: number, onChange: (isoDate: string) => void): Widget;
export function datePickerSetDate(widget: Widget, year: number, month: number, day: number): void;
export function datePickerGetSelectedDate(widget: Widget): string;

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
/**
 * Fired when the pointer enters (`true`) or leaves (`false`) the widget's
 * bounds. The callback signature changed in issue #1868 to deliver both
 * states through a single callback; previous code that wrote
 * `widgetSetOnHover(w, () => ...)` keeps compiling and running — extra
 * arguments are ignored at the call site — but only fires on enter.
 */
export function widgetSetOnHover(widget: Widget, callback: (isHovering: boolean) => void): void;
export function widgetSetOnDoubleClick(widget: Widget, callback: () => void): void;

/**
 * Which mouse button fired a pointer event. Values match the web
 * `MouseEvent.button` spec so the model is portable. On touch
 * platforms the value is always `MouseButton.Left`.
 */
export const enum MouseButton {
    Left = 0,
    Middle = 1,
    Right = 2,
    Back = 3,
    Forward = 4,
}

/**
 * Which input device fired a pointer event. Mirrors the web
 * `PointerEvent.pointerType` spec. Multi-touch is not yet surfaced;
 * stylus is reported as `"pen"` when the platform exposes it.
 */
export type PointerType = "mouse" | "touch" | "pen";

/**
 * Continuous pointer event payload delivered to `onMouseDown`,
 * `onMouseUp`, `onMouseMove`. Coordinates are widget-local points
 * (top-left origin). Issue #1868.
 */
export interface PointerEvent {
    x: number;
    y: number;
    button: MouseButton;
    pointerType: PointerType;
}

/**
 * Fire when a button is pressed over the widget. Use together with
 * `widgetSetOnMouseUp` to disambiguate clicks from drags, or with
 * `widgetSetOnMouseMove` to drive drawing / drag-and-drop. Issue #1868.
 */
export function widgetSetOnMouseDown(widget: Widget, callback: (e: PointerEvent) => void): void;

/** Fire when a button is released over the widget. See `widgetSetOnMouseDown`. */
export function widgetSetOnMouseUp(widget: Widget, callback: (e: PointerEvent) => void): void;

/**
 * Fire continuously while the pointer is over the widget. On macOS,
 * duplicate same-position events are coalesced to one call per
 * (x, y) pair. See `widgetSetOnMouseDown`.
 */
export function widgetSetOnMouseMove(widget: Widget, callback: (e: PointerEvent) => void): void;
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

/**
 * Issue #553 — infinite-scroll callback.
 *
 * Fires `onScrollEnd` once when the visible region's bottom edge gets
 * within `thresholdPx` of the content's bottom (default 200). Re-arms
 * after the user scrolls back up past the threshold so the callback
 * can fire repeatedly across pagination loads.
 *
 * Real on macOS (NSScrollView clip-view bounds observer) + iOS
 * (UIScrollViewDelegate.scrollViewDidScroll). No-op on platforms where
 * `setRefreshControl` is also no-op.
 */
export function scrollviewSetScrollEndCallback(scrollView: Widget, onScrollEnd: () => void, thresholdPx: number): void;
export function scrollViewSetScrollEndCallback(scrollView: Widget, onScrollEnd: () => void, thresholdPx: number): void;

/**
 * Issue #553 — pull-to-refresh on LazyVStack (parallel to ScrollView).
 *
 * Real on iOS (UIRefreshControl on the inner UITableView). No-op on
 * macOS — AppKit has no native pull-to-refresh idiom; desktop apps
 * should add an explicit "Refresh" button. Stubs on other platforms.
 */
export function lazyvstackSetRefreshControl(view: Widget, onPull: () => void): void;
export function lazyvstackEndRefreshing(view: Widget): void;

/**
 * Issue #553 — infinite-scroll callback on LazyVStack.
 *
 * Same backpressure contract as `scrollviewSetScrollEndCallback`, but
 * `thresholdItems` measures rows from the bottom rather than pixels —
 * works with variable view heights as long as the LazyVStack uses a
 * uniform row height (which is the only mode currently supported).
 */
export function lazyvstackSetScrollEndCallback(view: Widget, onScrollEnd: () => void, thresholdItems: number): void;

// ---------------------------------------------------------------------------
// Issue #553 — BottomNavigation (5-tab bottom bar with icon + label + badge)
// ---------------------------------------------------------------------------

/**
 * Create an empty bottom-navigation bar. Add tabs with `bottomNavAddItem`.
 * The `onSelect(index)` callback fires whenever the user taps a tab —
 * after the bar's internal selectedIndex is updated. Use
 * `bottomNavSetSelected(bar, i)` for programmatic selection (does NOT
 * fire `onSelect`).
 *
 * Real on macOS (custom NSStackView + NSButton strip with SF Symbol
 * icons) + iOS (UITabBar). Stubs on Android / GTK4 / Windows / tvOS /
 * watchOS / visionOS — those platforms reach the bar through the
 * BottomNavigationView / GtkBox / Pivot equivalents in a follow-up.
 */
export function BottomNavigation(onSelect: (index: number) => void): Widget;

/** Add a tab item — `icon` is an SF Symbol name on Apple platforms. */
export function bottomNavAddItem(bar: Widget, icon: string, label: string): void;

/** Set or clear the badge string on a tab. Empty string clears the badge. */
export function bottomNavSetBadge(bar: Widget, index: number, badge: string): void;

/** Programmatically select a tab. Does NOT fire `onSelect`. */
export function bottomNavSetSelected(bar: Widget, index: number): void;

/**
 * Issue #706 — set the active tab's icon/label tint (RGBA 0.0-1.0).
 * On iOS this maps to `UITabBar.tintColor`. On macOS this overrides the
 * iOS-default-blue used in the custom NSStackView styling. Stubbed on
 * GTK4 / Windows / Android / tvOS / watchOS / visionOS.
 */
export function bottomNavSetTintColor(
  bar: Widget,
  r: number,
  g: number,
  b: number,
  a: number,
): void;

/**
 * Issue #706 — set the inactive tabs' icon/label tint (RGBA 0.0-1.0).
 * On iOS this maps to `UITabBar.unselectedItemTintColor`.
 */
export function bottomNavSetUnselectedTintColor(
  bar: Widget,
  r: number,
  g: number,
  b: number,
  a: number,
): void;

// ---------------------------------------------------------------------------
// Issue #553 — ImageGallery (swipeable carousel)
// ---------------------------------------------------------------------------

/**
 * Create an empty image gallery. Add images with `imageGalleryAddImage`.
 * The `onIndexChange(index)` callback fires when the user pages to a
 * new image (or `imageGallerySetIndex` is called programmatically and
 * the index changes).
 *
 * Image source is a local file path or http(s) URL; remote images are
 * fetched on a background queue and applied on the main thread. Local
 * paths load synchronously.
 *
 * Real on macOS (horizontal-paging NSScrollView) + iOS (paging
 * UIScrollView). Stubs on other platforms.
 */
export function ImageGallery(onIndexChange: (index: number) => void): Widget;

/** Add an image to the gallery. `alt` is used as accessibilityLabel. */
export function imageGalleryAddImage(gallery: Widget, url: string, alt: string): void;

/** Programmatically jump to a given image index (animated). */
export function imageGallerySetIndex(gallery: Widget, index: number): void;

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

/**
 * Register a callback invoked for each captured camera frame, enabling
 * real-time processing such as QR / barcode detection.
 *
 * The callback receives the frame as tightly-packed 24-bit RGB
 * (`width * height * 3` bytes, 3 bytes per pixel), its pixel width and its
 * pixel height. The buffer is only valid for the duration of the
 * synchronous call — copy out anything you need to retain.
 *
 * Platform support:
 *   - Linux:   GStreamer `v4l2src` pipeline
 *   - iOS:     AVFoundation capture session
 *   - Android: CameraX / Camera2
 *   - Other:   no-op (the callback never fires)
 *
 * Requires camera permission.
 */
export function cameraRegisterFrameCallback(camera: Widget, callback: (frameData: Uint8Array, width: number, height: number) => void): void;

/** Unregister a previously-registered camera frame callback. */
export function cameraUnregisterFrameCallback(camera: Widget): void;

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
// Drag & drop (issue #4773)
// ---------------------------------------------------------------------------

/**
 * Payload delivered to a {@link widgetOnDrop} handler. Each field is present
 * only when the drag carried that representation:
 *  - `text` — plain text (`public.utf8-plain-text`)
 *  - `files` — absolute paths of dropped files (`public.file-url`)
 *  - `urls` — web links (`public.url`)
 */
export interface DropData {
  text?: string;
  files?: string[];
  urls?: string[];
}

/**
 * Make `widget` a drop destination. `handler` fires when text, files, or URLs
 * are dragged onto the widget, receiving a {@link DropData} describing the
 * payload. The drop defaults to a "copy" operation.
 */
export function widgetOnDrop(widget: Widget, handler: (data: DropData) => void): void;

/**
 * Make `widget` a drag source that offers plain text. `provider` is called
 * when a drag begins and returns the text to carry (`public.utf8-plain-text`).
 * May be combined with {@link widgetSetDragFile} / {@link widgetSetDragUrl} to
 * offer multiple representations of the same drag.
 */
export function widgetSetDragText(widget: Widget, provider: () => string): void;

/**
 * Make `widget` a drag source that offers a file. `provider` returns the
 * absolute path of the file to carry (`public.file-url`).
 */
export function widgetSetDragFile(widget: Widget, provider: () => string): void;

/**
 * Make `widget` a drag source that offers a web link. `provider` returns the
 * URL string to carry (`public.url`).
 */
export function widgetSetDragUrl(widget: Widget, provider: () => string): void;

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

// ---------------------------------------------------------------------------
// Continuous keyboard events (issue #1864)
// ---------------------------------------------------------------------------

/**
 * Bitfield for keyboard modifier state. Shared with `addKeyboardShortcut` /
 * `registerGlobalHotkey`. On Linux/Windows, `Cmd` maps to the Ctrl key.
 *
 * Use bitwise-or to combine: `Modifier.Cmd | Modifier.Shift`.
 */
export const enum Modifier {
    None = 0,
    Cmd = 1,
    Shift = 2,
    Alt = 4,
    Ctrl = 8,
}

/**
 * Canonical numeric key identifiers. Stable across releases. Values match the
 * native `KeyCode` table in `perry-ui::keys` — the runtime passes the raw `u16`
 * directly, so comparisons against `Key.*` compile to integer equality with
 * zero string allocation per event.
 *
 * Letters are lowercase (`Key.A`/`Key.B`/…). Use `onTextInput` (future API)
 * for IME-aware composed text — these `Key` values are *physical* keys.
 */
export const enum Key {
    Unknown = 0,
    A = 1, B = 2, C = 3, D = 4, E = 5, F = 6, G = 7, H = 8, I = 9, J = 10,
    K = 11, L = 12, M = 13, N = 14, O = 15, P = 16, Q = 17, R = 18, S = 19,
    T = 20, U = 21, V = 22, W = 23, X = 24, Y = 25, Z = 26,

    Digit0 = 27, Digit1 = 28, Digit2 = 29, Digit3 = 30, Digit4 = 31,
    Digit5 = 32, Digit6 = 33, Digit7 = 34, Digit8 = 35, Digit9 = 36,

    F1 = 37, F2 = 38, F3 = 39, F4 = 40, F5 = 41, F6 = 42,
    F7 = 43, F8 = 44, F9 = 45, F10 = 46, F11 = 47, F12 = 48,

    ArrowUp = 49, ArrowDown = 50, ArrowLeft = 51, ArrowRight = 52,
    Space = 53, Enter = 54, Tab = 55, Escape = 56,
    Backspace = 57, Delete = 58,

    Home = 59, End = 60, PageUp = 61, PageDown = 62, Insert = 63,

    Minus = 64, Equal = 65, BracketLeft = 66, BracketRight = 67,
    Backslash = 68, Semicolon = 69, Quote = 70, Comma = 71,
    Period = 72, Slash = 73, Backquote = 74,

    F13 = 75, F14 = 76, F15 = 77, F16 = 78,
    F17 = 79, F18 = 80, F19 = 81, F20 = 82,

    Numpad0 = 83, Numpad1 = 84, Numpad2 = 85, Numpad3 = 86, Numpad4 = 87,
    Numpad5 = 88, Numpad6 = 89, Numpad7 = 90, Numpad8 = 91, Numpad9 = 92,
    NumpadDecimal = 93, NumpadEnter = 94, NumpadAdd = 95, NumpadSubtract = 96,
    NumpadMultiply = 97, NumpadDivide = 98, NumpadEqual = 99, NumpadClear = 100,
}

/**
 * Subscribe to key-down events on a widget. Fires only while that widget owns
 * logical focus (set via `focus(widget)`). Use `onAppKeyDown` for events that
 * should fire regardless of which widget is focused.
 *
 * `repeat` is `true` when the OS auto-repeats the key while held. Filter it
 * out for edge-only detection.
 */
export function onKeyDown(
    widget: Widget,
    handler: (key: Key, modifiers: number, repeat: boolean) => void,
): void;

/** Subscribe to key-up events on a widget. Same focus semantics as `onKeyDown`. */
export function onKeyUp(
    widget: Widget,
    handler: (key: Key, modifiers: number) => void,
): void;

/**
 * App-level key-down handler. Fires whenever no widget currently owns focus.
 * Use this for games, modal HUDs, or any "I don't care which widget is
 * focused" capture.
 */
export function onAppKeyDown(
    handler: (key: Key, modifiers: number, repeat: boolean) => void,
): void;

/** App-level key-up handler. Same semantics as `onAppKeyDown`. */
export function onAppKeyUp(
    handler: (key: Key, modifiers: number) => void,
): void;

/** Route subsequent keyboard events to this widget. Pair with `style: { focusable: true }`. */
export function focus(widget: Widget): void;

/** Clear focus if `widget` currently owns it. */
export function blur(widget: Widget): void;

/**
 * O(1), branchless poll. Returns `1` while `key` is currently held, `0`
 * otherwise — use a truthy check: `if (isKeyDown(Key.Space)) { … }`.
 */
export function isKeyDown(key: Key): number;

/**
 * Snapshot of the currently-held modifier keys as a `Modifier` bitfield.
 * Updated continuously by the OS — accurate even outside of a key event
 * (e.g. while dragging the mouse with Shift held).
 *
 * Example: `if (currentModifiers() & Modifier.Shift) snapToGrid();`
 */
export function currentModifiers(): number;

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

/**
 * Set the application activation policy. Call before `App()`.
 *
 * - `"regular"` (default): normal app — dock icon, app-switcher entry,
 *   main window auto-presented at launch.
 * - `"accessory"`: menu-bar / status-item app — **no dock icon**, no
 *   app-switcher entry, and on macOS the launch window is **not**
 *   auto-presented (open windows on demand from the tray instead). The
 *   macOS equivalent of `LSUIElement`.
 * - `"background"`: fully background (`NSApplicationActivationPolicy.Prohibited`);
 *   also suppresses the launch window on macOS.
 *
 * Windows: `accessory`/`background` hide the taskbar button. Linux/GTK4:
 * maps to the equivalent hint. No-op on mobile/TV backends.
 */
export function appSetActivationPolicy(policy: "regular" | "accessory" | "background"): void;

// ---------------------------------------------------------------------------
// Embed
// ---------------------------------------------------------------------------

/** Embed a raw NSView pointer as a widget. Advanced use only. */
export function embedNSView(pointer: number): Widget;
