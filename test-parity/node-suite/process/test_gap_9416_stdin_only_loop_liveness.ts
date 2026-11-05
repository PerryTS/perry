// #9416: a program whose only pending work is a `process.stdin` read must keep
// the event loop turning, exactly as Node keeps a process alive for a ref'd
// stdin handle.
//
// The shape that failed is `process.stdin` reached as an OBJECT — an alias
// (`const s = process.stdin`), a parameter, or a field — rather than the
// literal `process.stdin.on(...)` spelling codegen lowers to perry-stdlib's
// readline extern. The object form files its listener in perry-runtime's own
// stdin registries and starts perry-runtime's own fd-0 reader, and #9407 taught
// *perry-stdlib's* `js_stdlib_has_active_handles` about those lists. But a
// program whose only stdlib-flavoured work IS that listener links RUNTIME-ONLY,
// and then the symbol the generated event loop calls is perry-runtime's
// `js_stdlib_has_active_handles` trampoline, whose registered stdlib pointer is
// null. `stdin_listeners_keep_loop_alive()` answered "keep running" on every
// single check and nothing ever asked it: the loop found no work and `main`
// returned with the pipe still open and the bytes unread.
//
// The parity runner gives a fixture no stdin of its own, so this test re-spawns
// itself with a pipe on the child's stdin and drives each shape in a child role.
// The payload is written after a delay so that "the loop stayed alive" is what
// is actually measured — the unfixed engine exits in ~20-50 ms, long before it
// arrives, which is why the pre-fix failure is deterministic here even though
// the bug reads as flaky when input is already buffered.
//
// The last two roles are negative controls: they must still exit PROMPTLY while
// the parent holds the pipe open forever. A fix that simply pins the loop open
// whenever stdin exists would hang them, and the watchdog would report it.
import { spawn } from "node:child_process";

const ROLE_ENV = "PERRY_9416_STDIN_ROLE";
const PAYLOAD = "alpha\nbeta\n";
const WRITE_DELAY_MS = 120;
// Generous on purpose: this machine runs many concurrent compiles, and the
// watchdog exists only so a REGRESSION that hangs reports as a readable diff
// instead of a harness timeout. A healthy role finishes in well under 1.2 s.
const WATCHDOG_MS = 3000;
const role = process.env[ROLE_ENV] ?? "";

// The roles that have their answer leave explicitly, so the fixture stays well
// inside the parity runner's per-test timeout. Perry's event loop sleeps for up
// to a second in `js_wait_for_event` before it re-checks liveness, so a role
// that drains naturally costs ~1 s of pure idle wait where Node costs ~20 ms;
// that lag is a separate matter from this issue and the two negative controls
// below still exercise the natural-drain path.
function finish(line: string): void {
  console.log(line);
  process.exit(0);
}

function reportText(label: string, text: string): void {
  finish(label + ' text: ' + JSON.stringify(text));
}

if (role === "aliased-data") {
  // The reported shape: an aliased receiver, data + end, nothing else pending.
  const stream: any = process.stdin;
  let acc = "";
  stream.on("data", (chunk: any) => {
    acc += String(chunk);
  });
  stream.on("end", () => reportText("aliased-data", acc));
} else if (role === "param-data") {
  // Same registry, reached through a parameter (claude-code's stdio transport
  // shape: `helper(process.stdin)` then `stream.on("data", ...)` inside).
  const read = (stream: any, done: (text: string) => void) => {
    let acc = "";
    stream.on("data", (chunk: any) => {
      acc += String(chunk);
    });
    stream.once("end", () => done(acc));
  };
  read(process.stdin, (text) => reportText("param-data", text));
} else if (role === "end-only") {
  // No data listener at all — only the terminal event. Node holds the process
  // open for it; the parent closes the pipe with nothing written.
  const stream: any = process.stdin;
  stream.once("end", () => finish("end-only fired: true"));
  stream.resume();
} else if (role === "in-timeout") {
  // The read is registered a turn later, so the very first liveness check sees
  // no stdin work at all and a timer must carry the loop to the registration.
  setTimeout(() => {
    const stream: any = process.stdin;
    let acc = "";
    stream.on("data", (chunk: any) => {
      acc += String(chunk);
    });
    stream.on("end", () => reportText("in-timeout", acc));
  }, 30);
} else if (role === "with-timer") {
  // stdin plus one short timer: the timer expires long before the payload
  // arrives, so the read still has to hold the loop by itself afterwards.
  const stream: any = process.stdin;
  let acc = "";
  stream.on("data", (chunk: any) => {
    acc += String(chunk);
  });
  stream.on("end", () => reportText("with-timer", acc));
  setTimeout(() => {}, 1);
} else if (role === "no-listener") {
  // NEGATIVE CONTROL: no stdin listener. The parent never writes and never
  // closes the pipe, so this must exit on its own.
  console.log("no-listener done: true");
} else if (role === "paused") {
  // NEGATIVE CONTROL: a listener that releases stdin again. `pause()` unrefs
  // the handle in Node, so the process exits even with the pipe held open.
  const stream: any = process.stdin;
  stream.on("data", () => {});
  setTimeout(() => {
    stream.pause();
    console.log("paused done: true");
  }, 30);
} else {
  const childArgs = [...process.execArgv, ...process.argv.slice(1)];
  // `feed` says what the parent does with the child's stdin:
  //   "delayed" — write the payload after WRITE_DELAY_MS, then close
  //   "close"   — close immediately with nothing written
  //   "hold"    — never write, never close (the negative controls)
  const runRole = (name: string, feed: "delayed" | "close" | "hold") =>
    new Promise<void>((resolve) => {
      const child = spawn(process.execPath, childArgs, {
        env: { ...process.env, [ROLE_ENV]: name },
        stdio: ["pipe", "inherit", "inherit"],
      });
      let settled = false;
      const watchdog = setTimeout(() => {
        if (settled) return;
        settled = true;
        console.log(name + " exit: WATCHDOG");
        child.kill("SIGKILL");
        resolve();
      }, WATCHDOG_MS);
      child.on("exit", (code) => {
        if (settled) return;
        settled = true;
        clearTimeout(watchdog);
        console.log(name + " exit:", code);
        resolve();
      });
      if (feed === "delayed") {
        setTimeout(() => {
          try {
            child.stdin!.write(PAYLOAD);
            child.stdin!.end();
          } catch {
            /* child already gone */
          }
        }, WRITE_DELAY_MS);
      } else if (feed === "close") {
        child.stdin!.end();
      }
    });

  (async () => {
    await runRole("aliased-data", "delayed");
    await runRole("param-data", "delayed");
    await runRole("end-only", "close");
    await runRole("in-timeout", "delayed");
    await runRole("with-timer", "delayed");
    await runRole("no-listener", "hold");
    await runRole("paused", "hold");
    console.log("done");
  })();
}
