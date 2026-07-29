import { Buffer } from "node:buffer";
import { deserialize, serialize } from "node:v8";

const arrayBuffer = new Uint8Array([10, 20, 30, 40]).buffer;
const dataView = new DataView(arrayBuffer, 1, 2);
const buffer = Buffer.from([5, 6, 7]);

for (const value of [arrayBuffer, dataView, buffer] as const) {
  const result: any = deserialize(serialize(value));
  const bytes = result instanceof ArrayBuffer
    ? [...new Uint8Array(result)]
    : [...new Uint8Array(result.buffer, result.byteOffset, result.byteLength)];
  console.log(
    value.constructor.name + ":",
    result.constructor.name,
    result.byteLength,
    bytes.join(","),
  );
}

const payload = serialize({ ok: 1 });
for (
  const input of [
    payload,
    new Uint8Array(payload),
    new DataView(payload.buffer, payload.byteOffset, payload.byteLength),
  ] as const
) {
  console.log(
    "input " + input.constructor.name + ":",
    JSON.stringify(deserialize(input)),
  );
}
