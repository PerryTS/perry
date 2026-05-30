import * as crypto from "node:crypto";

function probe(label: string, fn: () => unknown) {
  try {
    const value = fn();
    console.log(label, "OK", typeof value, Number.isSafeInteger(value));
  } catch (err: any) {
    console.log(label, "THROW", err?.name, err?.code || "no-code");
  }
}

probe("max less than min", () => crypto.randomInt(10, 0));
probe("max equals min", () => crypto.randomInt(0, 0));
probe("negative equal", () => crypto.randomInt(-5, -5));
probe("range too large", () => crypto.randomInt(0, 2 ** 48 + 1));
probe("valid high boundary", () => crypto.randomInt(2 ** 48, 2 ** 48 + 1));

let callbackCalled = false;
probe("callback max less than min", () =>
  crypto.randomInt(10, 0, () => {
    callbackCalled = true;
  })
);
console.log("callback called:", callbackCalled);
