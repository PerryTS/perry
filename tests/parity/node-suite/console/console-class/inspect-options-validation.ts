import { Console } from "node:console";
import { Writable } from "node:stream";

const sink = new Writable({
  write(_chunk: any, _enc: string, cb: (err?: Error | null) => void) {
    cb();
  },
});

for (const [label, inspectOptions] of [
  ["missing", undefined],
  ["valid object", {}],
  ["null", null],
  ["number", 1],
  ["array", []],
] as const) {
  try {
    new Console({ stdout: sink, inspectOptions });
    console.log(label, "OK");
  } catch (err: any) {
    console.log(label, "THROW", err?.name, err?.code || "no-code", err?.message);
  }
}
