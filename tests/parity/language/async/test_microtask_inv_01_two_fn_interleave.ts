// Two async functions started "concurrently". Each `await Promise.resolve()`
// must yield to the microtask queue, so resumptions interleave in FIFO order.
// Canary for: structural change that bypasses the queue for resolved inners
// would collapse this into a1/a2/a3/b1/b2/b3 instead of interleaving.
async function a() {
    console.log("a1");
    await Promise.resolve();
    console.log("a2");
    await Promise.resolve();
    console.log("a3");
}
async function b() {
    console.log("b1");
    await Promise.resolve();
    console.log("b2");
    await Promise.resolve();
    console.log("b3");
}
a();
b();
console.log("sync");
