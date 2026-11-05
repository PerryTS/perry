// A spawn that fails after its pipes exist (ENOENT) still has stdin/stdout/
// stderr sockets: Node wraps the pipes, destroys stdin, emits 'error', and
// emits 'close' once the exit and every readable socket closed. A failed fork
// also waits for its IPC channel.
import { spawn, fork } from "child_process";
import * as net from "net";

function probe(name: string, c: any, next?: () => void) {
  const ev: string[] = [];
  console.log(name, "pid", c.pid, "sockets", c.stdio.map((s: any) => (s === null ? "null" : s instanceof net.Socket ? "sock" : typeof s)).join(","), "needed", c._closesNeeded);
  for (const [n, s] of [["stdin", c.stdin], ["stdout", c.stdout], ["stderr", c.stderr]] as Array<[string, any]>) {
    if (!s) continue;
    console.log(" ", n, s.readable, s.writable, s.destroyed);
    for (const e of ["end", "close"]) s.on(e, () => ev.push(n + ":" + e));
  }
  c.on("error", (e: any) => ev.push("error:" + e.code + " exitCode=" + c.exitCode + " stdin.destroyed=" + (c.stdin && c.stdin.destroyed)));
  c.on("spawn", () => ev.push("spawn"));
  c.on("exit", () => ev.push("exit"));
  c.on("close", (code: any, sig: any) => ev.push("close:" + code + ":" + sig + " got=" + c._closesGot));
  setTimeout(() => {
    console.log(" ", ev.sort().join(" | "));
    if (next) next();
  }, 300);
}

probe("spawn-pipes", spawn("/no/such/perry-bin", ["a"]), () =>
  probe("spawn-ignore", spawn("/no/such/perry-bin", [], { stdio: ["ignore", "pipe", "ignore"] }), () =>
    probe("fork-silent", fork("x.js", [], { execPath: "/no/such/perry-node", silent: true }), () =>
      probe("fork-ignore", fork("x.js", [], { execPath: "/no/such/perry-node", stdio: ["ignore", "ignore", "ignore", "ipc"] })))));
