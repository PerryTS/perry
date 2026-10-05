// parity-env: PERRY_GC_FORCE_EVACUATE=1 PERRY_GC_VERIFY_EVACUATION=1 PERRY_GC_SCHEDULE_SEED=11931 PERRY_GC_SCHEDULE_RATE=0.2 PERRY_GC_SCHEDULE_ALLOC_KB=64
// Additional owner matrix settings: PERRY_GC_FORCE_EVACUATE=1 PERRY_GC_VERIFY_EVACUATION=1 PERRY_GC_VERIFY_MARK=1
import { Worker } from "node:worker_threads";
const before = Object.getOwnPropertyDescriptor(Object.keys, "name");
const guard = setTimeout(() => { console.log("timeout"); process.exit(2); }, 60000);
function run(mode: string): Promise<void> {
  return new Promise((done) => {
    const worker = new Worker(new URL("./_helpers/worker_function_attrs_11931.ts", import.meta.url), {workerData: mode});
    worker.on("message", (message: string) => console.log(mode, message));
    worker.on("error", (error: any) => console.log(mode, "error", error.message));
    worker.on("exit", (code: number) => { console.log(mode, "exit", code); done(); });
  });
}
for (const mode of ["complete", "midway", "complete", "midway"]) await run(mode);
const after = Object.getOwnPropertyDescriptor(Object.keys, "name");
console.log("main-function-unchanged", JSON.stringify(before) === JSON.stringify(after));
clearTimeout(guard);
