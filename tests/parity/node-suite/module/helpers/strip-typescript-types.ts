process.emitWarning = () => false;

import * as Module from "node:module";

function errorLine(label, fn) {
  try {
    fn();
    console.log(`${label}: no throw`);
  } catch (error) {
    console.log(`${label}:`, error.name, error.code, error.message);
  }
}

console.log(
  "simple:",
  JSON.stringify(Module.stripTypeScriptTypes("const value: number = 1;")),
);
const capturedStripTypeScriptTypes = Module.stripTypeScriptTypes;
console.log(
  "captured simple:",
  JSON.stringify(capturedStripTypeScriptTypes("let captured: string = 'ok';")),
);
console.log(
  "interface:",
  JSON.stringify(
    Module.stripTypeScriptTypes(
      "interface User { name: string }\nconst x: User = { name: 'a' };",
    ),
  ),
);
errorLine("transform enum", () =>
  Module.stripTypeScriptTypes("enum Color { Red = 1 }\nconsole.log(Color.Red);", {
    mode: "transform",
  }),
);
console.log(
  "sourceMap false:",
  JSON.stringify(
    Module.stripTypeScriptTypes("const mapped: number = 1;", {
      sourceMap: false,
    }),
  ),
);
errorLine("strip enum", () => Module.stripTypeScriptTypes("enum Color { Red = 1 }"));
errorLine("bad code", () => Module.stripTypeScriptTypes(1));
errorLine("bad mode", () =>
  Module.stripTypeScriptTypes("const x = 1;", { mode: "bad" }),
);
errorLine("sourceMap true", () =>
  Module.stripTypeScriptTypes("const x = 1;", {
    sourceMap: true,
  }),
);
errorLine("sourceMap string", () =>
  Module.stripTypeScriptTypes("const x = 1;", {
    sourceMap: "yes",
  }),
);
