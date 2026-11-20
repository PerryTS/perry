import { mock } from "node:test";

const target = { value: 1 };
const property = mock.property(target, "value", 2);
console.log("initial:", target.value, typeof property.mock);
property.mock.mockImplementation(3);
console.log("replacement:", target.value);
property.mock.mockImplementationOnce(4);
console.log("once:", target.value, target.value);
property.mock.restore();
console.log("restored:", target.value);
