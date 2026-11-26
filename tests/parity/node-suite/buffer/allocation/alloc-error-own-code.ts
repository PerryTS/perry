import { Buffer } from "node:buffer";

// A Node internal error carries its `code` as an ordinary own property, so
// reflection and serialization see it.
for (const value of [-1, "x"] as any[]) {
  try {
    Buffer.alloc(value);
  } catch (err: any) {
    console.log(err.name, Object.keys(err).join(","), JSON.stringify(err), Object.hasOwn(err, "code"));
    console.log(JSON.stringify({ ...err }));
  }
}
