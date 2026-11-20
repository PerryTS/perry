// #14973: `s.charCodeAt(i)` on a non-ASCII string parameter reads through a
// per-invocation cursor, so a sequential scan decodes each code point once.
// Every read order must still see the UTF-16 code unit at its index.
function sum(s: string): number {
  let h = 0;
  for (let i = 0; i < s.length; i++) h = (Math.imul(h, 31) + s.charCodeAt(i)) | 0;
  return h >>> 0;
}

function units(s: string, order: number[]): string {
  const out: number[] = [];
  for (const i of order) out.push(s.charCodeAt(i));
  return out.join(",");
}

function interleave(a: string, b: string): string {
  const out: number[] = [];
  const n = Math.max(a.length, b.length);
  for (let i = 0; i < n; i++) out.push(a.charCodeAt(i), b.charCodeAt(n - 1 - i));
  return out.join(",");
}

function churn(s: string): string {
  // Allocation in the loop lets the collector move `s` between reads.
  const keep: string[] = [];
  const out: number[] = [];
  for (let i = 0; i < s.length; i++) {
    for (let k = 0; k < 200; k++) keep.push("x" + k + "é" + i);
    if (keep.length > 4000) keep.length = 0;
    out.push(s.charCodeAt(i));
  }
  return out.join(",");
}

class Scanner {
  read(s: string, at: number): number { return s.charCodeAt(at); }
}

const line = "const value = await store.load(\"snapshot\"); // é ü – render(value.items[240])";
const astral = "a😀b中😀é";
console.log("sum", sum(line), sum(astral), sum("plain ascii"), sum(""));
console.log("forward", units(astral, [0, 1, 2, 3, 4, 5, 6, 7]));
console.log("backward", units(astral, [7, 6, 5, 4, 3, 2, 1, 0]));
console.log("jumps", units(astral, [2, 1, 2, 6, 5, 0, 7, 7]));
console.log("keys", units(astral, [-1, 8, 100, 1.5, NaN, Infinity, -0]));
console.log("two", interleave("éàü", "中😀x"));
console.log("churn", churn("añb€c😀d".repeat(3)));
const scanner = new Scanner();
console.log("method", [0, 1, 2, 1].map((i) => scanner.read("√∑≈", i)).join(","));
const read = (s: string, i: number) => s.charCodeAt(i);
console.log("closure", read("ÿ€", 1), read("ÿ€", 0), read("ÿ€", 2));
console.log("wrapper", units(new String("é€x") as any as string, [0, 1, 2, 1]));
