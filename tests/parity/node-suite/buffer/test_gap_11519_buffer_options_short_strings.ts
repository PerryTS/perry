// #11519: Buffer / KeyObject / File surfaces reading a string option or key
// that may be a SHORT (SSO, <= 5 bytes, built at runtime) string. They checked
// the heap string tag only, so the inline value read as "not a string".
import { createSecretKey } from "node:crypto";
const S = (s: string): string => s.charAt(0) + s.slice(1);

const b: any = Buffer.from("hey");
const probe = (x: any) => x;
const dyn = probe(b);
console.log("hasOwnProperty:", dyn.hasOwnProperty(S("0")), dyn.hasOwnProperty(S("2")), dyn.hasOwnProperty(S("3")), dyn.hasOwnProperty(String(1)));
console.log("propertyIsEnumerable:", dyn.propertyIsEnumerable(S("1")), dyn.propertyIsEnumerable(S("9")));
const key = createSecretKey(Buffer.from("secret"));
console.log("export jwk:", JSON.stringify(key.export({ format: S("jwk") as any })));
console.log("export buf:", (key.export({ format: S("buffer") as any }) as Buffer).toString());
const f = new File(["x"], "a.txt", { lastModified: S("12") as any });
console.log("lastModified:", f.lastModified, new File([], "b", { lastModified: String(0) as any }).lastModified);
