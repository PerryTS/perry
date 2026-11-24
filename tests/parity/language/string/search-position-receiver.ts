// includes / indexOf / startsWith / endsWith read their position argument, and the
// same method names on an array or class instance (typed `any`) run that receiver's
// own method, in sync and async bodies.
const s = "hello world";
const u: any = "abcabc";
function strings(): string {
  return [
    s.includes("hello", 1), s.includes("o", 5), s.includes("h", -5), s.includes("d", 99),
    s.startsWith("world", 6), s.startsWith("hello", 1),
    s.endsWith("hello", 5), s.endsWith("world", 5),
    s.indexOf("o", 5), s.indexOf("o", 8), s.indexOf("l", -3),
    u.includes("a", 4), u.startsWith("c", 2), u.indexOf("b", 2), u.slice(1, 3),
  ].join(",");
}
const b: any = [10, 20, 1];
const t = ["ab", "c", "ab"];
function arrays(): string {
  return [
    b.includes(2), b.includes(1, 2), b.indexOf(1), b.indexOf(20, 2),
    JSON.stringify(b.slice(0, 2)), t.includes("c", 2), t.indexOf("ab", 1),
    Object.keys({ p: 1, q: 2, r: 3 }).slice(0, 2).join(","),
  ].join(",");
}
class Bag {
  includes(x: any, from: any) { return "own:" + x + ":" + from; }
  indexOf(x: any) { return x * 2; }
}
const bag: any = new Bag();
console.log(strings());
console.log(arrays());
console.log(bag.includes(1, 2), bag.indexOf(4));
async function main() {
  console.log("async", strings());
  console.log("async", arrays());
  console.log("async", [s.includes("hello", 1), s.indexOf("o", 5), b.includes(2), JSON.stringify(b.slice(0, 2))].join(","));
}
main();
