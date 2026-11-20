import { spawn } from "node:child_process";

for (const expected of [0, 3, 23]) {
  const child = spawn("node", ["-e", `process.exit(${expected})`]);
  const events: string[] = [];
  child.on("exit", (code, signal) => events.push(`exit:${code}:${signal}`));
  await new Promise<void>((resolve) => {
    child.on("close", (code, signal) => {
      events.push(`close:${code}:${signal}`);
      console.log(`${expected} events:`, events.join(">"));
      console.log(`${expected} exitCode:`, child.exitCode);
      console.log(`${expected} signalCode:`, child.signalCode);
      resolve();
    });
  });
}
