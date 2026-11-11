import querystring from "node:querystring";

try {
  querystring.parse("a=1", undefined, undefined, { decodeURIComponent() { throw new Error("decode boom"); } });
  console.log("throwing decoder: no throw");
} catch (err: any) { console.log("throwing decoder:", err?.name, err?.message); }
const out = querystring.parse("a=1%202&b=x+y", undefined, undefined, { decodeURIComponent(v: string) { return "[" + v + "]"; } });
console.log("custom decoder:", JSON.stringify(out));
