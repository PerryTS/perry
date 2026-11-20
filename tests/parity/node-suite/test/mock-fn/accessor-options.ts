import { mock } from "node:test";

let stored = "initial";
const target = {
  get value() {
    return stored;
  },
  set value(value: string) {
    stored = value;
  },
};

const getter = mock.getter(target, "value", {});
const setter = mock.setter(target, "value", {});
console.log("getter options:", target.value, getter.mock.callCount());
target.value = "changed";
console.log("setter options:", stored, setter.mock.callCount());
mock.restoreAll();
