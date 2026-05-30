import { timingSafeEqual } from "node:crypto";
import { Buffer } from "node:buffer";

function show(label: string, fn: () => unknown) {
  try {
    console.log(label, "OK", fn());
  } catch (err) {
    const e = err as Error & { code?: string };
    console.log(label, "THROW", e.name, e.code, e.message.split("\n")[0]);
  }
}

show("buffer-equal", () => timingSafeEqual(Buffer.from([1, 2, 3]), Buffer.from([1, 2, 3])));
show("buffer-different", () => timingSafeEqual(Buffer.from([1, 2, 3]), Buffer.from([1, 2, 4])));
show("length-mismatch", () => timingSafeEqual(Buffer.from([1]), Buffer.from([1, 2])));
show("uint16array", () => timingSafeEqual(new Uint16Array([0x0102]), new Uint16Array([0x0102])));
show("arraybuffer", () => timingSafeEqual(new ArrayBuffer(2), new ArrayBuffer(2)));
show("dataview", () => timingSafeEqual(new DataView(new ArrayBuffer(2)), new DataView(new ArrayBuffer(2))));
show("invalid-buf1-string", () => timingSafeEqual("aa" as any, Buffer.from([1, 2])));
show("invalid-buf1-null", () => timingSafeEqual(null as any, Buffer.from([1, 2])));
show("invalid-buf2-number", () => timingSafeEqual(Buffer.from([1, 2]), 123 as any));
show("invalid-buf2-object", () => timingSafeEqual(Buffer.from([1, 2]), {} as any));
