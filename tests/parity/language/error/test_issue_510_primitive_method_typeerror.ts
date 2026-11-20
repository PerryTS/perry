// Issue #510: calling a method on a primitive whose name doesn't
// resolve on the auto-boxed prototype must throw `TypeError: <expr>
// is not a function`. Followup to #462 (which closed the
// undefined/null property-read case).
//
// Spec behavior (matches Node):
//   "hi".lengt()  → "TypeError: \"hi\".lengt is not a function"
//   (42).foo()    → "TypeError: 42.foo is not a function"
//   true.bar()    → "TypeError: true.bar is not a function"
//
// Property *reads* on primitives (no auto-boxed match) keep
// returning `undefined` — same as Node, which performs prototype
// lookup and yields undefined when the property doesn't exist.
//
// Perry has no primitive auto-boxing, so the runtime's
// `js_native_call_method` catch-all surfaces a node-shaped
// diagnostic when dispatch is exhausted on a primitive receiver.
// The wording carries the receiver KIND (string / number / boolean
// / bigint) since the original source text isn't reconstructible at
// dispatch time — close enough to Node's "x.foo" form for the
// debugging value (you see immediately that the receiver was a
// string, not a method-bearing object).

const s: any = "hello";

// Property *read* on a primitive resolves to undefined per Node's
// auto-boxed prototype lookup. Both Perry and Node print
// "undefined" here.
console.log("read missing on string:", s.lengt);

const n: any = 42;
console.log("read missing on number:", n.foo);

const b: any = true;
console.log("read missing on bool:", b.bar);

console.log("about to throw — last line of stdout");

// The next line throws. Execution stops here.
s.lengt();

console.log("UNREACHABLE — should never print");
