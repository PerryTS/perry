import { mock } from "node:test";

const fn = mock.fn(
  () => "original",
  () => "temporary",
  { times: 2 },
);
console.log("fn times:", fn(), fn(), fn(), fn.mock.callCount());

const target = {
  value: 5,
  read() {
    return this.value;
  },
};
mock.method(target, "read", () => 99, { times: 1 });
console.log("method times:", target.read(), target.read());
mock.restoreAll();
