import querystring from "node:querystring";

const input = "a=1&b=2&c=3";
for (const maxKeys of [0, 1, 2, -1, Infinity, NaN] as any[]) {
  const out = querystring.parse(input, undefined, undefined, { maxKeys });
  console.log("maxKeys", String(maxKeys) + ":", Object.keys(out).join(","));
}
