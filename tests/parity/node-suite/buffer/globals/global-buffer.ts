import { Buffer as ModuleBuffer } from "node:buffer";

const GlobalBuffer = globalThis.Buffer;
const { Buffer: DestructuredBuffer } = globalThis;
const desc = Object.getOwnPropertyDescriptor(globalThis, "Buffer");

console.log("global type:", typeof GlobalBuffer);
console.log(
  "identity:",
  GlobalBuffer === ModuleBuffer,
  Buffer === ModuleBuffer,
  DestructuredBuffer === ModuleBuffer,
);
console.log(
  "static:",
  typeof GlobalBuffer.from,
  typeof GlobalBuffer.alloc,
  typeof GlobalBuffer.isBuffer,
  typeof GlobalBuffer.byteLength,
  typeof GlobalBuffer.copyBytesFrom,
);

const from = GlobalBuffer.from("ok");
console.log(
  "from:",
  from.toString("hex"),
  GlobalBuffer.isBuffer(from),
  GlobalBuffer.byteLength("he"),
);
console.log("alloc:", DestructuredBuffer.alloc(3, 0x41).toString("ascii"));
console.log(
  "proto:",
  ["toString", "equals", "subarray", "readUInt8"]
    .map((name) => name + ":" + typeof GlobalBuffer.prototype[name])
    .join(","),
);
console.log(
  "proto constructor:",
  GlobalBuffer.prototype.constructor === GlobalBuffer,
  typeof GlobalBuffer.prototype.constructor,
);
console.log(
  "descriptor:",
  desc !== undefined,
  desc ? desc.enumerable : "missing",
  desc ? desc.configurable : "missing",
);
console.log("name-length:", GlobalBuffer.name, GlobalBuffer.length);
