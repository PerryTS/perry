import { TextEncoder } from "node:util";

const encoder = new TextEncoder();

function show(label: string, fn: () => any): void {
  try {
    console.log(`${label}:`, "OK", JSON.stringify(fn()));
  } catch (error) {
    const e = error as Error & { code?: string };
    console.log(`${label}:`, "THROW", e.name, e.code, String(e.message).split("\n")[0]);
  }
}

show("encodeInto omitted", () => encoder.encodeInto());
show("encodeInto dest missing", () => encoder.encodeInto("x"));
show("encodeInto bad dest object", () => encoder.encodeInto("x", {} as any));
show("encodeInto bad dest arraybuffer", () => encoder.encodeInto("x", new ArrayBuffer(4) as any));
show("encodeInto undefined source", () => encoder.encodeInto(undefined as any, new Uint8Array(20)));
show("encodeInto number source", () => encoder.encodeInto(123 as any, new Uint8Array(20)));
show("encodeInto symbol source", () => encoder.encodeInto(Symbol("x") as any, new Uint8Array(20)));

const dest = new Uint8Array(4);
dest[3] = 99;
show("encodeInto ok", () => ({
  result: encoder.encodeInto("å", dest),
  bytes: Array.from(dest).join(","),
}));
