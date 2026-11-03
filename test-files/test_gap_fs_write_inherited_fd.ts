// fs.writeSync on a descriptor inherited from the parent (fd 3 passed through
// spawn's stdio) must write to it, not throw EBADF, and closeSync must close
// the pipe so the parent sees EOF.
import { spawn } from "node:child_process";
import { closeSync, fstatSync, writeSync } from "node:fs";

if (process.argv[process.argv.length - 1] === "child") {
  const a = writeSync(3, Buffer.from("buffer-line\n", "utf8"));
  const b = writeSync(3, "string-line\n");
  const c = writeSync(3, Buffer.from("xxpartial-line\nyy"), 2, 13);
  const isPipe = fstatSync(3).isFIFO() || fstatSync(3).isSocket();
  closeSync(3);
  let afterClose = "no error";
  try {
    writeSync(3, "late");
  } catch (e: any) {
    afterClose = e.code;
  }
  console.error(`child wrote ${a} ${b} ${c} pipe=${isPipe} afterClose=${afterClose}`);
} else {
  const script = process.argv.find((arg) => arg.endsWith(".ts"));
  const [command, args] = script
    ? [process.execPath, ["--experimental-strip-types", script, "child"]]
    : [process.execPath, ["child"]];
  const child = spawn(command, args, {
    stdio: ["ignore", "ignore", "pipe", "pipe"],
  });
  let stderr = "";
  let fd3 = "";
  child.stderr!.on("data", (chunk: Buffer) => {
    stderr += chunk.toString("utf8");
  });
  child.stdio[3]!.on("data", (chunk: Buffer) => {
    fd3 += chunk.toString("utf8");
  });
  child.on("close", (code: number | null) => {
    console.log("exit", code);
    console.log("stderr", JSON.stringify(stderr));
    console.log("fd3", JSON.stringify(fd3));
  });
}
