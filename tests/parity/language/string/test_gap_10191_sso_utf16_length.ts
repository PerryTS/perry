// #10191: JSON parsing can keep up to five UTF-8 bytes in an SSO immediate.
// Every length dispatch must count UTF-16 units, just like a heap string.
function typedLength(s: string): number { return s.length; }
function erasedLength(s: any): number { return s.length; }
function computedLength(s: string, key: any): number { return s[key]; }
function suffixSum(s: string): number {
  let sum = 0;
  while (s.length > 0) {
    sum += s.length;
    s = s.slice(1);
  }
  return sum;
}

console.log("literals", "é".length, "éé".length, "中".length,
  "😀".length, "\ud800".length, "\0é".length);

const samples = [
  "", "abcde", "a\0b", "é", "éé", "éabc", "abcé", "中", "é中", "中é",
  "😀", "a😀", "😀a", "e\u0301", "\0é", "\ud800", "a\udfffb",
  "ééé", "😀😀", "long ASCII string",
];
const key = { toString() { return "length"; } };
for (let i = 0; i < samples.length; i++) {
  const literal = samples[i];
  const parsed = JSON.parse('{"a":' + JSON.stringify(literal) + '}');
  const scalar = JSON.parse(JSON.stringify(literal));
  console.log(i, literal.length, parsed.a.length, typedLength(parsed.a),
    erasedLength(parsed.a), computedLength(parsed.a, "length"),
    computedLength(parsed.a, key), Object(parsed.a).length,
    typedLength(scalar), suffixSum(parsed.a));
}

// The multi-field loop must keep the same answer in its element-shape clone.
function fields(rows: any, count: number, n: number): number {
  let cursor = 0;
  let sum = 0;
  for (let i = 0; i < count; i++) {
    const index = cursor;
    sum = sum + rows[index].id;
    sum = sum + rows[index].name.length;
    sum = sum + (rows[index].active ? 1 : 0);
    cursor = (cursor * 17 + 7) % n;
  }
  return sum;
}
const records: any[] = [];
for (let i = 0; i < samples.length; i++) {
  records.push({ id: i, name: samples[i], active: i % 2 === 0 });
}
const rows = JSON.parse(JSON.stringify(records));
console.log("fields", fields(rows, 1000, samples.length));
