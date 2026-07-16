import ModuleDefault from "node:module";

const source = "exports.answer = 42;\n";
const wrapped = ModuleDefault.wrap(source);
console.log(
  "wrapper array:",
  Array.isArray(ModuleDefault.wrapper),
  ModuleDefault.wrapper.length,
);
console.log("wrapper values:", JSON.stringify(ModuleDefault.wrapper));
console.log(
  "wrap exact:",
  wrapped === ModuleDefault.wrapper[0] + source + ModuleDefault.wrapper[1],
);
console.log("wrapped value:", JSON.stringify(wrapped));
console.log(
  "function shapes:",
  ModuleDefault.wrap.name,
  ModuleDefault.wrap.length,
);
