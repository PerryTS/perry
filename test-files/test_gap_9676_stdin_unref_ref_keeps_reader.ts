// #9676: `process.stdin.unref()` must not kill stdin delivery, and `.ref()`
// must undo it.
//
// THE BUG. On the stdin *object* path (an alias/parameter/field — which is what
// ink and every TUI built on it use), perry wired `unref` to the same
// `process_stdin_detach_stub` as `pause`/`destroy`: it set a process-global
// `STDIN_DETACHED` latch, and the fd-0 reader thread breaks its loop on that
// latch and EXITS. `ref` was wired to a no-op stub, so nothing ever cleared the
// latch or restarted the reader. ONE `unref()`/`ref()` pair therefore left the
// process with no reader on fd 0 for the rest of its life — the loop kept
// ticking, the terminal stayed in raw mode, and not one further keystroke
// reached JS. That is the "TUI input dies after a minute of real use" symptom:
// ink performs exactly that pair every time its raw-mode refcount drops to zero
// and comes back, i.e. whenever the last `useInput` component unmounts and a
// new one mounts — which is what a tool call does.
//
// Node's contract, which the roles below pin: `ref`/`unref` govern ONLY whether
// the handle keeps the event loop alive. An unref'd stdin still emits `'data'`.
//
// LOAD-BEARING CONSTRUCTION:
//
//  * Each role acts on the FIRST chunk and asserts on a SECOND chunk written
//    afterwards. A role that only ever saw one chunk cannot pass by accident,
//    and the toggle happens strictly between the two.
//  * Every role prints a `phase1` line before it toggles, so "the listener was
//    never registered" and "the listener died at the toggle" are different
//    outputs rather than the same silence.
//  * The `unref-*` roles hold the loop open with a short interval — that is the
//    POINT of `unref` (it drops stdin's own hold) and without it the process is
//    allowed to exit, which would make the test measure loop liveness instead
//    of byte delivery.
//  * `churn` runs across the toggle in one role so a future regression that
//    reintroduces the defect through a collected/relocated listener is caught
//    by the same fixture.
//  * `pause-resume` is a CONTROL: it must keep working, and it is the one
//    lifecycle pair that legitimately DOES stop the reader.
import { spawn } from "node:child_process";

const ROLE_ENV = "PERRY_9676_ROLE";
const WATCHDOG_MS = 20000;
const role = process.env[ROLE_ENV] ?? "";

function finish(line: string): void {
  console.log(line);
  process.exit(0);
}

// Escaping allocation: cells survive into the old generation and are dropped a
// few blocks later, so this forces real collections rather than a nursery flip.
function churn(rounds: number): number {
  let sink = 0;
  let keep: any[] = [];
  const held: any[] = [];
  for (let i = 0; i < rounds; i++) {
    const cell = { a: i, b: i + 1, c: "s" + (i & 1023), d: [i, i + 1] };
    keep.push(cell);
    if (keep.length >= 1024) {
      sink += keep[0].b;
      if ((i & 15) === 0) held.push(keep);
      if (held.length > 24) held.shift();
      keep = [];
    }
  }
  return sink + held.length;
}

function runRole(name: string, onFirst: (s: any) => void, doChurn: boolean): void {
  const s: any = process.stdin;
  // `unref()` releases stdin's hold on the loop by design, so hold it here.
  const ticker = setInterval(() => {}, 20);
  let phase = 0;
  s.on("data", (chunk: any) => {
    const text = String(chunk);
    if (phase === 0 && text.indexOf("ONE") >= 0) {
      phase = 1;
      console.log(name + " phase1: true");
      onFirst(s);
      if (doChurn) console.log(name + " churn: " + (churn(300000) > 0));
    } else if (phase === 1 && text.indexOf("TWO") >= 0) {
      clearInterval(ticker);
      finish(name + " phase2: true");
    }
  });
}

if (role === "unref-ref") {
  runRole("unref-ref", (s) => {
    s.unref();
    s.ref();
  }, false);
} else if (role === "unref-ref-churn") {
  runRole("unref-ref-churn", (s) => {
    s.unref();
    s.ref();
  }, true);
} else if (role === "unref-only") {
  // Node: an unref'd stdin still delivers. Only the loop hold is dropped.
  runRole("unref-only", (s) => {
    s.unref();
  }, false);
} else if (role === "pause-resume") {
  runRole("pause-resume", (s) => {
    s.pause();
    s.resume();
  }, false);
} else {
  const roles = ["unref-ref", "unref-ref-churn", "unref-only", "pause-resume"];
  const childArgs = [...process.execArgv, ...process.argv.slice(1)];
  const run = (name: string) =>
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
      setTimeout(() => {
        try {
          child.stdin!.write("ONE\n");
        } catch {
          /* child already gone */
        }
      }, 120);
      // Late enough that the churn role has finished collecting first.
      setTimeout(() => {
        try {
          child.stdin!.write("TWO\n");
        } catch {
          /* child already gone */
        }
      }, 2500);
    });

  (async () => {
    for (const r of roles) await run(r);
    console.log("done");
  })();
}
