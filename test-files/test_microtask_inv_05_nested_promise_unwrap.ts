// `await Promise.resolve(innerPromise)` should unwrap to innerPromise's
// resolved value, taking TWO microtask hops (one for the outer, one for the
// inner unwrap). A sibling .then queued after `f()` should land BETWEEN
// the two hops, not before or after both.
// Canary for: inlining `await` would skip one of the hops and reorder
// "sibling" relative to "got".
async function f() {
    const inner = Promise.resolve(42);
    const v = await Promise.resolve(inner);
    console.log("got " + v);
}
f();
Promise.resolve().then(() => console.log("sibling"));
console.log("sync");
