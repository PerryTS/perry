import { mock } from "node:test";

const target = { value: 1 };
const property = mock.property(target, "value", 2);
console.log("get:", target.value);
target.value = 3;
console.log("set-get:", target.value);
console.log(
  "accesses:",
  property.mock.accessCount(),
  property.mock.accesses.map((access: any) => `${access.type}:${access.value}`).join(","),
);
property.mock.resetAccesses();
console.log("reset accesses:", property.mock.accessCount(), target.value, property.mock.accessCount());
property.mock.restore();
