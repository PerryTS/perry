const { readFileSync, writeFileSync } = require("node:fs");
const { join } = require("node:path");
const { transformSync } = require("@babel/core");
const preset = require("babel-preset-solid");

const input = join(__dirname, "main.tsx");
const output = transformSync(readFileSync(input, "utf8"), {
  filename: input,
  configFile: false,
  babelrc: false,
  parserOpts: { plugins: ["typescript", "jsx"] },
  presets: [[preset, { generate: "universal", moduleName: "./host.ts", builtIns: [] }]],
});
writeFileSync(join(__dirname, "generated.ts"), output.code + "\n");
