// Test regex `d` flag (hasIndices) implementation
// Tests ECMA-262 RegExpBuiltinExec indices property

// Test 1: Basic indices for full match
const re1 = /hello/d;
const m1 = re1.exec("say hello world");
console.log("Test 1: Basic indices");
console.log(m1 !== null); // true
console.log(JSON.stringify(m1!.indices)); // [[4,9]]
console.log(m1!.indices[0][0]); // 4
console.log(m1!.indices[0][1]); // 9

// Test 2: Indices with capture groups
const re2 = /(\w+)@(\w+)/d;
const m2 = re2.exec("email: test@example");
console.log("Test 2: Capture groups");
console.log(m2 !== null); // true
console.log(JSON.stringify(m2!.indices)); // [[7,19],[7,11],[12,19]]
console.log(m2!.indices.length); // 3
console.log(JSON.stringify(m2!.indices[0])); // [7,19] - full match
console.log(JSON.stringify(m2!.indices[1])); // [7,11] - group 1
console.log(JSON.stringify(m2!.indices[2])); // [12,19] - group 2

// Test 3: Indices with named groups
const re3 = /(?<year>\d{4})-(?<month>\d{2})-(?<day>\d{2})/d;
const m3 = re3.exec("date: 2024-03-15");
console.log("Test 3: Named groups");
console.log(m3 !== null); // true
console.log(JSON.stringify(m3!.indices)); // [[6,16],[6,10],[11,13],[14,16]]
console.log(m3!.indices.groups !== undefined); // true
console.log(JSON.stringify(m3!.indices.groups!.year)); // [6,10]
console.log(JSON.stringify(m3!.indices.groups!.month)); // [11,13]
console.log(JSON.stringify(m3!.indices.groups!.day)); // [14,16]

// Test 4: Unmatched optional group
const re4 = /(\d+)(\.(\d+))?/d;
const m4 = re4.exec("integer: 42");
console.log("Test 4: Unmatched optional group");
console.log(m4 !== null); // true
console.log(JSON.stringify(m4!.indices)); // [[9,11],[9,11],null,null]
console.log(m4!.indices[0] !== null); // true - full match
console.log(m4!.indices[1] !== null); // true - group 1 matched
console.log(m4!.indices[2] === undefined); // true - group 2 unmatched
console.log(m4!.indices[3] === undefined); // true - group 3 unmatched

// Test 5: hasIndices getter
const re5 = /test/d;
console.log("Test 5: hasIndices getter");
console.log(re5.hasIndices); // true

const re5b = /test/;
console.log(re5b.hasIndices); // false

const re5c = /test/gi;
console.log(re5c.hasIndices); // false

const re5d = /test/dgi;
console.log(re5d.hasIndices); // true

// Test 6: Without d flag, no indices
const re6 = /test/;
const m6 = re6.exec("test");
console.log("Test 6: Without d flag");
console.log(m6 !== null); // true
console.log(m6!.indices); // undefined

// Test 7: match() with d flag
const re7 = /(\w+)@(\w+)/d;
const m7 = "email: test@example".match(re7);
console.log("Test 7: match() with d flag");
console.log(m7 !== null); // true
console.log(JSON.stringify(m7!.indices)); // [[7,19],[7,11],[12,19]]

// Test 8: match() without d flag
const re8 = /(\w+)@(\w+)/;
const m8 = "email: test@example".match(re8);
console.log("Test 8: match() without d flag");
console.log(m8 !== null); // true
console.log(m8!.indices); // undefined

// Test 9: Global match with d flag
const re9 = /\d+/dg;
const text9 = "a1b22c333";
const m9_1 = re9.exec(text9);
console.log("Test 9: Global match with d flag");
console.log(m9_1 !== null); // true
console.log(JSON.stringify(m9_1!.indices)); // [[1,2]]
console.log(re9.lastIndex); // 2

const m9_2 = re9.exec(text9);
console.log(m9_2 !== null); // true
console.log(JSON.stringify(m9_2!.indices)); // [[3,5]]
console.log(re9.lastIndex); // 5

const m9_3 = re9.exec(text9);
console.log(m9_3 !== null); // true
console.log(JSON.stringify(m9_3!.indices)); // [[6,9]]
console.log(re9.lastIndex); // 9

const m9_4 = re9.exec(text9);
console.log(m9_4); // null
console.log(re9.lastIndex); // 0

