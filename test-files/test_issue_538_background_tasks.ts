// Issue #538 — exercise the perry/background API so the dispatch tables
// resolve every symbol. The OS-side wake-up behavior is validated on
// device; this file just validates that the symbols link and the FFI
// shapes line up.
import { registerTask, schedule, cancel } from "perry/background";

const TASK_ID = "com.example.refresh";

registerTask(TASK_ID, async () => {
  console.log("background task fired");
});

// Earliest start ~1 minute from now; appRefresh kind; no power constraints.
const earliestStart = Date.now() + 60_000;
schedule(TASK_ID, "appRefresh", earliestStart, true, false);

// Processing kind exercises the second branch.
schedule(TASK_ID + ".processing", "processing", 0, true, true);

cancel(TASK_ID);
cancel(TASK_ID + ".processing");

console.log("issue #538 background API surface OK");
