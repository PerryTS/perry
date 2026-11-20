// Node wraps every 'pipe' stdio slot of a child in a net.Socket
// (createSocket(pipe, i > 0)); the slots are sockets in the child too. The
// ChildProcess emits 'close' once its exit and every readable socket's
// 'close' happened (_closesNeeded / _closesGot), and 'exit' destroys stdin.
// How the slots' events interleave depends on the child's timing, so each
// slot keeps its own event list (its order is fixed) and its data joined.
import { spawn } from "child_process";
import * as net from "net";

const c = spawn("sh", ["-c", "cat; echo err >&2; for f in 0 1 3; do if [ -S /dev/fd/$f ]; then echo sock; else echo other; fi; done >&3"], {
  stdio: ["pipe", "pipe", "pipe", "pipe"],
});
const ev: Record<string, string[]> = {};
const data: Record<string, string> = {};
const slots: Array<[string, any]> = [["stdin", c.stdin], ["stdout", c.stdout], ["stderr", c.stderr], ["fd3", c.stdio[3]]];
const note = (n: string, e: string) => {
  const list = ev[n];
  if (list[list.length - 1] !== e) list.push(e);
};
for (const [n, s] of slots) {
  ev[n] = [];
  data[n] = "";
  console.log(n, s instanceof net.Socket, s.readable, s.writable, s.readyState, s.allowHalfOpen);
  for (const e of ["end", "finish", "close", "prefinish"]) s.on(e, () => note(n, e));
}
console.log("same stdio", c.stdio[0] === c.stdin, c.stdio[1] === c.stdout, c.stdio[2] === c.stderr, c.stdio.length);
console.log("closes", (c as any)._closesNeeded, (c as any)._closesGot);
for (const n of ["stdout", "stderr", "fd3"]) {
  slots.find(([m]) => m === n)![1].on("data", (d: any) => { note(n, "data"); data[n] += String(d); });
}
c.stdin!.write("hi\n", () => note("stdin", "write-cb"));
c.stdin!.end(() => note("stdin", "end-cb"));
let exitSeen = "";
c.on("exit", (code) => { exitSeen = "exit:" + code + " stdin.destroyed=" + c.stdin!.destroyed; });
// 'close' is emitted inside the last readable socket's 'close', before that
// socket's own listeners here: two of the three have run by then.
c.on("close", (code) => {
  const closed = ["stdout", "stderr", "fd3"].filter((n) => ev[n].includes("close")).length;
  console.log(exitSeen, "close:" + code, "got=" + (c as any)._closesGot, "readable closed before:", closed);
  setImmediate(() => {
    for (const [n] of slots) console.log(n + ": " + ev[n].join(" ") + (data[n] ? " " + JSON.stringify(data[n]) : ""));
    console.log("stdin", c.stdin!.writableFinished, c.stdin!.closed, c.stdin!.destroyed, "stdout", c.stdout!.readableEnded, c.stdout!.closed);
  });
});
