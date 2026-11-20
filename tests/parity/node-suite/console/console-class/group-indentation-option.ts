import { Console } from "node:console";
import { Writable } from "node:stream";

const sink = new Writable({
  write(_chunk: any, _enc: string, cb: (err?: Error | null) => void) {
    cb();
  },
});

for (const [label, groupIndentation] of [
  ["missing", undefined],
  ["zero", 0],
  ["two", 2],
  ["max", 1000],
  ["negative", -1],
  ["fractional", 1.5],
  ["too large", 1001],
  ["string", "2"],
] as const) {
  try {
    new Console({ stdout: sink, groupIndentation });
    console.log(label, "OK");
  } catch (err: any) {
    console.log(label, "THROW", err?.name, err?.code || "no-code", err?.message);
  }
}
