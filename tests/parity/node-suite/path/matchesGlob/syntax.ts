// The glob syntax matchesGlob accepts: dot segments, braces, classes, a trailing
// globstar, empty patterns, and win32 drives and UNC roots. Inputs use one letter case,
// so the answer does not depend on the platform's case sensitivity.
import * as path from "node:path";

for (const [subject, pattern] of [
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
