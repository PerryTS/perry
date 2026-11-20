// node:path — `basename` / `dirname` are purely LEXICAL string operations in Node:
// `.` and `..` are ordinary segment names and separators are never collapsed.
// perry delegated them to Rust's `std::path::Path`, whose OS path semantics differ
// (file_name() is None for `.`/`..`/a trailing `..`, and DROPS a `.` component;
// parent() normalizes `.` and empty segments away). `normalize` also dropped a
// trailing separator, and single-arg `path.win32.join(x)` was an identity no-op.
//
// Validated byte-for-byte against `node --experimental-strip-types`.

import path from "node:path";

const P = path.posix;

// basename/dirname must NOT resolve "." or ".."
console.log("basename('.')        =", JSON.stringify(P.basename(".")));
console.log("basename('..')       =", JSON.stringify(P.basename("..")));
console.log("basename('./')       =", JSON.stringify(P.basename("./")));
console.log("basename('a/.')      =", JSON.stringify(P.basename("a/.")));
console.log("basename('a/..')     =", JSON.stringify(P.basename("a/..")));
console.log("basename('foo/bar/..')=", JSON.stringify(P.basename("foo/bar/..")));
console.log("basename('/.')       =", JSON.stringify(P.basename("/.")));

// dirname must NOT collapse separators or "." components
console.log("dirname('a/.')       =", JSON.stringify(P.dirname("a/.")));
console.log("dirname('a//b')      =", JSON.stringify(P.dirname("a//b")));
console.log("dirname('a/./b')     =", JSON.stringify(P.dirname("a/./b")));
console.log("dirname('/.')        =", JSON.stringify(P.dirname("/.")));
console.log("dirname('/')         =", JSON.stringify(P.dirname("/")));
console.log("dirname('//foo')     =", JSON.stringify(P.dirname("//foo")));

// normalize keeps a trailing separator, even when everything normalizes away
console.log("normalize('./')      =", JSON.stringify(P.normalize("./")));
console.log("normalize('')        =", JSON.stringify(P.normalize("")));

// basename(path, ext) — Node's backward-scan algorithm, quirks included
console.log("basename('.js','.js')      =", JSON.stringify(P.basename(".js", ".js")));
console.log("basename('aaa/bbb','bb')   =", JSON.stringify(P.basename("aaa/bbb", "bb")));
console.log("basename('aaa/bbb','b')    =", JSON.stringify(P.basename("aaa/bbb", "b")));
console.log("basename('file.js','.ts')  =", JSON.stringify(P.basename("file.js", ".ts")));
console.log("basename('/x/','.x')       =", JSON.stringify(P.basename("/x/", ".x")));

// single-arg win32 join is normalize(), not identity
console.log("win32.join('')       =", JSON.stringify(path.win32.join("")));
console.log("win32.join('a/b')    =", JSON.stringify(path.win32.join("a/b")));
console.log("win32.join('a/.')    =", JSON.stringify(path.win32.join("a/.")));
console.log("win32.join('/')      =", JSON.stringify(path.win32.join("/")));
console.log("win32.join('a','b')  =", JSON.stringify(path.win32.join("a", "b")));
