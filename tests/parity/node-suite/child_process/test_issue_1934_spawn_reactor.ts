// #1934 — async subprocess reactor: live streaming stdout, stdin.write() to a
// running child, and kill() on a running process. Byte-for-byte vs
// `node --experimental-strip-types`.
import * as cp from "node:child_process";

// (1) Incremental streaming: spawn / data / end / exit / close all fire, and
// stdout is observed before exit.
{
  const sp = cp.spawn("/bin/echo", ["stream-hello"]);
  let buf = "";
  const seen: string[] = [];
  sp.on("spawn", () => seen.push("spawn"));
  sp.stdout.on("data", (c: any) => {
    buf += c.toString();
    if (!seen.includes("data")) seen.push("data");
  });
  sp.stdout.on("end", () => seen.push("end"));
  sp.on("exit", () => seen.push("exit"));
  await new Promise<void>((resolve) => {
    sp.on("close", () => {
      seen.push("close");
      resolve();
    });
  });
  console.log("stream stdout:", buf.trim());
  console.log("stream events:", seen.join(","));
}

// (2) Live stdin.write()/end() to a running `cat`, which echoes it back.
{
  const c = cp.spawn("/bin/cat", []);
  let out = "";
  c.stdout.on("data", (d: any) => {
    out += d.toString();
  });
  await new Promise<void>((resolve) => {
    c.on("close", () => resolve());
    c.stdin.write("piped-stdin\n");
    c.stdin.end();
  });
  console.log("cat echoed:", out.trim());
}

// (3) kill() a long-running child — exit reports the terminating signal.
{
  const s = cp.spawn("/bin/sleep", ["30"]);
  await new Promise<void>((resolve) => {
    s.on("exit", (code: any, signal: any) => {
      console.log("killed code:", code, "signal:", signal);
      resolve();
    });
    setTimeout(() => {
      s.kill();
    }, 50);
  });
}

console.log("reactor done");
