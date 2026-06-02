# Widgets

`perry/tui` ships ~10 widgets that cover the typical interactive-CLI surface. All of them are factory functions returning a widget handle — pass them to `Box` as children, or to `render(widget)` / `run(() => widget)` as the root.

## `Box(opts?, children?)`

A flexbox container. Holds any number of children laid out by direction, gap, padding, and alignment rules.

```typescript,no-test
import { Box, Text } from "perry/tui";

// Bare children — vertical column by default.
Box([Text("first"), Text("second")]);

// With style.
Box({ flexDirection: "row", gap: 2, padding: 1 }, [
    Text("left"),
    Text("right"),
]);
```

### Style props

| Prop | Type | Notes |
|---|---|---|
| `flexDirection` | `"row" \| "column"` | Default `"column"`. |
| `justifyContent` | `"start" \| "center" \| "end" \| "space-between" \| "space-around"` | Main-axis distribution. |
| `alignItems` | `"start" \| "center" \| "end" \| "stretch"` | Cross-axis alignment. |
| `gap` | `number` | Cells of space between children. |
| `padding` | `number \| { top, right, bottom, left }` | Uniform or per-side. |
| `width` | `number \| string` | Cells, or `"50%"` of parent. |
| `height` | `number \| string` | Cells, or percent. |
| `flexGrow` | `number` | `1` = fill remaining space. |
| `flexShrink` | `number` | `1` = shrink when overflowing. `0` = never shrink. |
| `flexBasis` | `number \| string` | Base size before grow/shrink. |

Children can be a literal array (`[Text("a"), Text("b")]`) or any runtime expression that evaluates to an array — `messages.map(m => Text(m))` works the same.

## `Text(content, style?)`

A text node. Single-line; multi-line strings render with `\n` preserved.

```typescript,no-test
Text("plain");
Text("bold!", { bold: true });
Text("error", { color: "red", bold: true });
Text("subtle", { dimColor: true, italic: true });
Text("removed", { strikethrough: true });
Text("selected", { inverse: true });
Text("custom", { color: "#ff8800", backgroundColor: "#222" });
```

### Style props

| Prop | Type | SGR | Notes |
|---|---|---|---|
| `color` (alias `fg`) | named color or `#rrggbb` | 30-37 / 38;2 | Foreground. |
| `backgroundColor` (alias `bg`) | named color or `#rrggbb` | 40-47 / 48;2 | Background. |
| `bold` | `boolean` | 1 | |
| `dimColor` (alias `dim`) | `boolean` | 2 | |
| `italic` | `boolean` | 3 | |
| `underline` | `boolean` | 4 | |
| `inverse` (alias `reverse`) | `boolean` | 7 | Swaps fg/bg. |
| `strikethrough` | `boolean` | 9 | |

Named colors: `black`, `red`, `green`, `yellow`, `blue`, `magenta`, `cyan`, `white`, plus their `bright*` variants. Truecolor (`#rrggbb`) works on every modern terminal.

## `Spacer()`

A zero-content widget with `flexGrow: 1` baked in. Push siblings to the edges of a flex container without spelling out the grow factor:

```typescript,no-test
Box({ flexDirection: "row" }, [
    Text("left"),
    Spacer(),
    Text("right"),
]);
```

## `Input(value, cursor?)`

A single-line text-input widget. Render a string with an optional inline cursor position (0-indexed); pair with `useState` for the buffer and `useInput` to drive it.

```typescript,no-test
const [buf, setBuf] = useState("");
const [cur, setCur] = useState(0);
useInput((s) => { /* … update buf + cur on keypress … */ });
return Input(buf, cur);
```

`perry/tui` doesn't ship a full line editor — it gives you the rendering primitive and you wire the keys yourself. See the chat REPL in [Examples](examples.md) for a typical input loop.

## `TextArea(value)`

A multi-line text widget. Same shape as `Input` but accepts newlines.

## `List(items, selected?)`

A vertically-laid list of strings, with optional highlighted-row index.

```typescript,no-test
List(["Apple", "Banana", "Cherry"], 1);  // "Banana" highlighted
```

## `Select(items, selected?)`

Like `List` but with selection indicators (`▸` next to the focused row).

```typescript,no-test
const [idx, setIdx] = useState(0);
useInput((s) => {
    if (s === "\x1b[A" /* up */ ) setIdx(Math.max(0, idx - 1));
    if (s === "\x1b[B" /* down */) setIdx(Math.min(items.length - 1, idx + 1));
});
return Select(items, idx);
```

## `Spinner(frame)`

A static spinner character — `- \ | /` cycling through frames 0–3. Caller bumps `frame` from a state counter to animate.

```typescript,no-test
const [tick, setTick] = useState(0);
// On every Enter (or however you want to advance):
setTick(tick + 1);
return Box([Spinner(tick), Text(" working…")]);
```

`Spinner(0)` is a static `-` — useful as a stable bullet if you don't want animation.

For true wall-clock animation, see `AnimatedSpinner({ interval, frames })` which runs its own internal tick (it advances when the render loop polls between frames).

## `ProgressBar(filled, total, width?)`

A simple horizontal bar.

```typescript,no-test
ProgressBar(7, 10);          // ████████░░ at default width
ProgressBar(50, 100, 40);    // 40-cell wide bar
```

## `Table({ headers, rows, selected? })`

A bordered table. `headers` is a string array; `rows` is an array of string arrays.

```typescript,no-test
Table({
    headers: ["Name", "Status", "Latency"],
    rows: [
        ["api-east", "OK", "12ms"],
        ["api-west", "DEGRADED", "412ms"],
    ],
    selected: 1,
});
```

## `Tabs({ tabs, active, body })`

A horizontal tab bar over a body widget. `body` is an array parallel to `tabs` — only the active tab's body is rendered.

```typescript,no-test
const [active, setActive] = useState(0);
Tabs({
    tabs: ["Files", "Search", "Settings"],
    active,
    body: [filesView, searchView, settingsView],
});
```

---

For state + event hooks (the React-shape `useState`/`useInput`/`useApp`/etc.), see [Hooks](hooks.md). For complete worked examples, see [Examples](examples.md).
