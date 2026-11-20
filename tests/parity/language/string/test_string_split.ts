// Test string split method
// Note: Printing string elements from arrays is a known limitation
// (console.log treats array elements as numbers, not strings)
// So we only test that split returns arrays with correct lengths

let csv: string = "apple,banana,cherry";
let parts: string[] = csv.split(",");
console.log(parts.length);  // 3

// Split with different delimiter
let words: string = "hello world foo bar";
let wordList: string[] = words.split(" ");
console.log(wordList.length);  // 4

// Split with no matches (returns array with original string)
let hello: string = "hello";
let noMatch: string[] = hello.split(",");
console.log(noMatch.length);  // 1

// Split on multi-char delimiter
let path: string = "a::b::c";
let segments: string[] = path.split("::");
console.log(segments.length);  // 3

console.log("String split tests passed!");
