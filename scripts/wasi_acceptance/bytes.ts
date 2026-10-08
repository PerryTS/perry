const bytes = new Uint8Array([1, 2, 3, 255]);
console.log(bytes.length, bytes[0], bytes[3]);
bytes[1] = 7;
console.log(bytes.slice(1, 3).join(","));
const view = new DataView(bytes.buffer);
view.setUint16(0, 513, true);
console.log(bytes[0], bytes[1], view.getUint16(0, true));
