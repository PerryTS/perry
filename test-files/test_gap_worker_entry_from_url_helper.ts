// The worker file is named only by `new URL("<literal>", import.meta.url)` in a
// helper module, and the Workers are constructed elsewhere through values the
// compiler cannot follow: the worker_threads namespace from
// `process.getBuiltinModule`, `entry ??= helper()`, `options.entry ?? helper()`,
// an absolute path string and a `./` path relative to the current directory.
// The compiler finds the file from the URL literal and compiles it in.
import { Worker } from "node:worker_threads";
import { fileURLToPath } from "node:url";
import * as path from "node:path";
import { echoWorker, notes } from "./_helpers/worker_entry_url_helpers.ts";

const guard = setTimeout(() => {
    console.log("timeout");
    process.exit(2);
}, 10000);

const ns = (globalThis as any).process.getBuiltinModule("node:worker_threads");

function run(label: string, make: () => Worker): Promise<void> {
    return new Promise((done) => {
        let w: Worker;
        try {
            w = make();
        } catch (e: any) {
            console.log(label, "threw", e.code);
            done();
            return;
        }
        w.once("message", (hello: unknown) => {
            console.log(label, JSON.stringify(hello));
            w.postMessage(label);
            w.once("message", (reply: unknown) => {
                console.log(label, reply);
                w.terminate().then((code) => {
                    console.log(label, "exit", code);
                    done();
                });
            });
        });
    });
}

let entry: URL | undefined;
entry ??= echoWorker();
await run("lazy entry", () => new ns.Worker(entry, { workerData: 1 }));

const options: { entry?: URL } = {};
await run("option default", () => new Worker(options.entry ?? echoWorker(), { workerData: 2 }));

const absolute = fileURLToPath(echoWorker());
await run("absolute path", () => new ns.Worker(absolute, { workerData: 3 }));

const relative = path.relative(process.cwd(), absolute);
const fromCwd = relative.startsWith("..") ? relative : "./" + relative;
await run("cwd-relative path", () => new ns.Worker(fromCwd, { workerData: 4 }));

console.log("data URL kept:", notes().pathname.endsWith(".json"));
clearTimeout(guard);
