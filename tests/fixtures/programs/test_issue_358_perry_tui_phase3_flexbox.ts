// Regression test for #358 Phase 3: Taffy flexbox layout.
// Verifies that:
//   1. `Box({ flexDirection: "row" }, [Text("a"), Text("b")])`
//      lays children horizontally (vs. the v0.1 vertical default).
//   2. `Box({ gap: 2 }, [...])` inserts blank cells between children.
//   3. `Box({ padding: 1 }, [...])` offsets children from the box origin.
//   4. The same Box([...]) shape (no style) still works as before.
//
// Output is the rendered ANSI byte stream; we grep for specific
// move-to escapes that confirm Taffy placed children at the expected
// (row, col) — proves the layout pass is wired and the flexbox
// solver is producing rects, not just falling back to the v0.1
// vertical stack.

import { Box, Text, render } from "perry/tui";

// Test 1: row direction puts children side by side.
const row = Box({ flexDirection: "row" }, [Text("ab"), Text("cd")]);
render(row);
console.log("\n--row done--");

// Test 2: column with gap.
const col = Box({ flexDirection: "column", gap: 1 }, [Text("xy"), Text("zw")]);
render(col);
console.log("\n--col done--");

// Test 3: padding offsets the first child.
const padded = Box({ padding: 2 }, [Text("hi")]);
render(padded);
console.log("\n--padded done--");

// Test 4: bare Box() with no style behaves as v0.1 column.
const plain = Box([Text("p1"), Text("p2")]);
render(plain);
console.log("\n--plain done--");
