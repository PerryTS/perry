// Regression test for #405 Phase 3.5: BoxStyle additions —
// 24-bit truecolor on Text, per-side padding, flex-shrink, flex-basis,
// percentage units for width/height/flexBasis. Each section is a
// one-shot render — the parity runner can't drive interactive cases,
// but the styled emit lands in the diff stream and is byte-comparable.

import { Box, Text, render } from "perry/tui";

// 1. Text with truecolor fg + bg + bold. The renderer emits
//    `\x1b[0;1;38;2;255;136;0;48;2;0;0;0m...` which is what the eye sees
//    as orange-on-black bold.
render(Box([Text("truecolor", { fg: "#ff8800", bg: "#000000", bold: true })]));
console.log("\n=== truecolor done ===");

// 2. Named-palette colors still work unchanged.
render(Box([Text("named", { fg: "red", bg: "yellow" })]));
console.log("\n=== named done ===");

// 3. Per-side padding.
render(
    Box(
        { padding: { top: 1, left: 4, right: 0, bottom: 0 } },
        [Text("indented")]
    )
);
console.log("\n=== per-side padding done ===");

// 4. Percentage width — half the parent's width.
render(
    Box(
        { flexDirection: "row", width: 40 },
        [
            Box({ width: "50%" }, [Text("LEFT")]),
            Box({ width: "50%" }, [Text("RIGHT")]),
        ]
    )
);
console.log("\n=== percent width done ===");

// 5. flex-shrink + flex-basis.
render(
    Box(
        { flexDirection: "row", width: 30 },
        [
            Box({ flexBasis: 10, flexShrink: 1 }, [Text("ten")]),
            Box({ flexBasis: 20, flexShrink: 1 }, [Text("twenty")]),
        ]
    )
);
console.log("\n=== flex-basis done ===");
