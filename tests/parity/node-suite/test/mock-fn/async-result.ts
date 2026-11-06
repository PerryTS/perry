import { mock } from "node:test";

const fn = mock.fn(async (value: number) => value * 2);
const promise = fn(4);
console.log("record promise:", fn.mock.calls[0].result === promise, fn.mock.callCount());
console.log("resolved:", await promise);
mock.restoreAll();
