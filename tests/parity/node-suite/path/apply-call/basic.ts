// #1722: stdlib namespace methods invoked indirectly via
// Function.prototype.apply / .call must reach the native impl (they used
// to read as `undefined`). Surfaced by the #800 node-core radar
// (test-path-join.js uses `path.join.apply(...)`).
import * as path from "node:path";

// typeof is "function" both directly and after capture.
console.log("typeof join:", typeof path.join);

// .apply with a literal args array (the radar shape).
console.log("apply 2:", path.join.apply(null, ["a", "b"]));
console.log("apply 3:", path.join.apply(null, ["a", "b", "c"]));
console.log("apply empty:", path.join.apply(null, []));

// .call with positional args.
console.log("call 2:", path.join.call(null, "x", "y"));
console.log("call none:", path.join.call(null));

// Other single-arg path methods via apply / call.
console.log("dirname apply:", path.dirname.apply(null, ["/a/b/c.txt"]));
console.log("basename call:", path.basename.call(null, "/a/b/c.txt"));
console.log("extname apply:", path.extname.apply(null, ["/a/b/c.txt"]));
console.log("isAbsolute call:", path.isAbsolute.call(null, "/foo"));
console.log("isAbsolute apply:", path.isAbsolute.apply(null, ["foo"]));
console.log("resolve apply:", path.resolve.apply(null, ["/a", "b"]));
