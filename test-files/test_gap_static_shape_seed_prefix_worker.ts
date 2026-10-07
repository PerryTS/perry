// A static literal shape stays one shape in every agent after a receiver
// that grew through the literal's key prefix dies.
//
// Each agent seeds its literal shapes at start; the seed publishes the
// literal's whole key list. A receiver that then grows through the same first
// keys publishes those prefixes on its own keys backing. When it died, the
// collector freed the prefix nodes and cut the seeded list off the canonical
// keys trie, so the literal's module init built a second keys array for the
// same keys and the mint of the literal's static ShapeId aborted ("the static
// ShapeId ... was refused by the shape mint"). OpenCode's TUI died on this at
// startup. The main agent and a Worker agent each run the scenario on their
// own heap.
import { Worker } from "node:worker_threads";
import { run } from "./_helpers/static_seed_prefix_scenario.ts";

const guard = setTimeout(() => {
  console.log("timeout");
  process.exit(2);
}, 30000);

console.log(await run("main"));
const worker = new Worker(new URL("./_helpers/static_seed_prefix_worker.ts", import.meta.url));
const message = await new Promise((resolve) => worker.once("message", resolve));
console.log(message);
// Not awaiting "exit": a worker's own exit event is intermittently never
// delivered (a separate, pre-existing bug), and it is not what this test is
// about.
await worker.terminate();
console.log("worker done");
clearTimeout(guard);
