// Integration Test 3: CLI Tool Simulation
// Tests: process.env, fs, path, string methods, try-catch, JSON

import * as fs from 'fs';
import * as path from 'path';

// Test 1: Path operations
console.log("=== Test 1: Path Operations ===");
let filePath = "/home/user/documents/file.txt";
console.log(path.dirname(filePath));   // /home/user/documents
console.log(path.basename(filePath));  // file.txt
console.log(path.extname(filePath));   // .txt

let joined = path.join("/home", "user");
console.log(joined);  // /home/user

// Test 2: File operations with error handling
console.log("=== Test 2: File Operations ===");
let testFile = "/tmp/cli_test.txt";
let testData = "line1\nline2\nline3";

fs.writeFileSync(testFile, testData);
console.log(fs.existsSync(testFile));  // 1

let readBack = fs.readFileSync(testFile);
console.log(readBack);

// Clean up
fs.unlinkSync(testFile);
console.log(fs.existsSync(testFile));  // 0

// Test 3: String processing
console.log("=== Test 3: String Processing ===");
let input = "  HELLO WORLD  ";
let trimmed = input.trim();
console.log(trimmed);
console.log(trimmed.toLowerCase());
console.log(trimmed.toUpperCase());

let replaced = trimmed.replace(/WORLD/, "COMPILETS");
console.log(replaced);

// Test 4: JSON operations
console.log("=== Test 4: JSON ===");
let jsonStr = "{\"name\":\"test\",\"count\":42}";
let parsed = JSON.parse(jsonStr);
console.log(typeof parsed);  // object

let obj = { x: 10, y: 20 };
let serialized = JSON.stringify(obj);
console.log(serialized);

// Test 5: Exception handling
console.log("=== Test 5: Try-Catch ===");
let result = 0;
try {
    result = 100;
    throw "error";
} catch (e) {
    result = result + 50;
}
console.log(result);  // 150

// Test with finally
let finalResult = 0;
try {
    finalResult = 10;
} finally {
    finalResult = finalResult + 5;
}
console.log(finalResult);  // 15

// Test 6: Control flow
console.log("=== Test 6: Control Flow ===");
let total = 0;
for (let i = 0; i < 5; i = i + 1) {
    if (i === 2) {
        continue;
    }
    total = total + i;
}
console.log(total);  // 0+1+3+4 = 8

let count = 0;
let j = 0;
while (j < 10) {
    if (j === 5) {
        break;
    }
    count = count + 1;
    j = j + 1;
}
console.log(count);  // 5

console.log("=== Integration Test 3 PASSED ===");
