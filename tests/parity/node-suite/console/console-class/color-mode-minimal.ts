import { Console } from "node:console";
import { Writable } from "node:stream";

const sink = new Writable({
  write(_chunk: any, _enc: string, cb: (err?: Error | null) => void) {
    cb();
  },
});

for (const [label, colorMode] of [
  ["missing", undefined],
  ["auto", "auto"],
  ["true", true],
  ["false", false],
  ["bad string", "bad"],
  ["number", 1],
  ["null", null],
] as const) {
  try {
    new Console({ stdout: sink, colorMode });
    console.log(label, "OK");
  } catch (err: any) {
    console.log(label, "THROW", err?.name, err?.code || "no-code", err?.message);
  }
}
