import { spawn } from "node:child_process";

const child = spawn("node", [
  "-e",
  "process.stdout.write('ready'); process.stderr.write('warn');",
]);
const events: string[] = [];
let stdout = "";
let stderr = "";

child.on("spawn", () => events.push("spawn"));
child.stdout.on("data", (chunk: Buffer) => {
  events.push("stdout-data");
  stdout += chunk.toString("utf8");
});
child.stderr.on("data", (chunk: Buffer) => {
  events.push("stderr-data");
  stderr += chunk.toString("utf8");
});
child.stdout.on("end", () => events.push("stdout-end"));
child.stderr.on("end", () => events.push("stderr-end"));
child.on("exit", (code, signal) => events.push(`exit:${code}:${signal}`));

await new Promise<void>((resolve) => {
  child.on("close", (code, signal) => {
    events.push(`close:${code}:${signal}`);
    console.log("spawn first:", events[0] === "spawn");
    console.log(
      "exit before close:",
      events.indexOf("exit:0:null") < events.indexOf("close:0:null"),
    );
    console.log(
      "stdout end before close:",
      events.indexOf("stdout-end") < events.indexOf("close:0:null"),
    );
    console.log(
      "stderr end before close:",
      events.indexOf("stderr-end") < events.indexOf("close:0:null"),
    );
    console.log("output:", stdout, stderr);
    resolve();
  });
});
