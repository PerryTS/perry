import * as path from "node:path";

console.log("ext empty:", path.basename("foo.txt", ""));
console.log("ext exact whole:", path.basename("foo", "foo"));
console.log("ext longer than base:", path.basename("foo", "foobar"));
console.log("ext repeated suffix:", path.basename("foo.txt.txt", ".txt"));
console.log("ext dot only:", path.basename("foo.", "."));
