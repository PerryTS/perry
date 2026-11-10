// #2130/#1934 — real child_process spawn/exec option + error semantics, vs
// `node --experimental-strip-types`:
//   (1) spawn-failure 'error' carries Node's errno shape (code/errno/syscall/
//       path/spawnargs) and pid stays undefined;
//   (2) execFile spawn-failure error keys syscall/path off the FILE while
//       `cmd` keeps the display string;
//   (3) kill(9) — numeric signals (incl. NaN-boxed int32 forms) map to the
//       real signal, observable through the 'close' signal name;
//   (4) stdout.pipe(process.stdout) forwards chunks; piping into another
//       child's stdin forwards and closes it at source EOF.
import { spawn, execFile } from "node:child_process";

function stage1() {
  const p = spawn("definitely-not-a-cmd-xyz", ["a", "b"]);
  console.log("1.pid", p.pid);
  p.on("error", (e: any) => {
    console.log(
      "1.error",
      e.code,
      e.errno,
      JSON.stringify(e.syscall),
      JSON.stringify(e.path),
      JSON.stringify(e.spawnargs)
    );
    stage2();
  });
}

function stage2() {
  execFile("definitely-not-a-cmd-xyz", ["a"], (err: any) => {
    console.log(
      "2.execFile",
      err.code,
      err.errno,
      JSON.stringify(err.syscall),
      JSON.stringify(err.path),
      JSON.stringify(err.cmd)
    );
    stage3();
  });
}

function stage3() {
  const p = spawn("sleep", ["30"]);
  p.on("close", (c: any, s: any) => {
    console.log("3.close", c, s);
    stage4();
  });
  setTimeout(() => p.kill(9), 50);
}

function stage4() {
  const p = spawn("sh", ["-c", 'printf "piped-1\\n"']);
  p.stdout.pipe(process.stdout);
  p.on("close", (c: any) => {
    console.log("4.close", c);
    stage5();
  });
}

function stage5() {
  const src = spawn("sh", ["-c", 'printf "x\\ny\\n"']);
  const dst = spawn("cat", ["-n"]);
  src.stdout.pipe(dst.stdin);
  let out = "";
  dst.stdout.on("data", (d: any) => {
    out += d.toString();
  });
  dst.on("close", (c: any) => {
    process.stdout.write(out);
    console.log("5.close", c);
  });
}

stage1();
