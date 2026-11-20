import { readFileSync } from "node:fs";
import { randomBytes, randomUUID } from "node:crypto";
import { isatty } from "node:tty";
import { performance } from "node:perf_hooks";
const env = process.env;
const envNames = Object.keys(env);
const values = [
 process.env.SNAP21_SENTINEL, "SNAP21_ABSENT" in env, envNames.length,
 process.argv.join("|"), process.cwd(), navigator.language,
 new Intl.DateTimeFormat().resolvedOptions().timeZone,
 isatty(0), isatty(1), isatty(2), process.stdout.columns,
 process.getuid(), process.getgid(), process.pid, process.ppid,
 Date.now(), performance.now(), process.hrtime.bigint().toString(),
 Math.random(), randomBytes(4).toString("hex"), randomUUID(),
 readFileSync("package.json","utf8"), readFileSync("inputs.ts","utf8"),
 new Error("SNAP21").stack,
];
console.log("INPUTS21", values.length);
export { values };
