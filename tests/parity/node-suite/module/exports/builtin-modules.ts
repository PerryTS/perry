import { builtinModules, isBuiltin } from "node:module";

const sorted = [...builtinModules].sort();
console.log("array:", Array.isArray(builtinModules));
console.log("sorted unique:", sorted.length === new Set(sorted).size);
console.log(
  "already sorted:",
  JSON.stringify(sorted) === JSON.stringify(builtinModules),
);
console.log(
  "contains core:",
  ["assert", "fs", "fs/promises", "module", "sys"].every((name) =>
    builtinModules.includes(name)
  ),
);
console.log(
  "prefixed entries:",
  JSON.stringify(builtinModules.filter((name) => name.startsWith("node:"))),
);
console.log(
  "excludes internals:",
  builtinModules.every((name) => !name.startsWith("internal/")),
);
console.log(
  "prefix-only builtins:",
  isBuiltin("node:test"),
  isBuiltin("test"),
  isBuiltin("node:sea"),
  isBuiltin("sea"),
);
console.log(
  "inventory agrees:",
  builtinModules.every((name) => isBuiltin(name)),
);
console.log(
  "descriptor:",
  JSON.stringify(Object.getOwnPropertyDescriptor(builtinModules, "length")),
);
