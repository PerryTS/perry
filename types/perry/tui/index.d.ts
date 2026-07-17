// perry/tui — native TUI engine for Perry (#358).
//
// v0.2 surface (Phase 2): adds reactive state, keypress input, and the
// interactive `run()` render loop on top of Phase 1's Box / Text /
// render. Flexbox layout via Taffy is Phase 3; the wider widget set
// (Spacer, Input, TextArea, List, Select, Spinner, ProgressBar)
// lands in Phase 4.

declare module "perry/tui" {
    /**
     * Opaque widget handle returned by Box / Text. Pass to render(),
     * or to Box() as a child.
     */
    export type Widget = number & { readonly __perryTuiWidget: unique symbol };

    /**
     * Reactive state container. `.get()` returns the current value;
     * `.set(v)` writes a new value and triggers a re-render of the
     * `run()` loop on the next tick.
     */
    export interface State<T> {
        get(): T;
        set(value: T): void;
    }

    /**
     * Single-line text node. The 2-arg form applies fg / bg colors and
     * style flags (bold / italic / underline / reverse).
     */
    export function Text(content: string): Widget;
    export function Text(content: string, opts: TextStyle): Widget;

    /**
     * Per-side cell counts. Used for `padding`. Missing fields default
     * to 0 cells.
     */
    export interface Edges {
        top?: number;
        right?: number;
        bottom?: number;
        left?: number;
    }

    /**
     * A length expressed as integer cells (number) or as a percentage
     * of the parent's equivalent dimension (string `"50%"`). Phase 3.5
     * (#405) added the percentage form.
     */
    export type Dim = number | string;

    /**
     * Style options for a Box. Maps to Taffy's flexbox solver — the
     * Phase 3.5 (#405) surface adds 24-bit truecolor on Text, per-side
     * padding, flex-shrink, flex-basis, and percentage units for
     * width / height / flexBasis on top of the v1.0 (#358 Phase 3)
     * surface.
     */
    export interface BoxStyle {
        flexDirection?: "row" | "column";
        justifyContent?:
            | "start"
            | "center"
            | "end"
            | "flex-end"
            | "space-between"
            | "space-around";
        alignItems?: "start" | "center" | "end" | "flex-end" | "stretch";
        gap?: number;
        /** Cells of padding — uniform (number) or per-side ({ top, right, bottom, left }). */
        padding?: number | Edges;
        /** Width in cells (number) or percentage of parent (string `"50%"`). */
        width?: Dim;
        height?: Dim;
        /** CSS flex-grow factor. 0 = no grow (default); 1 = fill remaining space. */
        flexGrow?: number;
        /** CSS flex-shrink factor. 0 = don't shrink (default); 1 = shrink at default rate. */
        flexShrink?: number;
        /** CSS flex-basis. Cells (number) or percentage (string `"30%"`). */
        flexBasis?: Dim;
    }

    /**
     * Style options for `Text(content, opts)`. Colors accept the named
     * 16-color palette (`"red"`, `"bright-blue"`, …), CSS hex
     * (`"#ff8800"` / `"#fa0"`), or empty string for the terminal
     * default. (#405 Phase 3.5.)
     */
    export interface TextStyle {
        fg?: string;
        color?: string; // alias for fg (matches the CSS-style naming on Box)
        bg?: string;
        backgroundColor?: string; // alias for bg
        bold?: boolean;
        italic?: boolean;
        underline?: boolean;
        reverse?: boolean;
    }

    /**
     * Container with optional flexbox style and children. Box defaults
     * to `flexDirection: "column"`, gap=0, padding=0 — matches the
     * v0.1 vertical-stack behavior so existing code keeps working
     * without supplying a style.
     */
    export function Box(): Widget;
    export function Box(children: Widget[]): Widget;
    export function Box(style: BoxStyle): Widget;
    export function Box(style: BoxStyle, children: Widget[]): Widget;

    /**
     * Paint one frame of `root` to stdout and return. Diffs against
     * the previous frame and emits only the cells that changed.
     * Use `run()` instead for interactive apps that re-render on
     * input or state change.
     */
    export function render(root: Widget): void;

    /**
     * Clear the screen and home the cursor. Called implicitly on
     * first render; exposed separately for callers that want explicit
     * setup before any render.
     */
    export function enter(): void;

    /**
     * Empty Box with `flexGrow: 1` — pushes siblings apart in a row
     * layout (and up/down in a column). Equivalent to
     * `Box({ flexGrow: 1 })` with a more discoverable name.
     */
    export function Spacer(): Widget;

