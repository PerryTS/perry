// net.Socket({ fd }) over a FIFO's write end: a pipe has no write-side shutdown,
// so end() completes once the queued write drains, with no error, and the
// write-only socket then closes (prefinish, end callback, finish, close).
import * as net from "node:net";
import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";
import { execFileSync, spawn } from "node:child_process";

const fifo = path.join(os.tmpdir(), `perry-gap-fifo-${process.pid}`);
try { fs.unlinkSync(fifo); } catch {}
execFileSync("mkfifo", [fifo]);
const reader = spawn("cat", [fifo], { stdio: ["ignore", "pipe", "inherit"] });
let received = "";
reader.stdout.on("data", (chunk: any) => { received += chunk.toString(); });
const fd = fs.openSync(fifo, "w");

const s: any = new net.Socket({ fd, readable: false, writable: true });
const events: string[] = [];
for (const name of ["prefinish", "finish", "close", "error", "end"]) {
  s.on(name, (arg: any) => events.push(name + (arg === undefined ? "" : `:${arg && arg.code ? arg.code : arg}`)));
}
console.log("initial", s.readable, s.writable, s.readyState, s.writableFinished, s.closed, s.pending);
const ok = s.write("hello through a fifo\n", (err: any) => events.push(`write-cb:${err}`));
s.end((err: any) => events.push(`end-cb:${err}`));
console.log("after end", ok, s.writableEnded, s.writableFinished);

reader.on("close", (code: any) => {
  console.log("events", events.join(" "));
  console.log("final", s.writableFinished, s.closed, s.destroyed, s.bytesWritten, s.readableEnded);
  console.log("reader", code, JSON.stringify(received));
  fs.unlinkSync(fifo);
});
