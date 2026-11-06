import querystring from "node:querystring";

const parseCases = [
  ["sep empty", () => querystring.parse("a&b", "", "=")],
  ["sep number", () => querystring.parse("a1b1c", 1 as any, "=")],
  ["sep zero", () => querystring.parse("a0b", 0 as any, "=")],
  ["sep true", () => querystring.parse("a=trueb=truec", true as any, "=")],
  ["sep false", () => querystring.parse("afalseb", false as any, "=")],
  ["sep object", () => querystring.parse("a[object Object]b", {} as any, "=")],
  ["sep symbol", () => querystring.parse("aSymbol(x)b", Symbol("x") as any, "=")],
  ["eq empty", () => querystring.parse("a=1&b=2", "&", "")],
  ["eq number", () => querystring.parse("a=1&b=2", "&", 1 as any)],
  ["eq zero", () => querystring.parse("a0 1&b0 2", "&", 0 as any)],
  ["eq true", () => querystring.parse("a=1&b=2", "&", true as any)],
  ["eq false", () => querystring.parse("afalse1&bfalse2", "&", false as any)],
  ["eq object", () => querystring.parse("a[object Object]1&b[object Object]2", "&", {} as any)],
  ["eq symbol", () => querystring.parse("aSymbol(x)1&bSymbol(x)2", "&", Symbol("x") as any)],
] as const;

for (const [label, run] of parseCases) {
  console.log("parse", label + ":", JSON.stringify(run()));
}

const stringifyCases = [
  ["sep empty", () => querystring.stringify({ a: 1, b: 2 }, "", "=")],
  ["sep number", () => querystring.stringify({ a: 1, b: 2 }, 1 as any, "=")],
  ["sep zero", () => querystring.stringify({ a: 1, b: 2 }, 0 as any, "=")],
  ["sep true", () => querystring.stringify({ a: 1, b: 2 }, true as any, "=")],
  ["sep false", () => querystring.stringify({ a: 1, b: 2 }, false as any, "=")],
  ["sep object", () => querystring.stringify({ a: 1, b: 2 }, {} as any, "=")],
  ["eq empty", () => querystring.stringify({ a: 1, b: 2 }, "&", "")],
  ["eq number", () => querystring.stringify({ a: 1, b: 2 }, "&", 1 as any)],
  ["eq zero", () => querystring.stringify({ a: 1, b: 2 }, "&", 0 as any)],
  ["eq true", () => querystring.stringify({ a: 1, b: 2 }, "&", true as any)],
  ["eq false", () => querystring.stringify({ a: 1, b: 2 }, "&", false as any)],
  ["eq object", () => querystring.stringify({ a: 1, b: 2 }, "&", {} as any)],
] as const;

for (const [label, run] of stringifyCases) {
  console.log("stringify", label + ":", JSON.stringify(run()));
}

try {
  console.log("stringify sep symbol:", querystring.stringify({ a: 1, b: 2 }, Symbol("x") as any, "="));
} catch (error: any) {
  console.log("stringify sep symbol throw:", error.name);
}

try {
  console.log("stringify eq symbol:", querystring.stringify({ a: 1, b: 2 }, "&", Symbol("x") as any));
} catch (error: any) {
  console.log("stringify eq symbol throw:", error.name);
}
