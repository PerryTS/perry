import { Buffer } from "node:buffer";

const fromArray = Buffer.from([0x10, 0x20, 0x30]);
console.log(
  "from buffer instanceof ArrayBuffer:",
  fromArray.buffer instanceof ArrayBuffer,
);
console.log("from parent same as buffer:", fromArray.parent === fromArray.buffer);
console.log(
  "from parent instanceof ArrayBuffer:",
  fromArray.parent instanceof ArrayBuffer,
);

const slow = Buffer.allocUnsafeSlow(4);
slow.fill(0);
console.log("slow byteOffset:", slow.byteOffset);
console.log("slow parent same as buffer:", slow.parent === slow.buffer);
console.log("slow parent typeof:", typeof slow.parent);
console.log("slow parent instanceof ArrayBuffer:", slow.parent instanceof ArrayBuffer);
