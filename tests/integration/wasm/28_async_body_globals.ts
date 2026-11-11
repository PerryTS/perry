// An async function body is emitted as JavaScript. A builtin it reads
// (`Promise` below) lowers to the unresolved-global sentinel, which the JS
// emitter must print as globalThis rather than an undeclared `__gN`;
// otherwise the body throws ReferenceError on entry and nothing after the
// first `await` runs. A module-level `let` it reads and writes lives in a
// WASM global, which the JS body reaches through the module's exports.
let counter = 1;

async function run(): Promise<void> {
  const v = await Promise.resolve(41);
  counter = counter + 1;
  console.log("awaited " + (v + 1) + ", counter " + counter);
}

run();
// WASM code sees the value the async (JS) body wrote.
setTimeout(() => {
  console.log("later, counter " + counter);
}, 20);
console.log("sync done");
