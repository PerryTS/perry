// A missing argument to a JS global is NOT an error — the parameter is simply
// `undefined`, and each of these has well-defined behavior for it. Perry used to
// reject the zero-arg call at COMPILE time ("isNaN requires one argument"), which
// meant it refused to compile legal JS.
//
// `atob()` / `btoa()` / `structuredClone()` are deliberately NOT covered here:
// those are WebIDL required-argument throws where `f()` and `f(undefined)` really
// do differ, so they need arity plumbing rather than undefined-padding.
//
// Validated byte-for-byte against `node --experimental-strip-types`.

console.log("isNaN():", isNaN());
console.log("isFinite():", isFinite());
console.log("encodeURI():", encodeURI());
console.log("decodeURI():", decodeURI());
console.log("encodeURIComponent():", encodeURIComponent());
console.log("decodeURIComponent():", decodeURIComponent());

// explicit undefined behaves the same
console.log("isNaN(undefined):", isNaN(undefined));
console.log("encodeURI(undefined):", encodeURI(undefined as unknown as string));

// the normal one-argument forms keep working
console.log("isNaN('x'):", isNaN("x" as unknown as number));
console.log("isFinite(1):", isFinite(1));
console.log("encodeURI('a b'):", encodeURI("a b"));
console.log("decodeURI('a%20b'):", decodeURI("a%20b"));
console.log("encodeURIComponent('a&b'):", encodeURIComponent("a&b"));
console.log("decodeURIComponent('a%26b'):", decodeURIComponent("a%26b"));
