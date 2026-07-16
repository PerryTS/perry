import { WASI } from "node:wasi";

const W: any = WASI;

for (const option of ["stdin", "stdout", "stderr"] as const) {
  for (
    const [label, value] of [
      ["undefined", undefined],
      ["zero", 0],
      ["positive", 3],
      ["negative", -1],
      ["fraction", 1.5],
      ["nan", NaN],
      ["string", "1"],
      ["too large", 2 ** 31],
    ] as const
  ) {
    try {
      new W({ version: "preview1", [option]: value });
      console.log(option, label + ": ok");
    } catch (error: any) {
      console.log(
        option,
        label + ": throw",
        error?.name,
        error?.code || "no-code",
      );
    }
  }
}
