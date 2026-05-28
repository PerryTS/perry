import { Readable } from "node:stream";

// Returning an iterator with destroyOnReturn:false leaves the stream open and
// a later default async iterator resumes from the next unread chunk.
const r = Readable.from(["a", "b", "c"]);
const it = (r as any).iterator({ destroyOnReturn: false });
const first = await it.next();
await it.return?.();
const rest: string[] = [];
for await (const v of r) rest.push(String(v));
console.log("first:", first.value);
console.log("rest:", rest.join(","));
console.log("destroyed:", r.destroyed);
