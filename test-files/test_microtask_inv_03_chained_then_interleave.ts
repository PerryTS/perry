// Two independent `.then` chains. Each `.then` callback enqueues the NEXT
// `.then` onto the microtask queue, so two parallel 3-step chains interleave:
// A1, B1, A2, B2, A3, B3 — NOT A1, A2, A3, B1, B2, B3.
// Canary for: any optimization that batches `.then` callbacks together.
Promise.resolve()
    .then(() => console.log("A1"))
    .then(() => console.log("A2"))
    .then(() => console.log("A3"));
Promise.resolve()
    .then(() => console.log("B1"))
    .then(() => console.log("B2"))
    .then(() => console.log("B3"));
console.log("sync");
