import { Worker } from "node:worker_threads";
const guard = setTimeout(() => { console.log("timeout"); process.exit(2); }, 20000);
for (const mode of ["function", "class", "subclass", "cause", "type-subclass", "bad-cause", "unmarked"]) {
    await new Promise<void>((done) => {
        const w = new Worker(new URL("./_helpers/worker_error_uncloneable_properties.ts", import.meta.url), { workerData: mode });
        w.on("error", (e: any) => {
            console.log(mode, e instanceof Error, e.name, e.message, typeof e.stack === "string" && (mode === "unmarked" ? e.stack.includes(e.message) : e.stack === "stable worker stack"), mode === "bad-cause" ? "uncloneable" : JSON.stringify(e.cause));
        });
        w.on("exit", (code: number) => { console.log(mode, "exit", code); done(); });
        w.postMessage("go");
    });
}
clearTimeout(guard);
