// matchesGlob as Node's bundled minimatch answers: extglob arms that contain `[|]`,
// nested extglobs, a negative extglob nested in another, empty extglobs, unclosed
// classes, NUL text around braces, brace-expansion's limits and extreme ranges,
// `//?/C:` drive roots, one UTF-16 unit per `?`, and the errors for an over-long
// pattern and a regular expression too large.
import * as path from "node:path";

for (const [subject, pattern] of [
  ["a|b", "@(a[|]b|c)"],
  ["|", "@([|]|x)"],
  ["d", "@(a|@(b|@(c|@(d))))"],
  ["wxb", "@(w!(x)|q)b"],
  ["wyb", "@(w!(x)|q)b"],
  ["@()", "@()"],
  ["x", "@()"],
  ["[x", "[*"],
  ["[ab", "[@(a|b)b"],
  ["x\0oA", "x\0o{A,B}"],
  ["99999", "{1..200000}"],
  ["150000", "{1..200000}"],
  ["2", "{1..3..-9223372036854775808}"],
  ["9223372036854775807", "{9223372036854775807..9223372036854775807}"],
  ["\u{1F600}", "?"],
  ["\u{1F600}", "??"],
] as const) {
  console.log(JSON.stringify(subject), JSON.stringify(pattern), path.matchesGlob(subject, pattern));
}
console.log("win32 //?/C:", path.win32.matchesGlob("//?/C:/a.txt", "c:/*.txt"), path.win32.matchesGlob("//?/c:/src/a.ts", "C:/src/*.ts"));
try { path.matchesGlob("a", "x".repeat(65537)); } catch (e: any) { console.log("long:", e.name, e.message); }
try { path.matchesGlob("bbb", "!(a)".repeat(16)); } catch (e: any) { console.log("large:", e.name); }
