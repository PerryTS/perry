import * as moduleNS from "node:module";

for (
  const key of [
    "Module",
    "builtinModules",
    "constants",
    "createRequire",
    "default",
  ]
) {
  const descriptor = Object.getOwnPropertyDescriptor(moduleNS, key)!;
  console.log(
    key,
    JSON.stringify({
      enumerable: descriptor.enumerable,
      writable: descriptor.writable,
      configurable: descriptor.configurable,
      getter: typeof descriptor.get,
      setter: typeof descriptor.set,
    }),
  );
}

console.log(
  "namespace frozen/sealed:",
  Object.isFrozen(moduleNS),
  Object.isSealed(moduleNS),
);
console.log("symbol tag:", moduleNS[Symbol.toStringTag]);
