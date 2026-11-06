import querystring from "node:querystring";

const values = [
  ["undefined", undefined],
  ["null", null],
  ["number", 123],
  ["boolean", true],
  ["object", {}],
] as any[];

console.log("escape omitted:", JSON.stringify(querystring.escape()));
console.log("unescape omitted:", JSON.stringify(querystring.unescape()));

for (const [label, value] of values) {
  console.log("escape", label + ":", JSON.stringify(querystring.escape(value)));
  console.log("unescape", label + ":", JSON.stringify(querystring.unescape(value)));
}

try {
  console.log("escape symbol:", JSON.stringify(querystring.escape(Symbol("x") as any)));
} catch (error: any) {
  console.log("escape symbol throw:", String(error).slice(0, 9));
}

try {
  console.log("unescape symbol:", JSON.stringify(querystring.unescape(Symbol("x") as any)));
} catch (error: any) {
  console.log("unescape symbol throw:", String(error).slice(0, 9));
}
