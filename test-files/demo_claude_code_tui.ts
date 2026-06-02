// Demo: a Claude-Code-style TUI built entirely on perry/tui.
import {
    Box, Text, Spinner,
    useState, useEffect, useInput, useApp, useStdout, useRef,
    run,
} from "perry/tui";

const CANNED: string[] = [
    "Sure, that's a perry/tui demo — nothing is actually wired up.",
    "Read the file, check for null, write a test. Standard playbook.",
    "Got it. Anything else you want me to look at?",
    "Run `perry compile` and watch the diff. Should match Node byte-for-byte.",
    "TODO: real LLM behind this. For now I'm just cycling canned strings.",
];

const MODES: string[] = ["welcome", "normal", "debug"];

run(() => {
    const app = useApp();
    const stdout = useStdout();
    const [messages, setMessages] = useState([] as string[]);
    const inputRef = useRef("");
    const [, redraw] = useState(0);
    const [mode, setMode] = useState(0);
    const [tick, setTick] = useState(0);

    useEffect(() => {
        setMessages([
            "[bot] Hi! I'm not a real LLM — just a TUI demo. Try typing.",
            "[bot] /  cycles modes · Enter sends · Ctrl+C quits.",
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
            redraw(tick + 1);
            return;
        }
        if (s === "\x7f" || s === "\b") {
            const buf = inputRef.get();
            if (buf.length > 0) { inputRef.set(buf.substring(0, buf.length - 1)); redraw(buf.length - 1); }
            return;
        }
        if (s === "/") { setMode((mode + 1) % MODES.length); return; }
        if (s.length === 1) {
            const c = s.charCodeAt(0);
            if (c >= 0x20 && c <= 0x7e) { inputRef.set(inputRef.get() + s); redraw(c); }
        }
    });

    const cols = stdout.columns();
    const header = Box({ flexDirection: "row", padding: { top: 0, bottom: 1, left: 0, right: 0 } }, [
        Text("Perry-Code (demo)", { bold: true, color: "cyan" }),
        Text("  mode=" + MODES[mode], { dimColor: true, italic: true }),
    ]);
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
    const help = Text("Enter=send · Backspace=erase · /=cycle mode · Ctrl+C=quit", { dimColor: true });
    return Box({ flexDirection: "column", padding: 1 }, [header, history, divider, promptRow, help]);
});

console.log("\nbye!");
