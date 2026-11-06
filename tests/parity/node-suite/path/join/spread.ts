// #7720: spread invocation with VALID string segments. `type-errors-extra.ts`
// already spread-calls `path.join`, but only with segments Node rejects — so it
// stayed green while the spread operand was being folded in as one array
// argument (both lowerings threw ERR_INVALID_ARG_TYPE). These are the cases
// that tell the two apart.
import path from "node:path";
import { join, posix, win32 } from "node:path";

const parts = ["/tmp/x", "project.json"];

console.log("spread:", path.join(...parts));
console.log("mixed:", path.join("/base", ...parts));
console.log("trailing:", path.join(...parts, "extra"));
console.log("single:", path.join(...["/tmp/x"]));
console.log("empty:", path.join(...([] as string[])));
console.log("normalizing:", path.join(...["/foo", "bar", "..", "baz", "."]));

console.log("named import:", join(...parts));
console.log("posix:", posix.join(...parts));
console.log("win32:", win32.join(...parts));
console.log("subns member:", path.posix.join(...parts));

const alias = path.win32;
console.log("subns alias:", alias.join(...parts));

const value = path.join;
console.log("value read:", value(...parts));

const nested = [["/a", "b"], ["/c", "d"]];
console.log("in a loop:", nested.map((seg) => path.join(...seg)).join("|"));
