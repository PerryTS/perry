import * as path from "node:path";

for (const [name, subject, pattern] of [
  ["bslash segment", "foo\\bar", "foo\\*"],
  ["bslash too deep", "foo\\bar\\baz", "foo\\*"],
  ["slash pattern bslash path", "foo\\bar", "foo/*"],
  ["slash pattern too deep", "foo\\bar\\baz", "foo/*"],
  ["bslash pattern slash path", "foo/bar", "foo\\*"],
  ["globstar zero", "a\\c", "a/**/c"],
  ["globstar one", "a\\b\\c", "a/**/c"],
] as const) {
  console.log(`${name}:`, path.win32.matchesGlob(subject, pattern));
}
