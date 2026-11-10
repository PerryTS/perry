import { mock } from "node:test";

const target = { value: 1 };
const property = mock.property(target, "value", 2);
property.mock.mockImplementation(9);
property.mock.mockImplementationOnce(4, 1);
property.mock.mockImplementationOnce(6, 3);
console.log("access sequence:", target.value, target.value, target.value, target.value);
console.log("access count:", property.mock.accessCount());
property.mock.restore();
