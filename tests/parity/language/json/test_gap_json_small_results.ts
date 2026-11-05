// Exercise boxed inline JSON results through normal compiled consumers and
// callbacks. Hash the actual emitted UTF-16 code units against Node.
const inputs: any[] = [null, true, false, 0, -0, 1.5, "", "a", "ab", "abc", "abcd",
  '"', "\\", "\b", "\t", "\n", "\f", "\r", "\x00", "é", "東", "🙂", "\ud800", "\udfff", {}, []];
const saved: any[] = [];
let hash = 0;
let failures = 0;
for (let round = 0; round < 100; round++) {
  for (let i = 0; i < inputs.length; i++) {
    const encoded = JSON.stringify(inputs[i]);
    if (typeof encoded !== "string") failures++;
    if (("x" + encoded).slice(1) !== encoded) failures++;
    const decoded: any = JSON.parse(encoded);
    if (JSON.stringify(decoded) !== encoded) failures++;
    for (let j = 0; j < encoded.length; j++) hash = (hash * 33 + encoded.charCodeAt(j)) % 1000000007;
    saved.push(encoded);
  }
}
console.log("small results", saved.length, failures, hash, JSON.stringify(saved).length);

const calls: string[] = [];
console.log(JSON.stringify(null, (key: string, value: any) => {
  calls.push("replacer:" + key);
  return value === null ? "replacement" : value;
}));
const space: any = new Number(2);
space.valueOf = function() { calls.push("space"); return 2; };
console.log(JSON.stringify(null, null, space));
const throwingSpace: any = new Number(2);
throwingSpace.valueOf = function() { throw new Error("space observed"); };
try { JSON.stringify("a", null, throwingSpace); }
catch (e: any) { console.log(e.message); }

const nested: any = { toJSON() {
  calls.push("toJSON");
  return { a: JSON.stringify("a"), b: JSON.stringify(null), c: { toJSON() { return true; } } };
} };
console.log(JSON.stringify(nested));
console.log(JSON.stringify(nested, null, 2));
console.log(calls.join(","));
const stringify = JSON.stringify;
console.log(stringify(null), stringify("a"), stringify({}));
