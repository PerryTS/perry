import { mock } from "node:test";

const target: { value: number | undefined } = { value: 5 };
const property = mock.property(target, "value", undefined);
console.log("mocked undefined:", target.value === undefined, property.mock.accessCount());
property.mock.restore();
console.log("restored value:", target.value);
