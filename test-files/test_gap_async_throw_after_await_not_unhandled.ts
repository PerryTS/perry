// Regression: an async function that `throw`s AFTER an `await`, whose rejection
// is CAUGHT by the caller's `try/catch`, must NOT fire `unhandledRejection`.
//
// The async-to-generator transform lowers `await x; throw e` so the throwing
// step returns `Promise.reject(e)` (a wrapper), which is assimilated into the
// activation's result promise via the deferred native-adoption job
// (V8-hop-parity path). That job forgot to mark the wrapper's rejection as
// handled — unlike the synchronous `js_promise_resolve_with_promise` adoption,
// which does — so the already-rejected wrapper, having no reaction of its own,
// was spuriously reported "Uncaught (in promise)" EVEN THOUGH the caller caught
// it. Fired once per caught throw-after-await, it floods a server's
// `process.on('unhandledRejection')` (Next.js's RSC renders catch such
// rejections internally) and can crash it.
//
// The observable is byte-for-byte identical to `node --experimental-strip-types`:
// the caught throws produce NO `unhandledRejection`, while a genuinely-unawaited
// rejection still DOES — so the fix suppresses only the spurious reports.

const events: string[] = [];
process.on("unhandledRejection", (reason: any) => {
  events.push(`unhandled:${reason && reason.message}`);
});

async function throwsAfterAwait(tag: string): Promise<void> {
  await Promise.resolve();
  throw new Error(tag);
}

async function caughtInLoop(): Promise<void> {
  for (let i = 0; i < 5; i++) {
    try {
      await throwsAfterAwait(`caught-${i}`);
    } catch {
      // swallowed — must NOT surface as an unhandled rejection
    }
  }
  console.log("caught all 5");
}

caughtInLoop().then(() => {
  // A genuinely-unhandled rejection (not awaited, not caught) MUST still fire.
  void throwsAfterAwait("really-unhandled");
  // Give the rejection checkpoint a turn, then report what fired.
  setTimeout(() => {
    console.log("events:", JSON.stringify(events));
  }, 20);
});
