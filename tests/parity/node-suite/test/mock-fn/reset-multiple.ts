import { mock } from "node:test";

const first = mock.fn(() => "first");
const second = mock.fn(() => "second");
first();
second();
second();
console.log("before reset:", first.mock.callCount(), second.mock.callCount());
mock.reset();
console.log("after reset:", first.mock.callCount(), second.mock.callCount());
console.log("after calls:", first(), second(), first.mock.callCount(), second.mock.callCount());
mock.restoreAll();
