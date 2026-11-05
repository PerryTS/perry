// Test file for RegExp support in Perry

// Test 1: String.replace() with regex (non-global)
let str = "hello world";
console.log("Test 1: 'hello world'.replace(/world/, 'universe')");
let replaced1 = str.replace(/world/, "universe");
console.log(replaced1);  // should print "hello universe"

// Test 2: String.replace() with global regex
let str2 = "hello hello hello";
console.log("Test 2: 'hello hello hello'.replace(/hello/g, 'hi')");
let replaced2 = str2.replace(/hello/g, "hi");
console.log(replaced2);  // should print "hi hi hi"

// Test 3: Case insensitive replace
let str3 = "Hello World";
console.log("Test 3: Case insensitive replace");
let replaced3 = str3.replace(/hello/i, "HI");
console.log(replaced3);  // should print "HI World"

// Test 4: regex.test() - basic match
console.log("Test 4: regex.test()");
const re1 = /hello/i;
console.log(re1.test("Hello World"));  // true
console.log(re1.test("goodbye"));      // false

// Test 5: regex.test() with inline regex
console.log("Test 5: inline regex.test()");
console.log(/\d+/.test("abc123"));  // true
console.log(/\d+/.test("abcdef"));  // false

// Test 6: string.match() with global flag
console.log("Test 6: string.match() global");
const text = "cat bat sat";
const matches = text.match(/[a-z]at/g);
if (matches) {
  console.log(matches.length);  // 3
  console.log(matches[0]);      // cat
  console.log(matches[1]);      // bat
  console.log(matches[2]);      // sat
}

// Test 7: string.match() without global flag (first match only)
console.log("Test 7: string.match() non-global");
const m2 = "hello 42 world 99".match(/\d+/);
if (m2) {
  console.log(m2[0]);  // 42
}

console.log("All regex tests completed!");
