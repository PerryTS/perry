import { execFile } from "node:child_process";

const events: string[] = [];
let finishCallback!: () => void;
const callbackDone = new Promise<void>((resolve) => {
  finishCallback = resolve;
});
const child = execFile(
  "node",
  ["-e", "process.stdout.write('out'); process.stderr.write('err');"],
  { encoding: "utf8" },
  (error, stdout, stderr) => {
    events.push("callback");
    console.log("callback error:", error === null ? "null" : error?.name);
    console.log("callback output:", stdout, stderr);
    finishCallback();
  },
);

console.log("child present:", child !== undefined);
console.log(
  "stdio present:",
  child?.stdout !== undefined,
  child?.stderr !== undefined,
);
child?.on("spawn", () => events.push("spawn"));
child?.stdout?.on("end", () => events.push("stdout-end"));
child?.stderr?.on("end", () => events.push("stderr-end"));
child?.on("exit", () => events.push("exit"));

if (child) {
  await new Promise<void>((resolve) => {
    child.on("close", () => {
      events.push("close");
      console.log("order:", events.join(">"));
      console.log(
        "callback before listener close:",
        events.indexOf("callback") < events.indexOf("close"),
      );
      resolve();
    });
  });
} else {
  await callbackDone;
  console.log("order:", events.join(">"));
}
