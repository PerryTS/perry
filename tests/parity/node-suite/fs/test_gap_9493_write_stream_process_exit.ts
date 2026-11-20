// #9493: `fs.createWriteStream` was synchronous under the hood — the fd was
// opened at construction and every `write()` did `seek`+`write_all` inline —
// so a `process.exit()` in the same tick committed bytes Node abandons. Node
// opens the file on a later turn (`_construct` → `fs.open` on the thread
// pool) and each `write()` is a pool request; nothing has touched the disk
// when the call returns, and an exit in the same tick leaves no file behind.
//
// The fix parks the open, the queued writes and the close on successive
// event-loop turns — the #9442 mechanism (`fs/deferred.rs`). Byte-for-byte
// against `node --experimental-strip-types`.
//
// Silent roles write only to their own file; the PARENT reports what
// survived. Printing roles inherit stdout and pin the event ORDER: `'open'`
// then `'ready'`, a microtask boundary, then the write callbacks, the `end()`
// callback, `'finish'`, another microtask boundary, then `'close'`; a
// supplied fd emits no `'open'`/`'ready'`; an open failure delivers the error
// to every pending callback before `'error'`, then `'close'`.
//
// Controls that keep the fix from over-abandoning: `ws-natural` (no
// `process.exit()` — the loop drains and every record lands), `ws-finish-exit`
// (an exit from `'finish'` must find the bytes on disk), `ws-sync-control`.
import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";
import { spawn } from "node:child_process";

const ROLE_ENV = "PERRY_9493_WS_ROLE";
const FILE_ENV = "PERRY_9493_WS_FILE";
const WATCHDOG_MS = 5000;

const role = process.env[ROLE_ENV] ?? "";
const target = process.env[FILE_ENV] ?? "";

if (role === "ws-exit") {
  // The reported repro: the open has not even happened yet.
  const ws = fs.createWriteStream(target);
  ws.write("a\nb\nc\n");
  process.exit(0);
} else if (role === "ws-end-exit") {
  const ws = fs.createWriteStream(target);
  ws.write("a\n");
  ws.end("b\n");
  process.exit(0);
} else if (role === "ws-open-exit") {
  // Exit from the `'open'` listener: the file exists (the open ran) but the
  // queued write has not — writes are dispatched on a LATER turn.
  const ws = fs.createWriteStream(target);
  ws.write("a\nb\n");
  ws.on("open", () => process.exit(0));
} else if (role === "ws-fd-exit") {
  // A supplied fd skips the open; the write is still a pool request.
  const fd = fs.openSync(target, "w");
  const ws = fs.createWriteStream(target, { fd });
  ws.write("a\n");
  process.exit(0);
} else if (role === "ws-natural") {
  // CONTROL: no `process.exit()`. The pending write keeps the loop alive and
  // every record lands, even without `end()`.
  const ws = fs.createWriteStream(target);
  ws.write("a\nb\nc\n");
} else if (role === "ws-finish-exit") {
  // CONTROL: `'finish'` means the bytes are on disk.
  const ws = fs.createWriteStream(target);
  ws.end("x\ny\n");
  ws.on("finish", () => process.exit(0));
} else if (role === "ws-sync-control") {
  fs.writeFileSync(target, "sync\n");
  process.exit(0);
} else if (role === "ws-order") {
  const ws = fs.createWriteStream(target, { highWaterMark: 8 });
  console.log("ctor fd=" + ws.fd + " pending=" + ws.pending);
  ws.on("open", (fd: number) => console.log("open " + typeof fd + " pending=" + ws.pending));
  ws.on("ready", () => {
    console.log("ready");
    Promise.resolve().then(() => console.log("microtask after ready"));
  });
  ws.on("drain", () => {
    console.log("drain len=" + ws.writableLength + " needDrain=" + ws.writableNeedDrain);
    ws.end("tail\n", () => console.log("end-cb"));
  });
  ws.on("finish", () => {
    console.log("finish finished=" + ws.writableFinished + " bytesWritten=" + ws.bytesWritten);
    Promise.resolve().then(() => console.log("microtask after finish"));
  });
  ws.on("close", () => console.log("close fd=" + ws.fd + " closed=" + ws.closed));
  const w1 = ws.write("12345\n", () => console.log("cb1"));
  console.log("w1=" + w1 + " len=" + ws.writableLength + " needDrain=" + ws.writableNeedDrain);
  const w2 = ws.write("67890\n", () => console.log("cb2"));
  console.log("w2=" + w2 + " len=" + ws.writableLength + " needDrain=" + ws.writableNeedDrain);
  console.log("sync done ended=" + ws.writableEnded);
} else if (role === "ws-fd-order") {
  const fd = fs.openSync(target, "w");
  const ws = fs.createWriteStream(target, { fd });
  ws.on("open", () => console.log("open?!"));
  ws.on("ready", () => console.log("ready?!"));
  ws.on("finish", () => console.log("finish"));
  ws.on("close", () => console.log("close"));
  console.log("pending=" + ws.pending + " fd=" + typeof ws.fd);
  ws.end("x\n", () => console.log("end-cb"));
  console.log("sync done");
} else if (role === "ws-open-error") {
  const ws = fs.createWriteStream(path.join(target, "missing-dir", "x.txt"));
  ws.on("error", (e: NodeJS.ErrnoException) => console.log("error " + e.code));
  ws.on("close", () => console.log("close"));
  ws.write("a\n", (e?: NodeJS.ErrnoException | null) => console.log("write-cb " + (e ? e.code : "ok")));
  ws.end("b\n", (e?: NodeJS.ErrnoException | null) => console.log("end-cb " + (e ? e.code : "ok")));
  console.log("sync done");
} else {
  const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "perry9493ws-"));
  const childArgs = [...process.execArgv, ...process.argv.slice(1)];

  const runRole = (name: string, silent: boolean) =>
    new Promise<void>((resolve) => {
      const file = path.join(tmpDir, name + ".txt");
      if (!silent) console.log("== " + name);
      const child = spawn(process.execPath, childArgs, {
        env: { ...process.env, [ROLE_ENV]: name, [FILE_ENV]: file },
        stdio: ["ignore", "inherit", "inherit"],
      });
      let settled = false;
      const report = (code: number | null | string) => {
        if (silent) {
          // Existence is the issue's own observable (Node's exit-in-tick
          // leaves NO file); records count what a committed write landed.
          const exists = fs.existsSync(file);
          const lines = exists
            ? fs.readFileSync(file, "utf8").split("\n").filter((l) => l.length > 0)
            : [];
          console.log(name + " exit=" + code + " exists=" + exists + " records=" + lines.length);
        } else {
          console.log(name + " exit=" + code);
        }
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
    await runRole("ws-exit", true);
    await runRole("ws-end-exit", true);
    await runRole("ws-open-exit", true);
    await runRole("ws-fd-exit", true);
    await runRole("ws-natural", true);
    await runRole("ws-finish-exit", true);
    await runRole("ws-sync-control", true);
    await runRole("ws-order", false);
    await runRole("ws-fd-order", false);
    await runRole("ws-open-error", false);
    fs.rmSync(tmpDir, { recursive: true, force: true });
    console.log("done");
  })();
}
