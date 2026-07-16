import { mock } from "node:test";

const target = { value: 5 };
const property = mock.property(target, "value");
console.log("initial spy:", target.value, property.mock.accessCount());
target.value = 6;
console.log("updated spy:", target.value, property.mock.accessCount());
property.mock.restore();
console.log("restored:", target.value);
