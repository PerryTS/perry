// #6077 part 2: with no `unhandledRejection` listener, Node raises the
// rejection as an uncaught exception — so an `uncaughtException` listener still
// observes it (with `origin === 'unhandledRejection'`) and still suppresses the
// crash. Covers the three ways a rejection reaches the tracker: a bare
// `Promise.reject`, an `async` function that throws, and a `Promise.all` whose
// member rejects.

process.on("uncaughtException", (err: any, origin: any) => {
  console.log("uncaughtException:", err.message, "| origin:", origin);
});

async function boom(): Promise<number> {
  throw new Error("async boom");
}

boom();

Promise.all([Promise.resolve(1), Promise.reject(new Error("all boom"))]);

// Handled — must NOT reach the uncaughtException listener.
Promise.all([Promise.resolve(1), Promise.reject(new Error("all caught"))]).catch(
  (e: any) => console.log("all caught:", e.message),
);

setTimeout(() => console.log("still alive"), 5);

console.log("sync end");
