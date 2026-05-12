# Hooks

`perry/tui` implements the React-shape hook API on top of a call-site-indexed slot pool. Each `useXxx` call gets the slot at its position in the component body; the run loop resets the index at the top of every frame, so the second render's `useState` at the same position reads back what the first wrote.

This is the same rule-of-hooks model ink/React use: **call hooks in the same order on every render**. Don't call them inside `if`/loops — the slot index would skew and you'd read the wrong slot. Slot kinds are tagged (`State` / `Effect` / `Memo` / `Ref` / `Focus`); calling `useState` at a position previously used by `useMemo` re-tags the slot rather than corrupting it, but the value resets.

## `useState(initial)`

Per-frame state cell. Returns `[value, setter]`.

```typescript,no-test
const [count, setCount] = useState(0);
// Later, from an input handler:
setCount(count + 1);
```

The setter writes through to the slot's bits and flips a global `STATE_DIRTY` flag — the run loop sees it after `useInput` drains and immediately re-renders without sleeping.

Setting the same value twice (bit-identical) is a no-op — `STATE_DIRTY` stays clear and the loop idles. This avoids the "render storm" pattern where unconditional `setX(prev)` calls would loop forever.

### Stale-closure gotcha

The setter captured by a `useInput` handler reads `value` from **that frame's closure**, not from the slot. If many bytes arrive in one frame (paste, typing fast), the handler fires N times with the same `value`:

```typescript,no-test
const [n, setN] = useState(0);
useInput((s) => { if (s === "+") setN(n + 1); });
// User pastes "+++" — handler fires 3× with n=0, all three set the slot to 1.
```

If you need a functional setter for this case, use `useRef` as a mirror:

```typescript,no-test
const buf = useRef("");
const [, redraw] = useState(0);
useInput((s) => {
    if (s.length === 1 && s >= " " && s <= "~") {
        buf.set(buf.get() + s);     // canonical buffer (no stale capture)
        redraw(buf.get().length);   // trigger re-render
    }
});
```

## `useEffect(fn, deps?)`

Run a side effect after first render, and again whenever a dep changes.

```typescript,no-test
useEffect(() => {
    // Run-once on mount.
    fetchInitialData();
}, []);

useEffect(() => {
    // Re-run whenever `query` changes.
    runSearch(query);
}, [query]);

useEffect(() => {
    // No deps array → run every render. Rarely what you want.
});
```

Deps are compared by bit-identity using an FNV-1a hash of the deps' NaN-boxed values. An empty array `[]` hashes to a stable non-zero value, giving the React "run once" behaviour; passing no array runs the effect every render.

The effect closure runs synchronously inside the component call. Cleanup-on-dep-change (returning a cleanup function) is **not** wired yet — the return value is ignored.

## `useMemo(fn, deps)`

Cache the result of `fn()` keyed by `deps`. Same hash convention as `useEffect`.

```typescript,no-test
const sorted = useMemo(
    () => items.slice().sort((a, b) => a.priority - b.priority),
    [items],
);
```

Recomputes on first call or when `deps` change. Otherwise returns the cached value.

## `useRef(initial)`

A stable mutable cell that doesn't trigger re-renders. Use for values you want to mutate but don't want to drive the UI.

```typescript,no-test
const renderCount = useRef(0);
renderCount.set(renderCount.get() + 1);   // does NOT flip STATE_DIRTY
```

`.get()` reads, `.set(v)` writes. Identity is stable across renders — calling `useRef(0)` at the same position returns the same handle every time, so closures captured in `useEffect` / `useInput` always see the latest value.

Common pattern: use `useRef` as the canonical buffer for input that gets typed at terminal speed, and pair with a throwaway `useState` to trigger redraws (see the stale-closure gotcha above).

## `useApp()`

Returns a handle for imperative control of the run loop.

```typescript,no-test
const app = useApp();
// Later:
app.exit();                    // tells run() to break at the top of the next iteration
await app.waitUntilExit();     // blocks until EXIT_FLAG is set (rare; usually `run` itself blocks)
```

