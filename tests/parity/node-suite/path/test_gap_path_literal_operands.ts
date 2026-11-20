// Program string literals live in the executable image (perry_strings). Every
// path entry validates its operands as strings; an image literal must pass
// that validation exactly like a heap string. When the image layout grew an
// 8-aligned atom-hash trailer, the validator still required the old size, so
// every literal operand was "not a string": path.join(".next", "BUILD_ID")
// returned "." and path.normalize("a/b") threw ERR_INVALID_ARG_TYPE.
// Non-string operands must still throw ERR_INVALID_ARG_TYPE like node.
import * as path from "path";

function check(name: string, f: () => unknown): void {
  try {
    console.log(name, JSON.stringify(f()));
  } catch (e: any) {
    console.log(name, "threw", e.name, e.code);
  }
}

const rt = (s: string): string => s.split("").join("");

check("join-2", () => path.join(".next", "BUILD_ID"));
check("join-dot", () => path.join("a", ".next"));
check("join-1", () => path.join(".next"));
check("join-3", () => path.join("/srv", ".next", "server"));
check("join-mixed", () => path.join(rt(".next"), "BUILD_ID"));
check("join-empty", () => path.join("", ""));
check("join-long", () => path.join("/a/rather/long/literal/directory", "file.json"));
check("normalize", () => path.normalize("a/b/../c/./d"));
check("resolve", () => path.resolve("/r", ".next", "B"));
check("basename", () => path.basename("/a/b/file.txt", ".txt"));
check("dirname", () => path.dirname("/a/b/file.txt"));
check("extname", () => path.extname("index.html"));
check("isAbsolute", () => path.isAbsolute("/x"));
check("relative", () => path.relative("/a/b", "/a/c/d"));
check("parse", () => path.parse("/home/u/file.txt"));
check("format", () => path.format({ dir: "/x", base: "y.z" }));
check("posix-join", () => path.posix.join("a", "b"));

const bad: any[] = [null, undefined, 5, {}];
for (const v of bad) {
  check("join-bad-" + String(v), () => path.join("a", v));
  check("join-bad-first-" + String(v), () => path.join(v, "a"));
  check("normalize-bad-" + String(v), () => path.normalize(v));
}
