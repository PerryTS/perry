// #9493: `console.clear()` wrote its escape fragment into Rust's line-buffered
// stdout; with no newline behind it, a `process.exit()` (which terminates via
// `_exit`, no flush) swallowed it. Node's TTY writes are synchronous. The
// fragment also differed: Node emits `cursorTo(0, 0)` + `clearScreenDown()`
// (`\x1b[1;1H\x1b[0J`), and only when stdout is a TTY and `TERM` is not
// `dumb`.
//
// Roles run under a pseudo-terminal (python's `pty.spawn` — `script(1)`
// needs a real tty on ITS stdin, which a test harness does not have); the
// parent prints the captured bytes, `\r` stripped, as JSON. Byte-for-byte
// against `node --experimental-strip-types`. Skipped as a whole on win32
// (both engines print the same line).
import { spawn } from "node:child_process";

const ROLE_ENV = "PERRY_9493_CLEAR_ROLE";
const WATCHDOG_MS = 5000;
const role = process.env[ROLE_ENV] ?? "";

if (role === "clear-exit") {
  console.clear();
  process.exit(0);
} else if (role === "log-clear-exit") {
  console.log("before");
  console.clear();
  process.exit(0);
} else if (role === "clear-natural") {
  console.clear();
  console.log("after");
} else if (role === "clear-dumb-term") {
  console.clear();
  console.log("dumb");
} else if (role === "clear-piped") {
  console.clear();
  console.log("piped");
} else if (process.platform === "win32") {
  console.log("pty roles skipped on win32");
  console.log("done");
} else {
  const childArgs = [...process.execArgv, ...process.argv.slice(1)];
  const PTY = "import pty, sys, os\nst = pty.spawn(sys.argv[1:])\nsys.exit(os.waitstatus_to_exitcode(st))\n";

  const runRole = (name: string, term: string, viaPty: boolean) =>
    new Promise<void>((resolve) => {
      const env = { ...process.env, [ROLE_ENV]: name, TERM: term };
      const child = viaPty
        ? spawn("python3", ["-c", PTY, process.execPath, ...childArgs], { env, stdio: ["ignore", "pipe", "inherit"] })
        : spawn(process.execPath, childArgs, { env, stdio: ["ignore", "pipe", "inherit"] });
      let out = "";
      let settled = false;
      child.stdout.on("data", (d: Buffer) => { out += d.toString("latin1"); });
      const report = (code: number | null | string) => {
        console.log(name + " exit=" + code + " out=" + JSON.stringify(out.replace(/\r/g, "")));
        resolve();
      };
      const watchdog = setTimeout(() => {
        if (settled) return;
        settled = true;
        child.kill("SIGKILL");
        report("WATCHDOG");
      }, WATCHDOG_MS);
      child.on("close", (code) => {
        if (settled) return;
        settled = true;
        clearTimeout(watchdog);
        report(code);
      });
      child.on("error", (e: Error) => {
        if (settled) return;
        settled = true;
        clearTimeout(watchdog);
        report("SPAWN-ERROR " + e.message);
      });
    });

  (async () => {
    await runRole("clear-exit", "xterm", true);
    await runRole("log-clear-exit", "xterm", true);
    await runRole("clear-natural", "xterm", true);
    await runRole("clear-dumb-term", "dumb", true);
    await runRole("clear-piped", "xterm", false);
    console.log("done");
  })();
}
