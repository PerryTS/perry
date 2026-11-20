// Regression test for #358 Phase 1: perry/tui Box + Text + render.
// Verifies that:
//   1. `import { Box, Text, render } from "perry/tui"` resolves
//      (NATIVE_MODULES + RUNTIME_ONLY_MODULES entries in
//      perry-hir/src/ir.rs).
//   2. `Text("hello")` lowers to `js_perry_tui_text` and returns a
//      widget handle.
//   3. `Box([child1, child2])` lowers to `js_perry_tui_box` +
//      N×`js_perry_tui_box_add_child`.
//   4. `render(root)` paints to stdout (compile + link + run).
//
// We can't pin byte-for-byte against Node here — Node has no
// "perry/tui" surface. The test instead pins exit code 0 and prints
// a marker line that's easy to search for in CI logs. The actual
// rendered ANSI escape sequences are unit-tested in
// `perry-runtime::tui::render::tests` (6 tests).

import { Box, Text, render } from "perry/tui";

const root = Box([Text("Hello"), Text("World")]);
render(root);

// Marker line — printed AFTER render() completes, on its own row,
// so the test runner sees it after any cell-grid output.
console.log("\nperry/tui render ok");
