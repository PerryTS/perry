import * as crypto from "node:crypto";

const re = /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;

function probe(label: string, fn: () => unknown) {
  try {
    const value = fn();
    console.log(label, "OK", typeof value, re.test(String(value)));
  } catch (err: any) {
    console.log(label, "THROW", err?.name, err?.code || "no-code");
  }
}

probe("no options", () => crypto.randomUUID());
probe("empty options", () => crypto.randomUUID({}));
probe("disable cache true", () => crypto.randomUUID({ disableEntropyCache: true }));
probe("disable cache false", () => crypto.randomUUID({ disableEntropyCache: false }));
probe("bad option property", () => crypto.randomUUID({ disableEntropyCache: 1 } as any));
probe("null options", () => crypto.randomUUID(null as any));
probe("number options", () => crypto.randomUUID(123 as any));

const captured = crypto.randomUUID;
probe("captured bad property", () => captured({ disableEntropyCache: 1 } as any));
