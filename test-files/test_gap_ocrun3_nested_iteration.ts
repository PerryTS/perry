// Forwarding the array iterator selects its protocol arm without changing output.
const originalIterator = Array.prototype[Symbol.iterator];
Array.prototype[Symbol.iterator] = function () {
  return originalIterator.call(this);
};
async function visit(items: string[]) {
  await Promise.resolve();
  const values = new Set(["x", "y"]);
  for (const item of items) {
    for (const value of values) console.log(item, value);
  }
  console.log("closed");
}
await visit(["a", "b"]);
