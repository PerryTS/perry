import promisesDefault, * as promisesNamespace from "node:inspector/promises";
import callbackDefault, * as callbackNamespace from "node:inspector";

console.log("default:", promisesNamespace.default === promisesDefault);
console.log(
  "keys:",
  Reflect.ownKeys(promisesDefault).join(",") ===
    Reflect.ownKeys(callbackDefault).join(","),
  Reflect.ownKeys(promisesDefault).join(","),
);
for (const key of Reflect.ownKeys(callbackDefault)) {
  console.log(String(key), promisesDefault[key] === callbackDefault[key]);
}
console.log(
  "namespace sessions:",
  promisesNamespace.Session === promisesDefault.Session,
  callbackNamespace.Session === callbackDefault.Session,
);
console.log(
  "descriptors:",
  Reflect.ownKeys(promisesDefault).every((key) => {
    const descriptor = Object.getOwnPropertyDescriptor(promisesDefault, key);
    return descriptor?.enumerable && descriptor.writable &&
      descriptor.configurable;
  }),
);
