import querystring from "node:querystring";

const out = querystring.stringify({ a: "x y", b: "z" }, "&", "=", { encodeURIComponent(v: string) { return "[" + v + "]"; } });
console.log("custom encoder:", out);
try { querystring.stringify({ a: "x" }, undefined, undefined, { encodeURIComponent() { throw new Error("encode boom"); } }); console.log("throw encoder no throw"); } catch (err: any) { console.log("throw encoder:", err?.name, err?.message); }
