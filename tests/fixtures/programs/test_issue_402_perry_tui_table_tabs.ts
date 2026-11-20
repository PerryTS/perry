// Regression test for #402 perry/tui v1.5: Table + Tabs widgets.

import { Box, Text, Table, Tabs, render } from "perry/tui";

// 1. Table — header row bold, selected row reverse-video.
render(Table({
    headers: ["Name", "Age", "City"],
    rows: [
        ["Alice", "30", "NYC"],
        ["Bob",   "25", "SF"],
        ["Carol", "40", "Berlin"],
    ],
    selected: 1,
}));
console.log("\n=== table done ===");

// 2. Tabs — second tab active, body widget mounted below the bar.
render(Tabs({
    tabs: ["Files", "Search", "Help"],
    active: 1,
    body: [
        Text("file body"),
        Box([Text("search body line 1"), Text("search body line 2")]),
        Text("help body"),
    ],
}));
console.log("\n=== tabs done ===");

// 3. Table with no selection (selected = -1).
render(Table({
    headers: ["A", "BB", "CCC"],
    rows: [["1", "22", "333"]],
}));
console.log("\n=== unselected table done ===");

// 4. Tabs with active = 0.
render(Tabs({
    tabs: ["one", "two"],
    active: 0,
    body: [Text("first"), Text("second")],
}));
console.log("\n=== first-tab done ===");
