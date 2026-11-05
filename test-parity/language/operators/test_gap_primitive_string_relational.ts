function relations(a: any, b: any) {
  return [a < b, a > b, a <= b, a >= b];
}
const strings = JSON.parse('["","a","ab","abcde","abcdefghij","é","𐀀",""]');
for (const a of strings) {
  for (const b of strings) console.log(JSON.stringify([a, b, relations(a, b)]));
}
for (const a of ["2", "10", "", "NaN", "-1"]) {
  for (const b of [2, 10, null, undefined, true, NaN]) {
    console.log(JSON.stringify([a, b, relations(a, b), relations(b, a)]));
  }
}
let log: string[] = [];
const left = {[Symbol.toPrimitive](hint: string) { log.push("left:" + hint); return "a"; }};
const right = {[Symbol.toPrimitive](hint: string) { log.push("right:" + hint); return "b"; }};
console.log(JSON.stringify(relations(left, right)), log.join(","));
log = [];
console.log(JSON.stringify(relations(left, "b")), log.join(","));
const marker = {marker: true};
const throwing = {[Symbol.toPrimitive]() { throw marker; }};
try { console.log(throwing < "b"); } catch (e) { console.log(e === marker); }

const boundaries: any[] = ["abcdefg𐀀", "abcdefg\uE000", "abcd\u0000efgh", "abcdefgéx"];
const alphabet = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ";
for (const length of [7, 8, 9, 15, 16, 17, 32]) {
  const base = alphabet.slice(0, length);
  boundaries.push(base, base + "q", base.slice(0, length - 1) + "Z");
  if (length >= 8) boundaries.push(base.slice(0, 7) + "Z" + base.slice(8));
}
for (const a of boundaries) {
  for (const b of boundaries) console.log(JSON.stringify([a, b, relations(a, b)]));
}
