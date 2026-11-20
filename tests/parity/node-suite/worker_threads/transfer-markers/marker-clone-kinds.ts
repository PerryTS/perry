import { isMarkedAsUntransferable, markAsUncloneable, markAsUntransferable } from "node:worker_threads";

function outcome(value: any): string {
  try { structuredClone(value); return "ok"; }
  catch (error: any) { return `${error.name}:${error.code}`; }
}

const primitives = [undefined, null, false, 0, "text", 1n, Symbol(), Symbol.for("marker")];
for (const value of primitives) {
  markAsUntransferable(value);
  markAsUncloneable(value);
  console.log("primitive:", typeof value, isMarkedAsUntransferable(value));
}
const values = [{}, new Error("marked"), [], new Map(), new Set(), new Date(0), /x/, new ArrayBuffer(4), new Uint8Array(4), new DataView(new ArrayBuffer(4)), new Number(1)];
for (let index = 0; index < values.length; index++) {
  markAsUncloneable(values[index]);
  console.log("kind:", index, outcome(values[index]));
}
const buffer = new Uint8Array([9, 8, 7]).buffer;
markAsUntransferable(buffer);
try {
  structuredClone(buffer, { transfer: [buffer] });
  console.log("unexpected transfer");
} catch (error: any) {
  console.log("global transfer:", error.name, error.code, buffer.byteLength, Array.from(new Uint8Array(buffer)).join(","));
}
