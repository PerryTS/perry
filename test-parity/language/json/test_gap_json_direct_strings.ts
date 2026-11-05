// Retain input and output strings across allocations and moving collections.
const saved: string[] = [];
let hash = 0;
let failures = 0;
const units = ["a", "é", "東京", "🙂", "\u2028\u2029", "\ud800", "\udfff", "\n\"\\"];
const sizes = [3, 63, 64, 65, 127, 128, 129, 4096];
for (let round = 0; round < 12; round++) {
  for (let u = 0; u < units.length; u++) {
    for (let n = 0; n < sizes.length; n++) {
      const input = units[u].repeat(sizes[n]) + ":" + round;
      const output = JSON.stringify(input);
      if (JSON.parse(output) !== input) failures++;
      if (JSON.stringify(input, null, 2) !== output) failures++;
      for (let i = 0; i < output.length; i++) hash = (hash * 33 + output.charCodeAt(i)) % 1000000007;
      saved.push(input);
      saved.push(output);
    }
  }
}
for (let i = 0; i < saved.length; i += 2) {
  if (JSON.parse(saved[i + 1]) !== saved[i]) failures++;
}
console.log("direct strings", saved.length, failures, hash);

const text = "東京🙂plain".repeat(32);
let calls = "";
console.log(JSON.stringify(text, (key: string, value: any) => {
  calls += "replacer:" + key + ",";
  return "replaced";
}));
const spacer: any = new Number(2);
spacer.valueOf = function() { calls += "space,"; return 2; };
console.log(JSON.stringify(text, null, spacer) === '"' + text + '"');
const throwing: any = new Number(2);
throwing.valueOf = function() { throw new Error("space observed"); };
try { JSON.stringify(text, null, throwing); }
catch (error: any) { console.log(error.message); }
const nested = { toJSON() {
  const inner = JSON.stringify(text);
  return { inner: inner, next: { toJSON(key: string) { calls += key + ","; return 7; } } };
} };
console.log(JSON.stringify(nested).length, calls);
const stringify = JSON.stringify;
console.log(stringify(text).length, stringify(text) === JSON.stringify(text));
