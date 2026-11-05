// Regression test for #358 Phase 4.5: Spinner / Input / List / Select /
// TextArea widget set completion. Closes out the v1 widget surface
// from the issue's original spec.
//
// Each widget is exercised as a one-shot render — interactive variants
// (Input editing, Select navigation) work end-to-end via useInput +
// state.set, but the parity runner can't drive them, so this test
// covers the pure-render side. The widgets themselves are all "render
// a snapshot of the prop value" — interactivity is the user's
// responsibility (matches Ink's hooks-and-render-from-state model).

import {
    Box,
    Text,
    Spinner,
    Input,
    List,
    Select,
    TextArea,
    render,
} from "perry/tui";

// 1. Spinner — frame 0 = '-', frame 1 = '\', frame 2 = '|', frame 3 = '/'.
render(Box([Text("Spinner frames:"), Spinner(0), Spinner(1), Spinner(2), Spinner(3)]));
console.log("\n=== spinner done ===");

// 2. Input — shows value with trailing cursor character.
render(Box([Text("Name input:"), Input("alice")]));
console.log("\n=== input done ===");

// 3. List — vertical stack with selected row highlighted (reverse video).
const items = ["one", "two", "three", "four"];
render(Box([Text("Choose:"), List(items, 2)]));
console.log("\n=== list done ===");

// 4. Select — same as List but with non-negative selection enforced.
render(Box([Text("Pick:"), Select(items, 0)]));
console.log("\n=== select done ===");

// 5. TextArea — multi-line text, one Text per line.
render(Box([Text("Notes:"), TextArea("first line\nsecond line\nthird line")]));
console.log("\n=== textarea done ===");
