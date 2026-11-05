import { Buffer } from "node:buffer";

const regular = Buffer.from([0x10, 0x20, 0x30]);
console.log("regular buffer instanceof ArrayBuffer:", regular.buffer instanceof ArrayBuffer);
console.log("regular parent typeof:", typeof (regular as any).parent);
console.log("regular parent same as buffer:", (regular as any).parent === regular.buffer);

const slow = Buffer.allocUnsafeSlow(4);
slow.fill(0);
console.log("slow buffer instanceof ArrayBuffer:", slow.buffer instanceof ArrayBuffer);
console.log("slow byteOffset:", slow.byteOffset);
console.log("slow byteLength:", slow.byteLength);
console.log("slow parent typeof:", typeof (slow as any).parent);
console.log("slow parent same as buffer:", (slow as any).parent === slow.buffer);

const view = slow.subarray(1, 3);
console.log("view buffer same as slow:", view.buffer === slow.buffer);
console.log("view byteOffset:", view.byteOffset);
console.log("view byteLength:", view.byteLength);
console.log("view parent same as buffer:", (view as any).parent === view.buffer);

const backing = new Uint8Array(view.buffer);
backing[view.byteOffset] = 122;
console.log("backing write updates view:", view[0]);
