import { WASI } from "node:wasi";
import { openSync, closeSync } from "node:fs";

const W: any = WASI;
// Descriptor 3 may belong to Node's event loop but be absent in Perry.
// Use an explicitly opened descriptor to compare the same valid input.
const positiveFd = openSync("test-parity/node-suite/wasi/constructor/stdio-validation.ts", "r");

for (const option of ["stdin", "stdout", "stderr"] as const) {
  for (
    const [label, value] of [
      ["undefined", undefined],
      ["zero", 0],
      ["positive", positiveFd],
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

closeSync(positiveFd);
