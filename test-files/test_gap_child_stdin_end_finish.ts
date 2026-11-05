// child.stdin.end() flushes pending writes, runs the end callback, then emits
// `finish` and, once the descriptor is closed, `close`; the writable flags follow.
import { spawn } from "node:child_process";

const child = spawn("cat", [], { stdio: ["pipe", "pipe", "inherit"] });
const stdin = child.stdin as any;
const events: string[] = [];
const output: string[] = [];
child.stdout!.on("data", (chunk: any) => output.push(String(chunk)));

console.log("initial", stdin.writableEnded, stdin.writableFinished, stdin.closed);
stdin.once("finish", () => {
  events.push("finish");
  events.push(`finish-flags:${stdin.writableEnded},${stdin.writableFinished}`);
});
stdin.once("close", () => events.push("stdin-close"));
const exited = new Promise<void>((resolve) => child.once("close", () => resolve()));

const ok = stdin.write("hello ", () => events.push("write-cb"));
console.log("write returned", ok);
stdin.end("world\n", () => events.push("end-cb"));
console.log("after end", stdin.writableEnded, stdin.writableFinished, stdin.writable);
await exited;
console.log("events", events.join(","));
console.log("output", JSON.stringify(output.join("")));
console.log("final", stdin.writableEnded, stdin.writableFinished, stdin.closed, stdin.destroyed);

// A sink awaiting `finish` after a large write completes once the child has read it all.
const wc = spawn("wc", ["-c"], { stdio: ["pipe", "pipe", "inherit"] });
const count: string[] = [];
wc.stdout!.on("data", (chunk: any) => count.push(String(chunk)));
const finished = new Promise<void>((resolve) => (wc.stdin as any).once("finish", () => resolve()));
(wc.stdin as any).end("x".repeat(1024 * 1024));
await finished;
console.log("large finish", (wc.stdin as any).writableFinished);
await new Promise<void>((resolve) => wc.once("close", () => resolve()));
console.log("large count", count.join("").trim());
