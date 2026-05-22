import { callbackify } from "node:util";

function run(label: string, fn: Function): Promise<void> {
  return new Promise(resolve => {
    callbackify(fn)((err: any, value: any) => {
      console.log(label + ":", err ? err.name + ":" + (err.reason === undefined ? "undefined" : String(err.reason)) : "ok:" + value);
      resolve();
    });
  });
}
await run("resolve", async () => "ok");
await run("reject null", async () => { throw null; });
await run("reject undefined", async () => { throw undefined; });
await run("reject error", async () => { throw new Error("bad"); });
