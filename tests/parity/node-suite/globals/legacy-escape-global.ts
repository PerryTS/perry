function desc(name: string) {
  const d: any = Object.getOwnPropertyDescriptor(globalThis, name);
  console.log(`${name} desc:`, typeof (globalThis as any)[name], d?.writable, d?.enumerable, d?.configurable);
}

function codeUnits(value: string) {
  const out: string[] = [];
  for (let i = 0; i < value.length; i++) {
    out.push(value.charCodeAt(i).toString(16));
  }
  return out.join(",");
}

function errorName(label: string, fn: () => unknown) {
  try {
    fn();
    console.log(`${label}:`, "no error");
  } catch (error: any) {
    console.log(`${label}:`, error?.name ?? "");
  }
}

function lengthDesc(label: string, fn: Function) {
  const d: any = Object.getOwnPropertyDescriptor(fn, "length");
  console.log(`${label} length desc:`, d?.value, d?.writable, d?.enumerable, d?.configurable);
}

desc("global");
desc("escape");
desc("unescape");

const g: any = global;
console.log("global identity:", g === globalThis, (globalThis as any).global === globalThis, g.global === g);
console.log("function shape:", escape.name, escape.length, unescape.name, unescape.length);
console.log("global function shape:", g.escape.name, g.escape.length, g.unescape.name, g.unescape.length);
lengthDesc("escape", escape);
lengthDesc("unescape", unescape);

console.log(
  "escape basics:",
  JSON.stringify(escape("a b+c")),
  JSON.stringify(escape("!*'()~")),
  JSON.stringify(escape("a/b?c=d&e")),
);
console.log(
  "escape unicode:",
  JSON.stringify(escape("caf\u00e9")),
  JSON.stringify(escape("\u{1F642}")),
  JSON.stringify(escape("\u0000\u00ff\u0100")),
);
console.log("escape missing:", JSON.stringify(escape()));

console.log(
  "unescape units:",
  codeUnits(unescape("a%20b%2Bc")),
  codeUnits(unescape("caf%E9")),
  codeUnits(unescape("%u00E9")),
  codeUnits(unescape("%F0%9F%99%82")),
  codeUnits(unescape("abc%zzdef")),
  codeUnits(unescape("%uD83D%uDE42")),
);
console.log("unescape missing:", JSON.stringify(unescape()));

const reboundEscape = (globalThis as any).escape;
const reboundUnescape = (globalThis as any).unescape;
console.log("rebound:", JSON.stringify(reboundEscape("x y")), codeUnits(reboundUnescape("x%20y")));

errorName("escape symbol", () => escape(Symbol("x") as any));
errorName("unescape symbol", () => unescape(Symbol("x") as any));
