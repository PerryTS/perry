// Regression tests for #403 (AnimatedSpinner) and #404 (Input cursor
// positioning). One-shot render — the animation only animates inside
// run(), but the snapshot shape is what we care about for parity.

import { Box, Text, AnimatedSpinner, Input, render } from "perry/tui";

// 1. AnimatedSpinner with default opts → renders one of "-\|/" (the
//    exact frame depends on process elapsed time; we just verify the
//    widget spawns and renders something non-empty).
render(Box([Text("Loading"), AnimatedSpinner()]));
console.log("\n=== animated default done ===");

// 2. AnimatedSpinner with custom frames + interval.
render(Box([
    Text("Custom"),
    AnimatedSpinner({ interval: 80, frames: ["⠋", "⠙", "⠹", "⠸"] }),
]));
console.log("\n=== animated custom done ===");

// 3. Input with cursor at start.
render(Input("hello world", 0));
console.log("\n=== cursor 0 done ===");

// 4. Input with cursor in the middle.
render(Input("hello world", 6));
console.log("\n=== cursor 6 done ===");

// 5. Input with cursor at end (== value.length).
render(Input("hello", 5));
console.log("\n=== cursor end done ===");

// 6. Input 1-arg form — cursor as `_` at end (unchanged behavior).
render(Input("legacy"));
console.log("\n=== legacy 1-arg done ===");
