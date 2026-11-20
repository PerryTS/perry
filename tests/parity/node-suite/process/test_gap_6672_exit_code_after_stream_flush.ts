// #6672: a `process.exitCode` set by a continuation that runs only AFTER an
// awaited stdout flush must survive to natural exit. This is the pi print-mode
// shape reduced to its essence: `runPrintMode` returns a nonzero code from a
// `try` whose `finally` awaits `process.stdout.write`'s completion callback,
// and the caller assigns `process.exitCode` from the returned value.
//
// With the write callback dropped (pre-fix), the finally's await never
// resolved, so `runPrintMode`'s promise never settled, the caller's
// continuation never ran, `process.exitCode` stayed 0, and the process exited 0
// where Node exits 1 — the GATE 2a exit-code divergence.
//
// This test carries an expected-output file and expected-exit=1 so the harness
// asserts Perry both prints the continuation's line AND exits 1.
async function runPrintMode(): Promise<number> {
  let exitCode = 0;
  try {
    exitCode = 1; // stands in for: last assistant message stopReason === "error"
    return exitCode;
  } finally {
    await new Promise<void>((resolve) => {
      process.stdout.write("", () => resolve());
    });
  }
}

async function main() {
  const exitCode = await runPrintMode();
  console.log("continuation ran; exitCode=" + exitCode);
  if (exitCode !== 0) {
    process.exitCode = exitCode;
  }
}

main();
