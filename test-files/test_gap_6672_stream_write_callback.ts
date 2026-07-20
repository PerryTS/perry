// #6672: `process.stdout.write(chunk[, encoding][, callback])` (and stderr) must
// invoke the optional completion callback. Perry's write stubs took a single
// argument and dropped the callback entirely, so
//   await new Promise((r) => process.stdout.write(x, r))
// never resolved — the promise hung, its awaiter never resumed, and the event
// loop drained and exited with the continuation (and any `process.exitCode` it
// would set) left unrun. That produced the pi print-mode exit-code divergence
// (natural exit stayed 0 where Node exits 1), because pi's stdout flush awaits
// exactly this callback.
//
// The bug is invisible to a plain `write(x)` — only the *callback* was dropped —
// so every scenario awaits the callback and then logs a marker from the
// continuation. Under the bug the continuation never runs, so the markers (and
// the final line) are missing and the output diverges from Node. The written
// chunks are empty so the transcript is exactly the deterministic markers
// (no stdout/stderr interleaving to reason about).

const log = (s: string) => console.log(s);

// resolve only when the write's completion callback fires (chunk empty)
const awaitStdoutCb = () =>
  new Promise<void>((resolve) => {
    process.stdout.write("", () => resolve());
  });
const awaitStderrCb = () =>
  new Promise<void>((resolve) => {
    process.stderr.write("", () => resolve());
  });
// the 3-arg overload: write(chunk, encoding, callback)
const awaitStdoutEncCb = () =>
  new Promise<void>((resolve) => {
    process.stdout.write("", "utf8", () => resolve());
  });

// The pi shape: return a value from a `try` whose `finally` awaits a stdout
// write; the caller must still receive the returned value.
async function returnThroughAwaitingFinally(): Promise<number> {
  try {
    return 42;
  } finally {
    await awaitStdoutCb();
  }
}

async function main() {
  // A — continuation after an awaited stdout.write runs
  await awaitStdoutCb();
  log("A: stdout callback fired");

  // B — same for stderr
  await awaitStderrCb();
  log("B: stderr callback fired");

  // C — the write(chunk, encoding, callback) 3-arg overload
  await awaitStdoutEncCb();
  log("C: 3-arg callback fired");

  // D — return through an awaiting finally
  const d = await returnThroughAwaitingFinally();
  log("D: returned " + d);

  // E — the completion callback is async, never called synchronously
  let order = "";
  await new Promise<void>((resolve) => {
    process.stdout.write("", () => {
      order += "cb";
      resolve();
    });
    order += "sync";
  });
  log("E: order=" + order); // must be "synccb", not "cbsync"

  // F — a plain write with no callback still returns and does not hang
  const ret = process.stdout.write("");
  log("F: plain write returned " + ret);

  log("test complete");
}

main();
