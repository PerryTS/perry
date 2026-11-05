// net.Socket({ fd }) on a standard stream: destroying the socket leaves the
// descriptor open, as libuv never closes fds 0..2 (uv__stream_close); fs and
// console keep working on it. Above 2 the socket owns the descriptor and
// destroy() closes it (EBADF afterwards).
//
// The parity runner gives a fixture no stdin and a file as stdout, so this
// test re-spawns itself with pipes on the child's stdio and reads the child's
// report from its stdout.
import * as net from "node:net";
import * as fs from "node:fs";
import { spawn } from "node:child_process";

const ROLE_ENV = "PERRY_GAP_FD_STDIO_ROLE";

function fdState(fd: number): string {
  try {
    fs.fstatSync(fd);
    return "open";
  } catch (e: any) {
    return e.code;
  }
}

// Use the socket on `fd` (read one chunk from fd 0, write one line to the
// others), then destroy it and report whether the descriptor is still open.
function destroyOn(fd: number, readable: boolean, next: () => void) {
  const s: any = new net.Socket({ fd, readable, writable: !readable });
  s.on("error", (e: any) => fs.writeSync(1, `fd ${fd} error ${e.code}\n`));
  s.on("close", () => {
    fs.writeSync(1, `fd ${fd} after destroy: ${fdState(fd)}\n`);
    next();
  });
  if (readable) {
    s.once("data", (chunk: any) => {
      fs.writeSync(1, `fd ${fd} read ${JSON.stringify(chunk.toString())}\n`);
      s.destroy();
    });
  } else {
    s.write(`via socket on fd ${fd}\n`, () => s.destroy());
  }
}

if (process.env[ROLE_ENV] === "child") {
  destroyOn(0, true, () =>
    destroyOn(1, false, () =>
      destroyOn(2, false, () =>
        destroyOn(3, false, () => {
          console.log("console.log after destroy of fd 1");
          fs.writeSync(2, "stderr after destroy of fd 2\n");
        }),
      ),
    ),
  );
} else {
  const childArgs = [...process.execArgv, ...process.argv.slice(1)];
  const child = spawn(process.execPath, childArgs, {
    env: { ...process.env, [ROLE_ENV]: "child" },
    stdio: ["pipe", "pipe", "pipe", "pipe"],
  });
  child.stdin!.write("ping");
  let out = "";
  let err = "";
  let extra = "";
  child.stdio[3]!.on("data", (d: any) => { extra += d.toString(); });
  child.stdout!.on("data", (d: any) => { out += d.toString(); });
  child.stderr!.on("data", (d: any) => { err += d.toString(); });
  child.on("close", (code: number | null) => {
    console.log(out.trimEnd());
    console.log("stderr:", JSON.stringify(err));
    console.log("fd 3:", JSON.stringify(extra));
    console.log("exit", code);
  });
}
