import { exec, execFile } from "node:child_process";

function run(
  label: string,
  start: (
    callback: (error: Error | null, stdout: unknown, stderr: unknown) => void,
  ) => void,
) {
  return new Promise<void>((resolve) => {
    start((error, stdout, stderr) => {
      console.log(`${label} error:`, error === null ? "null" : error.name);
      console.log(`${label} stdout:`, JSON.stringify(String(stdout)));
      console.log(`${label} stderr:`, JSON.stringify(String(stderr)));
      resolve();
    });
  });
}

await run("exec undefined", (callback) =>
  exec(
    "node -e \"process.stdout.write('undefined-timeout')\"",
    { timeout: undefined, encoding: "utf8" },
    callback,
  ),
);

await run("execFile zero", (callback) =>
  execFile(
    "node",
    ["-e", "process.stdout.write('zero-timeout')"],
    { timeout: 0, encoding: "utf8" },
    callback,
  ),
);

await run("execFile large", (callback) =>
  execFile(
    "node",
    ["-e", "process.stdout.write('large-timeout')"],
    { timeout: 2 ** 31 - 1, encoding: "utf8" },
    callback,
  ),
);
