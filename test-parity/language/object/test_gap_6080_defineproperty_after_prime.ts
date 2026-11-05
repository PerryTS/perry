// #6080: the read inline-cache must not bypass a descriptor installed AFTER
// the call-site was primed. Priming f(a) caches a raw slot for `x`; a later
// Object.defineProperty converting `x` to a getter (or redefining the data
// value) must be honored on the next read, not served stale from the cache.
function f(o: any) { return o.x; }
const a: any = { x: 1 };
console.log(f(a)); // prime -> 1
Object.defineProperty(a, "x", { get() { return 42; } });
console.log(f(a)); // must observe the getter -> 42

function g(o: any) { return o.y; }
const b: any = { y: 10 };
g(b); // prime
Object.defineProperty(b, "y", { value: 99, writable: false });
console.log(g(b)); // must observe redefined value -> 99

// A receiver that gets a descriptor must still read its OTHER, untouched
// own keys correctly (slow path), and normal objects keep hitting the cache.
const d: any = { p: 1, q: 2 };
Object.defineProperty(d, "p", { get() { return 100; } });
console.log(d.q, d.p); // 2 100
