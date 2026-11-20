import { Console } from "node:console";
import { Writable } from "node:stream";

class Boom extends Writable {
  _write(_chunk: any, _enc: string, cb: Function) { cb(new Error("boom")); }
}
const stdout = new Boom();
const stderr = new Boom();
stdout.on("error", (err: any) => console.log("stdout error:", err.message));
stderr.on("error", (err: any) => console.log("stderr error:", err.message));
const c1 = new Console({ stdout, stderr, ignoreErrors: true });
console.log("ignore start");
c1.log("hidden");
c1.error("hidden err");
await new Promise(resolve => setImmediate(resolve));
console.log("ignore end");
try { new Console({ stdout: undefined as any, stderr }); console.log("invalid stream no throw"); } catch (err: any) { console.log("invalid stream:", err?.name, err?.code || "no-code"); }
