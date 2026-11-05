import { Buffer } from "node:buffer";

for (const [name, fn] of [
  ["swap16", () => Buffer.alloc(3).swap16()],
  ["swap32", () => Buffer.alloc(6).swap32()],
  ["swap64", () => Buffer.alloc(10).swap64()],
] as const) {
  try {
    fn();
    console.log(name, "no throw");
  } catch (e: any) {
    console.log(name, e.name, e.code);
  }
}
console.log("empty ok:", Buffer.alloc(0).swap16().length);
