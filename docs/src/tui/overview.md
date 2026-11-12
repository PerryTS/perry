# Terminal UI Overview

`perry/tui` is a native terminal-UI engine built into the Perry runtime. It targets the same use cases as [ink](https://github.com/vadimdemedes/ink) (interactive CLIs, dashboards, REPLs, log viewers) but compiles to native code — no Node, no React reconciler, no fiber tree. Your code runs as a single static binary that does a double-buffered ANSI diff each frame.

## When to use `perry/tui`

| You want… | Use |
|---|---|
| An interactive CLI tool (prompts, menus, live progress) | **`perry/tui`** |
| A long-running terminal dashboard / log viewer | **`perry/tui`** |
| A native desktop / mobile app | [`perry/ui`](../ui/overview.md) |
| A one-shot script that just prints to stdout | Plain `console.log` |

`perry/tui` enters the terminal's [alternate screen buffer](https://invisible-island.net/xterm/ctlseqs/ctlseqs.html#h2-The-Alternate-Screen-Buffer) (so your scrollback isn't polluted), captures raw-mode keypresses, and re-renders only the cells that changed between frames. The cell grid is a packed `Vec<Cell>` so an 80×24 terminal fits in ~15 KB — well within L2.

## Quick Start

The smallest interactive `perry/tui` program — a counter that increments on `+`, decrements on `-`, and quits on `q`:

```typescript,no-test
import { Box, Text, useState, useInput, run, exit } from "perry/tui";

run(() => {
    const [n, setN] = useState(0);

    useInput((s: string) => {
        if (s === "+") setN(n + 1);
        if (s === "-") setN(n - 1);
        if (s === "q") exit();
    });

    return Box([Text("count: " + n)]);
});
```

Compile and run:

```bash
perry compile app.ts -o app && ./app
```

The component closure is called every render. Hooks (`useState`/`useInput`/etc.) bind to a per-frame call-site index so the second render's `useState(0)` at the same position reads back what the first render wrote — same model as React. The run loop re-renders when any state setter is called and idles between renders.

## Mental Model

Perry's TUI uses the same authoring model as ink:

- **Components are functions** that return a widget tree. The function is called every render; the tree it returns is diffed against the previous frame's tree and only changed terminal cells get rewritten.
- **State lives in hooks** (`useState`, `useRef`, `useMemo`). A change triggers a re-render automatically.
- **Layout uses flexbox** (powered by [Taffy](https://github.com/DioxusLabs/taffy)) — `flexDirection: "row" | "column"`, `gap`, `padding`, `justifyContent`, `alignItems`, `flexGrow`, etc.

If you've used ink, the only real difference at the surface is the **factory call form** — `Box({…opts}, [children])` instead of `<Box>…</Box>` JSX. JSX works for user-defined component functions today (`<App />` calls `App(props)`), but the `<Box>` / `<Text>` intrinsics still need the function-call form until a compile-time JSX→intrinsic rewriter lands.

## Architecture in one paragraph

`run(component)` enters the alt screen, enables raw mode on stdin, spawns a reader thread, and loops: advance Perry's existing event pump → clear the previous dirty flag and reset the hook index → call the component closure → diff and flush the widget tree → drain pending keypresses → if any state changed, immediately re-render; else idle 16 ms. The pump advances promise jobs, timers and registered stdlib completions without requiring keyboard input. It runs outside component construction; its return count does not gate rendering or future polling. Updates made during a frame or input dispatch remain dirty for the next iteration. Exit requests are checked after pumping and before dispatch/idle so an async callback can stop the loop without constructing another frame. On normal exit, raw mode is restored and the alternate screen is left. This does not add recovery from an uncaught exception or process abort, nor change the existing polling cadence.

## What's next

- [Widgets](widgets.md) — `Box`, `Text`, `Input`, `List`, `Select`, `Spinner`, `ProgressBar`, `Table`, `Tabs`, and the per-widget style props.
- [Hooks](hooks.md) — `useState`, `useEffect`, `useMemo`, `useRef`, `useApp`, `useStdout`, `useFocus`, `useInput`.
- [Examples](examples.md) — counter, chat REPL, file picker, log viewer.
