import { fork } from "node:child_process";

const marker = "__perry_process_ipc_child__";
const markerIndex = process.argv.indexOf(marker);

if (markerIndex >= 0) {
  const proc = process as any;
  const keys = Object.keys(proc);

  for (const key of ["send", "disconnect", "connected", "channel"]) {
    console.log(
      "child",
      key,
      typeof proc[key],
      Object.prototype.hasOwnProperty.call(proc, key),
      keys.includes(key),
    );
  }

  console.log("child direct send type", typeof process.send);
  console.log("child direct connected", (process as any).connected);
  console.log("child direct channel type", typeof (process as any).channel);
  console.log("child send length", proc.send.length);
  console.log("child channel refs", typeof proc.channel.ref, typeof proc.channel.unref);
  console.log("child channel unref return", proc.channel.unref() === proc.channel);
  console.log("child channel ref return", proc.channel.ref() === proc.channel);

  proc.on("message", (msg: any) => {
    console.log("child message", msg && msg.kind);
    const sendReturn = proc.send({ kind: "reply", got: msg.kind }, (err: any) => {
      console.log("child send cb", err === null ? "null" : err?.code ?? err?.name);
      proc.disconnect();
      console.log("child connected after disconnect", proc.connected);
    });
    console.log("child send return", sendReturn);
  });

  console.log("child ready");
} else {
  const script = process.argv[1];
  const modulePath = typeof script === "string" && script.endsWith(".ts") ? script : process.execPath;
  const child = fork(modulePath, [marker], {
    execPath: process.execPath,
    stdio: ["ignore", "pipe", "pipe", "ipc"],
  });
  const lines: string[] = [];
  let sent = false;
  const timeout = setTimeout(() => {
    lines.push("timeout");
    child.kill();
  }, 3000);

  child.stdout?.on("data", (chunk) => {
    for (const line of chunk.toString().trim().split(/\n/).filter(Boolean)) {
      lines.push("stdout " + line);
      if (line === "child ready" && !sent) {
        sent = true;
        child.send({ kind: "ping" });
      }
    }
  });
  child.stderr?.on("data", (chunk) => {
    for (const line of chunk.toString().trim().split(/\n/).filter(Boolean)) {
      lines.push("stderr " + line);
    }
  });
  child.on("message", (msg: any) => {
    lines.push("parent message " + msg.kind + " " + msg.got);
  });
  child.on("disconnect", () => {
    lines.push("parent disconnect");
  });
  child.on("close", (code, signal) => {
    clearTimeout(timeout);
    lines.push("close " + code + " " + (signal === null ? "null" : signal));
    console.log(lines.sort().join("\n"));
  });
}
