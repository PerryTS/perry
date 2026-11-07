// A builtin read as a value from ordinary (non-async) code lowers to
// `PropertyGet { GlobalGet(0), "Promise" }` on the unresolved-global sentinel.
// That member is a property of globalThis; reading it off the sentinel, which
// lowers to undefined, made every such value undefined.
const P = Promise;
console.log(typeof P);
console.log(P === globalThis.Promise);
