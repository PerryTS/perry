// #9442: `process.exit()` must terminate WITHOUT completing async fs work that
// has not run yet — Node's exit does not drain the libuv work queue, so five
// fire-and-forget `fs.promises.appendFile` calls followed by `process.exit(0)`
// in the same tick land ZERO records on disk.
//
// Perry landed five. The root cause is not that perry's exit path drains a
// queue (it does not — `js_process_exit` runs the exit sequence and calls
// `libc::_exit`): it is that perry's *async* fs write entry points did the
// write SYNCHRONOUSLY inside the call and returned an already-settled promise
// (or invoked the callback immediately). There was no in-flight work left to
// abandon, so `process.exit()` "worked" for the wrong reason and committed
// state the program had deliberately walked away from.
//
// Every role writes only to its own file and prints nothing; the PARENT reports
// what survived, so the transcript is independent of child stdout ordering.
//
// The two controls are what keep the fix from over-abandoning:
//   * `awaited-control`   — a write that genuinely completed before the exit
//                           MUST still be on disk.
//   * `natural-control`   — a program that ends by draining its event loop
//                           (no `process.exit()` at all) MUST still land every
//                           record. This is also the arm that pins #9441: the
//                           idle-exit fix makes the pump notice emptiness
//                           sooner and must not cut the drain short.
//
// `exit-listener` answers the question the issue asks about the neighbouring
// known divergence: the `exit` event and the abandoned work do NOT share a
// root. The listener fires on an explicit exit (#9403) and its own synchronous
// write lands, while the pending async writes are still dropped.
import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";
import { spawn } from "node:child_process";

const ROLE_ENV = "PERRY_9442_ROLE";
const FILE_ENV = "PERRY_9442_FILE";
const WATCHDOG_MS = 5000;

const role = process.env[ROLE_ENV] ?? "";
const target = process.env[FILE_ENV] ?? "";

function fireAndForgetPromises(n: number): void {
  for (let i = 0; i < n; i++) {
    // Deliberately un-awaited: the promise is discarded on purpose.
    void fs.promises.appendFile(target, "line " + i + "\n");
  }
}

if (role === "promise-exit") {
  // The reported repro.
  fireAndForgetPromises(5);
  process.exit(0);
} else if (role === "callback-exit") {
  // The callback form of the same operation.
  for (let i = 0; i < 5; i++) {
    fs.appendFile(target, "cb " + i + "\n", () => {});
  }
  process.exit(0);
} else if (role === "writefile-exit") {
  // `writeFile` shares the entry point family with `appendFile`.
  void fs.promises.writeFile(target, "written\n");
  process.exit(0);
} else if (role === "timer-exit") {
  // A pending timer is abandoned by `process.exit()` in both engines; this arm
  // exists so a fix that starts draining the loop at exit reports here.
  setTimeout(() => {
    fs.appendFileSync(target, "timer\n");
  }, 0);
  process.exit(0);
} else if (role === "sync-control") {
  // CONTROL: a synchronous write is already committed and must survive.
  fs.appendFileSync(target, "sync\n");
  process.exit(0);
} else if (role === "awaited-control") {
  // CONTROL: an awaited write genuinely completed before the exit. A fix that
  // abandons this is worse than the bug.
  (async () => {
    await fs.promises.appendFile(target, "awaited\n");
    process.exit(0);
  })();
} else if (role === "natural-control") {
  // CONTROL: no `process.exit()` at all. The event loop drains and every
  // record must land, exactly as in Node.
  fireAndForgetPromises(5);
} else if (role === "exit-listener") {
  // The `exit` event fires on an explicit exit and its synchronous work lands;
  // the pending async writes are still abandoned.
  process.on("exit", () => {
    fs.appendFileSync(target, "exit-listener\n");
  });
  fireAndForgetPromises(3);
  process.exit(0);
} else {
  const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "perry9442-"));
  const childArgs = [...process.execArgv, ...process.argv.slice(1)];

  const runRole = (name: string) =>
    new Promise<void>((resolve) => {
      const file = path.join(tmpDir, name + ".txt");
      const child = spawn(process.execPath, childArgs, {
        env: { ...process.env, [ROLE_ENV]: name, [FILE_ENV]: file },
        stdio: ["ignore", "inherit", "inherit"],
      });
      let settled = false;
      // Record COUNT only, with a missing file counted as zero. Whether Node's
      // thread pool won the race to `open(2)` before `_exit` — leaving an empty
      // file behind rather than none — is a scheduling artifact, not a
      // semantic; the question this fixture asks is the issue's own, "how many
      // records did the abandoned program commit". Contents are sorted because
      // Node's pool completes concurrent appends in an arbitrary order.
      const report = (code: number | null | string) => {
        const lines = fs.existsSync(file)
          ? fs.readFileSync(file, "utf8").split("\n").filter((l) => l.length > 0)
          : [];
        console.log(name + " exit=" + code + " records=" + lines.length);
        if (lines.length > 0) {
          console.log("  " + name + ": " + JSON.stringify(lines.slice().sort().join("|")));
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
    await runRole("promise-exit");
    await runRole("callback-exit");
    await runRole("writefile-exit");
    await runRole("timer-exit");
    await runRole("sync-control");
    await runRole("awaited-control");
    await runRole("natural-control");
    await runRole("exit-listener");
    fs.rmSync(tmpDir, { recursive: true, force: true });
    console.log("done");
  })();
}
