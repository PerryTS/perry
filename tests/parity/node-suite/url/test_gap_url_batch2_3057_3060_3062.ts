// Gap test: node:url argument coercion / validation parity (#3057, #3060, #3062).
//
// #3057: URLSearchParams instance methods Web-IDL-stringify their name/value
//   arguments via String(value); Symbols throw TypeError.
// #3060: fileURLToPath accepts only string | URL instance, else TypeError;
//   a valid URL instance round-trips to a filesystem path.
// #3062: legacy url.parse rejects non-string url arguments with TypeError.
//
// For invalid-URL / invalid-arg throws we assert only err.name (Perry's
// message text differs from Node by design). The Symbol case also prints the
// message because that text is part of the spec'd coercion contract.

import { fileURLToPath } from "node:url";
import * as url from "node:url";

// --- #3057: URLSearchParams arg coercion -----------------------------------

const sp1 = new URLSearchParams();
sp1.append(123 as any, null as any);
console.log("append number/null:", sp1.toString());

const sp2 = new URLSearchParams();
sp2.set({ toString() { return "k"; } } as any, true as any);
console.log("set object/bool:", sp2.toString());

const sp3 = new URLSearchParams("123=x&null=y");
console.log("get number/null:", sp3.get(123 as any) + "," + sp3.get(null as any));

const sp4 = new URLSearchParams("a=1&a=2");
console.log("has name+value:", [sp4.has("a", 2 as any), sp4.has("a", 3 as any)].join(","));

const sp5 = new URLSearchParams("a=1&a=2&a=3");
sp5.delete("a", 2 as any);
console.log("delete name+value:", sp5.toString());

try {
  const sp6 = new URLSearchParams();
  sp6.append(Symbol("x") as any, "v");
  console.log("append symbol: OK");
} catch (err: any) {
  console.log("append symbol:", err.name, err.message);
}

// --- #3060: fileURLToPath validation ---------------------------------------

try {
  fileURLToPath(123 as any);
  console.log("fileURLToPath number: OK");
} catch (err: any) {
  console.log("fileURLToPath number:", err.name);
}

console.log("fileURLToPath URL:", fileURLToPath(new URL("file:///tmp/x")));

// --- #3062: legacy url.parse validation ------------------------------------

try {
  url.parse(123 as any);
  console.log("parse number: OK");
} catch (err: any) {
  console.log("parse number:", err.name);
}
