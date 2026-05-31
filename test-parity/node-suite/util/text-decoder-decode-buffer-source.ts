import { TextDecoder } from "node:util";

const decoder = new TextDecoder();
const invalidMessage = "SharedArrayBuffer, ArrayBuffer or ArrayBufferView";

function showDecode(label: string, value: any): void {
  try {
    console.log(`${label}:`, JSON.stringify(decoder.decode(value)));
  } catch (error: any) {
    console.log(
      `${label}:`,
      error?.name,
      error?.code,
      String(error?.message).includes(invalidMessage),
    );
  }
}

console.log("decode omitted:", JSON.stringify(decoder.decode()));
showDecode("decode undefined", undefined);

const ab = new ArrayBuffer(5);
new Uint8Array(ab).set([120, 65, 66, 67, 121]);
showDecode("arraybuffer", ab);
showDecode("uint8 view", new Uint8Array(ab, 1, 3));
showDecode("dataview", new DataView(ab, 1, 3));
showDecode("uint16 array", new Uint16Array([0x41, 0x42]));

const sab = new SharedArrayBuffer(4);
new Uint8Array(sab).set([104, 105, 33, 10]);
showDecode("sharedarraybuffer", sab);

showDecode("decode null", null);
showDecode("decode string", "abc");
showDecode("decode number", 123);
showDecode("decode object", { length: 3 });
