// `.then` and `await` resumptions both enqueue onto the same microtask queue.
// Resumption order should be FIFO of when each was enqueued.
// Canary for: structural change inlining `await` would cause B to run before
// then1/then2.
async function f() {
    await Promise.resolve();
    console.log("B");
}
Promise.resolve().then(() => console.log("then1"));
f();
Promise.resolve().then(() => console.log("then2"));
console.log("sync");
