import { Buffer } from "node:buffer";

function showError(label: string, fn: () => void) {
  try {
    fn();
    console.log(label + ":", "no-error");
  } catch (err: any) {
    console.log(label + ":", err?.name || "unknown");
  }
}

const arrayTarget = new Uint8Array(6);
const arrayReturn = arrayTarget.set([1, 2, 300, -1], 1);
console.log("array source:", Array.from(arrayTarget).join(","), arrayReturn === undefined);

const typedTarget = new Uint8Array(6);
typedTarget.set(new Uint8Array([9, 8, 7]), 2);
console.log("typed source:", Array.from(typedTarget).join(","));

const overlapRight = new Uint8Array([1, 2, 3, 4, 5]);
overlapRight.set(overlapRight.subarray(0, 3), 2);
console.log("overlap right:", Array.from(overlapRight).join(","));

const overlapLeft = new Uint8Array([1, 2, 3, 4, 5]);
overlapLeft.set(overlapLeft.subarray(2), 0);
console.log("overlap left:", Array.from(overlapLeft).join(","));

const bufferTarget = Buffer.alloc(5);
bufferTarget.set([65, 66, 67], 1);
const bufferReturn = bufferTarget.set([68], 4);
console.log("buffer target:", Array.from(bufferTarget).join(","), bufferReturn === undefined);

const arrayLikeTarget = new Uint8Array(4);
arrayLikeTarget.set({ 0: 4, 1: 5, length: 2 } as any, 1);
console.log("arraylike:", Array.from(arrayLikeTarget).join(","));

showError("range negative", () => new Uint8Array(2).set([1], -1));
showError("range overflow", () => new Uint8Array(2).set([1, 2], 1));
showError("source undefined", () => new Uint8Array(2).set(undefined as any));
showError("source null", () => new Uint8Array(2).set(null as any));