// Test 10: Empty match with d flag
const re10 = /a*/d;
const m10 = re10.exec("b");
console.log("Test 10: Empty match with d flag");
console.log(m10 !== null); // true
console.log(JSON.stringify(m10!.indices)); // [[0,0]]

// Test 11: Zero-width assertion with d flag
const re11 = /(?=hello)/d;
const m11 = re11.exec("say hello");
console.log("Test 11: Zero-width assertion with d flag");
console.log(m11 !== null); // true
console.log(JSON.stringify(m11!.indices)); // [[4,4]]

// Test 12: Lookbehind with d flag
const re12 = /(?<=\$)\d+/d;
const m12 = re12.exec("price: $100");
console.log("Test 12: Lookbehind with d flag");
console.log(m12 !== null); // true
console.log(JSON.stringify(m12!.indices)); // [[8,11]]

// Test 13: Backreference with d flag
const re13 = /(\w)\1/d;
const m13 = re13.exec("book");
console.log("Test 13: Backreference with d flag");
console.log(m13 !== null); // true
console.log(JSON.stringify(m13!.indices)); // [[1,3],[1,2]]

// Test 14: Complex pattern with d flag
const re14 = /(?<protocol>https?):\/\/(?<host>[^\/]+)(?<path>\/.*)?/d;
const m14 = re14.exec("url: https://example.com/path/to/resource");
console.log("Test 14: Complex pattern with d flag");
console.log(m14 !== null); // true
console.log(m14!.indices !== undefined); // true
console.log(m14!.indices.groups !== undefined); // true
console.log(JSON.stringify(m14!.indices.groups!.protocol)); // [5,10]
console.log(JSON.stringify(m14!.indices.groups!.host)); // [13,24]
console.log(JSON.stringify(m14!.indices.groups!.path)); // [24,41]

// Test 15: indices property is own property
const re15 = /test/d;
const m15 = re15.exec("test");
console.log("Test 15: indices is own property");
console.log(m15 !== null); // true
console.log(Object.prototype.hasOwnProperty.call(m15, "indices")); // true

// Test 16: indices survives aliasing
const re16 = /(\w+)/d;
const m16_1 = re16.exec("hello");
const m16_2 = re16.exec("world");
console.log("Test 16: indices survives aliasing");
console.log(JSON.stringify(m16_1!.indices)); // [[0,5]]
console.log(JSON.stringify(m16_2!.indices)); // [[0,5]]

// Test 17: d flag with other flags
const re17 = /hello/dgi;
console.log("Test 17: d flag with other flags");
console.log(re17.hasIndices); // true
console.log(re17.global); // true
console.log(re17.ignoreCase); // true

// Test 18: RegExp constructor with d flag
const re18 = new RegExp("test", "d");
console.log("Test 18: RegExp constructor with d flag");
console.log(re18.hasIndices); // true
const m18 = re18.exec("test");
console.log(m18 !== null); // true
console.log(JSON.stringify(m18!.indices)); // [[0,4]]

// Expected output:
// Test 1: Basic indices
// true
// [[4,9]]
// 4
// 9
// Test 2: Capture groups
// true
// [[7,19],[7,11],[12,19]]
// 3
// [7,19]
// [7,11]
// [12,19]
// Test 3: Named groups
// true
// [[6,16],[6,10],[11,13],[14,16]]
// true
// [6,10]
// [11,13]
// [14,16]
// Test 4: Unmatched optional group
// true
// [[9,11],[9,11],null,null]
// true
// true
// true
// true
// Test 5: hasIndices getter
// true
// false
// false
// true
// Test 6: Without d flag
// true
// undefined
// Test 7: match() with d flag
// true
// [[7,19],[7,11],[12,19]]
// Test 8: match() without d flag
// true
// undefined
// Test 9: Global match with d flag
// true
// [[1,2]]
// 2
// true
// [[3,5]]
// 5
// true
// [[6,9]]
// 9
// null
// 0
// Test 10: Empty match with d flag
// true
// [[0,0]]
// Test 11: Zero-width assertion with d flag
// true
// [[4,4]]
// Test 12: Lookbehind with d flag
// true
// [[8,11]]
// Test 13: Backreference with d flag
// true
// [[1,3],[1,2]]
// Test 14: Complex pattern with d flag
// true
// true
// true
// [5,10]
// [13,24]
// [24,41]
// Test 15: indices is own property
// true
// true
// Test 16: indices survives aliasing
// [[0,5]]
// [[0,5]]
// Test 17: d flag with other flags
// true
// true
// true
// Test 18: RegExp constructor with d flag
// true
// true
// [[0,4]]
