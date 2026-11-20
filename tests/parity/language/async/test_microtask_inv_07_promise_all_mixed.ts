// Promise.all of mixed input types (already-resolved, async-fn-result,
// already-pending). Resumption of `await Promise.all(...)` must happen in a
// microtask boundary AFTER all elements settle. Result order must match
// input order regardless of settle order.
async function slow(x: number) {
    await Promise.resolve();
    await Promise.resolve();
    return x;
}
async function fast(x: number) {
    return x;
}
async function main() {
    const r = await Promise.all([slow(1), fast(2), Promise.resolve(3), slow(4)]);
    console.log("done: " + r.join(","));
}
main();
Promise.resolve().then(() => console.log("sibling"));
console.log("sync");
