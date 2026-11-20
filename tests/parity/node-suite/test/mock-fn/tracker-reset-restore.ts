import { mock } from "node:test";

const fn = mock.fn(
  () => "original",
  () => "replacement",
);

console.log("initial:", fn());
mock.restoreAll();
console.log("after restoreAll:", fn());
fn.mock.mockImplementation(() => "again");
console.log("replaced again:", fn());
mock.reset();
console.log("after reset:", fn(), fn.mock.callCount());
fn.mock.mockImplementation(() => "post-reset");
mock.restoreAll();
console.log("disassociated:", fn());
mock.reset();
