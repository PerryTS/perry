// #9493, CommonJS half. `fs.createWriteStream` must open and write on later
// turns — so a `process.exit()` in the same tick leaves no file — in a `.cts`
// module too: the fix lives in the runtime's stream, not in the ESM lowering,
// and this fixture proves that rather than assuming it. See
// test_gap_9493_write_stream_process_exit.ts for the full role set; this one
// keeps the repro plus the two controls that stop a fix from over-abandoning.
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const { spawn } = require("node:child_process");

const ROLE_ENV = "PERRY_9493_WS_CJS_ROLE";
const FILE_ENV = "PERRY_9493_WS_CJS_FILE";
const WATCHDOG_MS = 5000;

const role = process.env[ROLE_ENV] ?? "";
const target = process.env[FILE_ENV] ?? "";

if (role === "ws-exit") {
  const ws = fs.createWriteStream(target);
  ws.write("a\nb\nc\n");
  process.exit(0);
} else if (role === "ws-natural") {
  const ws = fs.createWriteStream(target);
  ws.write("a\nb\nc\n");
} else if (role === "ws-finish-exit") {
  const ws = fs.createWriteStream(target);
  ws.end("x\ny\n");
  ws.on("finish", () => process.exit(0));
} else {
  const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "perry9493wscjs-"));
  const childArgs = [...process.execArgv, ...process.argv.slice(1)];

  const runRole = (name: string) =>
    new Promise<void>((resolve) => {
      const file = path.join(tmpDir, name + ".txt");
      const child = spawn(process.execPath, childArgs, {
        env: { ...process.env, [ROLE_ENV]: name, [FILE_ENV]: file },
        stdio: ["ignore", "inherit", "inherit"],
      });
      let settled = false;
      const report = (code: number | null | string) => {
        const exists = fs.existsSync(file);
        const lines = exists
          ? fs.readFileSync(file, "utf8").split("\n").filter((l: string) => l.length > 0)
          : [];
        console.log(name + " exit=" + code + " exists=" + exists + " records=" + lines.length);
        resolve();
      };
      const watchdog = setTimeout(() => {
        if (settled) return;
        settled = true;
        child.kill("SIGKILL");
        report("WATCHDOG");
      }, WATCHDOG_MS);
      child.on("exit", (code: number | null) => {
        if (settled) return;
        settled = true;
        clearTimeout(watchdog);
        report(code);
      });
    });

  (async () => {
    await runRole("ws-exit");
    await runRole("ws-natural");
    await runRole("ws-finish-exit");
    fs.rmSync(tmpDir, { recursive: true, force: true });
    console.log("done");
  })();
}
