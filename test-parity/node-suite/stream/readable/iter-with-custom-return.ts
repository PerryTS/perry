import { Readable } from "node:stream";
// When for-await breaks early, the iterator's return() is invoked.
let returnCalled = false;
const customIter = {
  [Symbol.asyncIterator]() {
    let i = 0;
    return {
      next: async () => ({ value: i++, done: i > 5 }),
      return: async () => {
        returnCalled = true;
        return { value: undefined, done: true };
      },
    };
  },
};
const r = Readable.from(customIter as any);
let count = 0;
for await (const _v of r) {
  count++;
  if (count === 2) break;
}
setImmediate(() => {
  console.log("count:", count);
  console.log("return called:", returnCalled);
});
