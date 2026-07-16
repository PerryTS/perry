import { WASI } from "node:wasi";

const W: any = WASI;

for (
  const [label, value] of [
    ["undefined", undefined],
    ["true", true],
    ["false", false],
    ["zero", 0],
    ["string", "true"],
    ["null", null],
  ] as const
) {
  try {
    new W({ version: "preview1", returnOnExit: value as any });
    console.log(label + ": ok");
  } catch (error: any) {
    console.log(label + ": throw", error?.name, error?.code || "no-code");
  }
}
