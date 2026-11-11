// #6077 parts 1+2: `process.on('unhandledRejection')` fires at the end of the
// microtask checkpoint in which the promise rejected (NOT at process exit), it
// suppresses the default crash, and a handler attached later — from a timer —
// is too late to suppress it and fires `'rejectionHandled'` instead.

process.on("unhandledRejection", (reason: any, promise: any) => {
  console.log(
    "unhandledRejection:",
    reason instanceof Error ? reason.message : String(reason),
  );
  console.log("  same promise object:", promise === late);
});

process.on("rejectionHandled", (promise: any) => {
  console.log("rejectionHandled:", promise === late);
});

// Rejected with no handler; the `.catch` below runs in a *later* macrotask, so
// Node reports it as unhandled first and then fires `rejectionHandled`.
const late = Promise.reject(new Error("late"));
setTimeout(() => {
  late.catch((e: any) => console.log("caught late:", e.message));
}, 10);

// Rejected but caught synchronously — never reported.
const caught = Promise.reject(new Error("caught sync"));
caught.catch((e: any) => console.log("sync catch:", e.message));

// The rejection report happens before the macrotask queues get a turn: this
// `setTimeout(0)`, scheduled BEFORE the rejection above, still runs after the
// `unhandledRejection` handler.
setTimeout(() => console.log("timer0"), 0);

console.log("sync end");
