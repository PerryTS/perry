// Regression: awaiting an already-rejected promise inside try/catch (or
// returning a rejected promise from an async fn) caught the rejection but
// STILL reported a spurious "Uncaught (in promise)" and exited non-zero.
//
// Root cause: `js_async_step_chain`'s fast path for an already-rejected
// awaited promise read the reason field directly and queued the async step
// without calling `mark_rejection_handled`, so the program-end unhandled-
// rejection detector still saw the (actually-handled) rejection. The Pending
// arm already marks via `js_promise_then`; the fast path now does too.
//
// Expected (matches Node): only the three "caught:" lines, then "done",
// with a clean exit — no "Uncaught (in promise)".

async function returnsRejected() {
  return Promise.reject(new Error("via-return"));
}

async function main() {
  // 1. await a directly-rejected promise
  try {
    await Promise.reject(new Error("direct"));
  } catch (e: any) {
    console.log("caught:", e.message);
  }
  // 2. await an async fn that *returns* a rejected promise (adoption)
  try {
    await returnsRejected();
  } catch (e: any) {
    console.log("caught:", e.message);
  }
  // 3. await an already-settled rejected promise bound earlier
  const r = Promise.reject(new Error("presettled"));
  try {
    await r;
  } catch (e: any) {
    console.log("caught:", e.message);
  }
  console.log("done");
}

main();
