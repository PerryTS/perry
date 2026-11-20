// Edge-case tests for JSON operations and regular expressions
// Tests: parse/stringify, nested JSON, regex matching, special characters

// === JSON Tests ===

// --- JSON.stringify basic types ---
console.log(JSON.stringify(42));        // 42
console.log(JSON.stringify("hello"));   // "hello"
console.log(JSON.stringify(true));      // true
console.log(JSON.stringify(false));     // false
console.log(JSON.stringify(null));      // null

// --- JSON.stringify object ---
const obj = { name: "Alice", age: 30 };
console.log(JSON.stringify(obj));  // {"name":"Alice","age":30}

// --- JSON.stringify array ---
console.log(JSON.stringify([1, 2, 3]));          // [1,2,3]
console.log(JSON.stringify(["a", "b", "c"]));    // ["a","b","c"]

// --- JSON.stringify nested ---
const nested = {
    user: { name: "Bob", scores: [95, 87, 92] },
    active: true
};
const jsonStr = JSON.stringify(nested);
console.log(jsonStr.includes("Bob"));     // true
console.log(jsonStr.includes("scores"));  // true

// --- JSON.parse basic ---
const parsed1 = JSON.parse("42");
console.log(parsed1);  // 42

const parsed2 = JSON.parse('"hello"');
console.log(parsed2);  // hello

const parsed3 = JSON.parse("true");
console.log(parsed3);  // true

// --- JSON.parse object ---
const parsed4 = JSON.parse('{"name":"Alice","age":30}');
console.log(parsed4.name);  // Alice
console.log(parsed4.age);   // 30

// --- JSON.parse array ---
const parsed5 = JSON.parse("[1,2,3]");
console.log(parsed5.length);     // 3
console.log(parsed5[0]);         // 1
console.log(parsed5[2]);         // 3

// --- JSON round-trip ---
const original = { x: 1, y: [2, 3], z: { w: 4 } };
const roundTrip = JSON.parse(JSON.stringify(original));
console.log(roundTrip.x);      // 1
console.log(roundTrip.y[0]);   // 2
console.log(roundTrip.y[1]);   // 3
console.log(roundTrip.z.w);    // 4

// --- JSON.parse with nested structure ---
const deepJson = '{"a":{"b":{"c":42}}}';
const deepParsed = JSON.parse(deepJson);
console.log(deepParsed.a.b.c);  // 42

// --- JSON with special characters ---
const withEscape = JSON.stringify({ msg: 'hello "world"' });
console.log(withEscape.includes("hello"));  // true

// --- JSON.stringify with null values ---
const withNull = JSON.stringify({ a: 1, b: null, c: 3 });
console.log(withNull.includes("null"));  // true

// === Regex Tests ===

// --- Basic regex test ---
const re1 = /hello/;
console.log(re1.test("hello world"));  // true
console.log(re1.test("goodbye"));      // false

// --- Regex with flags ---
const reI = /hello/i;
console.log(reI.test("Hello World"));  // true
console.log(reI.test("HELLO"));        // true

// --- Regex match ---
const match1 = "hello world".match(/(\w+)\s(\w+)/);
if (match1) {
    console.log(match1[0]);  // hello world
    console.log(match1[1]);  // hello
    console.log(match1[2]);  // world
}

// --- Regex replace ---
console.log("hello world".replace(/world/, "there"));     // hello there
console.log("aabbcc".replace(/bb/, "XX"));                 // aaXXcc

// --- Regex with global flag ---
const reG = /\d+/g;
const matches = "abc 123 def 456 ghi 789".match(reG);
if (matches) {
    console.log(matches.join(","));  // 123,456,789
}

// --- Regex character classes ---
console.log(/^\d+$/.test("12345"));  // true
console.log(/^\d+$/.test("123a5"));  // false
console.log(/^[a-z]+$/.test("hello"));  // true
console.log(/^[a-z]+$/.test("Hello"));  // false

// --- Regex anchors ---
console.log(/^hello/.test("hello world"));  // true
console.log(/^hello/.test("say hello"));    // false
console.log(/world$/.test("hello world"));  // true
console.log(/world$/.test("world hello"));  // false

// --- Regex quantifiers ---
console.log(/a{3}/.test("aaa"));    // true
console.log(/a{3}/.test("aa"));     // false
console.log(/a{2,4}/.test("aaa"));  // true
console.log(/a+/.test("aaa"));      // true
console.log(/a+/.test("bbb"));      // false
console.log(/a*/.test("bbb"));      // true (zero matches)
console.log(/a?b/.test("ab"));      // true
console.log(/a?b/.test("b"));       // true

// --- Regex alternation ---
console.log(/cat|dog/.test("I have a cat"));  // true
console.log(/cat|dog/.test("I have a dog"));  // true
console.log(/cat|dog/.test("I have a fish")); // false

// --- Regex split ---
const splitResult = "one:two:three".split(/:/);
console.log(splitResult.join(","));  // one,two,three

const splitWords = "hello   world  foo".split(/\s+/);
console.log(splitWords.join(","));  // hello,world,foo

// --- Regex search ---
console.log("hello world".search(/world/));  // 6
console.log("hello world".search(/xyz/));    // -1

// --- Regex with special characters ---
console.log(/\./.test("hello.world"));  // true
console.log(/\./.test("helloworld"));   // false

// --- Email-like pattern ---
const emailRe = /^[a-zA-Z0-9]+@[a-zA-Z0-9]+\.[a-zA-Z]+$/;
console.log(emailRe.test("user@example.com"));   // true
console.log(emailRe.test("invalid@"));            // false
console.log(emailRe.test("no-at-sign.com"));      // false

// --- Regex replace with pattern ---
const cleaned = "  hello   world  ".replace(/^\s+|\s+$/g, "");
console.log(cleaned);  // hello   world

// --- Regex replace all occurrences ---
const allReplaced = "aXbXcXd".replace(/X/g, "-");
console.log(allReplaced);  // a-b-c-d

// --- Regex with groups ---
const dateStr = "2024-01-15";
const dateMatch = dateStr.match(/(\d{4})-(\d{2})-(\d{2})/);
if (dateMatch) {
    console.log(dateMatch[1]);  // 2024
    console.log(dateMatch[2]);  // 01
    console.log(dateMatch[3]);  // 15
}