    /**
     * `[====    ]`-style filled bar. `value`/`max` → fraction of
     * `width` cells filled with `=`; the rest are spaces. Brackets
     * are added at both ends so the widget's total width is
     * `width + 2`.
     */
    export function ProgressBar(value: number, max: number, width: number): Widget;

    /**
     * Animated character cycling through `-\|/` based on a frame
     * counter. Caller bumps the frame number from a state slot to
     * animate (`Spinner(frame.get())` inside the component, with a
     * `setInterval(() => frame.set(frame.get() + 1), 100)` driver).
     */
    export function Spinner(frame: number): Widget;

    /**
     * Single-line text input renderer. Shows `value` followed by a
     * `_` cursor character. Wire keypresses via `useInput` and
     * mutate the value state — the widget itself is purely visual.
     *
     * The 2-arg form positions the cursor at an arbitrary index
     * inside the value (left/right arrow inside text). Cursor at the
     * value's length renders a trailing reverse-video space (matching
     * most terminal text editors' end-of-line cursor). (#404.)
     */
    export function Input(value: string): Widget;
    export function Input(value: string, cursor: number): Widget;

    /**
     * Vertical list of items as a Box of Text children. The
     * `selected` index (default -1 = no selection) is rendered with
     * reverse-video.
     */
    export function List(items: string[], selected?: number): Widget;

    /**
     * List with an enforced selection. Caller's state holds the
     * selected index.
     */
    export function Select(items: string[], selected: number): Widget;

    /**
     * Multi-line text renderer. Splits `value` on `\n` and emits
     * one Text per line inside a column-layout Box. Wire keypresses
     * via `useInput` to edit.
     */
    export function TextArea(value: string): Widget;

    /**
     * Animated spinner whose frame index is driven by an internal
     * timer — no `setInterval` wiring required. Defaults to a 100 ms
     * cycle through `["-", "\\", "|", "/"]`. Inside `run()`, the global
     * timer flips `STATE_DIRTY` every ~50 ms so the loop re-renders
     * cleanly; outside `run()` (one-shot `render()`), only the
     * snapshot prints. (#403.)
     */
    export function AnimatedSpinner(opts?: {
        interval?: number;
        frames?: string[];
    }): Widget;

    /**
     * Render a 2D grid as a column-stacked Box. Header row is bold;
     * the optional `selected` row index (default -1 = none) is drawn
     * with reverse video. Column widths auto-fit the longest header
     * or cell. (#402.)
     */
    export function Table(opts: {
        headers: string[];
        rows: string[][];
        selected?: number;
    }): Widget;

    /**
     * Horizontal tab bar (active label drawn with reverse video)
     * followed by the active tab's body widget. `body[i]` is mounted
     * only when `active === i` — non-active bodies aren't rendered
     * at all (matches React's null-render fallback for missing
     * keys). (#402.)
     */
    export function Tabs(opts: {
        tabs: string[];
        active: number;
        body: Widget[];
    }): Widget;

    /**
     * Allocate a reactive state slot with the given initial value.
     */
    export function state<T>(initial: T): State<T>;

    /**
     * Register a keypress handler. The handler is called with the raw
     * byte sequence as a string — single ASCII bytes for printable
     * keys, multi-byte ANSI sequences for arrow keys / function keys
     * (e.g. `"\x1b[A"` for Up). Only one handler is supported in v1;
     * subsequent calls replace the prior handler.
     */
    export function useInput(handler: (input: string) => void): void;

    /**
     * Enter the interactive render loop. `component()` is called on
     * every state change; the returned widget tree is diffed and
     * painted with no flicker. Call `exit()` from a useInput handler
     * to leave the loop.
     */
    export function run(component: () => Widget): void;

    /**
     * Exit the render loop. The current frame finishes; raw mode is
     * restored and the alt screen is left before `run()` returns.
     */
    export function exit(): void;

    // ---- ink-API ergonomics hooks (#679 Phase 1 / Phase 3) ----
    //
    // These bind to call-site position within a component body, not
    // to call count: the `run()` loop resets the position at the top
    // of every render, so a second render's `useState` at the same
    // position reads back what the first wrote. Follow the rule of
    // hooks — call in the same order every render, never inside
    // `if`/loops, or the position skews and you read the wrong slot.
    // See docs/src/tui/hooks.md for the full writeup and the ink
    // equivalence table.

