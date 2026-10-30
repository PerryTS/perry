import { Worker, isMainThread, parentPort } from "node:worker_threads";
import { argv as importedArgv } from "node:process";

if (isMainThread) {
  const filename = new URL("./internal-thread-worker.cjs", import.meta.url);
  function check(label: string, source: any, evalValue: any) {
    const buffer = new Uint8Array([9, 8, 7]).buffer;
    const reads: string[] = [];
    try {
      const worker = new Worker(source, {
        get argv() { reads.push("argv"); return undefined; },
        get eval() { reads.push("eval"); return evalValue; },
        get workerData() { reads.push("data"); return buffer; },
        transferList: [buffer],
      });
      console.log(label, "unexpected");
      worker.terminate();
    } catch (error: any) {
      console.log(label, error.name, error.code, reads.join(","), buffer.byteLength, Array.from(new Uint8Array(buffer)).join(","));
    }
  }
  check("url:", filename, true);
  check("truthy:", filename, 1);
  const factory = process.getBuiltinModule("node:worker_threads");
  try { new factory.Worker(filename, { eval: true }); }
  catch (error: any) { console.log("captured:", error.name, error.code); }

  const buffer = new ArrayBuffer(8);
  const reads: string[] = [];
  try {
    new Worker(filename, {
      get argv() { reads.push("argv"); return [{ toString() { reads.push("coerce"); throw new Error("ARGV"); } }]; },
      get eval() { reads.push("eval"); return true; },
      workerData: buffer,
      transferList: [buffer],
    });
  } catch (error: any) { console.log("argv first:", error.message, reads.join(","), buffer.byteLength); }

  const originalArg = importedArgv[2];
  const processValue = process.getBuiltinModule("node:process");
  processValue.argv = ["replacement", "arg", "changed"];
  console.log("main snapshot:", importedArgv[2] === originalArg);

  async function validEntries() {
    await new Promise<void>((resolve) => {
      const worker = new Worker(`
  const { parentPort } = require('node:worker_threads');
  parentPort.postMessage(process.argv.slice(2).map(v => [typeof v, String(v)]));
  `, { eval: true, argv: [, 1, undefined, Symbol("v")] as any });
      worker.on("message", (message) => console.log("argv:", JSON.stringify(message)));
      worker.on("error", (error: any) => console.log("error:", error.name, error.code));
      worker.on("exit", (code) => { console.log("literal eval:", code); resolve(); });
    });
    await new Promise<void>((resolve) => {
      const worker = new Worker("", { eval: true });
      worker.on("exit", (code) => { console.log("empty eval:", code); resolve(); });
    });
    await new Promise<void>((resolve) => {
      const worker = new Worker(new URL(import.meta.url), { argv: ["worker"] });
      worker.on("message", (message) => console.log("worker snapshot:", JSON.stringify(message)));
      worker.on("exit", (code) => { console.log("snapshot exit:", code); resolve(); });
    });
  }
  validEntries().catch((error) => console.log("unexpected:", error.name, error.code));
} else {
  const original = importedArgv;
  const processValue = process.getBuiltinModule("node:process");
  processValue.argv = ["replacement", "arg", "changed"];
  parentPort!.postMessage([importedArgv === original, importedArgv[2], process.argv[2]]);
}
