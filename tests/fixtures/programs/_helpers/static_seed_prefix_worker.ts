import { parentPort } from "node:worker_threads";
import { run } from "./static_seed_prefix_scenario.ts";

parentPort!.postMessage(await run("worker"));
