// Test missing path functions for Perry parity gap analysis
// Expected: all assertions pass under node --experimental-strip-types

import * as path from 'path';

// --- path.relative ---
const rel = path.relative('/a/b', '/a/c');
console.log(rel); // ../c

// --- path.parse ---
const parsed = path.parse('/home/user/file.txt');
console.log(parsed.root); // /
console.log(parsed.dir); // /home/user
console.log(parsed.base); // file.txt
console.log(parsed.ext); // .txt
console.log(parsed.name); // file

// --- path.format ---
const formatted = path.format({ dir: '/home', base: 'file.txt' });
console.log(formatted); // /home/file.txt

// --- path.normalize ---
const normalized = path.normalize('/a/b/../c');
console.log(normalized); // /a/c

// Normalize with redundant separators
const normalized2 = path.normalize('/a//b///c');
console.log(normalized2); // /a/b/c

// Normalize with dot segments
const normalized3 = path.normalize('/a/./b/./c');
console.log(normalized3); // /a/b/c

// --- path.sep ---
console.log(typeof path.sep === 'string'); // true
console.log(path.sep.length === 1); // true
// On POSIX it's '/', on Windows it's '\\'
console.log(path.sep === '/' || path.sep === '\\'); // true

// --- path.delimiter ---
console.log(typeof path.delimiter === 'string'); // true
// On POSIX it's ':', on Windows it's ';'
console.log(path.delimiter === ':' || path.delimiter === ';'); // true

// --- path.isAbsolute ---
console.log(path.isAbsolute('/foo')); // true
console.log(path.isAbsolute('/foo/bar')); // true
console.log(path.isAbsolute('foo')); // false
console.log(path.isAbsolute('foo/bar')); // false
console.log(path.isAbsolute('./foo')); // false
console.log(path.isAbsolute('')); // false

// --- path.join (already supported, but verify with edge cases) ---
const joined = path.join('/a', 'b', '..', 'c');
console.log(joined); // /a/c

// --- path.resolve (already supported, but verify) ---
const resolved = path.resolve('/a', 'b', 'c');
console.log(resolved); // /a/b/c

// --- path.extname ---
console.log(path.extname('file.txt')); // .txt
console.log(path.extname('file.tar.gz')); // .gz
console.log(path.extname('file')); // (empty string)
console.log(path.extname('.hidden')); // (empty string)

// --- path.basename ---
console.log(path.basename('/home/user/file.txt')); // file.txt
console.log(path.basename('/home/user/file.txt', '.txt')); // file

// --- path.dirname ---
console.log(path.dirname('/home/user/file.txt')); // /home/user

// --- Roundtrip: parse then format ---
const original = '/home/user/documents/report.pdf';
const roundtrip = path.format(path.parse(original));
console.log(roundtrip === original); // true

console.log("All path gap tests passed!");

// Expected output:
// ../c
// /
// /home/user
// file.txt
// .txt
// file
// /home/file.txt
// /a/c
// /a/b/c
// /a/b/c
// true
// true
// true
// true
// true
// true
// true
// false
// false
// false
// false
// /a/c
// /a/b/c
// .txt
// .gz
//
//
// file.txt
// file
// /home/user
// true
// All path gap tests passed!
