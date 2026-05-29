import * as consoleModule from "node:console";
import { Console } from "node:console";
import { Writable } from "node:stream";

let out = "";
const sink = new Writable({
  write(chunk, _enc, cb) {
    out += chunk.toString();
    cb();
  },
});

const called = (Console as any)({ stdout: sink, stderr: sink });
const namespaceCalled = (consoleModule.Console as any)({ stdout: sink, stderr: sink });
const constructed = new Console({ stdout: sink, stderr: sink });

console.log("Console shape:", typeof Console, Console.length, Console.name);
console.log("call instance:", typeof called.log, called instanceof Console);
console.log("namespace instance:", typeof namespaceCalled.error, namespaceCalled instanceof Console);
console.log("new instance:", typeof constructed.log, constructed instanceof Console);

called.log("called");
namespaceCalled.error("namespace");
constructed.log("constructed");

await new Promise(resolve => setImmediate(resolve));
console.log("captured:", JSON.stringify(out.trim().split(/\n/)));
