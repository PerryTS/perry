// net.Socket({ fd }) starts reading at construction, as Node's constructor
// calls read(0) for a readable handle: data that arrives before the first
// 'data' listener is buffered in paused mode (readableLength grows) and is
// delivered, not lost, once a consumer attaches.
import * as net from "node:net";
import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";
import { execFileSync, spawn } from "node:child_process";

const fifo = path.join(os.tmpdir(), `perry-gap-fifo-ctor-${process.pid}`);
try { fs.unlinkSync(fifo); } catch {}
execFileSync("mkfifo", [fifo]);
spawn("sh", ["-c", `printf 'hello' > '${fifo}'`], { stdio: "ignore" });
const fd = fs.openSync(fifo, "r");

const s: any = new net.Socket({ fd, writable: false });
console.log("ctor", s.readableLength, s.readableFlowing, s.isPaused());
const started = Date.now();
function poll() {
  if (s.readableLength === 0 && Date.now() - started < 5000) {
    setTimeout(poll, 20);
    return;
  }
  console.log("before listener", s.readableLength, s.readableFlowing);
  let text = "";
  s.on("data", (chunk: any) => { text += chunk.toString(); });
  s.on("end", () => console.log("end", JSON.stringify(text)));
  s.on("close", (hadError: boolean) => {
    console.log("close", hadError, s.bytesRead);
    fs.unlinkSync(fifo);
  });
}
setTimeout(poll, 20);
