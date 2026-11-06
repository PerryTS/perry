import { mock } from "node:test";

const fn = mock.fn(function (this: any, a: number, b: number) {
  return this.offset + a + b;
});
const bound = fn.bind({ offset: 10 }, 2);
console.log("bound result:", bound(3));
console.log("bound count:", fn.mock.callCount(), JSON.stringify(fn.mock.calls[0].arguments));
console.log("bound this:", fn.mock.calls[0].this.offset);
mock.restoreAll();
