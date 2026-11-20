import { Readable } from "node:stream";
// every(asyncFn) — rejection in fn propagates.
const r = Readable.from([1, 2, 3]);
let errMsg: string | null = null;
try {
  await (r as any).every(async (_x: number) => {
    throw new Error("every-fail");
  });
} catch (e: any) {
  errMsg = e && e.message;
}
console.log("rejected with:", errMsg);
