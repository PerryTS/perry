import { WASI } from "node:wasi";

const W: any = WASI;
for (
  const [label, value] of [
    ["undefined", undefined],
    ["null", null],
    ["number", 1],
    ["string", "instance"],
    ["array", []],
    ["plain object", {}],
    ["null exports", { exports: null }],
  ] as const
) {
  try {
    new W({ version: "preview1" }).initialize(value);
    console.log(label + ": ok");
  } catch (error: any) {
    console.log(label + ": throw", error?.name, error?.code || "no-code");
  }
}
