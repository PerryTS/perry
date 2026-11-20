// Issue #928: `throw new Error(...)` inside an async ARROW handler must
// flow through the Promise rejection path AND JSON.stringify of a
// built-in Error must not segfault (the Fastify rejection-body renderer
// invokes JSON.stringify on the rejection reason; before this fix it
// dereferenced ErrorHeader as a JSObject and crashed the process).

// 1. async-arrow throw flows through rejected Promise (was already
// working in the simple case — kept here as a guard against regressions).
const handler = async () => {
  throw new Error("test");
};
try {
  await handler();
  console.log("no-throw");
} catch (e: any) {
  console.log("caught:", e.message);
}

// 2. JSON.stringify of a built-in Error: Node returns "{}" because
// Error's intrinsic props are non-enumerable. Before #928 this
// segfaulted the process.
const err = new Error("boom");
console.log("json:", JSON.stringify(err));

// 3. Built-in Error subclasses use the same ErrorHeader layout.
const te = new TypeError("nope");
console.log("type:", JSON.stringify(te));
