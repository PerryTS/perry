// Regression test for #358 Phase 4: Spacer + ProgressBar widgets +
// the issue's acceptance-criterion-2 no-flicker proof.
//
// Acceptance criterion 2 from the issue:
//   "A streaming-log viewer that appends 1000 lines/sec to a
//    scrollable region shows no visible flicker on Terminal.app /
//    iTerm2 / Windows Terminal / GNOME Terminal."
//
// We can't measure visible flicker in a non-interactive parity test,
// but we CAN verify the cell-level diff is doing its job: when only
// one row changes between frames, the second frame's emitted ANSI
// must contain only the changed row's move_to + new content — never
// a re-emission of unchanged rows.
//
// The test renders three frames:
//   Frame 1: ["log 1", "log 2", "log 3"]
//   Frame 2: ["log 1", "log 2", "log 3", "log 4"]   (one row appended)
//   Frame 3: ["log 1", "log 2", "log 3", "log 4", "log 5"] (another)
//
// Frames 2 and 3 should each emit move_to(row N+1) + the new row's
// text only. Rows 1..N should NOT appear in their emitted ANSI.
//
// Plus widget smoke: render a Box containing Spacer + ProgressBar +
// some Text to exercise the new Phase 4 FFI.

import { Box, Text, Spacer, ProgressBar, render } from "perry/tui";

// Frame 1 — 3 lines.
const frame1 = Box([Text("log 1"), Text("log 2"), Text("log 3")]);
render(frame1);
console.log("\n=== frame1 done ===");

// Frame 2 — append "log 4". Only row 4 should be touched.
const frame2 = Box([
    Text("log 1"),
    Text("log 2"),
    Text("log 3"),
    Text("log 4"),
]);
render(frame2);
console.log("\n=== frame2 done ===");

// Frame 3 — append "log 5". Only row 5 should be touched.
const frame3 = Box([
    Text("log 1"),
    Text("log 2"),
    Text("log 3"),
    Text("log 4"),
    Text("log 5"),
]);
render(frame3);
console.log("\n=== frame3 done ===");

// Phase 4 widget smoke — Spacer + ProgressBar render in a row layout.
const widgets = Box({ flexDirection: "row", gap: 1 }, [
    Text("loading"),
    Spacer(),
    ProgressBar(3, 10, 10),
]);
render(widgets);
console.log("\n=== widgets done ===");
