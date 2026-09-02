// #9493 (measured negative): the issue suspected `fs.Utf8Stream` lost buffered
// data at `process.exit()` "that node does not have". Node 26.5.1 has no exit
// flush either — a chunk held back by `minLength` is lost on an explicit exit
// AND on a natural one; only `end()`, `flush()`/`flushSync()`, or a full
// buffer commit it. So no exit flush was added: this fixture pins the oracle,
// so that a later "flush on exit" cannot land as a parity improvement.
//
// Silent roles write to their own file; the parent reports what survived.
import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";
import { spawn } from "node:child_process";

const ROLE_ENV = "PERRY_9493_U8_ROLE";
const FILE_ENV = "PERRY_9493_U8_FILE";
const WATCHDOG_MS = 5000;

const role = process.env[ROLE_ENV] ?? "";
const target = process.env[FILE_ENV] ?? "";

// `minLength` well above the payload: the write is buffered, not flushed.
const opts = () => ({ fd: fs.openSync(target, "w"), minLength: 4096 });

if (role === "u8-exit") {
  const s = new fs.Utf8Stream(opts());
  s.write("hello\n");
  process.exit(0);
} else if (role === "u8-natural") {
  // Lost in Node too: nothing flushes a below-`minLength` buffer at exit.
  const s = new fs.Utf8Stream(opts());
  s.write("hello\n");
} else if (role === "u8-end-exit") {
  // CONTROL: `end()` drains synchronously for an already-open fd.
  const s = new fs.Utf8Stream(opts());
  s.write("hello\n");
  s.end();
  process.exit(0);
} else if (role === "u8-flushsync-on-exit") {
  // CONTROL: the documented way to keep the data — an `'exit'` listener
  // calling `flushSync()` (what pino's `on-exit-leak-free` hook does).
  const s = new fs.Utf8Stream(opts());
  s.write("hello\n");
  process.on("exit", () => s.flushSync());
  process.exit(0);
} else if (role === "u8-full-buffer-exit") {
  // CONTROL: a write that crosses `minLength` is committed before the exit.
  const s = new fs.Utf8Stream(opts());
  s.write("x".repeat(5000) + "\n");
  process.exit(0);
} else {
  const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "perry9493u8-"));
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
    await runRole("u8-exit");
    await runRole("u8-natural");
    await runRole("u8-end-exit");
    await runRole("u8-flushsync-on-exit");
    await runRole("u8-full-buffer-exit");
    fs.rmSync(tmpDir, { recursive: true, force: true });
    console.log("done");
  })();
}
