import { Serializer } from "node:v8";

const serializer = new Serializer();
serializer.writeUint32(7);
const first = serializer.releaseBuffer();
console.log(
  "first:",
  Buffer.isBuffer(first),
  first.length,
  first.toString("hex"),
);

for (const label of ["release again", "write after release"] as const) {
  try {
    const result = label === "release again"
      ? serializer.releaseBuffer()
      : serializer.writeUint32(8);
    console.log(label + ":", result === undefined ? "undefined" : "value");
  } catch (error: any) {
    console.log(label + ":", error.name, error.code ?? "no-code");
  }
}
