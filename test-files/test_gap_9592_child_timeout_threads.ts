// #9592 — a timed spawn must not retain its timeout OS thread after the child
// exits. The Linux thread census catches the old one-sleeper-per-timeout leak;
// the slow-child arm keeps the actual timeout behavior covered everywhere.
import { spawn } from "node:child_process";
import { readdirSync } from "node:fs";

/** Resolve after the child process and its stdio handles have closed. */
function close(child: any): Promise<void> {
  return new Promise((resolve) => child.on("close", () => resolve()));
}

/** Count this process's live OS threads when Linux exposes the task census. */
function threadCount(): number {
  return process.platform === "linux" ? readdirSync("/proc/self/task").length : 0;
}

const baseline = threadCount();
const quickChildren: Promise<void>[] = [];
for (let i = 0; i < 50; i++) {
  quickChildren.push(
    close(spawn("/bin/true", [], { stdio: "ignore", timeout: 60_000 })),
  );
}
await Promise.all(quickChildren);

let timeoutThreadsReleased = process.platform !== "linux";
const releaseDeadline = Date.now() + 1_000;
while (!timeoutThreadsReleased && Date.now() < releaseDeadline) {
  timeoutThreadsReleased = threadCount() <= baseline + 5;
  if (!timeoutThreadsReleased) {
    await new Promise<void>((resolve) => setTimeout(resolve, 20));
  }
}
console.log("timeout threads released:", timeoutThreadsReleased);

const started = Date.now();
const slow = spawn("/bin/sleep", ["30"], {
  stdio: "ignore",
  timeout: 100,
});
await close(slow);
const elapsed = Date.now() - started;
console.log(
  "slow child killed on time:",
  slow.killed &&
    slow.signalCode === "SIGTERM" &&
    elapsed >= 50 &&
    elapsed < 5_000,
);
