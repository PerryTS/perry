import { fork } from "node:child_process";
import { realpathSync } from "node:fs";
import { tmpdir } from "node:os";

const helper = new URL("../fixtures/fork-url-child.cjs", import.meta.url);

const child = fork(helper, ["first", "space value"], {
  cwd: tmpdir(),
  execArgv: [],
  execPath: "node",
  stdio: ["ignore", "ignore", "ignore", "ipc"],
});

try {
  const message = await new Promise<any>((resolve, reject) => {
    child.once("error", reject);
    child.once("message", resolve);
  });
  console.log("message argv:", message.argv.join("|"));
  console.log("message cwd matches:", message.cwd === realpathSync(tmpdir()));
  console.log("spawnfile:", child.spawnfile);
  console.log(
    "spawnargs has helper:",
    child.spawnargs.some((arg) => arg.endsWith("fork-url-child.cjs")),
  );
  console.log(
    "close:",
    await new Promise((resolve) => child.once("close", resolve)),
  );
} finally {
  if (child.connected) child.disconnect();
  if (child.exitCode === null) child.kill();
}
