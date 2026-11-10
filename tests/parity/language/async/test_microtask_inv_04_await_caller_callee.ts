// Async caller awaits another async fn that itself awaits. Tests that the
// caller's resumption sits behind any sibling microtasks queued before it.
// Spec ordering: caller is suspended at `await inner()`, inner runs, inner's
// own await yields, then THE CALLER does NOT get to resume yet — its
// resumption is enqueued AFTER any sibling .then that ran during inner.
// Canary for: structural change that returns directly from inner to caller
// when inner's body completes synchronously after one await.
async function inner() {
    console.log("i1");
    await Promise.resolve();
    console.log("i2");
}
async function outer() {
    console.log("o1");
    await inner();
    console.log("o2");
}
outer();
Promise.resolve().then(() => console.log("sibling"));
console.log("sync");
