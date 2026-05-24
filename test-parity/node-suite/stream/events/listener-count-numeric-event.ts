import { Readable } from "node:stream";
// listenerCount with a numeric event key (Node EE accepts strings only;
// non-string coerces / works as-is depending on impl).
const r = new Readable({ read() {} });
console.log("numeric:", r.listenerCount(42 as any));
console.log("string:", r.listenerCount("42"));
console.log("both 0:", r.listenerCount(42 as any) === 0 && r.listenerCount("42") === 0);
