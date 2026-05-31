import * as path from "node:path";

for (const [name, subject, pattern] of [
  ["extglob js", "foo.js", "*.@(js|ts)"],
  ["extglob ts", "foo.ts", "*.@(js|ts)"],
  ["extglob miss", "foo.txt", "*.@(js|ts)"],
  ["extglob plus", "foo.jsts", "*.+(js|ts)"],
  ["extglob star zero", "foo.", "*.*(js|ts)"],
  ["extglob star many", "foo.jsts", "*.*(js|ts)"],
  ["extglob question empty", "foo.", "*.?(js|ts)"],
  ["extglob question one", "foo.js", "*.?(js|ts)"],
  ["globstar zero", "a/c", "a/**/c"],
  ["globstar one", "a/b/c", "a/**/c"],
  ["globstar many", "a/b/d/c", "a/**/c"],
  ["embedded starstar slash", "a/b", "a**b"],
  ["embedded starstar no slash", "ab", "a**b"],
  ["pattern bslash slash path", "foo/bar", "foo\\*"],
  ["pattern bslash bslash path", "foo\\bar", "foo\\*"],
] as const) {
  console.log(`${name}:`, path.matchesGlob(subject, pattern));
}

console.log("posix globstar zero:", path.posix.matchesGlob("a/c", "a/**/c"));
console.log("posix extglob:", path.posix.matchesGlob("foo.ts", "*.@(js|ts)"));
