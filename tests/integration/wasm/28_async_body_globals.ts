// An async function body is emitted as JavaScript. A builtin it reads
// (`Promise` below) lowers to the unresolved-global sentinel, which the JS
// emitter must print as globalThis rather than an undeclared `__gN`;
// otherwise the body throws ReferenceError on entry and nothing after the
// first `await` runs.
async function run(): Promise<void> {
  const v = await Promise.resolve(41);
  console.log("awaited " + (v + 1));
}

run();
console.log("sync done");
