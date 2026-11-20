import { writeFileSync as __macroWrite } from "node:fs";

function __macroValidate(value: unknown, ancestors: unknown[]): void {
  if (value === null || typeof value === "string" || typeof value === "boolean") return;
  if (typeof value === "number") {
    if (!Number.isFinite(value)) throw new Error("non-finite number is not a JSON macro value");
    if (value === 0 && 1 / value < 0) throw new Error("negative zero is not preserved by JSON serialization");
    return;
  }
  if (typeof value !== "object") throw new Error("non-JSON macro value: " + typeof value);
  if (ancestors.includes(value)) throw new Error("cyclic macro value");
  const array = Array.isArray(value);
  const prototype = Object.getPrototypeOf(value);
  if (!array && prototype !== null && prototype !== Object.prototype) {
    throw new Error("unsupported macro object prototype");
  }
  ancestors.push(value);
  if (array) {
    const list = value as unknown[];
    if (Reflect.ownKeys(list).length !== list.length + 1) throw new Error("non-JSON array properties or holes");
    for (let i = 0; i < list.length; i++) {
      const descriptor = Object.getOwnPropertyDescriptor(list, String(i));
      if (descriptor === undefined || !("value" in descriptor)) throw new Error("array hole or accessor in macro value");
      __macroValidate(descriptor.value, ancestors);
    }
  } else {
    for (const key of Reflect.ownKeys(value)) {
      if (typeof key !== "string") throw new Error("symbol key in macro value");
      const descriptor = Object.getOwnPropertyDescriptor(value, key);
      if (descriptor === undefined || !descriptor.enumerable || !("value" in descriptor)) {
        throw new Error("non-enumerable property or accessor in macro value");
      }
      __macroValidate(descriptor.value, ancestors);
    }
  }
  ancestors.pop();
}
