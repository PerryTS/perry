import * as crypto from "node:crypto";
import { Buffer } from "node:buffer";

function probe(label: string, fn: () => unknown) {
  try {
    console.log(label, "OK", fn());
  } catch (err: any) {
    console.log(label, "THROW", err?.name, err?.code || "no-code");
  }
}

probe("buffer equal", () => crypto.timingSafeEqual(Buffer.from([1]), Buffer.from([1])));
probe("buffer diff", () => crypto.timingSafeEqual(Buffer.from([1]), Buffer.from([2])));
probe("length mismatch", () => crypto.timingSafeEqual(Buffer.from([1]), Buffer.from([1, 2])));
probe("typed array equal", () => crypto.timingSafeEqual(new Uint16Array([1]), new Uint16Array([1])));
probe("arraybuffer equal", () => crypto.timingSafeEqual(new ArrayBuffer(2), new ArrayBuffer(2)));
probe("dataview equal", () =>
  crypto.timingSafeEqual(new DataView(new ArrayBuffer(2)), new DataView(new ArrayBuffer(2)))
);
probe("string invalid", () => crypto.timingSafeEqual("a" as any, "a" as any));
probe("buf2 invalid", () => crypto.timingSafeEqual(Buffer.from([1]), 123 as any));

const captured = crypto.timingSafeEqual;
probe("captured length mismatch", () => captured(Buffer.from([1]), Buffer.from([1, 2])));
