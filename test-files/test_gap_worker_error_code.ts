import { Worker } from "node:worker_threads";
const guard = setTimeout(() => process.exit(2), 20000);
for (const mode of ["path", "custom"]) {
    await new Promise<void>((done) => {
        const worker = new Worker(new URL("./_helpers/worker_error_code.ts", import.meta.url), { workerData: mode });
        worker.on("error", (error: any) => console.log(mode, error instanceof Error, error.name, error.message, error.code));
        worker.on("exit", (code: number) => { console.log("exit", code); done(); });
    });
}
clearTimeout(guard);
