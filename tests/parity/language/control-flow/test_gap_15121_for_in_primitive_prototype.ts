// for-in over a primitive visits the enumerable keys its prototype chain
// carries, and skips one deleted before it is reached.
(Number.prototype as any).n1 = 1;
(Number.prototype as any).n2 = 2;
(String.prototype as any).s = 3;
(Boolean.prototype as any).b = 4;

const seen: string[] = [];
for (const k in 5) seen.push(k);
console.log("number", seen.join(","));

seen.length = 0;
for (const k in "ab") seen.push(k);
console.log("string", seen.join(","));

seen.length = 0;
const long = "x".repeat(3) + "yz-long-enough-for-a-heap-string";
for (const k in long) if (!/^\d+$/.test(k)) seen.push(k);
console.log("heap string", seen.join(","));

seen.length = 0;
for (const k in true) seen.push(k);
console.log("boolean", seen.join(","));

seen.length = 0;
const n: any = 7;
for (const k in n) {
  seen.push(k);
  if (k === "n1") delete (Number.prototype as any).n2;
}
console.log("deleted", seen.join(","));

delete (Number.prototype as any).n1;
delete (String.prototype as any).s;
delete (Boolean.prototype as any).b;
seen.length = 0;
for (const k in 5) seen.push(k);
console.log("restored", seen.length);
