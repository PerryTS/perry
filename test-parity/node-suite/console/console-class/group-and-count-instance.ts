import { Console } from "node:console";
import { Writable } from "node:stream";

let out = "";
const sink = new Writable({ write(chunk, _enc, cb) { out += chunk.toString(); cb(); } });
const c = new Console({ stdout: sink, stderr: sink });
c.count("x");
c.group("g");
c.log("inside");
c.groupEnd();
c.countReset("x");
c.count("x");
await new Promise(resolve => setImmediate(resolve));
console.log("captured:", JSON.stringify(out.trim().split(/\n/)));
