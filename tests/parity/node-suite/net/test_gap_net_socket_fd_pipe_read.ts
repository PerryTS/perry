// net.Socket({ fd, writable: false }) over a FIFO's read end: the writable side
// starts finished, so the socket reads to EOF and then closes by itself.
import * as net from "node:net";
import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";
import { execFileSync, spawn } from "node:child_process";

const fifo = path.join(os.tmpdir(), `perry-gap-fifo-read-${process.pid}`);
try { fs.unlinkSync(fifo); } catch {}
execFileSync("mkfifo", [fifo]);
spawn("sh", ["-c", `printf 'one\\ntwo\\n' > '${fifo}'`], { stdio: "ignore" });
const fd = fs.openSync(fifo, "r");

const s: any = new net.Socket({ fd, readable: true, writable: false });
const events: string[] = [];
let text = "";
s.on("data", (chunk: any) => { text += chunk.toString(); });
for (const name of ["end", "finish", "close", "error"]) {
  s.on(name, (arg: any) => events.push(name + (arg === undefined ? "" : `:${arg && arg.code ? arg.code : arg}`)));
}
console.log("initial", s.readable, s.writable, s.readyState, s.writableFinished, s.writableEnded, s.closed);
s.on("close", () => {
  console.log("events", events.join(" "));
  console.log("text", JSON.stringify(text));
  console.log("final", s.readableEnded, s.closed, s.destroyed, s.bytesRead);
  fs.unlinkSync(fifo);
});
