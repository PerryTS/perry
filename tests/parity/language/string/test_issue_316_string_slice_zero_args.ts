// Regression for #316: String.prototype.slice() / substring() 0-arg form
// used to fail at codegen with
// "perry-codegen: String.slice expects 1 or 2 args, got 0".
// Surfaced compiling effect/src/internal/redBlackTree/iterator.ts
// (RedBlackTreeIterator::clone uses `s.slice()` as the "clone string"
// idiom) during the #309 compat sweep.

const s = "hello";

// 0-arg slice — clone shape.
const clone = s.slice();
console.log(clone);                   // hello
console.log(clone === s);             // true (string interning equality)
console.log(clone.length);            // 5

// 0-arg substring — same clone shape, different runtime fn.
const sub = s.substring();
console.log(sub);                     // hello
console.log(sub === s);               // true
console.log(sub.length);              // 5

// 1-arg form still works.
console.log(s.slice(2));              // llo
console.log(s.substring(2));          // llo
console.log(s.slice(0));              // hello
console.log(s.substring(0));          // hello

// 2-arg form still works.
console.log(s.slice(1, 4));           // ell
console.log(s.substring(1, 4));       // ell
console.log(s.slice(0, s.length));    // hello

// Empty receiver: 0-arg slice of "" returns "".
const empty = "";
console.log(empty.slice());           // (empty line)
console.log(empty.substring());       // (empty line)
console.log(empty.slice().length);    // 0

// Multi-byte UTF-8 receiver — slice() clones the whole string verbatim.
const u = "αβγδε";
console.log(u.slice());               // αβγδε
console.log(u.slice().length);        // 5
console.log(u.substring());           // αβγδε

// Round-trip through method-chained calls.
console.log("abc".slice());                                   // abc
console.log("abcdef".slice(1).slice());                       // bcdef
console.log("abcdef".slice().slice(2, 4));                    // cd
