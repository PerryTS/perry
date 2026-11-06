import { Readable } from "node:stream";
// Plain object with [Symbol.iterator] is iterable; Readable.from should
// iterate it.
const obj = {
  [Symbol.iterator]() {
    let i = 0;
    return {
      next() {
        if (i < 3) return { value: i++, done: false };
        return { value: undefined, done: true };
      },
    };
  },
};
const r = Readable.from(obj as any);
const out: number[] = [];
for await (const v of r) out.push(v as number);
console.log("from iter-object:", out.join(","));
