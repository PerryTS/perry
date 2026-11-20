const text = "a🙂é🙂z";
const hi = text.slice(1, 2), lo = text.slice(2, 3);
console.log(text.length, hi.charCodeAt(0), lo.charCodeAt(0));
console.log((hi + lo).codePointAt(0), (hi + lo).isWellFormed());
console.log(hi.isWellFormed(), lo.toWellFormed().charCodeAt(0));
console.log(text.indexOf(hi), text.indexOf(lo), text.indexOf(lo, 3));
console.log(text.indexOf("🙂", 2), text.indexOf("é"), text.lastIndexOf(lo));
console.log("aaaa".lastIndexOf("aa", 1));
console.log(text.indexOf("z"), text.indexOf("" as string, Infinity));
const long = "aé🙂".repeat(150000);
console.log(long.length, long.slice(-4), long.indexOf("é🙂", 599996));
const parts = [hi, lo, "suffix"];
console.log(JSON.stringify(parts.join("")), JSON.stringify(parts.join("|")));
let build = "";
for (let i = 0; i < 500; i++) build += hi + lo;
console.log(build.length, build.codePointAt(998), build.isWellFormed());
for (const n of [-0, -1.9, 0.125, 999999999, 1e21, NaN, Infinity]) {
  console.log("id-" + n, `${n}-suffix`, n.toString());
}
console.log("abc".slice(NaN, Infinity), "abc".charCodeAt(1.9), "abc".charCodeAt("1" as any));
const parsed = JSON.parse('"' + 'safe-prefix-'.repeat(8) + '"');
for (const suffix of ['"', '\n', '\\', 'plain']) {
  console.log(JSON.stringify(parsed + suffix), JSON.stringify(suffix + parsed));
}
