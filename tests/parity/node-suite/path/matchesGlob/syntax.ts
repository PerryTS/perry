// The glob syntax matchesGlob accepts: leading dots, braces and ranges, classes,
// negated extglobs, globstars, `.` and `..` and repeated slashes in the subject, empty
// patterns, and win32 drives and UNC roots. Inputs use one letter case,
// so the answer does not depend on the platform's case sensitivity.
import * as path from "node:path";

for (const [subject, pattern] of [
  ["x/.hidden", "x/*"],
  [".", ".*"],
  ["a/.git/c", "a/**/c"],
  ["f03", "f{01..3}"],
  ["fc", "f{a..c}"],
  ["x.ts", "!(*.js)"],
  ["x.js", "!(*.js)"],
  ["a/b/../c", "a/c"],
  ["a//b", "a/b"],
  ["x/.hidden", "x/.*"],
  ["a", "a/**"],
  ["a/", "a/**"],
  ["readme.md", "*.{md,txt}"],
  ["b", "[a-c]"],
  ["d", "[!a-c]"],
  ["", ""],
  ["a", "{,a}"],
] as const) {
  console.log(`posix ${subject} ~ ${pattern}:`, path.matchesGlob(subject, pattern));
}
for (const [subject, pattern] of [
  ["c:\\a\\b.ts", "c:/a/*.ts"],
  ["\\\\srv\\share\\x", "//srv/share/*"],
  ["a\\b", "a\\b"],
] as const) {
  console.log(`win32 ${subject} ~ ${pattern}:`, path.win32.matchesGlob(subject, pattern));
}
