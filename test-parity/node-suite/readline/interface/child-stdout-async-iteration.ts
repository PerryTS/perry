// readline.createInterface({ input: child.stdout }) over a child_process
// stdout pipe: lines must be delivered both via the 'line' event and via
// `for await (const line of rl)` async iteration. Regression for the
// child_process-stdout readline gaps (#2569 follow-up).
import * as readline from "node:readline";
import { spawn } from "node:child_process";

// --- event-based delivery -------------------------------------------------
const child1 = spawn("sh", ["-c", "printf 'alpha\\nbeta\\ngamma\\n'"]);
const rl1 = readline.createInterface({
  input: child1.stdout!,
  crlfDelay: Infinity,
});
const events: string[] = [];
rl1.on("line", (line) => {
  events.push(`line:${line}`);
});
await new Promise<void>((resolve) => {
  rl1.on("close", () => resolve());
});
console.log("events:", events.join("|"));

// --- async iteration ------------------------------------------------------
const child2 = spawn("sh", ["-c", "printf 'one\\ntwo\\nthree\\n'"]);
const rl2 = readline.createInterface({
  input: child2.stdout!,
  crlfDelay: Infinity,
});
const lines: string[] = [];
for await (const line of rl2) {
  lines.push(line);
}
console.log("iterated:", lines.join("|"));
