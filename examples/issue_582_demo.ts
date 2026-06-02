// Issue #582 demo — network reachability stdlib (online/offline + change events).
//
// Acceptance flow from the issue:
//   1. Logs current connection status at startup.
//   2. Subscribes to network change events.
//   3. On the device: toggle Airplane Mode. The program prints the
//      transition within 1 second on each change.
//
// Compile + run on macOS host:
//   cargo run --release -- examples/issue_582_demo.ts -o /tmp/net_demo && /tmp/net_demo
//
// HEADLESS HOST CAVEAT: NWPathMonitor delivers events on the main dispatch
// queue, which only spins inside a UIApplication / NSApplication run loop.
// A bare CLI binary on macOS never pumps that queue, so `getStatus` reports
// `connected=false / kind="unknown"` (the pre-monitor seed) and `onChange`
// never fires. On a real iPhone / Android phone the platform's run loop is
// always active, so transitions print within milliseconds — toggle Airplane
// Mode and the lines `[change #1]…[change #2]…` flow through.

import {
    networkGetStatus,
    networkOnChange,
    networkStopOnChange,
} from "perry/system";

console.log("[startup] reading initial network state...");

networkGetStatus((connected, kind) => {
    console.log(`[startup] connected=${connected} type=${kind}`);
});

console.log("[startup] subscribing to change events; toggle Airplane Mode now...");

let count = 0;
const id = networkOnChange((connected, kind) => {
    count++;
    console.log(`[change #${count}] connected=${connected} type=${kind}`);
});

console.log(`[startup] subscription id=${id}`);

// Keep the program alive long enough on host to observe a few transitions.
// Real iOS / Android apps would just leave the subscription live for the
// app's lifetime; the runtime drives microtasks via the platform run loop.
setTimeout(() => {
    networkStopOnChange(id);
    console.log(`[shutdown] unsubscribed after ${count} change(s)`);
}, 30_000);
