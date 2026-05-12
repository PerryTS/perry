# Examples

End-to-end `perry/tui` programs covering the typical interactive-CLI shapes. Each example also lives in `test-files/test_perry_tui_inkcompat_*.ts` in the repo and is exercised by CI on every PR.

## Counter

The smallest meaningful program: `+` / `-` increment/decrement, `q` quits, the count renders to one row.

```typescript,no-test
import { Box, Text, useState, useInput, run, exit } from "perry/tui";

// Captured so we can print the final count after run() returns.
let finalValue = 0;

run(() => {
    const [n, setN] = useState(0);
    finalValue = n;

    useInput((s: string) => {
        if (s === "+") setN(n + 1);
        if (s === "-") setN(n - 1);
        if (s === "q") exit();
    });

    return Box([Text("count: " + n)]);
});

console.log("FINAL=" + finalValue);
```

Pipe `+++-q` and the program prints `FINAL=2`. The `useEffect`-less `useState(0)` initialises the slot on first frame; the handler captures `n` from the frame it was registered in, so each setter call computes from the value that frame saw.

## Chat REPL with stable input buffer

A Claude-Code-shaped chat UI: header row, message history, prompt with cursor, help footer. Demonstrates `useState` for the message list, `useRef` as a stale-closure-resistant input buffer, `useInput` for keypresses, `useApp().exit()` for Ctrl+C handling.

```typescript,no-test
import {
    Box, Text, Spinner,
    useState, useEffect, useInput, useApp, useStdout, useRef,
    run,
} from "perry/tui";

const CANNED = [
    "Sure, I can help with that.",
    "Read the file, check for null, write a test.",
    "Got it. Anything else?",
];

run(() => {
    const app = useApp();
    const stdout = useStdout();
    const [messages, setMessages] = useState([] as string[]);
    const inputRef = useRef("");
    const [, redraw] = useState(0);
    const [tick, setTick] = useState(0);

    useEffect(() => {
        setMessages([
            "[bot] Hi! Type a message and press Enter. Ctrl+C quits.",
        ]);
    }, []);

    useInput((s: string) => {
        if (s === "\x03") { app.exit(); return; }
        if (s === "\r" || s === "\n") {
            const buf = inputRef.get();
            if (buf.length === 0) return;
            const reply = CANNED[messages.length % CANNED.length];
            setMessages(messages.concat(["[you] " + buf, "[bot] " + reply]));
            inputRef.set("");
            setTick(tick + 1);
            return;
        }
        if (s === "\x7f" || s === "\b") {
            const buf = inputRef.get();
            if (buf.length > 0) {
                inputRef.set(buf.substring(0, buf.length - 1));
                redraw(buf.length - 1);
            }
            return;
        }
        if (s.length === 1) {
            const c = s.charCodeAt(0);
            if (c >= 0x20 && c <= 0x7e) {
                inputRef.set(inputRef.get() + s);
                redraw(c);
            }
        }
    });

    const cols = stdout.columns();
    const rows = messages.map((m: string) => {
        const isUser = m.indexOf("[you]") === 0;
        return Text(m, { color: isUser ? "yellow" : "green" });
    });
    const history = Box({ flexDirection: "column", flexGrow: 1 }, rows);

    let bar = "";
    for (let i = 0; i < cols - 2; i = i + 1) bar = bar + "─";
    const divider = Text(bar, { dimColor: true });

    const promptRow = Box({ flexDirection: "row" }, [
        Spinner(tick),
        Text(" › " + inputRef.get(), { bold: true }),
        Text("█", { color: "cyan" }),
    ]);

    return Box({ flexDirection: "column", padding: 1 }, [
        Text("Perry-Code (demo)", { bold: true, color: "cyan" }),
        history,
        divider,
        promptRow,
        Text("Enter=send · Backspace=erase · Ctrl+C=quit", { dimColor: true }),
    ]);
});
```

The key insight is `inputRef` as the canonical buffer: when the user types fast (or pastes), many bytes arrive in one frame; the handler fires N times with the same stale `input` if it lived in `useState`. `useRef.set()` mutates the cell directly, so each byte builds on the previous; the throwaway `redraw` `useState` just flips `STATE_DIRTY` so the loop repaints.

## Multi-step prompt with `useFocus`

A two-field form (name, email) with Tab/Shift-Tab navigation. Demonstrates `useFocus` with `autoFocus` + the automatic Tab cycling.

```typescript,no-test
import { Box, Text, useState, useFocus, useInput, run, exit } from "perry/tui";

run(() => {
    const nameFocused = useFocus(1, 1);  // auto-focus first
    const emailFocused = useFocus(0, 1);

    const [name, setName] = useState("");
    const [email, setEmail] = useState("");

    useInput((s: string) => {
        if (s === "\x03") exit();
        // Tab/Shift-Tab handled by the runtime — we don't see them here
        // unless we want to (they DO get dispatched after focus cycles).
        if (s.length === 1 && s >= " " && s <= "~") {
            if (nameFocused) setName(name + s);
            else if (emailFocused) setEmail(email + s);
        }
    });

    return Box({ flexDirection: "column", padding: 1, gap: 1 }, [
        Box({ flexDirection: "row" }, [
            Text(nameFocused ? "▸ " : "  ", { color: "cyan" }),
            Text("Name:  " + name),
        ]),
        Box({ flexDirection: "row" }, [
            Text(emailFocused ? "▸ " : "  ", { color: "cyan" }),
            Text("Email: " + email),
        ]),
        Text("Tab to switch · Ctrl+C to quit", { dimColor: true }),
    ]);
});
```

## Log viewer with `useStdout`

Sizes content to the terminal width using `useStdout().columns()`. Truncates each log line to fit; uses `useEffect` with `[]` to seed log data on mount.

```typescript,no-test
import { Box, Text, useState, useEffect, useStdout, useInput, run, exit } from "perry/tui";

run(() => {
    const stdout = useStdout();
    const cols = stdout.columns();
    const [lines, setLines] = useState([] as string[]);

    useEffect(() => {
        setLines([
            "2026-05-11 09:01:23 INFO  api-east started",
            "2026-05-11 09:01:24 INFO  api-west started",
            "2026-05-11 09:01:25 WARN  api-east latency degraded (412ms)",
            "2026-05-11 09:01:26 ERROR api-east connection refused",
        ]);
    }, []);

    useInput((s: string) => {
        if (s === "q" || s === "\x03") exit();
    });

    const rows = lines.map((line: string) => {
        const truncated = line.length > cols - 2 ? line.substring(0, cols - 5) + "..." : line;
        const color = line.indexOf("ERROR") >= 0 ? "red"
                    : line.indexOf("WARN") >= 0 ? "yellow"
                    : "white";
        return Text(truncated, { color });
    });

    return Box({ flexDirection: "column", padding: 1 }, rows);
});
```

## Notes on running these locally

```bash
perry compile myapp.ts -o myapp
./myapp
```

The binary enters the alt screen, takes over the terminal until you press Ctrl+C (or whatever exit key the program defines), and restores everything cleanly on exit. Your scrollback is untouched.

For piped (non-interactive) testing — useful for CI assertions — send your test input on stdin and grep stdout for the values your program prints after `run()` returns:

```bash
echo "+++-q" | ./myapp | grep "FINAL="
```

`run()` won't process inputs faster than the loop's 16 ms idle tick, so very fast piped input can deliver multiple bytes per frame. If your handler captures state via closure, design for that — either compute fresh state on every byte (with `useRef`) or accept that paste-style input behaves as a single bulk action.
