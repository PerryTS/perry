// Regression test for #358 Phase 2: state + useInput + run loop.
// Implements the issue's acceptance-criterion #1:
//
//   "A 30-line `perry/tui` counter program (increment on `+`,
//    decrement on `-`, quit on `q`) compiles via `perry compile`,
//    produces a single-file native binary, runs interactively."
//
// Verification: pipe stdin with a sequence like `++--q\n`, run the
// binary, then check the final count printed AFTER `run()` returns.
// `run()` enters the alt screen for the duration of the loop, so the
// final-count print happens AFTER the alt screen is left, on the
// primary screen — visible after the binary exits.
//
// Phase 2's renderer state changes happen in-process (state.set
// mutates the slot bits + flips STATE_DIRTY); verifying the slot
// matches what we expect after a sequence of keypresses proves the
// useInput dispatch + state setter + render loop are all wired up
// end-to-end.

import { Box, Text, run, state, useInput, exit } from "perry/tui";

const count = state(0);

useInput((s: string) => {
    if (s === "+") count.set(count.get() + 1);
    if (s === "-") count.set(count.get() - 1);
    if (s === "q") exit();
});

run(() => Box([Text("count: " + count.get())]));

// After run() returns, print the final count on the primary screen
// (alt screen has been left). Tests pipe stdin and grep for FINAL=.
console.log("FINAL=" + count.get());
