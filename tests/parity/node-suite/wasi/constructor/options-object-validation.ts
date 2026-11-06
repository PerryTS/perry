import { WASI } from "node:wasi";

const W: any = WASI;

for (
  const [label, value] of [
    ["null", null],
    ["string", "preview1"],
    ["number", 1],
    ["boolean", true],
    ["array", []],
    ["function", () => {}],
    ["symbol", Symbol("options")],
  ] as const
) {
  try {
    new W(value);
    console.log(label + ": accepted");
  } catch (error: any) {
    console.log(label + ":", error?.name, error?.code || "no-code");
  }
}
