import { mock } from "node:test";

let stored = 1;
const target = {
  get value() {
    return stored;
  },
  set value(value: number) {
    stored = value;
  },
};

const getter = mock.getter(target, "value", () => 10);
target.value = 4;
console.log("getter preserves setter:", target.value, stored);
getter.mock.restore();

const setter = mock.setter(target, "value", (value: number) => {
  stored = value * 2;
});
console.log("setter preserves getter:", target.value);
target.value = 5;
console.log("setter result:", stored, target.value);
setter.mock.restore();
