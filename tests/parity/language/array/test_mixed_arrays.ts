// Test mixed-type arrays
// This file tests arrays that can hold different types (strings, numbers)

// Create a mixed-type array with numbers and strings
let arr: (string | number)[] = [1, "hello", 2, "world"];

// Access and print elements
console.log(arr[0]);  // 1
console.log(arr[1]);  // hello
console.log(arr[2]);  // 2
console.log(arr[3]);  // world

// Test array element assignment (number)
arr[0] = 42;
console.log(arr[0]);  // 42

// Test array element assignment (string)
arr[1] = "updated";
console.log(arr[1]);  // updated

// Test array with inferred mixed type (no explicit type annotation)
let arr2 = [100, "test", 200, "string"];
console.log(arr2[0]);  // 100
console.log(arr2[1]);  // test
console.log(arr2[2]);  // 200
console.log(arr2[3]);  // string

// Test assigning different types to inferred array
arr2[0] = 999;
arr2[1] = "modified";
console.log(arr2[0]);  // 999
console.log(arr2[1]);  // modified
