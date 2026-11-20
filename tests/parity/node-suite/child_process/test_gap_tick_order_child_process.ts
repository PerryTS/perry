// A runtime-internal child_process event (the failed-spawn error) runs from
// the tick queue: before a setImmediate the program scheduled earlier in the
// same turn, as in node.
import { spawn } from "node:child_process";
setImmediate(() => console.log("immediate"));
const child = spawn("/nonexistent-perry-tick-order-witness");
child.on("error", (e: any) => console.log("error " + e.code));
