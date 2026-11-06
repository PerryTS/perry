// #679 Phase 5: Text styling parity with ink.
//
// Verifies that the Text style props ink users expect lower to the
// right SGR sequences:
//   - color / fg   → fg
//   - backgroundColor / bg → bg
//   - bold         → SGR 1
//   - italic       → SGR 3
//   - underline    → SGR 4
//   - inverse      → SGR 7 (ink uses "inverse"; #358 used "reverse" — both
//                            accepted post-#679)
//   - dimColor     → SGR 2 (new in #679 Phase 5)
//   - strikethrough → SGR 9 (new in #679 Phase 5)
//
// We render one Box per style and grep for the expected SGR escape in
// the byte stream.

import { Box, Text, render } from "perry/tui";

render(Box([Text("bold!", { bold: true })]));
console.log("\n--bold--");

render(Box([Text("italic!", { italic: true })]));
console.log("\n--italic--");

render(Box([Text("underline!", { underline: true })]));
console.log("\n--underline--");

render(Box([Text("inverse!", { inverse: true })]));
console.log("\n--inverse--");

render(Box([Text("dim!", { dimColor: true })]));
console.log("\n--dim--");

render(Box([Text("strike!", { strikethrough: true })]));
console.log("\n--strike--");

render(Box([Text("combo!", { bold: true, dimColor: true, underline: true })]));
console.log("\n--combo--");

render(Box([Text("red!", { color: "red" })]));
console.log("\n--red--");

render(Box([Text("bg!", { backgroundColor: "yellow" })]));
console.log("\n--bg--");
