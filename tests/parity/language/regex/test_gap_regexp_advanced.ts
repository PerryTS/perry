// Test advanced RegExp: exec, named groups, lookbehind, properties, constructor, replacer fn

// regex.exec returns match array with index
const re1 = /foo(\d+)/;
const m1 = re1.exec("abcfoo123xyz");
console.log(m1 !== null); // true
console.log(m1![0]); // foo123
console.log(m1![1]); // 123
console.log(m1!.index); // 3

// exec with global flag — lastIndex tracking
const reGlobal = /\d+/g;
const text = "a1b22c333";
const first = reGlobal.exec(text);
console.log(first![0]); // 1
console.log(reGlobal.lastIndex); // 2

const second = reGlobal.exec(text);
console.log(second![0]); // 22
console.log(reGlobal.lastIndex); // 5

const third = reGlobal.exec(text);
console.log(third![0]); // 333
console.log(reGlobal.lastIndex); // 9

const fourth = reGlobal.exec(text);
console.log(fourth); // null
console.log(reGlobal.lastIndex); // 0

// Named capture groups
const dateRe = /(?<year>\d{4})-(?<month>\d{2})-(?<day>\d{2})/;
const dateMatch = dateRe.exec("date: 2024-03-15");
console.log(dateMatch!.groups!.year); // 2024
console.log(dateMatch!.groups!.month); // 03
console.log(dateMatch!.groups!.day); // 15

// Named groups in replace
const dateStr = "2024-03-15";
const swapped = dateStr.replace(
  /(?<year>\d{4})-(?<month>\d{2})-(?<day>\d{2})/,
  "$<day>/$<month>/$<year>"
);
console.log(swapped); // 15/03/2024

// Lookbehind assertion
const prices = "Items: $10, $25, $100";
const priceRe = /(?<=\$)\d+/g;
const allPrices: string[] = [];
let pm: RegExpExecArray | null;
while ((pm = priceRe.exec(prices)) !== null) {
  allPrices.push(pm[0]);
}
console.log(allPrices.join(",")); // 10,25,100

// regex.source and regex.flags
const re2 = /hello\s+world/gi;
console.log(re2.source); // hello\s+world
console.log(re2.flags); // gi

// regex.lastIndex property (read/write)
const re3 = /x/g;
re3.lastIndex = 5;
console.log(re3.lastIndex); // 5
re3.exec("0123456x89");
console.log(re3.lastIndex); // 8

// RegExp constructor
const re4 = new RegExp("\\d+", "g");
const m4 = re4.exec("abc123def456");
console.log(m4![0]); // 123
const m5 = re4.exec("abc123def456");
console.log(m5![0]); // 456

// str.replace with function replacer
const result = "hello world foo".replace(
  /(\w+)/g,
  (match: string, p1: string) => {
    return p1.charAt(0).toUpperCase() + p1.slice(1);
  }
);
console.log(result); // Hello World Foo

// Replace function with offset
const withOffsets: number[] = [];
"aXbXc".replace(/X/g, (_match: string, offset: number) => {
  withOffsets.push(offset);
  return "Y";
});
console.log(withOffsets.join(",")); // 1,3

// test method
const re5 = /^hello/;
console.log(re5.test("hello world")); // true
console.log(re5.test("world hello")); // false

// Expected output:
// true
// foo123
// 123
// 3
// 1
// 2
// 22
// 5
// 333
// 9
// null
// 0
// 2024
// 03
// 15
// 15/03/2024
// 10,25,100
// hello\s+world
// gi
// 5
// 8
// 123
// 456
// Hello World Foo
// 1,3
// true
// false
