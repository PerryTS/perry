import { Readable } from "node:stream";
// some(asyncFn) — if asyncFn rejects, the promise rejects.
const r = Readable.from([1, 2, 3]);
let errMsg: string | null = null;
try {
  await (r as any).some(async (_x: number) => {
    throw new Error("some-fail");
  });
} catch (e: any) {
  errMsg = e && e.message;
}
console.log("rejected with:", errMsg);
