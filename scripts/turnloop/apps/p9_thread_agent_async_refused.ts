// turnloop P9 evidence: this program is EXPECTED NOT TO COMPILE.
//
// The P9 report claims a `perry/thread` agent has no event loop, and therefore
// nothing for a `turnloop::Loop` to be attached to. The strongest form of that
// claim is not a runtime row — it is the compiler refusing the program:
//
//   $ perry scripts/turnloop/apps/p9_thread_agent_async_refused.ts -o /tmp/x
//   Error compiling module 'p9_thread_agent_async_refused.ts' ...
//     perry/thread: closure passed to `spawn` must be synchronous — it is (or
//     contains) an async closure or `await`. An `await` inside a worker runs
//     the process-wide completion/timer pump on the worker thread: ... (#6185)
//
// So a `perry/thread` agent cannot await a `fetch`, a socket or a query at
// all, on either transport, before or after this lane. `p9_thread_agent.ts` is
// the companion that runs the cases which DO compile.
//
// Keep it out of any sweep that compiles every file in this directory: it is a
// negative fixture, and a green compile of it would be the finding.
import { spawn } from "perry/thread";

const url = process.env.P9_URL ?? "http://127.0.0.1:8099/";

const spawned = await spawn(async () => {
  const r = await fetch(url);
  return `status=${r.status}`;
});
console.log(`thread-agent spawn(async): ${JSON.stringify(spawned)}`);