    /**
     * Per-frame state cell. Returns `[value, setter]`, matching
     * React/ink's shape — `const [count, setCount] = useState(0)` is
     * recognized by the compiler and lowered to a real 2-element
     * array; calling `setCount` writes through to the slot and
     * triggers a re-render on the next tick if the value changed
     * (bit-identical writes are a no-op).
     *
     * The setter captured by a `useInput` handler reads from *that
     * frame's* closure, not from the live slot — if you need a
     * functional update inside a handler that may fire multiple
     * times per frame (e.g. pasted input), mirror the value in a
     * `useRef` instead.
     */
    export function useState<T>(initial: T): [T, (value: T) => void];

    /**
     * Run a side effect after first render, and again whenever an
     * element of `deps` changes (compared by bit-identity, like
     * `Object.is`). Omitting `deps` runs the effect on every render;
     * passing `[]` runs it once, on mount only.
     *
     * The effect closure runs synchronously inside the component
     * call. Cleanup-on-dep-change (returning a cleanup function from
     * `fn`) is not wired yet — a returned function is ignored.
     */
    export function useEffect(fn: () => void | (() => void), deps?: readonly unknown[]): void;

    /**
     * Cache the result of `fn()` keyed by `deps` (same bit-identity
     * comparison as `useEffect`). Recomputes on first call or when
     * `deps` change; otherwise returns the cached value.
     */
    export function useMemo<T>(fn: () => T, deps: readonly unknown[]): T;

    /**
     * A stable mutable cell that doesn't trigger a re-render when
     * written. Identity is stable across renders — `useRef` at the
     * same call-site position always returns the same handle, so
     * closures captured in `useEffect` / `useInput` see the latest
     * value through `.get()`.
     *
     * Unlike ink/React, there's no `.current` property: `.get()`
     * reads, `.set(v)` writes.
     */
    export function useRef<T>(initial: T): RefHandle<T>;

    /** Handle returned by `useRef`. */
    export interface RefHandle<T> {
        get(): T;
        /** Writes the cell. Does NOT trigger a re-render. */
        set(value: T): void;
    }

    /**
     * Imperative handle for the running `run()` loop. Stable across
     * renders — every `useApp()` call returns the same singleton
     * handle, so it's safe to stash in a `useRef` for a callback that
     * outlives the render.
     */
    export function useApp(): AppHandle;

    /** Handle returned by `useApp`. */
    export interface AppHandle {
        /** Tells `run()` to stop at the top of the next iteration. */
        exit(): void;
        /**
         * Blocks (synchronously, on the calling thread) until
         * `exit()` has been called. Safe to `await` — Perry resolves
         * an `await` of a non-promise value immediately.
         */
        waitUntilExit(): void;
    }

    /**
     * Terminal dimensions and a raw-write escape hatch. Stable
     * singleton handle, like `useApp()`.
     */
    export function useStdout(): StdoutHandle;

    /** Handle returned by `useStdout`. */
    export interface StdoutHandle {
        /** Write raw bytes to stdout, bypassing the cell-grid diff. */
        write(s: string): void;
        /** Terminal width in cells. Falls back to 80 when not a TTY. */
        columns(): number;
        /** Terminal height in cells. Falls back to 24 when not a TTY. */
        rows(): number;
    }

    /**
     * Register the calling widget as a focus candidate in the Tab
     * cycle. Returns `1` when this widget currently has focus, `0`
     * otherwise — treat as truthy/falsy, there's no boolean wrapper.
     *
     * - `autoFocus`: pass `1` for exactly one widget to take focus on
     *   the first render. Later `useFocus` calls with `autoFocus=1`
     *   are ignored once focus has been claimed.
     * - `isActive`: pass `0` to remove this widget from the Tab cycle
     *   (e.g. a disabled field).
     *
     * Tab / Shift-Tab cycle focus automatically — the run loop's
     * input drain handles `\x09` / `\x1b[Z` before forwarding the
     * byte chunk to `useInput`.
     */
    export function useFocus(autoFocus: number, isActive: number): number;

    /**
     * Imperative focus control, for driving the Tab cycle from code
     * instead of (or in addition to) the keyboard. Stable singleton
     * handle, like `useApp()`.
     */
    export function useFocusManager(): FocusManagerHandle;

    /** Handle returned by `useFocusManager`. */
    export interface FocusManagerHandle {
        focusNext(): void;
        focusPrevious(): void;
        /** Focus a specific widget by its 1-based registration-order id. */
        focus(id: number): void;
    }
}
