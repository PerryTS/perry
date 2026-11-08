// parity-env: PERRY_GC_SCHEDULE_SEED=3 PERRY_GC_SCHEDULE_RATE=1 PERRY_GC_SCHEDULE_ALLOC_KB=0 PERRY_GC_PROTECT_FROMSPACE=1
// GC witness for a console call the facts cannot prove native. A store under an
// unknown key into a value from unmodeled code (a message clone) may replace
// any console member, so each console call reads globalThis.console and its
// member first, then evaluates the arguments, then calls the member it read
// with the console as `this`. The console and the member are live across every
// argument. An argument whose evaluation runs a loop that allocates gives a
// moving minor the chance to move both; a callee held only in a register comes
// back as a retired from-space address.
import { MessageChannel } from "node:worker_threads";

const { port1, port2 } = new MessageChannel();
const got: any = await new Promise((resolve) => {
    port2.once("message", resolve);
    port1.postMessage({ n: 7 });
});
port1.close();
port2.close();
got["k" + got.n] = got.n;

function churn(n: number): number {
    let s = 0;
    for (let i = 0; i < n; i++) {
        const o = { i, t: "v" + i };
        s += o.t.length;
    }
    return s;
}

for (let k = 0; k < 3; k++) console.log("round", k, churn(200), "tail" + k);
console.log("done", got.k7);