The handle is stable — calling `useApp()` on every render returns the same singleton. Wrap it in `useRef` if you want to stash it for a callback that outlives the render.

## `useStdout()`

Terminal dimensions and a raw-write escape hatch.

```typescript,no-test
const stdout = useStdout();
const cols = stdout.columns();    // terminal width in cells (falls back to 80 if not a TTY)
const rows = stdout.rows();       // height in cells (fallback 24)
stdout.write("raw bytes\n");      // bypass the cell-grid diff
```

Use `columns`/`rows` to size dividers, truncate content to fit, or pick a layout direction. `write` is rarely needed — almost everything should go through widgets so the cell-grid diff can render it efficiently.

## `useFocus(autoFocus, isActive)`

Register the calling widget as a focus candidate. Returns `1.0` when this widget is the currently focused one, else `0.0` (treat as truthy/falsy).

```typescript,no-test
const isFocused = useFocus(1 /* autoFocus */, 1 /* isActive */);
return Box({ flexDirection: "row" }, [
    Text("> ", isFocused ? { color: "cyan", bold: true } : { dimColor: true }),
    Text("name input"),
]);
```

- `autoFocus`: pass `1` for one widget to take focus on first render. Subsequent `useFocus` calls with `autoFocus=1` are ignored once focus has been claimed.
- `isActive`: pass `0` to remove this widget from the Tab cycle (e.g. a disabled field).

Tab and Shift-Tab cycle focus automatically — no boilerplate. The run loop's input drain handles the `\x09` / `\x1b[Z` byte sequences before forwarding them to your `useInput` handler.

For imperative focus control, pair with `useFocusManager()`:

```typescript,no-test
const focus = useFocusManager();
// Later:
focus.focusNext();
focus.focusPrevious();
focus.focus(id);   // by focus id (1-based, in registration order)
```

## `useInput(handler)`

Register a keypress handler. Called once per byte chunk arriving on stdin, in raw mode.

```typescript,no-test
useInput((s: string) => {
    if (s === "\x03") app.exit();               // Ctrl+C
    if (s === "\r" || s === "\n") onSubmit();   // Enter
    if (s === "\x7f" || s === "\b") onErase();  // Backspace
    if (s === "\x1b[A") onUpArrow();            // ANSI up
    if (s.length === 1 && s >= " " && s <= "~") onPrintable(s);
});
```

The `s` argument is the raw byte chunk as a string. ANSI escape sequences like arrow keys arrive as a single chunk (`\x1b[A`, `\x1b[B`, `\x1b[C`, `\x1b[D`); printable characters as one byte; control codes (Ctrl+C, Tab, Enter, Backspace) as their literal byte.

**Tab handling**: Tab (`\x09`) and Shift-Tab (`\x1b[Z`) cycle the focus ring *before* the handler is called. The handler still sees the byte, so you can branch on it if you want custom Tab behaviour — but for the typical "Tab moves focus" case the framework already did it.

Only one handler is registered at a time (last `useInput` call wins). For multiple focusable widgets, dispatch from one handler by checking `useFocus`'s return.

## Equivalence with ink

| ink | `perry/tui` | Notes |
|---|---|---|
| `useState(0)` | `useState(0)` | Identical. |
| `useEffect(fn, [])` | `useEffect(fn, [])` | Cleanup return not yet wired. |
| `useMemo(fn, [])` | `useMemo(fn, [])` | Identical. |
| `useRef(0)` | `useRef(0)` | `.get()`/`.set(v)` instead of `.current`. |
| `useApp().exit()` | `useApp().exit()` | Identical. |
| `useStdout().columns` (prop) | `useStdout().columns()` (method) | Function call, not property. |
| `useFocus({ autoFocus })` | `useFocus(1, 1)` | Positional args. |
| `useInput(handler)` | `useInput(handler)` | Same signature; raw byte chunks. |
| `<App />` | `run(() => App())` | JSX user components work (`<App />` lowers to `App(props)`); built-in `<Box>` JSX is still deferred. |
