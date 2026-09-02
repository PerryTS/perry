// #9493: `child.stdin.write()` did a blocking `write_all` into the pipe. Node
// (libuv `uv_try_write`) commits what the pipe accepts right now, queues the
// remainder for the loop, returns `writableLength < writableHighWaterMark`,
// and emits `'drain'` once the queue empties. Perry therefore (a) parked the
// main thread on a full pipe until the child read, (b) always returned `true`
// and never emitted `'drain'` — the MCP stdio client waits on exactly that
// pair — and (c) at `process.exit()` committed every byte, where Node commits
// only the pipe-capacity prefix and abandons the rest.
//
// Byte counts are platform-dependent (pipe/socketpair capacity), so the
// silent roles report a CLASSIFICATION: `none`, `partial` or `full`. The
// grandchild sleeps before reading so the pipe cannot drain during the write,
// and drops a marker file once its copy is done; the parent waits for it.
//
// Byte-for-byte against `node --experimental-strip-types`.
import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";
import { spawn } from "node:child_process";

const ROLE_ENV = "PERRY_9493_STDIN_ROLE";
const FILE_ENV = "PERRY_9493_STDIN_FILE";
const WATCHDOG_MS = 8000;
const BIG = 4 * 1024 * 1024;

const role = process.env[ROLE_ENV] ?? "";
const target = process.env[FILE_ENV] ?? "";

const shq = (s: string) => "'" + s.replace(/'/g, "'\\''") + "'";
const sink = (delay: string) =>
  spawn("sh", ["-c", "sleep " + delay + "; cat > " + shq(target) + "; echo done > " + shq(target + ".done")], {
    stdio: ["pipe", "ignore", "ignore"],
  });

if (role === "stdin-small-exit") {
  // Fits the pipe: committed synchronously in both engines, exit or not.
  const c = sink("0");
  c.stdin.write("hello\n");
  process.exit(0);
} else if (role === "stdin-big-exit") {
  // Larger than the pipe: only the try-write prefix lands; the queued
  // remainder is abandoned by the exit.
  const c = sink("0.5");
  c.stdin.write(Buffer.alloc(BIG, 120));
  process.exit(0);
} else if (role === "stdin-big-end-exit") {
  // `end()` behind queued bytes must not close the pipe ahead of them, and
  // the exit still abandons what was queued.
  const c = sink("0.5");
  c.stdin.write(Buffer.alloc(BIG, 120));
  c.stdin.end();
  process.exit(0);
} else if (role === "stdin-natural") {
  // CONTROL: no exit. The queue drains once the child reads; `end()` closes
  // the pipe only after the last byte, so the child sees all of them.
  const c = sink("0.5");
  c.stdin.write(Buffer.alloc(BIG, 120));
  c.stdin.end();
} else if (role === "stdin-backpressure") {
  const c = spawn("sh", ["-c", "sleep 0.3; cat > /dev/null"], { stdio: ["pipe", "ignore", "ignore"] });
  console.log("hwm=" + c.stdin.writableHighWaterMark + " len=" + c.stdin.writableLength);
  c.stdin.on("drain", () => {
    console.log("drain len=" + c.stdin.writableLength + " needDrain=" + c.stdin.writableNeedDrain);
    c.stdin.end(() => console.log("end-cb"));
  });
  const w1 = c.stdin.write(Buffer.alloc(BIG, 120), () => console.log("cb1"));
  console.log("w1=" + w1 + " queued=" + (c.stdin.writableLength > 0) + " needDrain=" + c.stdin.writableNeedDrain);
  const w2 = c.stdin.write("tail\n", () => console.log("cb2"));
  console.log("w2=" + w2);
  c.on("close", (code) => console.log("child close " + code));
  console.log("sync done");
} else if (role === "stdin-small-write-callback") {
  // CONTROL: the pre-#9493 completion path — a small write returns `true`
  // and its callback still fires on a later turn, before the child's data.
  const c = spawn("sh", ["-c", "cat"], { stdio: ["pipe", "pipe", "ignore"] });
  let out = "";
  c.stdout.on("data", (d: Buffer) => { out += d.toString(); });
  c.on("close", () => console.log("echoed " + JSON.stringify(out)));
  const w = c.stdin.write("ping\n", () => console.log("cb"));
  console.log("w=" + w + " len=" + c.stdin.writableLength);
  c.stdin.end();
  console.log("sync done");
} else {
  const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "perry9493stdin-"));
  const childArgs = [...process.execArgv, ...process.argv.slice(1)];

  const waitForMarker = (marker: string) =>
    new Promise<boolean>((resolve) => {
      const deadline = Date.now() + WATCHDOG_MS;
      const poll = () => {
        if (fs.existsSync(marker)) return resolve(true);
        if (Date.now() > deadline) return resolve(false);
        setTimeout(poll, 20);
      };
      poll();
    });

  const runRole = (name: string, silent: boolean) =>
    new Promise<void>((resolve) => {
      const file = path.join(tmpDir, name + ".txt");
      if (!silent) console.log("== " + name);
      const child = spawn(process.execPath, childArgs, {
        env: { ...process.env, [ROLE_ENV]: name, [FILE_ENV]: file },
        stdio: ["ignore", "inherit", "inherit"],
      });
      let settled = false;
      const report = async (code: number | null | string) => {
        if (silent) {
          const landed = (await waitForMarker(file + ".done")) && fs.existsSync(file)
            ? fs.statSync(file).size
            : -1;
          const total = name === "stdin-small-exit" ? 6 : BIG;
          const kind = landed < 0 ? "no-marker" : landed === 0 ? "none" : landed >= total ? "full" : "partial";
          console.log(name + " exit=" + code + " landed=" + kind);
        } else {
          console.log(name + " exit=" + code);
        }
        resolve();
      };
      const watchdog = setTimeout(() => {
        if (settled) return;
        settled = true;
        child.kill("SIGKILL");
        void report("WATCHDOG");
      }, WATCHDOG_MS);
      child.on("exit", (code) => {
        if (settled) return;
        settled = true;
        clearTimeout(watchdog);
        void report(code);
      });
    });

  (async () => {
    await runRole("stdin-small-exit", true);
    await runRole("stdin-big-exit", true);
    await runRole("stdin-big-end-exit", true);
    await runRole("stdin-natural", true);
    await runRole("stdin-backpressure", false);
    await runRole("stdin-small-write-callback", false);
    fs.rmSync(tmpDir, { recursive: true, force: true });
    console.log("done");
  })();
}
