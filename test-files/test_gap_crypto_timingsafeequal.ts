// @covers crypto.timingSafeEqual(a, b) BufferSource validation (#3065)
// Node accepts ArrayBuffer/Buffer/TypedArray/DataView, throws
// ERR_CRYPTO_TIMING_SAFE_EQUAL_LENGTH (RangeError) on a byte-length mismatch,
// and ERR_INVALID_ARG_TYPE (TypeError) for non-BufferSource inputs.
import crypto from "node:crypto";

function show(label: string, fn: () => unknown): void {
  try {
    console.log(label, "OK", fn());
  } catch (err: any) {
    console.log(label, "THROW", err.name, err.code, err.message);
  }
}

show("equal-buffers", () =>
  crypto.timingSafeEqual(Buffer.from([1, 2, 3]), Buffer.from([1, 2, 3])),
);
show("unequal-buffers", () =>
  crypto.timingSafeEqual(Buffer.from([1, 2, 3]), Buffer.from([1, 2, 4])),
);
show("length-mismatch", () =>
  crypto.timingSafeEqual(Buffer.from([1]), Buffer.from([1, 2])),
);
show("uint16-equal", () =>
  crypto.timingSafeEqual(new Uint16Array([1, 2]), new Uint16Array([1, 2])),
);
show("arraybuffer-equal", () =>
  crypto.timingSafeEqual(new ArrayBuffer(4), new ArrayBuffer(4)),
);
show("dataview-equal", () =>
  crypto.timingSafeEqual(
    new DataView(new ArrayBuffer(2)),
    new DataView(new ArrayBuffer(2)),
  ),
);
show("string-arg", () => crypto.timingSafeEqual("a" as any, "a" as any));
show("number-arg", () =>
  crypto.timingSafeEqual(Buffer.from([1]), 123 as any),
);
