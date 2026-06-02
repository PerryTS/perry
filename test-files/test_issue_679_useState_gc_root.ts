// Regression test for #679 follow-up — `tui::hooks::SLOTS` and
// `tui::state::SLOTS` weren't registered as GC roots, so a JS array
// stashed via `setMessages(...)` could be reclaimed before the next
// render read it back.
//
// Repro shape: store an array into useState, allocate a bunch of
// pressure to trigger a GC, re-read the state, assert the array
// elements are still intact. Pre-fix, the second read returned a
// dangling pointer and `.length` either crashed or returned garbage.

import { useState, useRef, run, Box, Text, useInput, exit } from "perry/tui";

let postLoop_length = -1;
let postLoop_first = "MISSING";
let postLoop_last = "MISSING";

run(() => {
    const [arr, setArr] = useState([] as string[]);
    const tickRef = useRef(0);

    // Frame 1: tickRef.get() == 0 → seed the slot with a 5-element
    // array. Each entry is a freshly-allocated string so the GC has
    // real heap objects to reclaim if it loses track of the parent
    // array.
    if (tickRef.get() === 0) {
        const seeded = [
            "first-" + Math.random(),
            "second-" + Math.random(),
            "third-" + Math.random(),
            "fourth-" + Math.random(),
            "last-" + Math.random(),
        ];
        setArr(seeded);
        tickRef.set(1);
    } else if (tickRef.get() === 1) {
        // Frame 2: read the array back. Allocate enough garbage to
        // force a minor GC mid-flight, then sample length + ends.
        const len = arr.length;
        // Garbage churn — 1000 throwaway arrays + strings. Triggers
        // gc_check_trigger on the malloc-count threshold.
        for (let i = 0; i < 1000; i = i + 1) {
            const tmp = ["garbage", "more-garbage-" + i, "another-" + (i * 2)];
            if (tmp.length === -1) console.log("unreachable"); // keep tmp live
        }
        // Now re-read.
        postLoop_length = arr.length;
        if (arr.length === 5) {
            postLoop_first = arr[0];
            postLoop_last = arr[4];
        }
        tickRef.set(2);
        exit();
    }
    useInput((_s: string) => {});
    return Box([Text("len=" + arr.length)]);
});

console.log("LENGTH=" + postLoop_length);
console.log("FIRST_STARTS_WITH_first=" + (postLoop_first.indexOf("first-") === 0));
console.log("LAST_STARTS_WITH_last=" + (postLoop_last.indexOf("last-") === 0));
