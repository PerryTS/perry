import { exec, execFile } from "node:child_process";
import { promisify } from "node:util";

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

const execPromise = promisify(exec);
const execFilePromise = promisify(execFile);
const execResult = execPromise(
  "node -e \"process.stdout.write('exec-out'); process.stderr.write('exec-err')\"",
  { encoding: "utf8" },
);
console.log(
  "promise exec child:",
  typeof execResult.child?.pid,
  execResult.child?.spawnfile,
);
console.log("promise exec result:", JSON.stringify(await execResult));

const fileResult = execFilePromise(
  "node",
  ["-e", "process.stdout.write(process.argv[1])", "file-out"],
  { encoding: "utf8" },
);
console.log(
  "promise file child:",
  typeof fileResult.child?.pid,
  fileResult.child?.spawnfile,
);
console.log("promise file result:", JSON.stringify(await fileResult));

try {
  await execFilePromise(
    "node",
    [
      "-e",
      "process.stdout.write('partial'); process.stderr.write('problem'); process.exit(7)",
    ],
    { encoding: "utf8" },
  );
  console.log("promise failure: no rejection");
} catch (error: any) {
  console.log(
    "promise failure:",
    error.name,
    error.code,
    error.killed,
    error.signal,
  );
  console.log("promise failure output:", error.stdout, error.stderr);
}
