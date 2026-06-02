import querystring from "node:querystring";

const input = "a=1&b=2&c=3";
for (const maxKeys of [0, 0.1, 0.9, 1, 1.1, 2, 2.1, -1, Infinity, NaN] as any[]) {
  const out = querystring.parse(input, undefined, undefined, { maxKeys });
  console.log("maxKeys", String(maxKeys) + ":", Object.keys(out).join(","));
}

let large = "";
for (let i = 0; i < 1005; i++) {
  if (i > 0) large += "&";
  large += "k" + i + "=v";
}

console.log(
  "fractional large:",
  Object.keys(querystring.parse(large, undefined, undefined, { maxKeys: 1.1 })).length,
);
