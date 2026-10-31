// Children must not inherit extra copies of the parent's stdin/stdout/stderr:
// the fs descriptor registry duplicates fds 0-2, and those duplicates have to
// be close-on-exec so a detached child never keeps the parent's pipes open.
import { createRequire } from "node:module";
const req = createRequire(import.meta.url);
const fs = req("node:fs");
const os = req("node:os");
const path = req("node:path");
const { spawn } = req("node:child_process");

fs.writeSync(1, "parent stdout\n");
fs.writeSync(2, "parent stderr\n");

const probe =
  "for fd in 3 4 5 6 7 8 9; do if ( true >&$fd ) 2>/dev/null; then echo open $fd; fi; done; echo probe done";

function pipeCase(): Promise<void> {
  return new Promise((resolve) => {
    const child = spawn("sh", ["-c", probe], { stdio: ["ignore", "pipe", "ignore"] });
    let out = "";
    child.stdout.on("data", (d: Buffer) => {
      out += d.toString();
    });
    child.on("close", () => {
      console.log("pipe case:", JSON.stringify(out.trim().split("\n")));
      resolve();
    });
  });
}

function detachedCase(): Promise<void> {
  return new Promise((resolve) => {
    const file = path.join(os.tmpdir(), "perry-cloexec-" + process.pid + ".txt");
    const child = spawn("sh", ["-c", probe + ' > "$0"', file], {
      stdio: "ignore",
      detached: true,
    });
    child.on("exit", () => {
      const out = fs.readFileSync(file, "utf8");
      fs.unlinkSync(file);
      console.log("detached case:", JSON.stringify(out.trim().split("\n")));
      resolve();
    });
  });
}

pipeCase().then(detachedCase);
