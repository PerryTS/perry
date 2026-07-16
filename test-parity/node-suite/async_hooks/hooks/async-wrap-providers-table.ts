import { asyncWrapProviders } from "node:async_hooks";

const keys = Object.keys(asyncWrapProviders);
const values = keys.map((key) => (asyncWrapProviders as any)[key]);
const first = keys[0];
const original = (asyncWrapProviders as any)[first];
let mutation = "ok";
try {
  (asyncWrapProviders as any)[first] = original + 1;
} catch (error: any) {
  mutation = `${error.name}:${error.code || "no-code"}`;
}
console.log(
  "provider table shape:",
  typeof asyncWrapProviders,
  Object.getPrototypeOf(asyncWrapProviders) === null,
  Object.isFrozen(asyncWrapProviders),
);
console.log(
  "provider table values:",
  keys.length > 0,
  values.every((value) => Number.isInteger(value) && value >= 0),
  new Set(values).size === values.length,
  asyncWrapProviders.NONE === 0,
);
console.log(
  "provider table mutation:",
  mutation,
  (asyncWrapProviders as any)[first] === original,
);
