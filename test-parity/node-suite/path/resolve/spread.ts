// #7720: `path.resolve(...segments)` — the reset-on-absolute sibling of
// `join`'s spread case. Every result is anchored on an absolute first segment
// so the output does not depend on the process cwd.
import path from "node:path";
import { resolve } from "node:path";

const parts = ["/tmp/x", "project.json"];

console.log("spread:", path.resolve(...parts));
console.log("mixed:", path.resolve("/base", ...parts));
console.log("reset on absolute:", path.resolve("/base", ...["rel", "/abs", "tail"]));
console.log("single:", path.resolve(...["/tmp/x"]));
console.log("dotdot:", path.resolve(...["/foo/bar", "..", "baz"]));
console.log("named import:", resolve(...parts));
console.log("posix:", path.posix.resolve(...parts));
