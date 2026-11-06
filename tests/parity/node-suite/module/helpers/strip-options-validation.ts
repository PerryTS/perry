process.emitWarning = () => false;
import { stripTypeScriptTypes } from "node:module";

for (
  const [label, options] of [
    ["null", null],
    ["number", 1],
    ["bad mode", { mode: "erase" }],
    ["strip map", { mode: "strip", sourceMap: true }],
    ["map string", { mode: "transform", sourceMap: "yes" }],
    ["url number", { mode: "transform", sourceUrl: 1 }],
  ] as const
) {
  try {
    const result = stripTypeScriptTypes("const x: number = 1;", options as any);
    console.log(label, "ok", JSON.stringify(result));
  } catch (error) {
    console.log(label, (error as any).name, (error as any).code ?? "no-code");
  }
}
