import {
  arrayBuffer,
  blob,
  buffer,
  bytes,
  json,
  text,
} from "node:stream/consumers";

const invalidCases = [
  ["text", text],
  ["buffer", buffer],
  ["arrayBuffer", arrayBuffer],
  ["json", json],
  ["blob", blob],
  ["bytes", bytes],
] as const;

for (const [name, fn] of invalidCases) {
  try {
    await fn(123 as any);
    console.log(name, "OK");
  } catch (err: any) {
    console.log(name, "THROW", err.name, String(err.code), err.message);
  }
}

console.log("valid string:", await text("abc" as any));
