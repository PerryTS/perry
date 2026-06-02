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
}
