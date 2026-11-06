// Holistic test for the perry/tui Phase 3.5 + v1.5 batch:
// closes #405 (BoxStyle truecolor / per-side padding / pct units),
// #402 (Table + Tabs), #403 (AnimatedSpinner), #404 (Input cursor).
// #406 (Windows raw mode) is a platform-only change verified via the
// build step — the ANSI escapes this test emits are what arrives on
// stdout once VIRTUAL_TERMINAL_PROCESSING is enabled.
//
// One `render()` call paints a single coherent dashboard. The five
// new surfaces compose in one tree: a truecolor header with per-side
// padding, a Tabs strip with the active body mounted, a Table inside
// it with selection, an AnimatedSpinner, an Input with cursor, and a
// 50%/50% percentage-width row. That's the "thorough" part — every
// feature lands in the same widget tree and the renderer emits one
// frame the parity runner can diff against.
//
// Out of scope: the interactive `run()` + `useInput` flow. That needs
// a TTY to drive stdin and would hang in CI. A separate manual demo
// (e.g. `examples/perry-tui/dashboard.ts`) is the right home for the
// arrow-keys-cycle-tabs version of this same layout.

import {
    Box,
    Text,
    Table,
    Tabs,
    AnimatedSpinner,
    Input,
    render,
} from "perry/tui";

// Phase 3.5 styled header — truecolor fg `#ff8800` (orange-on-black)
// + bold + per-side padding `{ top: 1, right: 0, bottom: 1, left: 2 }`.
// The renderer emits `\x1b[0;1;38;2;255;136;0;48;2;0;26;51m...` for
// the SGR payload (#405). Width 60 cells matches the rest of the
// dashboard so the bg fill extends edge to edge.
const header = Box(
    {
        padding: { top: 1, right: 0, bottom: 1, left: 2 },
        width: 60,
    },
    [
        Text("Perry TUI Demo Dashboard", {
            fg: "#ff8800",
            bg: "#001a33",
            bold: true,
        }),
    ]
);

// Tabs (#402) with the third tab active. Only `body[2]` (the Table)
// is mounted; the other two body widgets exist in the args but never
// render — matching React/ink's null-render behaviour for inactive
// children.
const tabbedBody = Tabs({
    tabs: ["Files", "Search", "Help"],
    active: 2,
    body: [
        Text("(files body — not rendered)"),
        Text("(search body — not rendered)"),
        // Table (#402) — header row bold, selected row reverse-video,
        // column widths auto-fit longest cell per column.
        Table({
            headers: ["Name", "Age", "City"],
            rows: [
                ["Alice", "30", "NYC"],
                ["Bob", "25", "SF"],
                ["Carol", "40", "Berlin"],
            ],
            selected: 1,
        }),
    ],
});

// Status row — AnimatedSpinner (#403) + a static label, split 50/50
// across a 60-cell row (#405 percentage units). The spinner picks a
// frame from `process_elapsed % frames.length`; in a one-shot render
// that's whichever frame the snapshot captures, but the global
// 50 ms timer thread spawns regardless so the test also exercises
// the spawn path.
const statusRow = Box({ flexDirection: "row", width: 60 }, [
    Box({ width: "50%", flexDirection: "row", gap: 1 }, [
        Text("Loading"),
        AnimatedSpinner({
            interval: 80,
            frames: ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"],
        }),
    ]),
    Box({ width: "50%" }, [
        Text("active=Help  selected=Bob", { fg: "bright-black" }),
    ]),
]);

// Input row (#404 — the 2-arg `Input(value, cursor)` form).
// "who_is_thinking?" with cursor at index 11 puts the reverse-video
// cursor on the `k` of "thinking". The 1-arg legacy form (trailing
// `_` cursor) is exercised by the dedicated #404 smoke test.
const inputRow = Box({ flexDirection: "row", gap: 1 }, [
    Text("Search ›"),
    Input("who_is_thinking?", 11),
]);

// Two halves at 50% each in a 60-cell row — proves the Phase 3.5
// percent-units arithmetic still resolves correctly when nested
// inside a column-direction parent (the dashboard's outer Box).
const halves = Box({ flexDirection: "row", width: 60 }, [
    Box({ width: "50%" }, [
        Text("left half — 30 cells", { fg: "bright-cyan" }),
    ]),
    Box({ width: "50%" }, [
        Text("right half — 30 cells", { fg: "bright-magenta" }),
    ]),
]);

// Footer — keybinding hints in dim text, with `padding.top=1` to
// separate it from the halves row above. Demonstrates that per-side
// padding works on inner Boxes too, not just the top-level header.
const footer = Box(
    { padding: { top: 1, right: 0, bottom: 0, left: 2 } },
    [
        Text(
            "feat batch: #402 Table+Tabs · #403 AnimatedSpinner · #404 Input cursor · #405 BoxStyle Phase 3.5 · #406 Windows raw mode",
            { fg: "bright-black" }
        ),
    ]
);

// Compose. Outer column gap of 1 leaves a blank row between sections;
// outer padding of 1 gives the dashboard a one-cell margin on every
// side.
render(
    Box({ flexDirection: "column", gap: 1, padding: 1 }, [
        header,
        tabbedBody,
        statusRow,
        inputRow,
        halves,
        footer,
    ])
);

// Single trailing newline so the next shell prompt doesn't land on
// top of the footer's last cell.
console.log("");
