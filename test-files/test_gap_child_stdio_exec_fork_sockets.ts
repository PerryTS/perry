// exec/execFile are spawns with three pipes: stdin/stdout/stderr are sockets
// and the callback carries what the stdout/stderr sockets read. A fork's
// 'close' also waits for its IPC channel. Destroying a readable socket counts
// as its close.
import { exec, execFile, fork, spawn } from "child_process";
import * as fs from "fs";
import * as net from "net";
import * as os from "os";
import * as path from "path";

function step1(next: () => void) {
  const c = exec("echo out; echo err >&2; exit 3", (err: any, stdout, stderr) => {
    console.log("exec cb", err && err.code, JSON.stringify(stdout), JSON.stringify(stderr));
    next();
  });
  console.log("exec stdio", c.stdin instanceof net.Socket, c.stdout instanceof net.Socket, c.stderr instanceof net.Socket, (c as any)._closesNeeded);
  const ev: string[] = [];
  c.on("exit", (code) => ev.push("exit:" + code));
  c.on("close", (code) => { ev.push("close:" + code); console.log("exec events", ev.join(" ")); });
}

function step2(next: () => void) {
  execFile("sh", ["-c", "printf abc; printf def >&2"], { encoding: "buffer" }, (err, stdout: any, stderr: any) => {
    console.log("execFile cb", err, Buffer.isBuffer(stdout), stdout.toString(), stderr.toString());
    next();
  });
}

function step3(next: () => void) {
  const script = path.join(os.tmpdir(), "perry_gap_fork_sock_" + process.pid + ".sh");
  fs.writeFileSync(script, "echo forked\nexit 4\n");
  const c = fork(script, [], { execPath: "/bin/sh", silent: true });
  const ev: string[] = [];
  console.log("fork stdio", c.stdin instanceof net.Socket, c.stdout instanceof net.Socket, c.stdio.length, c.stdio[3], (c as any)._closesNeeded);
  c.stdout!.on("data", (d) => ev.push("data:" + String(d).trim()));
  c.stdout!.on("close", () => ev.push("stdout:close"));
  c.on("disconnect", () => ev.push("disconnect"));
  c.on("exit", (code) => ev.push("exit:" + code));
  c.on("close", (code) => {
    ev.push("close:" + code + " got=" + (c as any)._closesGot);
    console.log("fork events", ev.sort().join(" "));
    fs.unlinkSync(script);
    next();
  });
}

function step4() {
  const c = spawn("sh", ["-c", "sleep 0.2; echo late"]);
  const ev: string[] = [];
  c.stdout!.destroy();
  c.stdout!.on("close", () => ev.push("stdout:close"));
  c.stderr!.on("close", () => ev.push("stderr:close"));
  c.on("exit", (code) => ev.push("exit:" + code));
  c.on("close", (code) => {
    ev.push("close:" + code + " got=" + (c as any)._closesGot);
    console.log("destroy events", ev.sort().join(" "), c.stdout!.destroyed);
  });
}

step1(() => step2(() => step3(step4)));
