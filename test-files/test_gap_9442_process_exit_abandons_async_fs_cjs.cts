// #9442, CommonJS half. `process.exit()` must abandon async fs work that has
// not run yet in a `.cts` module too — the fix lives in the runtime's fs entry
// points, not in the ESM lowering, and this fixture is what proves that rather
// than assuming it. See test_gap_9442_process_exit_abandons_async_fs.ts for the
// full role set and the reasoning; this one keeps the repro plus the two
// controls that stop a fix from over-abandoning.
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const { spawn } = require("node:child_process");

const ROLE_ENV = "PERRY_9442_CJS_ROLE";
const FILE_ENV = "PERRY_9442_CJS_FILE";
const WATCHDOG_MS = 5000;

const role = process.env[ROLE_ENV] || "";
const target = process.env[FILE_ENV] || "";

if (role === "promise-exit") {
  for (let i = 0; i < 5; i++) {
    void fs.promises.appendFile(target, "line " + i + "\n");
  }
  process.exit(0);
} else if (role === "callback-exit") {
  for (let i = 0; i < 5; i++) {
    fs.appendFile(target, "cb " + i + "\n", () => {});
  }
  process.exit(0);
} else if (role === "awaited-control") {
  (async () => {
    await fs.promises.appendFile(target, "awaited\n");
    process.exit(0);
  })();
} else if (role === "natural-control") {
  for (let i = 0; i < 5; i++) {
    void fs.promises.appendFile(target, "line " + i + "\n");
  }
} else {
  const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "perry9442c-"));
  const childArgs = [...process.execArgv, ...process.argv.slice(1)];

  const runRole = (name) =>
    new Promise((resolve) => {
      const file = path.join(tmpDir, name + ".txt");
      const child = spawn(process.execPath, childArgs, {
        env: Object.assign({}, process.env, { [ROLE_ENV]: name, [FILE_ENV]: file }),
        stdio: ["ignore", "inherit", "inherit"],
      });
      let settled = false;
      const report = (code) => {
        const lines = fs.existsSync(file)
          ? fs.readFileSync(file, "utf8").split("\n").filter((l) => l.length > 0)
          : [];
        console.log(name + " exit=" + code + " records=" + lines.length);
        resolve();
      };
      const watchdog = setTimeout(() => {
        if (settled) return;
        settled = true;
        child.kill("SIGKILL");
        report("WATCHDOG");
      }, WATCHDOG_MS);
      child.on("exit", (code) => {
        if (settled) return;
        settled = true;
        clearTimeout(watchdog);
        report(code);
      });
    });

  (async () => {
    await runRole("promise-exit");
    await runRole("callback-exit");
    await runRole("awaited-control");
    await runRole("natural-control");
    fs.rmSync(tmpDir, { recursive: true, force: true });
    console.log("done");
  })();
}
