import { Console } from "console";

const stdout = process.stdout;
const stderr = process.stderr;

if (typeof stdout.write !== "function") {
  throw new TypeError("stdout.write shape mismatch");
}
if (typeof stderr.write !== "function") {
  throw new TypeError("stderr.write shape mismatch");
}
if (stdout.writable !== true) {
  throw new TypeError("stdout.writable shape mismatch");
}
if (stderr.writable !== true) {
  throw new TypeError("stderr.writable shape mismatch");
}

const wroteStdout = stdout.write("stdout-direct\n");
const wroteStderr = stderr.write("stderr-direct\n");

if (wroteStdout !== true) {
  throw new TypeError("stdout.write return mismatch");
}
if (wroteStderr !== true) {
  throw new TypeError("stderr.write return mismatch");
}

const scopedConsole = new Console({ stdout, stderr });
scopedConsole.log("console-stdout");
scopedConsole.error("console-stderr");

await Promise.resolve();

stderr.write("stderr-async\n");
stdout.write("stdout-async\n");
